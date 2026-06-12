// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Receiver-side accounting at shutdown.
//!
//! Verifies that messages accepted into a mailbox but never processed —
//! queued behind a graceful-stop signal, or pending at `kill()` — are
//! recorded as `DiscardedAtShutdown` dead letters, that `ask` envelopes are
//! not double-counted (their callers already record `ReplyDropped`), and that
//! `subscribe_idle` fails fast once the actor has begun stopping.
//!
//! Reads the process-global dead-letter counter, so the counter-reading tests
//! serialize on a local mutex and fully join their actors before releasing it.

use rsactor::{
    dead_letter_count, message_handlers, spawn_with_mailbox_capacity, spawn_with_options, Actor,
    ActorRef, SpawnOptions,
};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

fn serial_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug)]
struct WedgeActor {
    entered: Arc<Notify>,
    proceed: Arc<Notify>,
}

#[derive(Debug)]
struct Block;
#[derive(Debug)]
struct Note;
#[derive(Debug)]
struct Query;

impl Actor for WedgeActor {
    type Args = (Arc<Notify>, Arc<Notify>);
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(
        (entered, proceed): Self::Args,
        _: &ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        Ok(WedgeActor { entered, proceed })
    }
}

#[message_handlers]
impl WedgeActor {
    #[handler]
    async fn handle_block(&mut self, _: Block, _: &ActorRef<Self>) {
        self.entered.notify_one();
        self.proceed.notified().await;
    }

    #[handler]
    async fn handle_note(&mut self, _: Note, _: &ActorRef<Self>) {}

    #[handler]
    async fn handle_query(&mut self, _: Query, _: &ActorRef<Self>) -> u32 {
        1
    }
}

fn spawn_wedged() -> (
    Arc<Notify>,
    Arc<Notify>,
    ActorRef<WedgeActor>,
    tokio::task::JoinHandle<rsactor::ActorResult<WedgeActor>>,
) {
    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let (actor_ref, handle) =
        spawn_with_mailbox_capacity::<WedgeActor>((entered.clone(), proceed.clone()), 16);
    (entered, proceed, actor_ref, handle)
}

#[tokio::test]
async fn tells_discarded_after_stop_are_recorded_and_asks_not_double_counted() {
    let _serial = serial_lock().lock().await;

    let (entered, proceed, actor_ref, handle) = spawn_wedged();

    // Wedge the actor, queue the stop marker, then queue messages behind it.
    actor_ref.tell(Block).await.unwrap();
    entered.notified().await;
    actor_ref.stop().await;
    for _ in 0..5 {
        actor_ref.tell(Note).await.unwrap(); // accepted: sender sees Ok
    }
    let asker = actor_ref.clone();
    let ask_task = tokio::spawn(async move { asker.ask(Query).await });
    // Let the ask envelope reach the mailbox before measuring.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let before = dead_letter_count();
    proceed.notify_one();

    let result = handle.await.unwrap();
    assert!(result.is_completed(), "graceful stop must complete");

    let ask_res = ask_task.await.unwrap();
    assert!(
        matches!(ask_res, Err(rsactor::Error::Receive { .. })),
        "ask queued behind stop must fail with Receive, got {ask_res:?}"
    );

    let delta = dead_letter_count() - before;
    assert_eq!(
        delta, 6,
        "expected 5 DiscardedAtShutdown tells + 1 caller-side ReplyDropped ask \
         (no receiver-side double count for the ask)"
    );
}

#[tokio::test]
async fn tells_pending_at_kill_are_recorded() {
    let _serial = serial_lock().lock().await;

    let (entered, proceed, actor_ref, handle) = spawn_wedged();

    actor_ref.tell(Block).await.unwrap();
    entered.notified().await;
    for _ in 0..4 {
        actor_ref.tell(Note).await.unwrap();
    }

    let before = dead_letter_count();
    actor_ref.kill();
    proceed.notify_one();

    let result = handle.await.unwrap();
    assert!(result.was_killed());

    let delta = dead_letter_count() - before;
    assert_eq!(
        delta, 4,
        "all tells pending at kill() must be recorded as DiscardedAtShutdown"
    );
}

// ---------------------------------------------------------------------------
// subscribe_idle fails fast once the actor has begun stopping
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SlowStopActor {
    entered: Arc<Notify>,
    proceed: Arc<Notify>,
}

impl Actor for SlowStopActor {
    type Args = (Arc<Notify>, Arc<Notify>);
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(
        (entered, proceed): Self::Args,
        _: &ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        Ok(SlowStopActor { entered, proceed })
    }

    async fn on_stop(
        &mut self,
        _: &rsactor::ActorWeak<Self>,
        _killed: bool,
    ) -> Result<(), Self::Error> {
        self.entered.notify_one();
        self.proceed.notified().await;
        Ok(())
    }
}

#[message_handlers]
impl SlowStopActor {}

#[tokio::test]
async fn subscribe_idle_fails_once_stop_began() {
    let _serial = serial_lock().lock().await;

    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let opts = SpawnOptions::new().with_idle();
    let (actor_ref, handle) =
        spawn_with_options::<SlowStopActor>((entered.clone(), proceed.clone()), opts);

    actor_ref.stop().await;
    // on_stop has started: the idle-subscribe channel is closed by then.
    entered.notified().await;

    let res = actor_ref.subscribe_idle(futures::stream::pending::<()>());
    assert!(
        matches!(
            res.as_ref().map_err(|e| &e.error),
            Err(rsactor::Error::Send { .. })
        ),
        "subscribe_idle during shutdown must fail fast, got {res:?}"
    );

    proceed.notify_one();
    handle.await.unwrap();
}

#[tokio::test]
async fn subscribe_idle_fails_once_kill_began() {
    let _serial = serial_lock().lock().await;

    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let opts = SpawnOptions::new().with_idle();
    let (actor_ref, handle) =
        spawn_with_options::<SlowStopActor>((entered.clone(), proceed.clone()), opts);

    actor_ref.kill();
    // on_stop has started: the idle-subscribe channel is closed by then.
    entered.notified().await;

    let res = actor_ref.subscribe_idle(futures::stream::pending::<()>());
    assert!(
        matches!(
            res.as_ref().map_err(|e| &e.error),
            Err(rsactor::Error::Send { .. })
        ),
        "subscribe_idle during kill shutdown must fail fast, got {res:?}"
    );

    proceed.notify_one();
    let result = handle.await.unwrap();
    assert!(result.was_killed());
}

// ---------------------------------------------------------------------------
// First signal wins: a kill() arriving after a graceful stop began is not
// observed — the actor completes the graceful stop with killed = false.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kill_after_stop_began_keeps_graceful_result() {
    let _serial = serial_lock().lock().await;

    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let (actor_ref, handle) = spawn_with_options::<SlowStopActor>(
        (entered.clone(), proceed.clone()),
        SpawnOptions::new(),
    );

    actor_ref.stop().await;
    // The actor dequeued StopGracefully and is inside on_stop(killed = false).
    entered.notified().await;

    // Late kill: enqueued but never observed (first signal wins).
    actor_ref.kill();
    proceed.notify_one();

    let result = handle.await.unwrap();
    assert!(result.is_completed(), "graceful stop must complete");
    assert!(
        !result.was_killed(),
        "a kill() arriving after the graceful stop began must not flip the \
         result to killed=true (first signal wins)"
    );
}
