# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

### Changed

- Replaced `log` crate with `tracing` for all internal logging
- `tracing` feature now only controls `#[tracing::instrument]` attributes

### Removed

- `log` crate dependency

### Migration Guide

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
