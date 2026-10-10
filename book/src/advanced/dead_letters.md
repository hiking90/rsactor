# Dead Letter Tracking

Dead letters are messages that could not be delivered to their intended recipients. rsActor automatically tracks and logs all dead letters via structured tracing.

## When Dead Letters Occur

| Reason | Description | Example |
|--------|-------------|---------|
| `ActorStopped` | Actor's mailbox channel closed | Sending to a stopped actor |
| `Timeout` | Send or ask operation exceeded timeout | `tell_with_timeout` expired |
| `ReplyDropped` | Reply channel dropped before response | Actor crashed during `ask` processing |
| `DiscardedAtShutdown` | A `tell` (or an `ask` whose caller already gave up) was accepted into the mailbox, but the actor terminated before processing it; a still-awaited `ask` is recorded as `ReplyDropped` instead | `tell` still queued when `kill()` runs |
| `RuntimeUnavailable` | A `blocking_*` call could not start its dedicated thread or runtime | Process thread limit reached (`EAGAIN`) |

## Observability

Dead letters are always logged as structured `tracing::warn!` events:

```text
WARN dead_letter: Dead letter: message could not be delivered
  actor.id=42
  actor.type_name="MyActor"
  message.type_name="PingMessage"
  dead_letter.reason="actor stopped"
  dead_letter.operation="tell"
```

To see these logs, initialize a tracing subscriber:

```rust
tracing_subscriber::fmt()
    .with_env_filter("rsactor=warn")
    .init();
```

## Performance

Dead letter recording has minimal overhead:

| Scenario | Overhead |
|----------|----------|
| Successful message delivery (hot path) | **Zero** — no code executes |
| Dead letter, no tracing subscriber | ~5-50 ns |
| Dead letter, subscriber active | ~1-10 us |

The `record` function is marked `#[cold]` to optimize the hot path.

## Testing with `test-utils`

Enable the `test-utils` feature to count dead letters in tests:

```toml
[dev-dependencies]
rsactor = { version = "0.18", features = ["test-utils"] }
```

```rust
use rsactor::{dead_letter_count, reset_dead_letter_count, spawn};

#[tokio::test]
async fn test_dead_letters() {
    // The counter is process-global and `cargo test` runs tests in parallel:
    // serialize this test against every other test that records dead letters
    // (e.g. a shared `tokio::sync::Mutex`, or `serial_test`).
    reset_dead_letter_count();

    let (actor_ref, handle) = spawn::<MyActor>(args);
    actor_ref.stop().await;
    handle.await.unwrap();

    // Sending to a stopped actor creates a dead letter
    let _ = actor_ref.tell(MyMessage).await;
    assert_eq!(dead_letter_count(), 1);
}
```

**Warning**: Never enable `test-utils` in production builds. It exposes internal metrics that could be misused.

## Error Integration

Dead letter tracking works seamlessly with rsActor's error types:

```rust
match actor_ref.tell_with_timeout(msg, timeout).await {
    Ok(_) => { /* delivered successfully */ }
    Err(rsactor::Error::Send { .. }) => {
        // Dead letter was automatically logged
        // Error contains actor identity and details
    }
    Err(e @ rsactor::Error::Timeout { .. }) => {
        // Dead letter recorded with Timeout reason
        // Check if error is retryable
        if e.is_retryable() {
            // Retry logic
        }
    }
    _ => {}
}
```
