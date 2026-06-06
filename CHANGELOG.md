# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### ⚠️ BREAKING CHANGES

- **`ActorRef::tell_blocking` / `ask_blocking` removed** (deprecated since 0.10.0).
  Use `blocking_tell` / `blocking_ask`. Note the old methods silently ignored
  their `timeout` argument; the replacements honor `Option<Duration>`.
- **`Actor::Error` now requires `Display`** in addition to `Debug`
  (`Send + Display + Debug`). This guarantees lifecycle failures surfaced through
  `ActorResult` render as human-readable messages. `std::error::Error` is
  intentionally *not* required, so `type Error = String` still works.
- **`Error` now derives `Clone`.** To make this possible, `Error::Join.source`
  changed from `tokio::task::JoinError` to `Arc<tokio::task::JoinError>`
  (`JoinError` is not `Clone`). Deref still exposes `is_panic()` / `is_cancelled()`.
- **Several `Error` fields changed from `String` to `&'static str`**:
  `Error::Timeout.operation`, `Error::Downcast.expected_type`, and
  `Error::MailboxCapacity.message` (all are fixed labels — no allocation).

### Fixed

- **Deadlock detection now covers `ask_priority` and concurrent asks.** Priority
  asks register a wait-for edge (a cycle through the priority channel previously
  went undetected and only resolved by timeout). The wait-for graph is now a
  multiset keyed per caller, so two concurrent asks from one handler
  (`tokio::join!(a.ask(..), b.ask(..))`) no longer clobber each other's edges.

### Changed

- Deadlock-detection internals moved to a dedicated `deadlock` module (gated by
  the `deadlock-detection` feature); no public API change.
- Metrics accumulate processing time with a plain atomic `fetch_add` instead of a
  `fetch_update` CAS loop (the total wraps only after ~584 years).

## [0.16.0] - 2026-06-06

This release reworks the actor idle / periodic-work model, makes the lifecycle
control APIs (`stop` / `kill`) infallible, and hardens error reporting. Most
call sites are mechanical to update — the compiler points at each one. See
[Migrating from 0.15.x to 0.16.0](#migrating-from-015x-to-0160).

### ⚠️ BREAKING CHANGES

- **`Actor::on_run` is removed and replaced by a stream-based `Actor::on_idle`.**
  The old `on_run` future was rebuilt inside the runtime's `select!` on every
  iteration, so any timer or await state created inside it (e.g.
  `tokio::time::sleep`) was dropped whenever a competing branch (message
  arrival, priority signal) won — a 1-second sleep racing sub-second message
  traffic would never fire. The new model is a subscription: register `Stream`s
  of events via `ActorRef::subscribe_idle` and react to each yielded event in
  `on_idle(&mut self, event, actor_weak)`. Stream state (interval schedules,
  channel buffers) lives in the runtime and survives `select!` cancellation, so
  periodic work fires reliably even under message pressure, and subscriptions
  can be added dynamically from message handlers — not only `on_start`.
  - New **required** associated type `Actor::IdleEvent: Send + 'static`.
    `#[derive(Actor)]` auto-fills it as `()`; manual `Actor` impls must add it.
  - `FailurePhase::OnRun` → `OnIdle`, `FailurePhase::OnRunThenOnStop` →
    `OnIdleThenOnStop`.

- **The idle-event channel is now opt-in.** `on_idle` / `subscribe_idle`
  require the actor to be spawned with
  `spawn_with_options(args, SpawnOptions::new().with_idle())`. The channel is
  off by default so that actors which never use idle events no longer pay for
  an always-active branch in the runtime's `select!` loop on every message.

  Same-binary A/B (idle on vs off, all else equal): `tell` throughput +29%
  (4.57 → 5.90 M msg/s), `tell` latency −27% (238 ns → 174 ns). `ask` is
  unaffected (dominated by two context switches).

- **`ActorRef::stop()` and `ActorRef::kill()` no longer return `Result<()>`.**
  Both are idempotent, so the "always `Ok`" return type was misleading.
  `stop()` now returns `()` (still `async`) and `kill()` returns `()`. The same
  applies to `ActorControl::stop` (now `BoxFuture<'_, ()>`) and
  `ActorControl::kill` (now `()`). Drop any `?`, `.unwrap()`, `.expect(...)`,
  or `let _ =` at call sites.

- **`Error` is now `#[non_exhaustive]`** and gains an
  `Error::ChannelFull { identity, channel }` variant. `subscribe_idle` emits it
  on bounded-buffer saturation (previously an `Error::Send` distinguishable only
  by substring). `Error::is_retryable()` now returns `true` for `ChannelFull` in
  addition to `Timeout`. Exhaustive `match` on `Error` must add a wildcard arm.

- **`ActorResult::Failed` gains a `secondary_error: Option<T::Error>` field.**
  It is populated for `FailurePhase::OnIdleThenOnStop` with the `on_stop`
  cleanup error, closing a gap where a dual failure (an `on_idle` error followed
  by an `on_stop` error) could not be recovered programmatically. New accessors
  `ActorResult::secondary_error()` and `into_secondary_error()`. Exhaustive
  `ActorResult::Failed { .. }` patterns must add the new field or `..`.

- **`error_count` is removed** from `MetricsCollector` (`error_count`,
  `record_error`), `MetricsSnapshot` (`error_count`), and `ActorRef`
  (`error_count()`) — no writer for it ever existed in the framework code.

### Added

- `Actor::on_idle(&mut self, event, actor_weak)` and the required associated
  type `Actor::IdleEvent` — react to events yielded by subscribed idle streams.
- `ActorRef::subscribe_idle(stream)` — register a `Stream` of `IdleEvent`s for
  the actor's idle loop. Synchronous and `try_send`-based so subscribing from
  `on_start` cannot deadlock the runtime; can also be called from message
  handlers to attach sources dynamically.
- `SpawnOptions::with_idle()` — enables the idle-event channel for the spawned
  actor (mirrors `with_priority()`).
- `ActorRef::has_idle_channel()` — reports whether the idle channel is enabled.
- `Error::IdleChannelNotEnabled` — returned by `subscribe_idle` when the actor
  was spawned without `with_idle()`. A configuration error; **not** recorded as
  a dead letter.
- `Error::ChannelFull { identity, channel }` — bounded-buffer saturation
  (currently emitted by `subscribe_idle`). Retryable.
- `ActorResult::secondary_error()` / `ActorResult::into_secondary_error()` —
  recover the `on_stop` cleanup error after an `OnIdleThenOnStop` failure.
- `Identity` now derives `Hash`, so it can be used directly as a `HashMap` key.

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
- `ActorWeak::upgrade` / `ActorWeak::is_alive` now treat the idle-subscribe and
  priority channels as **secondary**: only the mailbox and terminate channels
  are required for an upgrade to succeed (previously idle-subscribe was also
  required).

### Removed

- `Actor::on_run` — replaced by `Actor::on_idle` (see breaking changes).
- `MetricsCollector::error_count` / `MetricsCollector::record_error`,
  `MetricsSnapshot::error_count`, and `ActorRef::error_count()`.

### Fixed

- A dropped `ask` reply (the caller timed out, was cancelled, or a hedged
  request lost the race) was logged unconditionally at `error!`, even though it
  is a normal caller-driven outcome already recorded as a dead letter on the
  caller side. Downgraded to `debug!` so routine timeouts and cancellations no
  longer flood error-level monitoring.
- Handler errors were routed to `eprintln!` or `tracing::error!` depending on
  the `tracing` *feature*, which only gates instrumentation spans and is
  unrelated to whether a subscriber exists — so errors could hit stderr even
  with a subscriber configured, or be dropped silently without one. They now
  always route through `tracing::error!`, consistent with dead-letter logging.

### Migrating from 0.15.x to 0.16.0

Most of these are mechanical; the compiler flags each affected call site.

- **`on_run` → `on_idle` (the main change).** Move periodic logic out of
  `on_run` into a `Stream` subscribed from `on_start`, and react to its events
  in `on_idle`:

  ```rust
  // Before (0.15.x): periodic work driven by on_run
  impl Actor for MyActor {
      type Args = ();
      type Error = anyhow::Error;

      async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
          Ok(MyActor { interval: tokio::time::interval(Duration::from_secs(1)) })
      }

      // Returned `bool` controlled whether on_run was called again.
      async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<bool, Self::Error> {
          self.interval.tick().await;
          self.do_periodic_work();
          Ok(true)
      }
  }

  // After (0.16.0): subscribe a stream, react in on_idle
  use tokio_stream::{wrappers::IntervalStream, StreamExt};

  struct Tick; // your IdleEvent type

  impl Actor for MyActor {
      type Args = ();
      type Error = anyhow::Error;
      type IdleEvent = Tick; // NEW: required associated type

      async fn on_start(_: (), actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
          actor_ref.subscribe_idle(
              IntervalStream::new(tokio::time::interval(Duration::from_secs(1)))
                  .map(|_| Tick),
          )?;
          Ok(MyActor { /* ... */ })
      }

      async fn on_idle(&mut self, _: Tick, _: &ActorWeak<Self>) -> Result<(), Self::Error> {
          self.do_periodic_work();
          Ok(())
      }
  }
  ```

  Actors that don't do periodic work need no `on_idle`; just set
  `type IdleEvent = ();` (or use `#[derive(Actor)]`, which fills it in).

- **Enable the idle channel at spawn time** if you use `on_idle` /
  `subscribe_idle`:

  ```rust
  // Before (0.15.x): idle was always available
  let (actor_ref, join) = spawn::<MyActor>(args);

  // After (0.16.0): opt in explicitly
  use rsactor::{spawn_with_options, SpawnOptions};
  let (actor_ref, join) =
      spawn_with_options::<MyActor>(args, SpawnOptions::new().with_idle());
  ```

  `spawn_with_mailbox_capacity(args, n)` becomes
  `spawn_with_options(args, SpawnOptions::new().mailbox_capacity(n).with_idle())`.

  Without `with_idle()`, `subscribe_idle` returns `Error::IdleChannelNotEnabled`
  (a configuration error, not a dead letter) and `on_idle` is never driven.
  Propagating that error with `?` in `on_start` makes the actor fail to start
  with a clear message instead of silently no-op'ing; guard proactively with
  `ActorRef::has_idle_channel()` if needed.

- **`stop()` / `kill()` are now infallible.** Remove `?` / `.unwrap()` /
  `.expect(...)` / `let _ =`:

  ```rust
  // Before
  actor_ref.stop().await?;
  actor_ref.kill()?;

  // After
  actor_ref.stop().await;
  actor_ref.kill();
  ```

- **`Error` is `#[non_exhaustive]`.** Add a wildcard arm to any exhaustive
  `match` on `Error`, and note `is_retryable()` now also covers `ChannelFull`.

- **`ActorResult::Failed` has a new field.** Exhaustive patterns must account
  for `secondary_error`:

  ```rust
  // Before
  ActorResult::Failed { error, phase } => { /* ... */ }

  // After — add `..`, or bind the new field
  ActorResult::Failed { error, phase, .. } => { /* ... */ }
  ```

- **`FailurePhase` variants renamed:** `OnRun` → `OnIdle`,
  `OnRunThenOnStop` → `OnIdleThenOnStop`.

- **`error_count` removed.** It never had a writer, so it always read `0`;
  remove any reads of `MetricsSnapshot::error_count` /
  `ActorRef::error_count()`.

- **`ActorWeak` upgrade semantics:** the idle-subscribe and priority channels
  are now *secondary* — `ActorWeak::upgrade` / `is_alive` succeed based on the
  mailbox + terminate channels alone. This only matters if you relied on a
  dropped idle-subscribe sender causing `upgrade()` to fail (it no longer does;
  in practice all `ActorRef` clones carried one of each, so counts moved in
  lockstep anyway).

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
