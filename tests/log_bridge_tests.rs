// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Verifies the `log` feature: rsactor's `tracing` events reach a `log::Log`
//! implementation when no `tracing` subscriber is installed.
//!
//! This file is registered as its own test binary (see `[[test]]` in
//! `Cargo.toml`) for one reason: **nothing here may install a `tracing`
//! subscriber**. tracing's `log` bridge forwards an event only while
//! `tracing::dispatcher::has_been_set()` is false, and that dispatcher is
//! process-global. A `tracing_subscriber::fmt().init()` anywhere in the same
//! binary — including in an unrelated test — would silently disable the bridge
//! and turn every assertion below into a failure with a misleading cause.
//!
//! The capturing logger is likewise process-global, and tests within a binary
//! run in parallel, so no test asserts a record *count*. Each test tags its
//! actor type and error text with a unique marker and filters the captured
//! records by it.

use std::sync::{Mutex, OnceLock};

use rsactor::{spawn, Actor, ActorRef};

// ==================== Capturing logger ====================

#[derive(Clone, Debug)]
struct Captured {
    level: log::Level,
    target: String,
    message: String,
}

fn records() -> &'static Mutex<Vec<Captured>> {
    static RECORDS: OnceLock<Mutex<Vec<Captured>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(Vec::new()))
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        // A poisoned mutex here would mean an assertion panicked while
        // holding it; keep logging rather than cascading the panic into
        // unrelated tests.
        if let Ok(mut guard) = records().lock() {
            guard.push(Captured {
                level: record.level(),
                target: record.target().to_string(),
                message: format!("{}", record.args()),
            });
        }
    }

    fn flush(&self) {}
}

/// Installs the capturing logger exactly once per process.
///
/// `set_boxed_logger` fails if a logger is already installed; since this
/// binary is the only installer, a failure means the call raced itself, which
/// `OnceLock` already prevents.
fn init_capture() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        log::set_boxed_logger(Box::new(Capture)).expect("no other logger in this binary");
        log::set_max_level(log::LevelFilter::Trace);
    });
}

/// Every captured record whose message contains `marker`.
fn captured_with(marker: &str) -> Vec<Captured> {
    records()
        .lock()
        .expect("capture mutex")
        .iter()
        .filter(|r| r.message.contains(marker))
        .cloned()
        .collect()
}

/// Polls [`captured_with`] until it is non-empty or the deadline passes.
///
/// The actor runtime logs from its own task, so a record can land after the
/// awaited operation returns. Only the dead-letter case is truly concurrent,
/// but polling keeps both tests free of arbitrary sleeps.
async fn wait_for(marker: &str) -> Vec<Captured> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let found = captured_with(marker);
        if !found.is_empty() {
            return found;
        }
        if std::time::Instant::now() >= deadline {
            let all = records().lock().expect("capture mutex").len();
            panic!("no log record containing {marker:?} within 5s ({all} records captured)");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

// ==================== Actors ====================

/// Fails `on_start`, which the runtime reports with `tracing::error!`.
struct StartFailureActor;

impl Actor for StartFailureActor {
    type Args = ();
    type Error = String;
    type IdleEvent = ();

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Err("marker-on-start-failed".to_string())
    }
}

/// Accepts messages until stopped; used to provoke an `ActorStopped` dead
/// letter, which the runtime reports with `tracing::warn!`.
#[derive(rsactor::Actor)]
struct DeadLetterMarkerActor;

struct Ping;

#[rsactor::message_handlers]
impl DeadLetterMarkerActor {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _actor_ref: &ActorRef<Self>) {}
}

// ==================== Tests ====================

/// An `on_start` failure reaches the `log` logger as an Error record.
#[tokio::test]
async fn on_start_failure_reaches_log_logger() {
    init_capture();

    let (_actor_ref, handle) = spawn::<StartFailureActor>(());
    let result = handle.await.expect("actor task must not panic");
    assert!(result.is_startup_failed(), "on_start was expected to fail");

    let found = wait_for("marker-on-start-failed").await;
    let record = &found[0];

    assert_eq!(
        record.level,
        log::Level::Error,
        "on_start failure must arrive at Error level, got {:?}",
        record.level
    );
    assert!(
        record.target.starts_with("rsactor"),
        "record target should identify rsactor, got {:?}",
        record.target
    );
    assert!(
        record.message.contains("on_start failed"),
        "message should name the failing hook, got {:?}",
        record.message
    );
}

/// A dead letter reaches the `log` logger as a Warn record carrying the
/// structured fields tracing recorded.
#[tokio::test]
async fn dead_letter_reaches_log_logger() {
    init_capture();

    let (actor_ref, handle) = spawn::<DeadLetterMarkerActor>(DeadLetterMarkerActor);
    actor_ref.stop().await;
    handle.await.expect("actor task must not panic");

    // The actor is gone, so this send is dead-lettered.
    let send_result = actor_ref.tell(Ping).await;
    assert!(send_result.is_err(), "tell to a stopped actor must fail");

    // The actor type name is unique to this file, so it isolates this test's
    // records from any other dead letter in the binary.
    let found = wait_for("DeadLetterMarkerActor").await;
    let dead_letters: Vec<_> = found
        .iter()
        .filter(|r| r.message.contains("Dead letter"))
        .collect();

    assert!(
        !dead_letters.is_empty(),
        "expected a dead-letter record, captured: {found:?}"
    );

    let record = dead_letters[0];
    assert_eq!(
        record.level,
        log::Level::Warn,
        "dead letters must arrive at Warn level, got {:?}",
        record.level
    );
    assert!(
        record.target.starts_with("rsactor"),
        "record target should identify rsactor, got {:?}",
        record.target
    );
    assert!(
        record.message.contains("Ping"),
        "the bridged record should carry the message type field, got {:?}",
        record.message
    );
}

/// The bridge must not be defeated by the `tracing` feature's instrumentation.
///
/// Under `--all-features` the `tracing` feature adds `#[instrument]` spans,
/// and the `log` bridge emits an extra Trace record for each span lifecycle
/// event. This test asserts the event records above are still distinguishable
/// from that noise: span records carry the `tracing::span` target, rsactor's
/// own events do not.
#[tokio::test]
async fn span_records_do_not_masquerade_as_events() {
    init_capture();

    let (_actor_ref, handle) = spawn::<StartFailureActor>(());
    let _ = handle.await;

    let found = wait_for("marker-on-start-failed").await;
    assert!(
        found.iter().all(|r| r.target != "tracing::span"),
        "an event record must not be attributed to the span target: {found:?}"
    );
}
