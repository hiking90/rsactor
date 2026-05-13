# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Faster `blocking_*` APIs from `async fn` contexts.** When called on a
  multi-thread Tokio runtime (e.g. from an `async fn → sync fn → blocking_*`
  bridge or from `spawn_blocking`), `blocking_tell`, `blocking_ask`,
  `blocking_tell_priority`, and `blocking_ask_priority` now reuse the
  caller's runtime via `tokio::task::block_in_place` +
  `Handle::block_on` instead of spawning a fresh thread and runtime. Cost
  drops from ~tens of μs to sub-μs on the most common call shape.
- **`try_send` fast path for `tell` variants on the fallback.** When no
  Tokio runtime is active (or it is `current_thread`) and the mailbox /
  priority slot has room, `blocking_tell` with a timeout and
  `blocking_tell_priority` now complete without spawning a thread or
  building a runtime. `ask` variants still take the slow path because a
  sync `recv_timeout` for the reply channel is unavailable.
- Mailbox-closed dead-letter records emitted by `blocking_tell` with a
  timeout now use the operation label `"blocking_tell"` (previously
  `"tell"` when the slow path delegated through the async `tell`). The
  timeout case and the no-timeout case were already labeled
  `"blocking_tell"`; this aligns the remaining edge case.

## [0.15.0] - 2026-05-03

### Added

- **Priority channel** (opt-in, off by default): a second mpsc channel of fixed
  capacity 1 that is polled with higher priority than the regular mailbox
  but lower priority than the `kill()` (terminate) signal. Designed for
  short, infrequent control-plane messages such as health checks and
  pause/resume signals.
  - `SpawnOptions` builder + `spawn_with_options()` entry point.
    `SpawnOptions::new().with_priority()` enables the priority channel.
  - `ActorRef::tell_priority(msg, timeout)` /
    `ActorRef::ask_priority(msg, timeout)` and their
    `blocking_tell_priority` / `blocking_ask_priority` counterparts.
    `Duration` is mandatory — the priority slot has capacity 1, so a wedged
    actor would otherwise block the sender indefinitely.
  - `ActorRef::has_priority_channel()` to detect whether the channel was
    enabled at spawn time.
  - `Error::PriorityChannelNotEnabled` returned when priority APIs are
    called on an actor spawned without `with_priority()`. This is a
    configuration error and is **not** recorded as a dead letter.
  - `stop()` drains the priority queue before invoking `on_stop` (close-then-
    drain), so a priority message sent immediately before `stop()` is not
    lost. `kill()` does not drain.
  - `metrics` feature now tracks priority messages separately:
    `priority_message_count`, `avg_priority_processing_time`,
    `max_priority_processing_time` are exposed both on `MetricsSnapshot`
    and as `ActorRef` accessors. The regular `message_count` no longer
    includes priority messages.
  - **Note on starvation:** the priority branch wins biased select against
    the regular mailbox, so sustained priority traffic can starve regular
    handlers. Reserve the priority channel for short, infrequent signals —
    the `metrics` feature exposes both counters so abuse is detectable.
  - New example: `examples/priority_signal.rs` (health check + pause/resume).

## [0.12.0] - 2025-01-18

### ⚠️ BREAKING CHANGES

- **Logging Unification**: `tracing` is now a required dependency
  - Previously: `log` was required, `tracing` was optional via feature flag
  - Now: `tracing` is always included for core logging
  - The `tracing` feature flag now controls **instrumentation only** (spans, `#[instrument]`), not the logging system itself

### Added

- `Error::is_retryable()` - Check if an error can be retried (only `Timeout` errors are retryable)
- `Error::debugging_tips()` - Get actionable debugging tips for all error types
- `DeadLetterReason` enum - Categorize why messages couldn't be delivered:
  - `ActorStopped` - Actor's mailbox channel was closed
  - `Timeout` - Send or ask operation exceeded its timeout
  - `ReplyDropped` - Reply channel was dropped before response
- Dead letter tracking with structured `tracing::warn!` logging for all failed message deliveries
- `test-utils` feature with `dead_letter_count()` and `reset_dead_letter_count()` for testing
- `metrics` feature for actor performance monitoring:
  - `MetricsSnapshot` - Comprehensive metrics data structure
  - Per-actor metrics: message count, processing times, error count, uptime
  - Accessible via `ActorRef::metrics()` and convenience methods

### Changed

- Replaced `log` crate with `tracing` for all internal logging
- `tracing` feature now only controls `#[tracing::instrument]` attributes
- **Documentation updates**:
  - Updated all version references from 0.9/0.11 to 0.12
  - Fixed deprecated `ask_blocking`/`tell_blocking` references to use `blocking_ask`/`blocking_tell`
  - Corrected blocking API signatures with `Option<Duration>` timeout parameter

### Removed

- `log` crate dependency
- Premature v0.12 migration guide from debugging_guide.md (now current version)

### Deprecated

- `ask_blocking` and `tell_blocking` methods (since v0.10.0) - Use `blocking_ask` and `blocking_tell` instead

### Migration Guide

#### Blocking API Changes

```rust
// Old (deprecated)
actor_ref.ask_blocking(msg, timeout);
actor_ref.tell_blocking(msg, timeout);

// New (recommended)
actor_ref.blocking_ask(msg, None);              // No timeout
actor_ref.blocking_ask(msg, Some(timeout));     // With timeout
actor_ref.blocking_tell(msg, None);             // No timeout
actor_ref.blocking_tell(msg, Some(timeout));    // With timeout
```

#### If you were using default features (no `tracing`)

Add `tracing-subscriber` to see logs:

```rust
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    // Your actor code
}
```

#### If you were using `env_logger`

Option A: Use `tracing-log` bridge:
```toml
[dependencies]
tracing-log = "0.2"
```

```rust
fn main() {
    tracing_log::LogTracer::init().expect("Failed to set logger");
    env_logger::init();
    // Your actor code
}
```

Option B: Switch to `tracing-subscriber` (recommended):
```toml
[dependencies]
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## [0.9.0] - 2025-06-28

### Added
- `#[message_handlers]` attribute macro with `#[handler]` method attributes for simplified message handling
- **Tracing Support**: Optional `tracing` feature for comprehensive actor observability
  - Full lifecycle tracing for actor events (start, stop, termination scenarios)
  - Message sending and handling with detailed timing information
  - Reply processing and error handling tracing
  - Performance metrics including message processing duration
  - Clear distinction between different termination scenarios (kill, graceful stop, reference drop)
- New examples: `tracing_demo.rs`, `weak_reference_demo.rs`, `kill_demo.rs` demonstrating tracing capabilities
- Comprehensive migration guide for moving from `impl_message_handler!` to the new macro approach
- Better documentation showcasing the recommended patterns

### Deprecated
- `impl_message_handler!` macro - Use `#[message_handlers]` with `#[handler]` attributes instead
- `__impl_message_handler_body!` internal helper macro
- Manual `Message<T>` trait implementations when using the new macro approach

### Changed
- Documentation now prioritizes the `#[message_handlers]` approach as the recommended method
- Updated examples to demonstrate the new macro patterns
- Added migration timeline: deprecated macros will be removed in version 1.0

### Migration Guide
To migrate from the deprecated approach:

**Before:**
```rust
impl Message<MyMessage> for MyActor {
    type Reply = String;
    async fn handle(&mut self, msg: MyMessage, actor_ref: &ActorRef<Self>) -> Self::Reply {
        "response".to_string()
    }
}

impl_message_handler!(MyActor, [MyMessage]);
```

**After:**
```rust
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_my_message(&mut self, msg: MyMessage, actor_ref: &ActorRef<Self>) -> String {
        "response".to_string()
    }
}
```

### Breaking Changes
None in this release. The deprecated macros continue to work but will emit deprecation warnings.

### Note
The deprecated `impl_message_handler!` macro will be removed in version 1.0. Please migrate to the new `#[message_handlers]` approach.
