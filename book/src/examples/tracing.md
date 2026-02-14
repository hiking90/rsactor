# Tracing Demo

This example demonstrates rsActor's structured tracing and observability features.

## Key Concepts

- **Core logging**: Always available via `tracing::warn!`, `tracing::error!`, etc.
- **Instrumentation spans** (opt-in): `#[tracing::instrument]` attributes for timing and context
- **Structured fields**: Actor IDs, message types, processing duration in every span

## Code Walkthrough

### Setup

```rust
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .with_target(false)
    .init();
```

### What gets traced

The example exercises multiple traced operations:

```rust
// ask — creates actor_ask span with actor_id and message_type fields
let response: String = actor_ref.ask(Ping).await?;

// tell — creates actor_tell span
actor_ref.tell(Increment).await?;

// ask_with_timeout — creates actor_ask_with_timeout span with timeout_ms
actor_ref.ask_with_timeout(SlowOperation(200), Duration::from_millis(150)).await;

// tell_with_timeout — creates actor_tell_with_timeout span
actor_ref.tell_with_timeout(Increment, Duration::from_millis(100)).await?;

// stop — creates actor_stop span
actor_ref.stop().await?;
```

### Sample output

With `RUST_LOG=debug`:

```text
DEBUG actor_lifecycle{actor_id=1 actor_type="TracingDemoActor"}: rsactor: Actor started
DEBUG actor_ask{actor_id=1 message_type="Ping" reply_type="String"}: rsactor: ask
DEBUG actor_process_message: rsactor: Processing message
DEBUG actor_stop{actor_id=1}: rsactor: stop
DEBUG actor_on_stop{killed=false}: rsactor: on_stop
```

## Running

```bash
# With instrumentation spans
RUST_LOG=debug cargo run --example tracing_demo --features tracing

# Without instrumentation (core logging only)
RUST_LOG=debug cargo run --example tracing_demo
```
