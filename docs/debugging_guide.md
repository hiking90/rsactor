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
use rsactor::{Actor, ActorRef, Error, Message};
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
        // A tell `Timeout` means the message was not admitted, so resending
        // cannot duplicate it. Plain `tell` never returns a retryable error.
        match actor.tell_with_timeout(msg.clone(), Duration::from_millis(50)).await {
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
| `Timeout` | Yes | Transient; may succeed with longer timeout. For ask variants the original message may still be processed — retry only idempotent messages |
| `ChannelFull` | Yes | Transient; the bounded buffer drains as the actor makes progress |
| `Send` | No | Actor stopped; channel permanently closed |
| `Receive` | No | Reply channel dropped; cannot recover |
| `Downcast` | No | Type mismatch; programming error |
| `Runtime` | No | A `blocking_*` call could not spawn its thread or build its temporary runtime, or the worker thread panicked |
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
| `tell` accepted into the mailbox, but the actor terminated before processing it (e.g. still queued when `kill()` runs) | `DiscardedAtShutdown` |
| `blocking_*` call cannot spawn its dedicated thread or build its runtime | `RuntimeUnavailable` |

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
rsactor = { version = "0.18", features = ["test-utils"] }
```

```rust
use rsactor::{spawn, dead_letter_count, reset_dead_letter_count};

#[tokio::test]
async fn test_dead_letter_tracking() {
    // The counter is process-global and `cargo test` runs tests in parallel:
    // serialize this test against every other test that records dead letters
    // (e.g. a shared `tokio::sync::Mutex`, or `serial_test`).
    reset_dead_letter_count();
    let initial = dead_letter_count();

    let (actor_ref, handle) = spawn::<MyActor>(MyActor);
    actor_ref.stop().await;
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
rsactor = "0.18"
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
| `log` | Enables tracing's own `log` bridge (tracing's `log` feature, not `log-always`), so tracing's forwarding rules apply unchanged: the `log` records tracing produces for its events and spans reach the `log` crate — see [Emitting `log` Records](https://docs.rs/tracing/0.1/tracing/#emitting-log-records) for the exact set, targets and levels. Cargo unifies the feature across the build, so this covers every crate that emits through `tracing`, not only rsactor. Forwarding runs only until a `tracing` dispatcher (global or scoped) is first set in the process, and then stays off for the rest of the process, even after a scoped dispatcher's guard is dropped. rsactor's own spans exist only with the `tracing` feature |
| `metrics` | Enables actor performance metrics |
| `test-utils` | Enables `dead_letter_count()` for testing |
| `deadlock-detection` | Runtime deadlock detection for `ask` cycles (see [Deadlock Detection](deadlock_detection.md)) |

### Log Levels

| Level | What's Logged |
|-------|---------------|
| `ERROR` | Lifecycle errors; an actor task ending by a panic, or dropped while its thread unwinds another panic (e.g. a runtime dropped by a panicking test) — the runtime does not recover from panics |
| `WARN` | Dead letters, timeout events |
| `INFO` | Actor lifecycle events |
| `DEBUG` | Message send/receive details |

These events are emitted with or without the `tracing` feature. The `tracing`
feature adds spans (`#[instrument]` and one per lifecycle hook and message)
and a `DEBUG` event with each message's processing time.

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

    let (actor_ref, handle) = spawn::<WorkerActor>(WorkerActor);

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
    actor_ref.stop().await;
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
