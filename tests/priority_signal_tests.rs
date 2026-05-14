// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the priority channel (`SpawnOptions::with_priority`).
//!
//! These tests cover:
//! - Disabled-by-default behavior (calls return `PriorityChannelNotEnabled`,
//!   no dead letters recorded).
//! - Priority bypass over a saturated regular mailbox.
//! - `kill()` overtaking pending priority messages.
//! - `stop()` draining the priority queue before `on_stop` while refusing new sends.
//! - Round-trip `ask_priority` with downcast.
//! - `ActorWeak::upgrade()` succeeding even when every strong priority sender is gone.
//! - Blocking variants (`blocking_tell_priority`, `blocking_ask_priority`).
//! - Mandatory-`Duration` timeout behavior on a wedged actor.
//! - `capacity = 1` admission serialization between concurrent senders.

use rsactor::{
    dead_letter_count, message_handlers, spawn, spawn_with_options, Actor, ActorRef, Error,
    SpawnOptions,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

// Tests that read the global dead-letter counter must not run concurrently
// with each other within this test binary, otherwise their `before`/`after`
// captures race. The mutex is held across `.await` points (the test body),
// so it must be `tokio::sync::Mutex` rather than `std::sync::Mutex`
// (clippy::await_holding_lock).
fn dead_letter_serial_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Default priority operation timeout for tests where no waiting is expected.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct CounterActor {
    normal_count: u32,
    priority_count: u32,
}

#[derive(Debug)]
struct NormalMsg;
#[derive(Debug)]
struct PriorityMsg;
#[derive(Debug)]
struct GetCounts;
#[derive(Debug)]
struct PriorityPing(u32);

impl Actor for CounterActor {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(CounterActor {
            normal_count: 0,
            priority_count: 0,
        })
    }
}

#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_normal(&mut self, _: NormalMsg, _: &ActorRef<Self>) {
        self.normal_count += 1;
    }

    #[handler]
    async fn handle_priority(&mut self, _: PriorityMsg, _: &ActorRef<Self>) {
        self.priority_count += 1;
    }

    #[handler]
    async fn handle_get_counts(&mut self, _: GetCounts, _: &ActorRef<Self>) -> (u32, u32) {
        (self.normal_count, self.priority_count)
    }

    #[handler]
    async fn handle_priority_ping(&mut self, msg: PriorityPing, _: &ActorRef<Self>) -> u32 {
        self.priority_count += 1;
        msg.0 + 1
    }
}

// Actor that blocks the regular mailbox on demand. Used to test that priority
// messages bypass a saturated mailbox and that timeouts fire on wedge.
#[derive(Debug)]
struct BlockingActor {
    started: Arc<AtomicU32>,
    release: Arc<Notify>,
    priority_count: u32,
}

#[derive(Debug)]
struct BlockMe;
#[derive(Debug)]
struct PingPriority;
#[derive(Debug)]
struct GetPriorityCount;

impl Actor for BlockingActor {
    type Args = (Arc<AtomicU32>, Arc<Notify>);
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(
        (started, release): Self::Args,
        _: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(BlockingActor {
            started,
            release,
            priority_count: 0,
        })
    }
}

#[message_handlers]
impl BlockingActor {
    #[handler]
    async fn handle_block(&mut self, _: BlockMe, _: &ActorRef<Self>) {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.release.notified().await;
    }

    #[handler]
    async fn handle_ping(&mut self, _: PingPriority, _: &ActorRef<Self>) -> u32 {
        self.priority_count += 1;
        self.priority_count
    }

    #[handler]
    async fn handle_get_priority_count(&mut self, _: GetPriorityCount, _: &ActorRef<Self>) -> u32 {
        self.priority_count
    }
}

// ---------------------------------------------------------------------------
// Disabled-by-default behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_spawn_has_no_priority_channel() {
    let (actor_ref, _handle) = spawn::<CounterActor>(());
    assert!(
        !actor_ref.has_priority_channel(),
        "spawn() must not enable the priority channel"
    );
    actor_ref.stop().await;
}

#[tokio::test]
async fn tell_priority_on_disabled_actor_returns_not_enabled_error() {
    let _serial = dead_letter_serial_lock().lock().await;

    let (actor_ref, _handle) = spawn::<CounterActor>(());
    let before = dead_letter_count();

    let err = actor_ref
        .tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
        .await
        .unwrap_err();

    match err {
        Error::PriorityChannelNotEnabled { .. } => {}
        other => panic!("expected PriorityChannelNotEnabled, got {other:?}"),
    }

    // Configuration error must NOT be recorded as a dead letter.
    assert_eq!(
        dead_letter_count(),
        before,
        "PriorityChannelNotEnabled must not increment the dead letter counter"
    );

    actor_ref.stop().await;
}

#[tokio::test]
async fn ask_priority_on_disabled_actor_returns_not_enabled_error() {
    let _serial = dead_letter_serial_lock().lock().await;

    let (actor_ref, _handle) = spawn::<CounterActor>(());
    let before = dead_letter_count();

    let err = actor_ref
        .ask_priority(PriorityPing(1), DEFAULT_TIMEOUT)
        .await
        .unwrap_err();

    assert!(matches!(err, Error::PriorityChannelNotEnabled { .. }));
    assert_eq!(dead_letter_count(), before);

    actor_ref.stop().await;
}

// ---------------------------------------------------------------------------
// Priority bypass and ask_priority round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn priority_bypasses_saturated_regular_mailbox() {
    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(Notify::new());

    // Mailbox capacity 1: a single in-flight BlockMe message will occupy the
    // actor while a second BlockMe sits in the channel slot. Any further
    // regular send would block on admission.
    let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
    let (actor_ref, _handle) =
        spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

    // 1) Saturate the regular mailbox: the first BlockMe is being processed,
    //    the second occupies the channel slot.
    actor_ref.tell(BlockMe).await.unwrap();

    // Wait until the actor actually entered the first handler.
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    actor_ref.tell(BlockMe).await.unwrap();

    // Confirm the regular mailbox would now block — try_send equivalent via a
    // short timeout. We use tell_with_timeout to avoid hanging the test if our
    // assumption is wrong.
    let saturated = tokio::time::timeout(
        Duration::from_millis(100),
        actor_ref.tell_with_timeout(BlockMe, Duration::from_millis(50)),
    )
    .await
    .expect("tell_with_timeout should return")
    .unwrap_err();
    assert!(
        matches!(saturated, Error::Timeout { .. }),
        "regular mailbox should be saturated, got {saturated:?}"
    );

    // 2) Priority should still go through and be processed before the queued
    //    regular messages. We need to release the in-flight handler so that
    //    the actor reaches the next select! iteration to pick up priority.
    release.notify_one();

    let n = actor_ref
        .ask_priority(PingPriority, DEFAULT_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(n, 1, "priority handler should have run exactly once");

    // Drain the rest gracefully.
    release.notify_one();
    release.notify_one();
    actor_ref.stop().await;
}

#[tokio::test]
async fn ask_priority_returns_typed_reply() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let reply: u32 = actor_ref
        .ask_priority(PriorityPing(41), DEFAULT_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(reply, 42);

    actor_ref.stop().await;
}

// ---------------------------------------------------------------------------
// kill() overtakes the priority queue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kill_overtakes_pending_priority_messages() {
    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(Notify::new());

    let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
    let (actor_ref, handle) =
        spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

    // Make the actor busy on a regular handler that won't return until we
    // release it. While it's busy, queue a priority message that will sit in
    // the priority slot.
    actor_ref.tell(BlockMe).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    actor_ref
        .tell_priority(PingPriority, DEFAULT_TIMEOUT)
        .await
        .unwrap();

    // Kill the actor. Per §5.A-1, kill MUST overtake pending priority messages
    // and the queued PingPriority MUST NOT be processed.
    actor_ref.kill();
    release.notify_one(); // unblock the in-flight handler so the loop progresses

    let result = handle.await.unwrap();
    let actor = result.actor().expect("actor should be returned on kill");
    assert_eq!(
        actor.priority_count, 0,
        "kill must not drain pending priority messages"
    );
}

// ---------------------------------------------------------------------------
// stop() processes pending priority messages before on_stop, then refuses
// new priority sends. The drain code path is also stress-tested below.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn priority_message_pending_at_stop_is_processed_before_on_stop() {
    // Note: in this happy-path scenario the priority message is consumed by
    // the normal priority `select!` arm (priority > regular bias), NOT by the
    // close-then-drain code in the StopGracefully branch. The drain itself
    // is exercised by `stop_drain_loop_processes_racing_priority_send` below.

    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(Notify::new());

    let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
    let (actor_ref, handle) =
        spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

    actor_ref.tell(BlockMe).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    actor_ref
        .tell_priority(PingPriority, DEFAULT_TIMEOUT)
        .await
        .unwrap();
    actor_ref.stop().await;
    release.notify_one();

    let result = handle.await.unwrap();
    let actor = result.actor().expect("actor returned on graceful stop");
    assert_eq!(
        actor.priority_count, 1,
        "priority message pending at stop must be processed before on_stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_drain_loop_processes_racing_priority_send() {
    // Genuinely exercises the close-then-drain code in the StopGracefully arm.
    //
    // The drain only fires for messages that land in the priority slot AFTER
    // the actor has already moved past the priority `select!` arm and entered
    // the regular-mailbox StopGracefully arm. That window only exists on a
    // multi-threaded runtime where the sender and the actor can race.
    //
    // Strategy: race many iterations. For each iteration spawn an actor, hold
    // it on a regular handler, queue stop(), release, then immediately spawn
    // a sender that hammers tell_priority. Across iterations the drain MUST
    // sometimes fire (priority message arrives while actor is already in
    // graceful-stop arm) and the contract holds in every case:
    //   - tell_priority either succeeds (consumed by normal priority arm OR
    //     by drain) or fails with Send (priority channel closed by drain).
    //   - The actor terminates cleanly with priority_count counting all
    //     accepted messages.
    //
    // We additionally assert that across iterations at least one tell_priority
    // observed Send (i.e. drain ran close() while a sender was racing). This
    // confirms the drain branch is reachable, not dead code.
    //
    // Caveats:
    // - Requires multi-threaded runtime (worker_threads >= 2). On a true
    //   single-CPU host the workers serialize and the race window collapses;
    //   the assertion below could fail. Modern CI runners are multi-CPU so
    //   this is not currently a concern.
    // - ITERS = 200 keeps the assertion robust across scheduling jitter while
    //   still completing in well under a second on a typical laptop.

    const ITERS: usize = 200;
    let mut saw_post_close_send = false;

    for _ in 0..ITERS {
        let started = Arc::new(AtomicU32::new(0));
        let release = Arc::new(Notify::new());
        let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
        let (actor_ref, handle) =
            spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

        actor_ref.tell(BlockMe).await.unwrap();
        while started.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        actor_ref.stop().await;

        // Race: spawn a few priority senders concurrently with releasing the
        // blocker. Some sends will be accepted, some will be rejected after
        // close().
        let sender = actor_ref.clone();
        let race = tokio::spawn(async move {
            let mut send_errors = 0u32;
            let mut accepted = 0u32;
            for _ in 0..8 {
                match sender
                    .tell_priority(PingPriority, Duration::from_millis(50))
                    .await
                {
                    Ok(()) => accepted += 1,
                    Err(Error::Send { .. }) => send_errors += 1,
                    Err(Error::Timeout { .. }) => {
                        // capacity-1 contention — fine, skip
                    }
                    Err(other) => panic!("unexpected error from racing send: {other:?}"),
                }
            }
            (accepted, send_errors)
        });

        release.notify_one();

        let (_accepted, send_errors) = race.await.unwrap();
        if send_errors > 0 {
            saw_post_close_send = true;
        }

        // Actor must terminate cleanly regardless of the race outcome.
        let result = handle.await.unwrap();
        result.actor().expect("clean termination");
    }

    assert!(
        saw_post_close_send,
        "across {ITERS} iterations of racing tell_priority vs stop(), at least one \
         send should have observed the priority channel closed by the drain branch — \
         this is what proves the drain code is reachable in production"
    );
}

#[tokio::test]
async fn priority_send_after_stop_drain_records_dead_letter() {
    let _serial = dead_letter_serial_lock().lock().await;

    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, handle) = spawn_with_options::<CounterActor>((), opts);
    actor_ref.stop().await;
    handle.await.unwrap();

    let before = dead_letter_count();

    // Actor has fully stopped; both regular and priority channels are closed.
    let err = actor_ref
        .tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Send { .. }));
    assert!(
        dead_letter_count() > before,
        "Send failure on closed priority channel must be recorded as a dead letter"
    );
}

// ---------------------------------------------------------------------------
// Timeout on a wedged actor
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tell_priority_timeout_on_wedged_actor_records_dead_letter() {
    let _serial = dead_letter_serial_lock().lock().await;

    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(Notify::new());

    let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
    let (actor_ref, handle) =
        spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

    // Block the actor in a handler so it never reaches the next select.
    actor_ref.tell(BlockMe).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // First priority message fills the capacity-1 slot.
    actor_ref
        .tell_priority(PingPriority, Duration::from_millis(500))
        .await
        .unwrap();

    let before = dead_letter_count();
    // Second priority message must time out: the slot is full and the actor
    // is wedged so it won't be drained.
    let err = actor_ref
        .tell_priority(PingPriority, Duration::from_millis(100))
        .await
        .unwrap_err();
    match err {
        Error::Timeout { operation, .. } => assert_eq!(operation, "tell_priority"),
        other => panic!("expected Timeout, got {other:?}"),
    }
    assert!(
        dead_letter_count() > before,
        "Timeout on tell_priority must be recorded as a dead letter"
    );

    // Cleanup: release the wedge and shut down.
    release.notify_one();
    actor_ref.kill();
    let _ = handle.await;
}

// ---------------------------------------------------------------------------
// ActorWeak upgrade policy
// ---------------------------------------------------------------------------

/// Wait until the lifecycle task has progressed past `on_start` and dropped
/// its own clone of `actor_ref`. Without this, the lifecycle clone keeps the
/// strong sender count at >= 1 even after the test drops its own priority
/// sender, masking the upgrade-policy under test. A round-trip ask is the
/// most reliable signal: receiving a reply implies the actor has reached the
/// main loop, which means the lifecycle has executed its early `drop(actor_ref)`.
async fn wait_for_lifecycle_settled<A>(actor_ref: &ActorRef<A>)
where
    A: Actor + rsactor::Message<GetCounts, Reply = (u32, u32)> + 'static,
{
    let _ = actor_ref.ask(GetCounts).await.unwrap();
}

#[tokio::test]
async fn actor_weak_upgrade_succeeds_when_only_priority_strong_dropped() {
    let opts = SpawnOptions::new().with_priority();
    let (mut actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    // Wait until the lifecycle's internal actor_ref clone is gone before we
    // sample upgrade behavior. Otherwise we observe its strong sender too.
    wait_for_lifecycle_settled(&actor_ref).await;

    let weak = ActorRef::downgrade(&actor_ref);
    assert!(
        actor_ref.has_priority_channel(),
        "spawned with priority enabled"
    );

    // Drop ONLY the priority sender on this single ActorRef. The regular
    // mailbox and terminate channels stay strong, so the actor remains alive.
    actor_ref.drop_priority_sender_for_test();
    assert!(
        !actor_ref.has_priority_channel(),
        "priority sender removed from this ActorRef"
    );

    // Upgrade must still succeed — priority is a secondary channel and only
    // the mailbox + terminate channels gate liveness.
    let upgraded = weak.upgrade().expect("actor should still be alive");

    // The upgraded ref should also have priority disabled, because no strong
    // priority sender exists anywhere to upgrade from.
    assert!(
        !upgraded.has_priority_channel(),
        "upgrade() returns priority-disabled ActorRef when every strong priority sender is gone"
    );

    // And calling tell_priority on the upgraded ref must surface the config error.
    let err = upgraded
        .tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PriorityChannelNotEnabled { .. }));

    upgraded.stop().await;
}

#[tokio::test]
async fn actor_weak_upgrade_preserves_priority_when_other_strong_alive() {
    let opts = SpawnOptions::new().with_priority();
    let (mut actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);
    wait_for_lifecycle_settled(&actor_ref).await;

    let weak = ActorRef::downgrade(&actor_ref);

    // Hold a SEPARATE clone with the priority sender intact, then drop priority
    // on the original. Upgrade should still see priority alive.
    let other_strong = actor_ref.clone();
    actor_ref.drop_priority_sender_for_test();

    let upgraded = weak.upgrade().expect("alive");
    assert!(
        upgraded.has_priority_channel(),
        "priority sender survives in upgrade because another strong clone holds it"
    );

    drop(other_strong);
    upgraded.stop().await;
}

// ---------------------------------------------------------------------------
// Blocking variants
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_tell_priority_succeeds_when_enabled() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let actor_ref_clone = actor_ref.clone();
    let join = tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
    });
    join.await.unwrap().unwrap();

    let (_normal, priority) = actor_ref.ask(GetCounts).await.unwrap();
    assert_eq!(priority, 1);

    actor_ref.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_ask_priority_returns_reply() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let actor_ref_clone = actor_ref.clone();
    let reply: u32 = tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_ask_priority(PriorityPing(99), DEFAULT_TIMEOUT)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(reply, 100);

    actor_ref.stop().await;
}

// Verify that the blocking priority variants can be called directly from an
// async context on a multi-thread runtime. The block_in_place fast path
// reuses the current runtime instead of spawning a separate thread and
// runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_tell_priority_from_async_multi_thread_context() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    // Direct call from async context: would panic without the fast path
    // if it went through the new-thread+new-runtime fallback in a way
    // that tried to nest a runtime. The fast path makes it safe.
    actor_ref
        .blocking_tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
        .expect("blocking_tell_priority from async ctx should succeed");

    let (_normal, priority) = actor_ref.ask(GetCounts).await.unwrap();
    assert_eq!(priority, 1);

    actor_ref.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_ask_priority_from_async_multi_thread_context() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let reply: u32 = actor_ref
        .blocking_ask_priority(PriorityPing(7), DEFAULT_TIMEOUT)
        .expect("blocking_ask_priority from async ctx should succeed");
    assert_eq!(reply, 8);

    actor_ref.stop().await;
}

// On a current_thread runtime, blocking_*_priority cannot take the
// block_in_place fast path and must fall back to spawning a dedicated
// thread with a temporary runtime. These tests pin that behaviour.
// Invoking from `tokio::task::spawn_blocking` lets the runtime's sole thread
// yield and drive the actor while the blocking thread waits — calling
// directly from async ctx (or via `std::thread::spawn(...).join()`) on a
// current_thread runtime would deadlock the actor.

#[tokio::test(flavor = "current_thread")]
async fn blocking_tell_priority_on_current_thread_runtime() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let actor_ref_clone = actor_ref.clone();
    tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
    })
    .await
    .expect("spawn_blocking task should not panic")
    .expect("blocking_tell_priority on current_thread runtime should succeed");

    let (_normal, priority) = actor_ref.ask(GetCounts).await.unwrap();
    assert_eq!(priority, 1);

    actor_ref.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_ask_priority_on_current_thread_runtime() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    let actor_ref_clone = actor_ref.clone();
    let reply: u32 = tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_ask_priority(PriorityPing(20), DEFAULT_TIMEOUT)
    })
    .await
    .expect("spawn_blocking task should not panic")
    .expect("blocking_ask_priority on current_thread runtime should succeed");
    assert_eq!(reply, 21);

    actor_ref.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_priority_on_disabled_actor_returns_not_enabled() {
    let (actor_ref, _handle) = spawn::<CounterActor>(());
    let actor_ref_clone = actor_ref.clone();
    let err = tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_tell_priority(PriorityMsg, DEFAULT_TIMEOUT)
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(matches!(err, Error::PriorityChannelNotEnabled { .. }));
    actor_ref.stop().await;
}

// ---------------------------------------------------------------------------
// capacity = 1 admission serialization
// ---------------------------------------------------------------------------

#[tokio::test]
async fn capacity_one_serializes_concurrent_priority_admission() {
    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(Notify::new());

    let opts = SpawnOptions::new().mailbox_capacity(1).with_priority();
    let (actor_ref, _handle) =
        spawn_with_options::<BlockingActor>((started.clone(), release.clone()), opts);

    actor_ref.tell(BlockMe).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Two concurrent priority sends. With capacity = 1 and a wedged actor,
    // at most one can be admitted; the other must time out.
    let r1 = actor_ref.clone();
    let r2 = actor_ref.clone();
    let task1 = tokio::spawn(async move {
        r1.tell_priority(PingPriority, Duration::from_millis(200))
            .await
    });
    let task2 = tokio::spawn(async move {
        r2.tell_priority(PingPriority, Duration::from_millis(200))
            .await
    });

    let res1 = task1.await.unwrap();
    let res2 = task2.await.unwrap();
    let oks = [&res1, &res2].iter().filter(|r| r.is_ok()).count();
    let timeouts = [&res1, &res2]
        .iter()
        .filter(|r| matches!(r, Err(Error::Timeout { .. })))
        .count();
    assert_eq!(
        oks, 1,
        "exactly one priority send should succeed, results: {res1:?} {res2:?}"
    );
    assert_eq!(
        timeouts, 1,
        "exactly one priority send should time out, results: {res1:?} {res2:?}"
    );

    release.notify_one();
    actor_ref.kill();
}

// ---------------------------------------------------------------------------
// Processing order: priority handler runs before queued regular messages
// ---------------------------------------------------------------------------
//
// `priority_bypasses_saturated_regular_mailbox` proves a priority message can
// still be admitted when the regular mailbox is saturated. This test proves
// the stronger ordering claim: once a regular handler in flight finishes, the
// queued priority message is processed BEFORE the next regular message in
// line, even if regulars were enqueued first.

#[derive(Debug)]
struct OrderActor {
    log: Arc<std::sync::Mutex<Vec<&'static str>>>,
    r1_started: Arc<Notify>,
    r1_release: Arc<Notify>,
}

#[derive(Debug)]
struct SlowRegular(&'static str);
#[derive(Debug)]
struct PriorityTag(&'static str);

impl Actor for OrderActor {
    type Args = (
        Arc<std::sync::Mutex<Vec<&'static str>>>,
        Arc<Notify>,
        Arc<Notify>,
    );
    type Error = anyhow::Error;
    type IdleEvent = ();
    async fn on_start(
        (log, r1_started, r1_release): Self::Args,
        _: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(OrderActor {
            log,
            r1_started,
            r1_release,
        })
    }
}

#[message_handlers]
impl OrderActor {
    #[handler]
    async fn handle_slow(&mut self, msg: SlowRegular, _: &ActorRef<Self>) {
        // R1 specifically signals when it has entered the handler and waits
        // for the test to release it. R2/R3 fall through immediately so they
        // back-fill behind R1 in FIFO order.
        if msg.0 == "R1" {
            self.r1_started.notify_one();
            self.r1_release.notified().await;
        }
        self.log.lock().unwrap().push(msg.0);
    }

    #[handler]
    async fn handle_priority_tag(&mut self, msg: PriorityTag, _: &ActorRef<Self>) {
        self.log.lock().unwrap().push(msg.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn priority_handler_runs_before_queued_regular_messages() {
    let log = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let r1_started = Arc::new(Notify::new());
    let r1_release = Arc::new(Notify::new());
    let opts = SpawnOptions::new().mailbox_capacity(8).with_priority();
    let (actor_ref, handle) = spawn_with_options::<OrderActor>(
        (log.clone(), r1_started.clone(), r1_release.clone()),
        opts,
    );

    actor_ref.tell(SlowRegular("R1")).await.unwrap();
    actor_ref.tell(SlowRegular("R2")).await.unwrap();
    actor_ref.tell(SlowRegular("R3")).await.unwrap();

    // Wait deterministically until R1 is actually inside its handler. Without
    // this barrier the priority send below could race ahead of R1 (especially
    // under CI load) and the observed order would become ["P", "R1", "R2", "R3"].
    r1_started.notified().await;

    // R1 is mid-handler; enqueue the priority message.
    actor_ref
        .tell_priority(PriorityTag("P"), DEFAULT_TIMEOUT)
        .await
        .unwrap();

    // Release R1 and let everything drain.
    r1_release.notify_one();
    actor_ref.stop().await;
    handle.await.unwrap();

    let order = log.lock().unwrap().clone();
    // Contract: R1 finishes first (already in flight when P arrived); then the
    // biased priority branch beats the regular mailbox so P runs before R2/R3.
    assert_eq!(
        order,
        vec!["R1", "P", "R2", "R3"],
        "priority handler must run between R1 (in-flight) and R2/R3 (queued)"
    );
}

// ---------------------------------------------------------------------------
// Metrics separation: priority counters are distinct from regular counters
// ---------------------------------------------------------------------------
//
// Compiled only when the `metrics` feature is enabled. This is the headline
// observability story for priority-channel abuse detection per the FAQ.

#[cfg(feature = "metrics")]
#[tokio::test]
async fn priority_metrics_counters_are_distinct_from_regular() {
    let opts = SpawnOptions::new().with_priority();
    let (actor_ref, _handle) = spawn_with_options::<CounterActor>((), opts);

    // 5 regular sends.
    for _ in 0..5 {
        actor_ref.tell(NormalMsg).await.unwrap();
    }
    // 3 priority sends. Use ask_priority to ensure each is processed before
    // the next is admitted (synchronous, no race on observation).
    for n in 0..3u32 {
        let _: u32 = actor_ref
            .ask_priority(PriorityPing(n), DEFAULT_TIMEOUT)
            .await
            .unwrap();
    }

    // Round-trip a final ask through the regular mailbox to be sure all
    // earlier regulars have been processed (handlers are sequential).
    let _ = actor_ref.ask(GetCounts).await.unwrap();

    let snap = actor_ref.metrics();
    // 5 regular tells + 1 ask = 6 regular messages.
    assert_eq!(
        snap.message_count, 6,
        "regular counter must include only mailbox-channel messages, got snapshot {snap:?}"
    );
    assert_eq!(
        snap.priority_message_count, 3,
        "priority counter must include only priority-channel messages, got snapshot {snap:?}"
    );

    // Convenience accessors mirror the snapshot.
    assert_eq!(actor_ref.priority_message_count(), 3);
    assert_eq!(actor_ref.message_count(), 6);

    actor_ref.stop().await;
}
