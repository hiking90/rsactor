# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Testing
- `cargo test` - Run all tests (unit and integration)
- `cargo test --all-features` - Run tests with all features enabled
- `cargo test --example <name>` - Test a specific example
- `RUST_LOG=debug cargo run --example <name> --features tracing` - Run example with tracing enabled

### Code Quality
- `cargo fmt` - Format all code
- `cargo fmt --check` - Check formatting without modifying files (CI validation)
- `cargo clippy --all-targets --all-features -- -D warnings` - Lint with clippy (CI configuration)
- `cargo check --all-features` - Type check without building

**Before committing:** Always run `cargo fmt` and `cargo clippy --all-targets --all-features -- -D warnings` to ensure CI passes.

### Building and Examples
- `cargo build --all-features` - Build with all features
- `cargo run --example <name>` - Run a specific example (see examples/ directory)
- `cargo doc --no-deps --all-features` - Build documentation

### Security and Dependencies
- `cargo audit` - Run security audit (requires cargo-audit installation)

## Repository Architecture

### Core Actor Framework (src/)
- `lib.rs` - Main library entry point, core actor system, spawn functions
- `actor.rs` - Actor trait definition and lifecycle management
- `actor_ref.rs` - ActorRef and ActorWeak for actor communication
- `actor_result.rs` - ActorResult enum for lifecycle outcomes
- `error.rs` - Error types used throughout the framework

### Derive Macros (rsactor-derive/)
- Procedural macro crate providing `#[derive(Actor)]` and `#[message_handlers]` attributes
- Separate crate to isolate proc-macro dependencies

### Key Design Patterns

#### Message Handling
Two approaches supported:
1. **Recommended**: `#[message_handlers]` macro with `#[handler]` method attributes
2. **Manual**: Implement `Message<T>` trait directly (for advanced use cases)

#### Actor Creation
Two patterns:
1. **Simple**: `#[derive(Actor)]` for basic actors without complex initialization
2. **Manual**: Implement `Actor` trait with custom `on_start` logic

#### Type Safety System
- `ActorRef<T>` provides compile-time type safety (primary usage)
- `ActorControl` trait provides type-erased lifecycle management for heterogeneous collections
- Handler traits (`TellHandler`, `AskHandler`) enable type-erased message sending
- Generic actors fully supported with proper trait bounds

### Testing Strategy
- Integration tests in `tests/` directory cover various scenarios
- Examples serve as both documentation and integration tests
- CI runs all examples automatically to verify functionality
- Both unit testing and actor lifecycle testing patterns supported

### Feature Flags
- `tracing` - Enables `#[tracing::instrument]` spans (logging via `tracing` crate is always available)
- `metrics` - Actor performance metrics (message count, processing time, etc.)
- `test-utils` - Testing utilities including dead letter counter
- `deadlock-detection` - Runtime deadlock detection for `ask` cycles (panics on detection)
- All examples conditionally support tracing when feature is enabled

### Actor Lifecycle
1. **Creation**: `spawn()` calls `Actor::on_start()`
2. **Idle**: `on_idle()` reacts to events from `Stream`s registered via `ActorRef::subscribe_idle`, running concurrently with message processing. Opt-in: enable the idle channel with `SpawnOptions::with_idle()`. (Replaced the removed `on_run()` in 0.16.0.)
3. **Termination**: `on_stop()` called during graceful/immediate shutdown
4. **Result**: `ActorResult` enum indicates completion or failure with detailed phase information

### Cross-Platform Support
- Workspace supports multiple platforms (Linux, Windows, macOS)
- Android cross-compilation tested in CI
- MSRV: Rust 1.75.0+

### Documentation
- Extensive inline documentation with examples
- FAQ.md covers common usage patterns and advanced scenarios
- Examples demonstrate various actor patterns and use cases