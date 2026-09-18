// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for `Actor::MAILBOX_CAPACITY` — steps 1, 2 and 4 of the resolution
//! order documented on that constant.
//!
//! Step 3 (`set_default_mailbox_capacity`) is covered in
//! `mailbox_capacity_global_tests.rs`, which must be its own binary: the global
//! default is a `OnceLock` that can be written once per process, and installing
//! one here would change what `no_const_falls_back_to_default_capacity` below
//! is allowed to observe.
//!
//! Capacity is measured rather than asserted indirectly: an actor is parked
//! inside its first handler, and the test counts how many further `tell`s the
//! mailbox admits before it refuses one. For a tokio mpsc channel of capacity
//! `n`, the message being handled has already left the buffer, so exactly `n`
//! more are admitted.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rsactor::{message_handlers, spawn, spawn_with_options, Actor, ActorRef, SpawnOptions};
use tokio::sync::Semaphore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// How long a `tell` may wait for admission before the test treats the mailbox
/// as full. Admission to a mailbox with a free slot takes a `try_send`, so this
/// only has to outlast scheduling noise.
const ADMISSION_PROBE: Duration = Duration::from_millis(200);

/// Shared state for the parked actors below.
///
/// `gate` is a `Semaphore` with zero permits rather than a `Notify`: closing it
/// releases both the currently parked handler and every message still queued
/// behind it, with no chance of a wakeup landing before its waiter registers.
#[derive(Clone)]
struct Gate {
    started: Arc<AtomicU32>,
    gate: Arc<Semaphore>,
}

impl Gate {
    fn new() -> Self {
        Self {
            started: Arc::new(AtomicU32::new(0)),
            gate: Arc::new(Semaphore::new(0)),
        }
    }

    /// Parks the calling handler until [`Gate::release`] is called.
    async fn block(&self) {
        self.started.fetch_add(1, Ordering::SeqCst);
        // `Err` once the semaphore is closed — that is the release signal.
        let _ = self.gate.acquire().await;
    }

    /// Waits until a handler has actually entered [`Gate::block`], so the
    /// mailbox buffer is known to be empty before the test starts filling it.
    async fn wait_until_blocked(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while self.started.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "actor never entered its handler"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn release(&self) {
        self.gate.close();
    }
}

struct BlockMe;

/// Declares an actor that parks in its handler, with the given
/// `MAILBOX_CAPACITY`. A macro rather than a const-generic type because the
/// `None` case has no numeric parameter to be generic over.
macro_rules! blocking_actor {
    ($name:ident, $capacity:expr) => {
        struct $name(Gate);

        impl Actor for $name {
            type Args = Gate;
            type Error = std::convert::Infallible;
            type IdleEvent = ();

            const MAILBOX_CAPACITY: Option<usize> = $capacity;

            async fn on_start(
                args: Self::Args,
                _actor_ref: &ActorRef<Self>,
            ) -> Result<Self, Self::Error> {
                Ok($name(args))
            }
        }

        #[message_handlers]
        impl $name {
            #[handler]
            async fn handle_block(&mut self, _msg: BlockMe, _actor_ref: &ActorRef<Self>) {
                self.0.block().await;
            }
        }
    };
}

blocking_actor!(CapacityTwo, Some(2));
blocking_actor!(CapacityUnset, None);
blocking_actor!(CapacityZero, Some(0));

/// Counts how many `tell`s the mailbox admits, stopping at the first refusal or
/// at `limit`. `limit` is set above the expected capacity so that admitting
/// *too many* is reported as a wrong number rather than hanging the test.
async fn count_admitted<T>(actor_ref: &ActorRef<T>, limit: usize) -> usize
where
    T: Actor + rsactor::Message<BlockMe>,
{
    let mut admitted = 0;
    while admitted < limit {
        match actor_ref.tell_with_timeout(BlockMe, ADMISSION_PROBE).await {
            Ok(()) => admitted += 1,
            Err(_) => break,
        }
    }
    admitted
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Step 2: a plain `spawn` uses the actor type's own constant.
#[tokio::test]
async fn const_capacity_is_used_by_plain_spawn() {
    let gate = Gate::new();
    let (actor_ref, handle) = spawn::<CapacityTwo>(gate.clone());

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, 8).await;
    assert_eq!(
        admitted, 2,
        "MAILBOX_CAPACITY = Some(2) should admit exactly 2 queued messages"
    );

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

/// Step 1 beats step 2: an explicit capacity at the spawn site wins.
#[tokio::test]
async fn spawn_site_capacity_overrides_const() {
    let gate = Gate::new();
    let opts = SpawnOptions::new().mailbox_capacity(5);
    let (actor_ref, handle) = spawn_with_options::<CapacityTwo>(gate.clone(), opts);

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, 11).await;
    assert_eq!(
        admitted, 5,
        "SpawnOptions::mailbox_capacity(5) should override MAILBOX_CAPACITY = Some(2)"
    );

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

/// `spawn_with_mailbox_capacity` is the same override through the older API.
#[tokio::test]
async fn spawn_with_mailbox_capacity_overrides_const() {
    let gate = Gate::new();
    let (actor_ref, handle) = rsactor::spawn_with_mailbox_capacity::<CapacityTwo>(gate.clone(), 4);

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, 10).await;
    assert_eq!(admitted, 4, "the explicit capacity argument should win");

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

/// Step 4: `None` and no global default leaves `DEFAULT_MAILBOX_CAPACITY`.
#[tokio::test]
async fn no_const_falls_back_to_default_capacity() {
    let gate = Gate::new();
    let (actor_ref, handle) = spawn::<CapacityUnset>(gate.clone());

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, rsactor::DEFAULT_MAILBOX_CAPACITY + 4).await;
    assert_eq!(
        admitted,
        rsactor::DEFAULT_MAILBOX_CAPACITY,
        "MAILBOX_CAPACITY = None should fall through to DEFAULT_MAILBOX_CAPACITY"
    );

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

/// A zero capacity from the trait constant is rejected at spawn, with a message
/// that names the constant — the spawn site has no number to look at.
#[tokio::test]
#[should_panic(expected = "MAILBOX_CAPACITY")]
async fn zero_const_panics_at_spawn() {
    let _ = spawn::<CapacityZero>(Gate::new());
}
