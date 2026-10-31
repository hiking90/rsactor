# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`Debug` output and the text returned by `Error::debugging_tips()` are not a
stable interface: they may change in any release, and such changes are not
listed as breaking changes or migration steps. `Display` output, public API,
runtime behavior, dead-letter records and log/`tracing` records are part of the
stable interface, and changes to them are recorded.

## [Unreleased]

Changes since 0.18.0, including new API and diagnostics (see Added) and the
results of a review pass over `src/` and the derive macros. Many review
findings were documentation contracts that disagreed with the implementation —
this crate specifies panic conditions, deadlock-vs-stall semantics and
dead-letter labels in rustdoc, and none of that is checked by
`fmt`/`clippy`/`rustdoc`/tests. Every entry under BREAKING CHANGES and Added
changes code; Changed and Fixed mix code changes with corrections to
documentation and tests, so read each entry. Upgrade steps are in
[Migrating from 0.18.x](#migrating-from-018x).

### ⚠️ BREAKING CHANGES

- **MSRV raised from 1.75 to 1.85.** `tokio` 1.54 and the `rand` 0.10
  dev-dependency (used by the examples) both require Rust 1.85; under the old
  MSRV the lockfile stayed on `tokio` 1.53 and `rand` 0.9. The `tokio`
  requirement itself is unchanged (`1.22`), so downstream crates still choose
  their own `tokio` version. Building with an older compiler fails with
  "cannot be built because it requires rustc 1.85.0 or newer". Such a crate can
  stay on 0.18.x; cargo picks 0.18.x on its own only when MSRV-aware resolution
  is enabled in that crate (cargo 1.84+, with `resolver = "3"` or
  `incompatible-rust-versions = "fallback"`).
- **`FailurePhase` and `ActorFailure` are now `#[non_exhaustive]`.** A match in
  a downstream crate must add a `_` arm. This lets a future release report a new
  lifecycle phase without a breaking change. Note the attribute is applied at the
  *enum* level only: unlike `Error`, these variants stay externally constructible
  so downstream tests can still build an `ActorResult` by hand.
- **`Error::Join`'s `Display` no longer embeds its own source.** It now reads
  `Failed to join spawned task from actor Worker(#3)`; the `JoinError` (and the
  panic message inside it) is reachable only through `source()`. A `Display` that
  repeats its source makes chain-walking reporters print it twice. Code that
  logged `{err}` and relied on seeing the panic text must walk the chain — see
  the updated example on `Error::debugging_tips`.
- **`Operation`, `Channel` and `FailurePhase` `Display` now honor format
  specifiers.** They pad and align as expected (`{:>24}`), but a *precision*
  now truncates (`{:.10}` renders both `BlockingAsk` and `BlockingAskPriority`
  as `blocking_a`). Use `as_str()` where an unabridged label is required.
- **`#[derive(Actor)]` and `#[message_handlers]` now generate absolute
  `::rsactor::…` paths** (previously `rsactor::…`). An absolute path resolves
  only through the extern prelude, so a crate that names rsactor through a `use`
  alias (`use actors as rsactor;`) or reaches it only through a facade crate's
  re-export (`use facade::rsactor;`) now fails with "could not find `rsactor`".
  Depend on `rsactor` directly; a renamed dependency can instead add
  `extern crate <name> as rsactor;` at the crate root.
- **`#[message_handlers]` matches a handler's error through
  `::core::result::Result::Err`** (previously a bare `Err`). A handler that
  returns a user-defined enum with its own `Err` variant — an enum named
  `Result`, or any type marked `#[handler(result)]` — compiled when the call
  site imported that variant (`use Outcome::*;`), and now fails with E0308.
  Return `core::result::Result`, or replace that handler's attribute with
  `#[handler(no_log)]` (it cannot be combined with `result`); see Migrating.
  The bare pattern could not stay: it broke handlers that return the standard
  `Result` in a module that imports another enum's `Err` (see Fixed).
- **`#[message_handlers]` rejects arguments.** `#[message_handlers(no_log)]` and
  similar used to be accepted and silently ignored, so the option never took
  effect. Put handler options on each `#[handler(...)]`.
- **`#[handler]` rejects a by-value `mut self` receiver.** It used to compile
  for a `Copy` actor, where the generated call ran the handler on a copy of the
  actor, so every state change was lost. It now fails with "First parameter
  must be '&mut self'". Take `&mut self`.

### Added

- **`log` feature** — forwards the `log` records tracing produces for its
  events and spans to the [`log`](https://crates.io/crates/log) crate; which
  records, under which targets and levels, follows tracing's
  [Emitting `log` Records](https://docs.rs/tracing/0.1/tracing/#emitting-log-records).
  Applications built on `log` and `env_logger` that install no `tracing`
  subscriber previously saw none of the
  crate's diagnostics: an actor could fail its `on_start`, drop messages to dead
  letters, or return a handler error, and leave no record anywhere. The feature
  enables tracing's own `log` bridge (`tracing/log`, not `log-always`), and
  Cargo unifies that feature across the build, so the bridge applies to every
  crate that uses `tracing` — the application and its other dependencies as
  well as rsActor. A `log`-based application that enables it can therefore
  receive new records from unrelated crates at the levels its logger admits. A
  record is emitted only until a `tracing` dispatcher (global or scoped) is
  first set in the process, so an application that later installs a subscriber
  is never fed both forms of the same event.
- **`Actor::MAILBOX_CAPACITY`** — an associated constant
  (`Option<usize>`, default `None`) that lets an actor type declare its own
  mailbox capacity. The capacity that suits an actor follows from its message
  profile, which is a property of the actor rather than of the call site that
  spawns it; until now the only places to put it were a `spawn` argument and
  the process-wide `set_default_mailbox_capacity`, both away from the actor
  definition. Resolution order, first match wins: `SpawnOptions::mailbox_capacity`
  / `spawn_with_mailbox_capacity`, then `Actor::MAILBOX_CAPACITY`, then
  `set_default_mailbox_capacity`, then `DEFAULT_MAILBOX_CAPACITY`. `None` is
  distinct from `Some(32)`: only the former defers to the process-wide default.
  `Some(0)` panics at spawn, naming the constant. Adding a defaulted associated
  const is not a breaking change for existing `Actor` impls or for
  `#[derive(Actor)]`.
- **`FailurePhase::as_str()`** — the stable label, replacing the previous
  delegation to the derived `Debug` as the single source of truth.
- **An `error!` record naming the actor when its task ends during a panic
  unwind** — in `on_start`, a message handler, `on_idle` or `on_stop`. The
  standard panic hook prints the thread and source location but not the actor,
  and bypasses `tracing`, so an application that drops the `JoinHandle` had no
  record of which actor ended. Panics are still not caught: the `JoinHandle`
  resolves to `Err(JoinError)` as before. The record is emitted under its own
  `catch_unwind`, so a subscriber that panics on it neither skips the
  dead-letter drain nor aborts the process. Task cancellation emits nothing,
  unless the thread dropping the task is itself unwinding — e.g. a
  current-thread runtime (the `#[tokio::test]` default) dropped by a panicking
  test, in which case every live actor task on that runtime, polled or not,
  emits the record. A multi-thread runtime drops its tasks on its worker
  threads, which are not unwinding, so no record is emitted there.
- **`DeadLetterReason::RuntimeUnavailable`** — recorded when a `blocking_*`
  call outside a multi-thread runtime cannot spawn its dedicated thread or
  build the temporary runtime on it. The message was dropped before reaching
  the mailbox and the caller got `Error::Runtime`, but until now no dead letter
  was recorded, so the loss was missing from the `tracing` dead-letter event and
  `dead_letter_count()`. A worker-thread panic is still not recorded, because
  the message may already have been admitted. `DeadLetterReason` is
  `#[non_exhaustive]`, so the new variant does not break existing matches.

### Changed

- `rsactor-derive` now depends on `syn` 3 (was 2). Accepted and rejected
  `#[handler]` receivers are unchanged: `&mut self` and `self: &mut Self` are
  accepted; `&self`, by-value `self`/`mut self` and any other typed receiver
  fail with "First parameter must be '&mut self'". Builds of rsactor already
  compiled `syn` 3 through `futures-macro`, and still compile `syn` 2 through
  `tracing-attributes`.
- **The process-wide default mailbox capacity is now read when the actor is
  spawned, not when `SpawnOptions` is constructed.** Resolution moved into
  `spawn_with_options`, which is the first point where the actor type — and so
  `Actor::MAILBOX_CAPACITY` — is known. One sequence observes the difference:
  building a `SpawnOptions` *before* calling `set_default_mailbox_capacity` and
  spawning with it afterwards used to use the old default and now uses the new
  one. Code that installs the default during startup, before any spawn, is
  unaffected. `SpawnOptions`'s `Debug` output now shows the unresolved
  `mailbox_capacity` — `None` unless set on the options, `Some(64)` after
  `.mailbox_capacity(64)` — instead of a number; the resolved capacity is known
  only at spawn.
- `Cargo.toml` now sets `exclude`, so developer tooling and working notes
  (`plan/`, `plans/`, `book/`, `skills/`, `.github/`, `.claude/`, `.vscode/`)
  no longer ship in the published `.crate`.
- The dead-letter recorder takes a typed `Operation` instead of a `&'static str`,
  so the emitted label and `Error::Timeout`'s operation can no longer drift.
  Internal only — `record` is `pub(crate)`.
- With the `metrics` feature, the message loop records each processed message
  through its own handle to the actor's metrics collector instead of cloning
  the envelope's `Arc` first, removing one atomic increment and decrement per
  message. Recorded values are unchanged.
- `#[handler]` receiver validation now checks the receiver's type, so `self:
  &mut Self` is accepted. The by-value `mut self` rejection is listed under
  BREAKING CHANGES.
- `#[handler]` signature errors now point at the offending token instead of the
  `#[message_handlers]` line: a `&mut ActorRef<_>` third parameter, a
  method-level type or const parameter, and `impl Trait` in the message or
  return type are rejected by the macro. All of these inputs already failed to
  compile, with a less specific error.
- The `tokio` requirement now states its actual minimum, `1.22`
  (`Handle::runtime_flavor`, `Semaphore::MAX_PERMITS`). It read `"1"`, so a
  lockfile pinned to tokio 1.21 or older satisfied it and then failed to
  compile.
- `Error::debugging_tips()` returns different tips for two variants. A
  tell-family `Timeout` (`Tell`, `TellPriority`, `BlockingTell`,
  `BlockingTellPriority`) now says the message was not admitted, instead of
  the ask-oriented tips; an ask-family `Timeout` adds that the message may
  still be processed after the timeout, so retry only idempotent messages.
  `MailboxCapacity` states both bounds (greater than 0, at most
  `Semaphore::MAX_PERMITS`) and points to `Actor::MAILBOX_CAPACITY`.

### Fixed

- **The `tracing` feature's documentation contradicted its implementation.**
  The crate docs and README described it as the switch that turns logging on,
  while `Cargo.toml` and `Message::on_tell_result` stated the opposite: the
  `tracing` *crate* is a required dependency and always logs, and the *feature*
  only adds `#[tracing::instrument]` spans and the per-message timing they
  report. The docs now say so, and the logging and instrumentation sections are
  separate.
- **`ask` / `ask_with_timeout` documented no self-ask hazard at all**, while
  every sibling API did. A self-`ask` deadlocks even on an empty mailbox (the
  runtime loop that would produce the reply is parked awaiting the caller), and
  panics under `deadlock-detection` — neither was documented. `ask_with_timeout`
  is a bounded stall rather than a deadlock, and it can panic on a runtime built
  without a time driver; both are now stated.
- **`blocking_ask` and `blocking_ask_priority` rendered their `# Panics` twice**,
  with the two copies contradicting each other, and one copy claimed
  `blocking_ask(msg, None)` panics on a timerless runtime (it does not — the
  timeout-less path never arms a timer).
- **`blocking_*` panic conditions are now single-sourced** via
  `include_str!`, having drifted across three of the six methods. The shared text
  distinguishes a bounded stall (mandatory `timeout`) from an unbounded hang
  (`timeout: None`), which the unified wording had previously flattened.
- **`Actor::on_stop`'s contract was wrong**: a message handler returning `Err`
  does *not* terminate the actor, so `on_stop` still runs. Task cancellation,
  which does skip `on_stop`, was missing from the list.
- **The dead-letter `operation` contract ignored the shutdown drain**, which
  cannot know the sending API and records only the channel-level label —
  `"tell"`/`"ask"` for the regular mailbox, `"tell_priority"`/`"ask_priority"`
  for the priority channel — including for messages enqueued by
  `blocking_tell` or `blocking_tell_priority`.
- **`register_ask_edge` claimed no cycle can form outside an actor context.** It
  is a detection blind spot, not a proof: `tokio::spawn`, `spawn_blocking` (the
  pattern `blocking_ask` itself recommends) and hand-rolled threads do not
  inherit the task-local, so even a self-`ask` from there hangs silently.
- **`MetricsCollector` reported an active actor as idle** when the wall clock sat
  before `UNIX_EPOCH` + 1 ms: such a clock is stored as `0` milliseconds, and
  `0` doubled as the "no activity yet" sentinel.
  Timestamps are now stored biased by one, keeping the type's all-`Relaxed`
  contract intact.
- The `ActorResult` supervision example discarded the `on_stop` cleanup error and
  restarted the actor, because `is_runtime_failed()` matches the composite
  `OnIdleThenOnStop` phase before `is_stop_failed()` can see it.
- `BoxFuture`'s rationale claimed it keeps `futures` out of the public API;
  `subscribe_idle` and `IdleSubscribeError` already expose `futures` types, so a
  semver-breaking `futures` release is breaking for this crate too.
- `error_display_all_variants` asserted only that messages were non-empty, and
  skipped `PriorityChannelNotEnabled`/`IdleChannelNotEnabled` entirely — two
  near-identical messages where a copy-paste swap was undetectable.
- **The FAQ supervision example (Q17) did not compile against the API** — it
  matched `ActorResult::Failed { error, .. }`, a field that does not exist — and
  labelled every `JoinError` "panicked", which also covers cancellation. It now
  splits `is_panic()` from cancellation and reads the payload with
  `JoinError::into_panic()`; `tests/panic_diagnostics_tests.rs` runs the same
  match.
- **The FAQ described `ActorResult::Failed` by fields it no longer has**
  (`actor`, `error`, `secondary_error`, `phase`) in Q11, Q12, Q13 and Q22. It
  now describes `Failed { failure, killed }` with a table of the
  `ActorFailure` variants, the accessors, and the `on_stop` error case Q13
  omitted. Q11 no longer says the actor is destroyed after `on_stop`; the
  instance is returned in the `ActorResult`.
- **What happens on a panic was documented only in part.** The `Actor` trait
  docs, `on_stop` and FAQ Q13 now state it in one place: `on_stop` is not
  called, the actor struct is dropped, queued `tell`s become
  `DiscardedAtShutdown` dead letters, an in-flight `ask` gets `Error::Receive`,
  and under `panic = "abort"` none of it runs. Those drain and `Drop` behaviours
  had no test; they now do. The debugging guide no longer lists
  "panic recovery", which the runtime does not do.
- **An actor task cancelled before its first poll dropped its queued `tell`s
  without a dead-letter record.** The shutdown drain guard was built inside the
  task future, so `JoinHandle::abort()` or a runtime shutdown right after
  `spawn` — on a `current_thread` runtime, a `tell` returns `Ok` without
  yielding, so the task has not run yet — dropped the receivers with no
  `DiscardedAtShutdown` record. `spawn_with_options` now builds the guard
  before spawning the task.
- **A `#[cfg(...)]` on a `#[handler]` method did not reach its generated
  `impl Message<_>`.** With the cfg off, the method was removed but the impl
  still called it, so a feature-gated handler failed to compile ("no method
  named …") in exactly the builds that disabled the feature. The cfg is now
  copied onto the impl.
- **A `where` clause on a `#[handler]` method did not reach its generated
  `impl Message<_>`.** A bound the impl block lacked (`where T: Clone` on one
  handler of a generic actor) failed with E0277 inside the generated call. The
  predicates are now copied onto the impl, except those naming the method's
  own lifetime parameters, which the impl cannot declare.
- **The derive macros named standard-library items by paths that the call site
  could shadow.** The generated code used `std::convert::Infallible`,
  `std::result::Result`, `std::any::type_name` and bare `Ok(..)` / `Err(..)`,
  so a crate with its own module named `std`, or with an `Ok`/`Err` brought
  into scope by a glob import (`use Status::*;`), failed to compile inside the
  generated `impl Actor` / `impl Message<_>`. These are now absolute `::core::…`
  paths. The reverse case now fails instead: a handler returning a user-defined
  enum whose own `Err` variant was imported at the call site — see
  [Migrating from 0.18.x](#migrating-from-018x).
- **`set_default_mailbox_capacity` accepted capacities above
  `tokio::sync::Semaphore::MAX_PERMITS`** and returned `Ok`, after which every
  spawn that fell back to that default panicked inside `mpsc::channel`. It now
  returns `Error::MailboxCapacity`; `SpawnOptions::mailbox_capacity` and
  `with_idle_capacity` assert the same bound at the call site.

### Migrating from 0.18.x

The items are split by whether the compiler reports the affected code. Items in
the second group compile unchanged and behave differently at run time; check
them by searching for the affected calls.

**Reported by the compiler:**

- **Add a `_` arm when matching `ActorFailure` or `FailurePhase`:**

  ```rust
  match failure {
      ActorFailure::OnStart { error } => { /* ... */ }
      ActorFailure::OnIdle { actor, error } => { /* ... */ }
      ActorFailure::OnStop { actor, error } => { /* ... */ }
      ActorFailure::OnIdleThenOnStop { actor, error, stop_error } => { /* ... */ }
      _ => { /* a phase added by a later release */ }
  }
  ```

- **The derive macros need `rsactor` in the extern prelude.** A crate that
  reached rsactor only through a facade re-export or a `use … as rsactor` alias
  adds `rsactor` to its own `[dependencies]`. A renamed dependency
  (`actors = { package = "rsactor", … }`) can instead declare, at the crate root:

  ```rust
  extern crate actors as rsactor;
  ```

- **Move `#[message_handlers(...)]` arguments onto `#[handler(...)]`.**

- **Replace a by-value `mut self` handler receiver with `&mut self`.** Only a
  `Copy` actor could compile it, and its state changes were made on a copy and
  discarded; with `&mut self` they persist, so check the handler did not depend
  on that.

- **A `#[handler]` returning a user-defined enum with its own `Err` variant
  fails with E0308 on the `#[message_handlers]` line.** This covers an enum
  named `Result` (the macro detects result types by name) and any type marked
  `#[handler(result)]`. 0.18.x generated a bare `Err(..)` pattern in
  `on_tell_result`, which resolved to the enum's variant when the call site
  imported it (`use Outcome::*;` or `use Outcome::Err;`); the pattern is now
  `::core::result::Result::Err`, which only matches the standard `Result`.
  Return `core::result::Result` (a type alias for it works with
  `#[handler(result)]`), or replace that handler's `#[handler]` /
  `#[handler(result)]` with `#[handler(no_log)]`. Keep a single attribute:
  `#[handler(result, no_log)]` is rejected (`result` and `no_log` are mutually
  exclusive), and of several `#[handler]` attributes on one method only the
  first is read, so a `#[handler(no_log)]` added after the existing one is
  ignored. `no_log` also removes the automatic error log; to keep logging the
  enum's error, remove `#[handler]` from that method, implement `Message<_>`
  for its message by hand and override `on_tell_result`.

**Not reported by the compiler:**

- **`Error::Join`'s `{err}` no longer contains the panic message.** Walk the
  `source()` chain, or inspect the `JoinError` directly:

  ```rust
  use std::error::Error as _;

  // Before (0.18.x): the panic text was part of `{err}`
  tracing::error!("actor task failed: {err}");

  // After: append each source
  let mut text = err.to_string();
  let mut cause = err.source();
  while let Some(c) = cause {
      text.push_str(": ");
      text.push_str(&c.to_string());
      cause = c.source();
  }
  tracing::error!("actor task failed: {text}");

  // Or branch on the cause
  if let rsactor::Error::Join { source, .. } = &err {
      if source.is_panic() { /* ... */ }
  }
  ```

- **A precision in `Operation` / `Channel` / `FailurePhase` formatting now
  truncates.** Use `as_str()` for the full label:

  ```rust
  // Before (0.18.x): `{op:.10}` printed "blocking_ask_priority"
  // After: it prints "blocking_a"; this prints the full label
  let label = op.as_str();
  ```

  A width now pads as well: 0.18.x ignored it for all three types, so
  `format!("{:>10}", FailurePhase::OnStart)` returned `"OnStart"` and now
  returns `"   OnStart"`. Check column-aligned log lines and string assertions
  that format these types with a width.

- **The process-wide default capacity is read at spawn.** A `SpawnOptions`
  built before `set_default_mailbox_capacity` now picks up the new default. Pin
  the capacity on the options to keep the previous result:

  ```rust
  let opts = SpawnOptions::new().with_priority();
  rsactor::set_default_mailbox_capacity(1024)?;
  // 0.18.x: this spawn used the default in effect at `SpawnOptions::new()`
  // now:    it uses 1024
  let (actor_ref, handle) = rsactor::spawn_with_options::<MyActor>(args, opts);

  // To keep the old capacity for this spawn
  let opts = SpawnOptions::new().with_priority().mailbox_capacity(32);
  ```

- **`set_default_mailbox_capacity` rejects capacities above
  `tokio::sync::Semaphore::MAX_PERMITS`** with `Error::MailboxCapacity`. 0.18.x
  returned `Ok(())` and panicked later, inside `mpsc::channel`, at the first
  spawn that fell back to that default; a program that set such a value but
  passed a capacity to every spawn ran without error and now gets `Err` from
  the setter. Pass a value within the bound. `SpawnOptions::mailbox_capacity`
  and `with_idle_capacity` panic for the same values at the builder call
  instead of at spawn.

  ```rust
  // 0.18.x: Ok(())
  // now:    Err(Error::MailboxCapacity { .. })
  let result = rsactor::set_default_mailbox_capacity(usize::MAX);
  ```

- **An actor task cancelled before its first poll now records its queued
  `tell`s** — whether by `JoinHandle::abort()` or by dropping or shutting down
  its runtime. 0.18.x recorded nothing on that path; each such `tell` now
  produces a `DiscardedAtShutdown` dead letter (a `tracing` event and a
  `dead_letter_count()` increment). The runtime path is deterministic on a
  current-thread runtime: a `#[tokio::test]` (current-thread by default) whose
  body returns right after `spawn` and a `tell` (which does not yield while the
  mailbox has capacity) never polls the actor task, and the runtime drops it on
  the test thread when the test ends. On a multi-thread runtime a worker thread
  drops the task, and whether it was polled first depends on scheduling. A test that asserts an exact
  `dead_letter_count()` after aborting a freshly spawned actor, or that runs in
  the same binary as a test like the one above (the counter is
  process-global), must expect these records.

- **A `blocking_*` call that cannot start its thread or runtime now records a
  `RuntimeUnavailable` dead letter.** Outside a multi-thread runtime, a failed
  thread spawn or runtime build returned `Error::Runtime` in 0.18.x and
  recorded nothing; it still returns `Error::Runtime`, and now also produces a
  `DeadLetterReason::RuntimeUnavailable` dead letter (a `tracing` event and a
  `dead_letter_count()` increment). A test that asserts an exact
  `dead_letter_count()` around such a failure must expect these records.

- **An actor task ending during a panic unwind now logs an `error!` record.**
  0.18.x logged nothing on that path. One ERROR-level event,
  `Actor <identity> task is ending during a panic unwind; queued messages are discarded`,
  is emitted when an actor's `on_start`, a message handler, `on_idle` or
  `on_stop` panics, and for every live actor task on a current-thread runtime
  (the `#[tokio::test]` default) that a panicking thread drops — so one
  panicking test can produce several. A multi-thread runtime drops its tasks on
  its worker threads and emits none for them; under `panic = "abort"` nothing
  is emitted. A test that captures logs and asserts no ERROR event, or an alert
  on ERROR counts, must expect these records.

- **With `metrics`, `last_activity` is `Some(UNIX_EPOCH)` instead of `None`**
  for an actor whose work completed while the wall clock read before
  `UNIX_EPOCH` + 1 ms. This applies to `ActorRef::last_activity()` and
  `MetricsSnapshot::last_activity`. `None` now means only that no message or
  idle event has completed, so code that branches on `None` takes the other
  branch for such an actor.

## [0.18.0] - 2026-07-11

A focused API-hardening follow-up from a second source-review pass. It
future-proofs the `Error` type against field additions, replaces two
stringly-typed error discriminants with real enums, adds a named accessor for
the `ActorResult` tuple conversion, and lets `ActorRef` / `ActorWeak` implement
`Debug` regardless of the actor type. The compiler flags every site a breaking
change affects; all but one (constructing an `Error` variant outside the crate)
are mechanical to update. Two output changes are not reported by the
compiler, and both are listed under Changed. The `Debug` output of
`Error::Timeout` and `Error::ChannelFull` changed; per the note at the top, it
is not a breaking change. Formatting the `operation` or `channel` field with a
width or precision (`{:<14}`, `{:.5}`) also changed: the 0.17.x
`&'static str` fields honored both, while the new enums' `Display` ignores
them (the following release honors them again). See
[Migrating from 0.17.x to 0.18.0](#migrating-from-017x-to-0180).

### ⚠️ BREAKING CHANGES

- **`Error`'s struct variants are now individually `#[non_exhaustive]`.** A match
  in a downstream crate that binds *every* field of a variant must now add `..`
  (e.g. `Error::ChannelFull { identity, channel, .. }`). This lets a future
  release add a field to an existing variant without a breaking change —
  something the enum-level `#[non_exhaustive]` alone did not permit. Errors are
  library-generated, so external construction was never part of the contract.
- **`Error::Timeout.operation` changed from `&'static str` to the new
  `Operation` enum.** Code that compared it against string literals must match
  the enum (or call `operation.as_str()`).
- **`Error::ChannelFull.channel` changed from `&'static str` to the new
  `Channel` enum.**

### Added

- **`Operation` and `Channel` enums** (both `#[non_exhaustive]`, each with a
  `const fn as_str()`, `Display`, and derived `Debug`, `Clone`, `Copy`,
  `PartialEq`, `Eq` and `Hash`) identify the operation family behind an
  `Error::Timeout` and the bounded channel behind an `Error::ChannelFull`,
  replacing the previous doc-maintained string sets with compiler-checked
  variants.
- **`ActorResult::into_parts()`** — a named equivalent of the
  `From<ActorResult<T>>` tuple conversion, since the anonymous
  `(Option<T>, Option<T::Error>)` tuple does not carry its slot meanings in its
  type. The `From` impl is retained and now delegates to it.

### Changed

- **`ActorRef<T>` and `ActorWeak<T>` now implement `Debug` for every actor
  type**, not only those that are themselves `Debug`. A manual impl replaces
  `#[derive(Debug)]`, dropping the derive's spurious `T: Debug` bound (all
  fields are unconditionally `Debug`). Purely additive.
- `{:?}` of `Error::Timeout` and `Error::ChannelFull` prints the enum variant
  (`operation: Ask`, `channel: IdleSubscribe`) instead of a quoted string
  (`operation: "ask"`); `Display` and `as_str()` keep the snake_case labels.
- `Display` of `Error::Timeout.operation` and `Error::ChannelFull.channel`
  ignores width and precision. The 0.17.x `&'static str` fields padded and
  truncated (`format!("{:<6}|", operation)` gave `"ask   |"`); the enums'
  `Display` writes the label unchanged (`"ask|"`). The following release
  restores padding and truncation.
- Deadlock-detection panic messages now route through a single shared prefix
  constant, so the slow-path re-raise check cannot silently drift from the
  panic text (internal).

### Documentation

- Install snippets bumped to `0.18`. Doc-comment accuracy fixes for
  `subscribe_idle`'s wrapped return type, the `blocking_*` panic conditions
  (timer-disabled multi-thread runtimes), weak-handle `upgrade` semantics after
  the runtime loop exits, the graceful-stop priority-drain quiet window, and the
  process-global dead-letter counter's parallel-test caveat.

### Migrating from 0.17.x to 0.18.0

The compiler flags each site affected by the items below.

- **`Error` variants are `#[non_exhaustive]` — add `..` when binding all
  fields:**

  ```rust
  // Before (0.17.x)
  if let Error::ChannelFull { identity, channel } = &err { /* ... */ }
  // After (0.18.0)
  if let Error::ChannelFull { identity, channel, .. } = &err { /* ... */ }
  ```

  The same attribute rejects a struct literal of any `Error` variant outside
  rsactor (E0639), so code that built one by hand — typically a test double
  returning `Error::Send { .. }` — no longer compiles. There is no public
  constructor; obtain the error from the call that returns it, or have the
  test double return its own error type.

- **`operation` is now an `Operation` enum, not a `&str`:**

  ```rust
  use rsactor::Operation;

  // Before (0.17.x)
  match err {
      Error::Timeout { operation, .. } if operation == "ask" => { /* ... */ }
      _ => {}
  }
  // After (0.18.0) — match the enum, or compare the label
  match err {
      Error::Timeout { operation: Operation::Ask, .. } => { /* ... */ }
      // or: Error::Timeout { operation, .. } if operation.as_str() == "ask" => { /* ... */ }
      _ => {}
  }
  ```

- **Prefer the named accessor over the tuple `From` (the `From` still works):**

  ```rust
  let (actor, error) = result.into_parts(); // was: let (actor, error) = result.into();
  ```

## [0.17.0] - 2026-06-15

A correctness and API-hardening pass over the runtime, driven by a
multi-perspective source review and a dedicated concurrency audit of the
termination paths (each finding empirically reproduced before fixing). The
headline breaking change reworks `ActorResult::Failed` into a phase-specific
`ActorFailure` payload that makes the old doc-only invariants unrepresentable;
the rest tightens the `Error` API, the `blocking_*` execution paths, deadlock
detection, shutdown accounting, and metrics consistency. Most call sites are
mechanical to update — the compiler points at each one. See
[Migrating from 0.16.x to 0.17.0](#migrating-from-016x-to-0170).

### ⚠️ BREAKING CHANGES

- **`ActorResult::Failed` now carries a phase-specific `ActorFailure<T>`
  payload.** The variant changed from
  `Failed { error, phase, secondary_error, .. }` to
  `Failed { failure: ActorFailure<T>, killed }`. `ActorFailure<T>` has one
  variant per phase (`OnStart` / `OnIdle` / `OnStop` / `OnIdleThenOnStop`), each
  carrying exactly the data valid for that phase — only `OnStart` lacks a
  recovered actor, and only `OnIdleThenOnStop` carries the `on_stop` cleanup
  `stop_error`. The invariants that were previously documentation-only
  ("`actor` is `None` only for `OnStart`", "`secondary_error` only for
  `OnIdleThenOnStop`") are now enforced by the type system. The `ActorResult`
  accessors are preserved (`error()` / `into_error()`, `actor()` /
  `into_actor()`, `secondary_error()` / `into_secondary_error()`), and a
  `phase()` accessor was added.
- **`ActorResult::to_result()` renamed to `into_result()`** (per C-CONV, since it
  consumes `self`); its lossiness is now documented.
- **`Actor::Error` now requires `Display`** in addition to `Debug`
  (`Send + Display + Debug`). This guarantees lifecycle failures surfaced through
  `ActorResult` render as human-readable messages. `std::error::Error` is
  intentionally *not* required, so `type Error = String` still works.
- **`ActorRef::subscribe_idle` now returns `Result<(), IdleSubscribeError<T>>`**
  (was `Result<()>`). On rejection the error hands the rejected stream back
  (`take_stream` / `into_parts`), so a retryable `ChannelFull` failure can reuse
  the stream instead of dropping it. `IdleSubscribeError<T>` implements
  `std::error::Error` and `From<IdleSubscribeError<T>> for Error`.
- **`Error` now derives `Clone`.** To make this possible, `Error::Join.source`
  changed from `tokio::task::JoinError` to `Arc<tokio::task::JoinError>`
  (`JoinError` is not `Clone`). Deref still exposes `is_panic()` / `is_cancelled()`.
- **`Error::Runtime` gains a `source` field**, and `blocking_*` failures to build
  or run their temporary Tokio runtime now return `Error::Runtime` (carrying the
  underlying `std::io::Error` via `source()`) instead of the semantically-wrong
  `Error::Send`.
- **Several `Error` fields changed from `String` to `&'static str`** (every
  emitter was already a fixed literal — no allocation on these failure paths):
  `Error::Timeout.operation`, `Error::Downcast.expected_type`,
  `Error::MailboxCapacity.message`, and `Error::Send.details` /
  `Error::Receive.details`.
- **`ActorRef::tell_blocking` / `ask_blocking` removed** (deprecated since
  0.10.0). Use `blocking_tell` / `blocking_ask`. Note the old methods silently
  ignored their `timeout` argument; the replacements honor `Option<Duration>`.
- **`ActorResult::is_stop_failed()` now also returns `true` for
  `OnIdleThenOnStop`**, mirroring `is_runtime_failed()`'s treatment of the
  composite phase.

### Added

- `ActorRef::wait_stopped()` and `ActorControl::wait_stopped()` — resolve once
  the actor has *fully* stopped, complementing `stop()`, whose future only
  guarantees the stop signal was enqueued.
- `ActorFailure<T>` — the phase-specific failure payload (see breaking changes),
  with `phase()`, `error()` / `into_error()`, `actor()` / `into_actor()`, and
  `stop_error()` / `into_stop_error()` accessors. `ActorResult::phase()` returns
  the failure phase (or `None` when completed).
- `ActorWeak::metrics()` — post-mortem metrics access after the actor has stopped.
- `SpawnOptions::with_idle_capacity(n)` — configure the bounded idle-subscribe
  buffer (default `IDLE_SUBSCRIBE_CHANNEL_CAPACITY` = 32).
- `IdleSubscribeError<T>` — returned by `subscribe_idle`, carrying the rejected
  stream back for a retry.
- The `metrics` feature now counts `on_idle` events.

### Changed

- `ActorRef::downgrade` is now an inherent `&self` method, so calls no longer hit
  `E0034` ambiguity when `TellHandler` / `AskHandler` / `ActorControl` are in
  scope.
- `reset_dead_letter_count()` (the `test-utils` counter) now atomically swaps and
  returns the prior count, closing the read-then-store race some tests worked
  around.
- `Debug` for the handler / control trait objects moved from `Box<dyn T>` to
  `dyn T`, so `&dyn` and `Arc<dyn>` are `Debug` for free (and a future coherence
  clash with std's `Box` blanket impl is avoided). `Box<dyn T>: Debug` still
  holds.
- `ActorWeak::is_alive` / `upgrade` now detect a fully stopped actor via channel
  closure (so they report liveness accurately post-shutdown).
- All logging is un-gated from the `tracing` feature, per the documented
  semantics (the feature only controls `#[tracing::instrument]` spans):
  error / warn paths in send / ask / subscribe are now always compiled. The
  per-reply "sent successfully" event on the hot path was removed.
- Deadlock-detection internals moved to a dedicated `deadlock` module (gated by
  the `deadlock-detection` feature); no public API change.
- Metrics accumulate processing time with a plain atomic `fetch_add` instead of a
  `fetch_update` CAS loop (the total wraps only after ~584 years).
- Dead-letter and `Error::Timeout` operation labels are unified with the public
  API actually invoked (e.g. `blocking_tell` no longer mislabels its records as
  `tell`).
- Removed redundant `+ 'static` bounds on `T: Actor` crate-wide.

### Fixed

#### Deadlock detection (`deadlock-detection` feature)

- **`ask_priority` and concurrent asks are now tracked.** Priority asks register
  a wait-for edge (a cycle through the priority channel previously went
  undetected and only resolved by timeout), and the wait-for graph is now a
  per-caller multiset, so two concurrent asks from one handler
  (`tokio::join!(a.ask(..), b.ask(..))`) no longer clobber each other's edges.
- **No more false-positive cycle panics from the reply window.** Wait-for edges
  are removed at reply-send time (via an `AskEdgeToken` the runtime drops right
  after the reply goes out) and ask edges are claimed-and-removed under the graph
  lock, closing the stale-edge window between a callee replying and the caller's
  ask future resuming. The caller-side guard remains as the
  cancellation / timeout backstop, made idempotent against the multiset graph.
- **`blocking_ask` cycles are detected immediately** instead of hanging until
  timeout: the current-actor context now propagates into the blocking slow path's
  dedicated thread / runtime.
- **Self-send deadlock detection.** A no-timeout `tell` / `stop` / `blocking_tell`
  to the actor's *own* full mailbox from inside one of its own handlers is a
  guaranteed, `kill()`-proof hang (the mailbox's only consumer is the loop parked
  awaiting that very handler). These now panic, matching the feature's
  panic-on-detection contract. `tell_with_timeout` is exempt — its bounded wait
  resolves as a recoverable `Error::Timeout`.

#### `blocking_*` execution paths

- `blocking_tell` / `blocking_ask(msg, None)` no longer panic with "Cannot block
  the current thread from within a runtime" when called from inside an async
  context — they route through the dedicated-thread slow path, with a `try_send`
  fast path for the common non-full case.
- Restored Tokio's genuine "cannot block" panic for the unsupported case (calling
  a blocking API on a `current_thread` runtime) instead of silently deadlocking.
- The dedicated worker thread is spawned via `thread::Builder` (named); a spawn
  failure now maps to `Error::Runtime` instead of panicking, and join panic
  payloads are preserved in the error details.

#### Shutdown accounting & observability

- **Discarded messages are now recorded.** Every lifecycle exit drains the
  regular and priority mailboxes and records a `DiscardedAtShutdown` dead letter
  for each accepted-but-unprocessed `tell` envelope. Previously these vanished
  silently — the sender had already seen `Ok` and no record existed anywhere,
  contradicting the dead-letter module's "all failed deliveries are logged"
  contract. `ask` envelopes are skipped (dropping their reply channel already
  produces a caller-side `ReplyDropped` record; counting them here would
  double-count). The drain runs from a drop guard, so handler panics and task
  aborts still record.
- **Graceful-stop priority drain uses `recv()`** (bounded by a quiet period)
  instead of `try_recv`, so `tell_priority` messages from permits acquired just
  before `close()` are no longer silently dropped (a Tokio lost-wakeup edge).
- **Idle-subscribe shutdown window closed.** The subscribe channel now closes the
  moment the actor commits to stopping, so a racing `subscribe_idle` fails fast
  with `Error::Send` instead of returning `Ok` for a stream that would be
  silently discarded.
- **`kill()` during a graceful stop.** First-signal-wins semantics are kept (a
  `kill()` arriving after the actor dequeued a `stop` signal is not observed),
  but the misleading "Kill signal sent successfully" log no longer implies the
  kill took effect.

#### Metrics

- A message whose handler panicked is no longer counted (`message_count` is
  documented as *successfully processed* messages). Processing is now recorded
  explicitly after the handler returns, rather than via an RAII guard that also
  fired when the task was aborted mid-handler — which had counted a cancelled
  message as processed and polluted max / last-activity.
- Uptime is measured from `on_start` completion via `mark_started()`, matching
  the documented contract.
- `snapshot()` and the average accessors clamp each average to its corresponding
  maximum, fixing transient `avg > max` readings caused by reading two
  separately-updated `Relaxed` atomics during a concurrent record.

### Migrating from 0.16.x to 0.17.0

Most of these are mechanical; the compiler flags each affected call site.

- **`ActorResult::Failed` is now an `ActorFailure<T>` payload (the main change).**
  If you used the accessors, nothing changes. If you matched the struct fields
  directly, switch to matching `failure`:

  ```rust
  // Before (0.16.x)
  match result {
      ActorResult::Failed { error, phase, secondary_error, killed } => { /* ... */ }
      ActorResult::Completed { actor, killed } => { /* ... */ }
  }

  // After (0.17.0) — match the phase-specific ActorFailure, or use accessors
  match result {
      ActorResult::Failed { failure, killed } => match failure {
          ActorFailure::OnStart { error } => { /* no actor here */ }
          ActorFailure::OnIdle { actor, error } => { /* ... */ }
          ActorFailure::OnStop { actor, error } => { /* ... */ }
          ActorFailure::OnIdleThenOnStop { actor, error, stop_error } => { /* ... */ }
      },
      ActorResult::Completed { actor, killed } => { /* ... */ }
  }

  // Or stay accessor-based (unchanged):
  if let Some(err) = result.error() { /* primary error */ }
  let secondary = result.secondary_error(); // on_stop cleanup error, if any
  ```

- **`to_result()` → `into_result()`:**

  ```rust
  let actor = result.to_result()?;   // before
  let actor = result.into_result()?; // after
  ```

- **`Actor::Error` must implement `Display`.** Built-in choices (`String`,
  `anyhow::Error`, most `thiserror` enums) already do. For a hand-rolled error
  type, add an `impl std::fmt::Display`.

- **`subscribe_idle` returns `IdleSubscribeError<T>`.** If your `on_start` uses
  `anyhow::Error` (or any error with a blanket `From<std::error::Error>`), `?`
  still works. Otherwise convert explicitly and, if you want to retry, recover
  the stream:

  ```rust
  // Convert to rsactor::Error (or your own) and propagate:
  actor_ref.subscribe_idle(stream).map_err(|e| e.error)?;

  // Or recover the rejected stream on a retryable ChannelFull:
  if let Err(e) = actor_ref.subscribe_idle(stream) {
      if let (Error::ChannelFull { .. }, Some(stream)) = e.into_parts() {
          // retry later with `stream`
      }
  }
  ```

- **`tell_blocking` / `ask_blocking` → `blocking_tell` / `blocking_ask`:**

  ```rust
  actor_ref.tell_blocking(msg, timeout);            // removed
  actor_ref.blocking_tell(msg, Some(timeout));      // replacement (honors the timeout)
  actor_ref.blocking_tell(msg, None);               // no timeout
  ```

- **`Error` matching:** the `&'static str` field changes are source-compatible
  for read-only matches; only code that *assigned a `String`* into those fields
  (uncommon — these are framework-emitted) needs updating. `Error` remains
  `#[non_exhaustive]`, so keep a `_` arm. `Error::Runtime` gained a `source`
  field — exhaustive struct patterns on it need `..`.

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
