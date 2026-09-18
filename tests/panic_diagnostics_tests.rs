// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! What an application can observe when an actor task ends by a panic.
//!
//! The runtime does not catch panics: the `JoinHandle` resolves to
//! `Err(JoinError)`. These tests pin down what happens around that unwind —
//! the `error!` record that names the actor, the dead-letter drain, the
//! actor's own `Drop` — and compile the supervision pattern documented in
//! `docs/FAQ.md` (Q17).
//!
//! Two pieces of process state are shared, so the setup is deliberate:
//!
//! - **tracing events** are captured with `tracing::subscriber::set_default`,
//!   which is thread-local. `#[tokio::test]` uses a current-thread runtime, so
//!   every actor task spawned by a test runs on that test's thread and reports
//!   to that test's subscriber only.
//! - **the dead-letter counter** is process-global. An `ask` to a panicking
//!   handler records a caller-side dead letter too, so *every* test here takes
//!   `serial_lock` for its whole body, not only the counter-reading ones.

use rsactor::{dead_letter_count, spawn, Actor, ActorRef, ActorWeak, Message};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Notify;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

/// Substring of the runtime's panic record; see `LifecycleChannels::drop`.
const PANIC_RECORD: &str = "panic unwind";

fn serial_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Event capture
// ---------------------------------------------------------------------------

type Records = Arc<Mutex<Vec<(Level, String)>>>;

/// Stores `(level, message)` for every event. With `panic_on` set, it panics
/// on an event whose message contains that text — standing in for a
/// misbehaving subscriber.
struct CaptureLayer {
    records: Records,
    panic_on: Option<&'static str>,
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        let message = visitor.0;
        let hit = self.panic_on.is_some_and(|text| message.contains(text));
        self.records
            .lock()
            .unwrap()
            .push((*event.metadata().level(), message));
        if hit {
            panic!("capture layer: deliberate panic on the runtime's panic record");
        }
    }
}

fn capture(panic_on: Option<&'static str>) -> (Records, tracing::subscriber::DefaultGuard) {
    let records = Records::default();
    let subscriber = tracing_subscriber::registry().with(CaptureLayer {
        records: records.clone(),
        panic_on,
    });
    let guard = tracing::subscriber::set_default(subscriber);
    (records, guard)
}

/// The runtime's panic records, as `(level, message)`.
fn panic_records(records: &Records) -> Vec<(Level, String)> {
    records
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, message)| message.contains(PANIC_RECORD))
        .cloned()
        .collect()
}

/// Asserts exactly one ERROR panic record, naming `identity`.
fn assert_one_record_naming(records: &Records, identity: &str) {
    let found = panic_records(records);
    assert_eq!(found.len(), 1, "expected one panic record, got {found:?}");
    let (level, message) = &found[0];
    assert_eq!(*level, Level::ERROR, "panic record level: {message}");
    assert!(
        message.contains(identity),
        "panic record must name the actor {identity}: {message}"
    );
}

// ---------------------------------------------------------------------------
// Actors
// ---------------------------------------------------------------------------

/// Where a `Fragile` actor panics.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PanicAt {
    Handler,
    OnStart,
    OnStop,
}

/// Panics at the configured point. `Wedge` parks the handler until `proceed`
/// is notified and then panics, so messages can be queued behind it. `Drop`
/// sets `dropped`.
#[derive(Debug)]
struct Fragile {
    at: PanicAt,
    dropped: Arc<AtomicBool>,
}

struct FragileArgs {
    at: PanicAt,
    dropped: Arc<AtomicBool>,
}

impl Actor for Fragile {
    type Args = FragileArgs;
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        if args.at == PanicAt::OnStart {
            panic!("fragile: on_start");
        }
        Ok(Fragile {
            at: args.at,
            dropped: args.dropped,
        })
    }

    async fn on_stop(&mut self, _: &ActorWeak<Self>, _: bool) -> Result<(), Self::Error> {
        if self.at == PanicAt::OnStop {
            panic!("fragile: on_stop");
        }
        Ok(())
    }
}

impl Drop for Fragile {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

struct Boom;

impl Message<Boom> for Fragile {
    type Reply = ();

    async fn handle(&mut self, _: Boom, _: &ActorRef<Self>) {
        panic!("fragile: handler");
    }
}

struct Wedge {
    entered: Arc<Notify>,
    proceed: Arc<Notify>,
}

impl Message<Wedge> for Fragile {
    type Reply = ();

    async fn handle(&mut self, msg: Wedge, _: &ActorRef<Self>) {
        msg.entered.notify_one();
        msg.proceed.notified().await;
        panic!("fragile: wedged handler");
    }
}

struct Note;

impl Message<Note> for Fragile {
    type Reply = ();

    async fn handle(&mut self, _: Note, _: &ActorRef<Self>) {}
}

fn spawn_fragile(
    at: PanicAt,
) -> (
    ActorRef<Fragile>,
    tokio::task::JoinHandle<rsactor::ActorResult<Fragile>>,
    Arc<AtomicBool>,
) {
    let dropped = Arc::new(AtomicBool::new(false));
    let (actor_ref, handle) = spawn::<Fragile>(FragileArgs {
        at,
        dropped: dropped.clone(),
    });
    (actor_ref, handle, dropped)
}

// ---------------------------------------------------------------------------
// The panic record
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handler_panic_logs_actor_identity() {
    let _serial = serial_lock().lock().await;
    let (records, _guard) = capture(None);

    let (actor_ref, handle, _) = spawn_fragile(PanicAt::Handler);
    let identity = actor_ref.identity().to_string();
    actor_ref.tell(Boom).await.unwrap();

    let err = handle.await.expect_err("a handler panic must end the task");
    assert!(err.is_panic());
    assert_one_record_naming(&records, &identity);
}

#[tokio::test]
async fn on_start_panic_logs_actor_identity() {
    let _serial = serial_lock().lock().await;
    let (records, _guard) = capture(None);

    let (actor_ref, handle, _) = spawn_fragile(PanicAt::OnStart);
    let identity = actor_ref.identity().to_string();

    let err = handle
        .await
        .expect_err("an on_start panic must end the task");
    assert!(err.is_panic());
    assert_one_record_naming(&records, &identity);
}

#[tokio::test]
async fn on_stop_panic_logs_actor_identity() {
    let _serial = serial_lock().lock().await;
    let (records, _guard) = capture(None);

    let (actor_ref, handle, _) = spawn_fragile(PanicAt::OnStop);
    let identity = actor_ref.identity().to_string();
    actor_ref.stop().await;

    let err = handle
        .await
        .expect_err("an on_stop panic must end the task");
    assert!(err.is_panic());
    assert_one_record_naming(&records, &identity);
}

/// Cancellation drops the task future without unwinding, so it is not
/// reported as a panic.
#[tokio::test]
async fn abort_does_not_log_panic_record() {
    let _serial = serial_lock().lock().await;
    let (records, _guard) = capture(None);

    let (_actor_ref, handle, _) = spawn_fragile(PanicAt::Handler);
    // Let on_start run so the abort hits the main loop, not the spawn.
    tokio::task::yield_now().await;
    handle.abort();

    let err = handle.await.expect_err("an aborted task must not complete");
    assert!(err.is_cancelled());
    assert!(
        panic_records(&records).is_empty(),
        "abort must not produce a panic record: {:?}",
        records.lock().unwrap()
    );
}

// ---------------------------------------------------------------------------
// Around the unwind
// ---------------------------------------------------------------------------

const QUEUED: u64 = 5;

/// Parks the actor in `Wedge`, queues `QUEUED` tells behind it, releases the
/// handler so it panics, and returns how many dead letters that recorded.
async fn queued_tells_discarded_by_panic() -> u64 {
    let (actor_ref, handle, _) = spawn_fragile(PanicAt::Handler);
    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());

    actor_ref
        .tell(Wedge {
            entered: entered.clone(),
            proceed: proceed.clone(),
        })
        .await
        .unwrap();
    entered.notified().await;
    for _ in 0..QUEUED {
        actor_ref.tell(Note).await.unwrap(); // accepted: the sender sees Ok
    }

    let before = dead_letter_count();
    proceed.notify_one();
    let err = handle.await.expect_err("the wedged handler panics");
    assert!(err.is_panic());
    dead_letter_count() - before
}

#[tokio::test]
async fn panic_drains_mailbox_to_dead_letters() {
    let _serial = serial_lock().lock().await;
    let (_records, _guard) = capture(None);

    assert_eq!(
        queued_tells_discarded_by_panic().await,
        QUEUED,
        "every tell queued behind the panicking handler is a DiscardedAtShutdown dead letter"
    );
}

/// A subscriber that panics on the runtime's panic record must not skip the
/// drain, and must not escape `LifecycleChannels::drop`.
///
/// A regression here does not fail this test: a second panic escaping a
/// `Drop` during an unwind aborts the process, taking the whole test binary
/// with it.
#[tokio::test]
async fn panicking_subscriber_does_not_skip_drain() {
    let _serial = serial_lock().lock().await;
    let (records, _guard) = capture(Some(PANIC_RECORD));

    assert_eq!(
        queued_tells_discarded_by_panic().await,
        QUEUED,
        "the drain must still run after the subscriber panicked on the panic record"
    );
    assert_eq!(
        panic_records(&records).len(),
        1,
        "the capture layer must have seen, and panicked on, the panic record"
    );
}

#[tokio::test]
async fn panic_drops_actor_instance_during_unwind() {
    let _serial = serial_lock().lock().await;

    let (actor_ref, handle, dropped) = spawn_fragile(PanicAt::Handler);
    actor_ref.tell(Boom).await.unwrap();

    assert!(handle.await.expect_err("handler panics").is_panic());
    assert!(
        dropped.load(Ordering::SeqCst),
        "the actor struct must be dropped during the unwind; Drop is the cleanup path"
    );
}

#[tokio::test]
async fn ask_to_panicking_handler_gets_receive_error() {
    let _serial = serial_lock().lock().await;

    let (actor_ref, handle, _) = spawn_fragile(PanicAt::Handler);
    let reply = actor_ref.ask(Boom).await;

    assert!(
        matches!(reply, Err(rsactor::Error::Receive { .. })),
        "the ask was delivered and its reply sender dropped: {reply:?}"
    );
    assert!(handle.await.expect_err("handler panics").is_panic());
}

// ---------------------------------------------------------------------------
// The documented supervision pattern (docs/FAQ.md, Q17)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn supervisor_reads_panic_payload() {
    let _serial = serial_lock().lock().await;

    let (actor_ref, handle, _) = spawn_fragile(PanicAt::Handler);
    actor_ref.tell(Boom).await.unwrap();

    let observed = match handle.await {
        Ok(_) => panic!("a handler panic is never reported as ActorResult"),
        Err(join_error) if join_error.is_panic() => {
            let payload = join_error.into_panic();
            let text = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
            text.map(str::to_owned)
        }
        Err(_cancelled) => panic!("the task was not cancelled"),
    };
    assert_eq!(observed.as_deref(), Some("fragile: handler"));
}
