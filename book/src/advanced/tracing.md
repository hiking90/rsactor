# Tracing & Observability

rsActor provides comprehensive observability through the `tracing` crate.

## Architecture

Tracing in rsActor has two layers:

1. **Core logging** (always available): `tracing::warn!`, `tracing::error!`, `tracing::debug!` for lifecycle events, dead letters, and errors. The `tracing` crate is a required dependency.

2. **Instrumentation spans** (opt-in via `tracing` feature): `#[tracing::instrument]` attributes that create structured spans with timing and context for actor lifecycle and message processing.

## Enabling Instrumentation

```toml
[dependencies]
rsactor = { version = "0.13", features = ["tracing"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

## What Gets Traced

### Always Available (Core Logging)

- Dead letter warnings with structured fields
- Actor `on_start` success/failure
- Actor `on_stop` errors
- `on_run` errors
- Reply channel failures

### With `tracing` Feature (Instrumentation Spans)

| Span | Fields | Description |
|------|--------|-------------|
| `actor_lifecycle` | `actor_id`, `actor_type` | Entire actor lifecycle |
| `actor_on_start` | — | Initialization phase |
| `actor_on_run` | — | Each idle handler invocation |
| `actor_on_stop` | `killed` | Shutdown phase |
| `actor_process_message` | — | Each message processing |
| `actor_tell` | `actor_id`, `message_type` | Tell operation |
| `actor_ask` | `actor_id`, `message_type`, `reply_type` | Ask operation |
| `actor_tell_with_timeout` | `actor_id`, `message_type`, `timeout_ms` | Tell with timeout |
| `actor_ask_with_timeout` | `actor_id`, `message_type`, `reply_type`, `timeout_ms` | Ask with timeout |
| `actor_kill` | `actor_id` | Kill signal |
| `actor_stop` | `actor_id` | Stop signal |

## Setup Example

```rust
use tracing_subscriber;

#[tokio::main]
async fn main() {
    // Simple setup
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Your actor code here...
}
```

## Running with Tracing

```bash
# Basic logging (always works)
RUST_LOG=debug cargo run --example basic

# With instrumentation spans
RUST_LOG=debug cargo run --example tracing_demo --features tracing
```

## Feature Flag Behavior

| Configuration | Core Logging | Instrumentation Spans |
|--------------|-------------|----------------------|
| Default | Available | Disabled |
| `features = ["tracing"]` | Available | Enabled |

The `tracing` feature only controls `#[tracing::instrument]` attributes. Core logging via `tracing::warn!`, `tracing::error!`, etc. is always available regardless of feature flags.

## Running the Example

```bash
RUST_LOG=debug cargo run --example tracing_demo --features tracing
```
