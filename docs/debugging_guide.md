# Debugging Guide for rsactor

This guide covers the debugging and observability tools available in rsactor, including enhanced error messages, dead letter tracking, and blocking operation timeout support.

## Table of Contents

- [Enhanced Error Messages](#enhanced-error-messages)
- [Dead Letter Tracking](#dead-letter-tracking)
- [Blocking Operations with Timeout](#blocking-operations-with-timeout)
- [Logging System](#logging-system)
- [Complete Example](#complete-example)

---

## Enhanced Error Messages

rsactor provides two methods on the `Error` type to help diagnose issues:

### `is_retryable()`

Determines whether an operation might succeed if retried.

```rust
use rsactor::Error;
use std::time::Duration;

async fn send_with_retry<T, M>(
    actor: &ActorRef<T>,
    msg: M,
    max_attempts: usize,
) -> Result<(), Error>
where
    T: Actor + Message<M>,
    M: Clone + Send + 'static,
{
    let mut attempts = 0;
    loop {
        match actor.tell(msg.clone()).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable() && attempts < max_attempts => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

**Retryable vs Non-Retryable Errors:**

| Error Type | Retryable | Reason |
|------------|-----------|--------|
| `Timeout` | Yes | Transient; may succeed with longer timeout |
| `Send` | No | Actor stopped; channel permanently closed |
| `Receive` | No | Reply channel dropped; cannot recover |
| `Downcast` | No | Type mismatch; programming error |
| `Runtime` | No | Actor lifecycle failure |
| `MailboxCapacity` | No | Configuration error |
| `Join` | No | Task panic or cancellation |

> **Important:** `is_retryable()` checks only the error type, not elapsed time. Always use fresh error instances for retry decisions.

### `debugging_tips()`

Returns actionable suggestions for resolving each error type.

```rust
use rsactor::Error;

fn log_error_with_tips(err: &Error) {
    eprintln!("Error: {}", err);
    eprintln!("Debugging tips:");
    for tip in err.debugging_tips() {
        eprintln!("  - {}", tip);
    }
}
```

**Example output for a `Send` error:**

```text
Error: Failed to send message to actor MyActor: Mailbox channel closed
Debugging tips:
  - Verify the actor is still running with `actor_ref.is_alive()`
  - The actor's mailbox is closed - the actor has terminated
  - Consider using `ActorWeak` for long-lived references
```

---

## Dead Letter Tracking

Dead letters are messages that could not be delivered to their intended recipients. rsactor automatically tracks and logs these events.

### When Dead Letters Occur

| Scenario | `DeadLetterReason` |
|----------|-------------------|
| Message sent to stopped actor | `ActorStopped` |
| `tell_with_timeout` or `ask_with_timeout` exceeds duration | `Timeout` |
| Handler fails before sending reply (in `ask`) | `ReplyDropped` |

### Observing Dead Letters

Dead letters are automatically logged via `tracing` at the `WARN` level:

```text
WARN rsactor::dead_letter: Dead letter: message could not be delivered
    actor.id=42
    actor.type_name="MyActor"
    message.type_name="PingMessage"
    dead_letter.reason="actor stopped"
    dead_letter.operation="tell"
```

To see these logs, initialize a tracing subscriber:

```rust
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("rsactor=warn")
        .init();

    // Your application code
}
```

### Testing Dead Letter Behavior

Enable the `test-utils` feature to access dead letter counters in tests:

```toml
[dev-dependencies]
rsactor = { version = "0.12", features = ["test-utils"] }
```

```rust
use rsactor::{spawn, dead_letter_count, reset_dead_letter_count};

#[tokio::test]
async fn test_dead_letter_tracking() {
    reset_dead_letter_count();
    let initial = dead_letter_count();

    let (actor_ref, handle) = spawn::<MyActor>(MyActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // This will generate a dead letter
    let _ = actor_ref.tell(Ping).await;

    assert_eq!(dead_letter_count() - initial, 1);
}
```

> **Warning:** Never enable `test-utils` in production builds. Use `tracing` logs for production observability.

### Performance Characteristics

Dead letter tracking is optimized for minimal overhead:

| Scenario | Overhead |
|----------|----------|
| Successful message delivery (hot path) | **Zero** - no code executes |
| Dead letter, no tracing subscriber | ~5-50 ns |
| Dead letter, subscriber active | ~1-10 μs |

---

## Blocking Operations with Timeout

The `blocking_tell` and `blocking_ask` methods now support optional timeouts for use in synchronous contexts.

### API Signatures

```rust
// Fire-and-forget with optional timeout
fn blocking_tell<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
where
    M: Send + 'static,
    T: Message<M>;

// Request-reply with optional timeout
fn blocking_ask<M>(&self, msg: M, timeout: Option<Duration>) -> Result<T::Reply>
where
    T: Message<M>,
    M: Send + 'static,
    T::Reply: Send + 'static;
```

### Usage Examples

```rust
use std::time::Duration;

// Without timeout (blocks indefinitely)
let result = actor_ref.blocking_tell(MyMessage, None);

// With 5-second timeout
let result = actor_ref.blocking_tell(MyMessage, Some(Duration::from_secs(5)));

// blocking_ask with timeout
let response: String = actor_ref
    .blocking_ask(Query, Some(Duration::from_secs(10)))?;
```

### Use Cases

These methods are useful when:

- Calling actor operations from synchronous code (e.g., FFI boundaries)
- Running in `spawn_blocking` contexts
- Integrating with non-async libraries

```rust
// Example: Using blocking operations in spawn_blocking
let actor = actor_ref.clone();
let result = tokio::task::spawn_blocking(move || {
    actor.blocking_ask(ComputeRequest, Some(Duration::from_secs(30)))
}).await?;
```

---

## Logging System

rsactor uses `tracing` as its logging infrastructure (since v0.12).

### Setup

```toml
[dependencies]
rsactor = "0.12"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

```rust
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Your actor code
}
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| (default) | `tracing` crate included for logging (`warn!`, `error!`, etc.) |
| `tracing` | Enables `#[tracing::instrument]` for detailed spans |
| `metrics` | Enables actor performance metrics |
| `test-utils` | Enables `dead_letter_count()` for testing |
| `deadlock-detection` | Runtime deadlock detection for `ask` cycles (see [Deadlock Detection](deadlock_detection.md)) |

### Log Levels

| Level | What's Logged |
|-------|---------------|
| `ERROR` | Critical failures (panic recovery, lifecycle errors) |
| `WARN` | Dead letters, timeout events |
| `INFO` | Actor lifecycle events (when `tracing` feature enabled) |
| `DEBUG` | Message send/receive details (when `tracing` feature enabled) |

### Environment Variable

Control log output via `RUST_LOG`:

```bash
# Show all rsactor warnings
RUST_LOG=rsactor=warn cargo run

# Show dead letter events only
RUST_LOG=rsactor::dead_letter=warn cargo run

# Full debugging output
RUST_LOG=rsactor=debug cargo run
```

---

## Complete Example

Here's a complete example demonstrating all debugging features:

```rust
use rsactor::{spawn, Actor, ActorRef, Error, message_handlers};
use std::time::Duration;

#[derive(Actor)]
struct WorkerActor;

struct Work { id: u32 }
struct Query;

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_work(&mut self, msg: Work, _ctx: &ActorRef<Self>) {
        println!("Processing work {}", msg.id);
    }

    #[handler]
    async fn handle_query(&mut self, _msg: Query, _ctx: &ActorRef<Self>) -> String {
        "status: ok".to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to observe dead letters
    tracing_subscriber::fmt()
        .with_env_filter("rsactor=warn")
        .init();

    let (actor_ref, handle) = spawn::<WorkerActor>(());

    // Normal operation
    actor_ref.tell(Work { id: 1 }).await?;

    // With timeout
    match actor_ref.ask_with_timeout(Query, Duration::from_secs(5)).await {
        Ok(response) => println!("Response: {}", response),
        Err(e) => {
            eprintln!("Error: {}", e);
            if e.is_retryable() {
                eprintln!("This error is retryable");
            }
            for tip in e.debugging_tips() {
                eprintln!("Tip: {}", tip);
            }
        }
    }

    // Stop the actor
    actor_ref.stop().await?;
    handle.await?;

    // This will generate a dead letter (logged automatically)
    let result = actor_ref.tell(Work { id: 2 }).await;
    assert!(result.is_err());

    Ok(())
}
```

---

## See Also

- [Deadlock Detection](deadlock_detection.md) - Runtime deadlock detection for `ask` cycles
- [Tracing Guide](tracing.md) - Detailed tracing feature documentation
- [Metrics Guide](metrics.md) - Actor performance metrics
- [FAQ](FAQ.md) - Common questions and answers
