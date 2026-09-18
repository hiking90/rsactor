// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Step 3 of the `Actor::MAILBOX_CAPACITY` resolution order: how the
//! process-wide default set by `set_default_mailbox_capacity` interacts with
//! the trait constant.
//!
//! This is a separate test binary because that default is backed by a
//! `OnceLock`. Writing it is a one-shot, process-wide act, so it cannot share a
//! binary with tests that need to observe `DEFAULT_MAILBOX_CAPACITY`, nor can
//! two tests here both install one. Exactly one call to
//! `set_default_mailbox_capacity` exists in this file, in `install_global`.
//!
//! The capacity-measuring technique is the same as in
//! `mailbox_capacity_const_tests.rs`: park the actor in its first handler, then
//! count how many further `tell`s the mailbox admits.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rsactor::{message_handlers, set_default_mailbox_capacity, spawn, Actor, ActorRef};
use tokio::sync::Semaphore;

/// The process-wide default this binary installs. Deliberately different from
/// [`rsactor::DEFAULT_MAILBOX_CAPACITY`] (32) and from every constant below, so
/// each assertion identifies which step of the order produced the number.
const GLOBAL_CAPACITY: usize = 7;

const ADMISSION_PROBE: Duration = Duration::from_millis(200);

/// Installs the process-wide default exactly once, whichever test runs first.
///
/// Tests within a binary run in parallel, so every test that depends on the
/// global default must go through this rather than calling the setter directly:
/// a second call would return `Err` and, more importantly, a test could
/// otherwise observe the default before it was installed.
fn install_global() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        set_default_mailbox_capacity(GLOBAL_CAPACITY)
            .expect("this binary installs the global default exactly once");
    });
}

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

    async fn block(&self) {
        self.started.fetch_add(1, Ordering::SeqCst);
        let _ = self.gate.acquire().await;
    }

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

blocking_actor!(DefersToGlobal, None);
blocking_actor!(AsksForThree, Some(3));

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

/// An actor that declares no capacity picks up the process-wide default.
#[tokio::test]
async fn unset_const_uses_global_default() {
    install_global();

    let gate = Gate::new();
    let (actor_ref, handle) = spawn::<DefersToGlobal>(gate.clone());

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, GLOBAL_CAPACITY + 4).await;
    assert_eq!(
        admitted, GLOBAL_CAPACITY,
        "MAILBOX_CAPACITY = None should fall through to the global default"
    );

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

/// The trait constant outranks the process-wide default: an actor that asked
/// for 3 keeps 3 even though the process default is larger.
#[tokio::test]
async fn const_outranks_global_default() {
    install_global();

    let gate = Gate::new();
    let (actor_ref, handle) = spawn::<AsksForThree>(gate.clone());

    actor_ref.tell(BlockMe).await.expect("first tell");
    gate.wait_until_blocked().await;

    let admitted = count_admitted(&actor_ref, GLOBAL_CAPACITY + 4).await;
    assert_eq!(
        admitted, 3,
        "MAILBOX_CAPACITY = Some(3) must win over the global default of {GLOBAL_CAPACITY}"
    );

    gate.release();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}

// The remaining step-3 behaviour — that the global default is read at spawn
// time rather than when `SpawnOptions` is constructed — needs a guaranteed
// ordering between `SpawnOptions::new()` and the setter. It lives in
// `mailbox_capacity_options_timing_tests.rs`, alone in its binary; asserting it
// here would pass vacuously whenever another test installed the default first.
