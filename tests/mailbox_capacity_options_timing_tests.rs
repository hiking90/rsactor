// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! The one behaviour change from resolving mailbox capacity inside
//! `spawn_with_options` instead of inside `SpawnOptions::new`: the
//! process-wide default is read at spawn time.
//!
//! Previously `SpawnOptions::new()` copied `set_default_mailbox_capacity`'s
//! value into the options at construction, so options built *before* the
//! default was installed carried the old number to the spawn. Now they observe
//! whatever is installed when the actor is spawned.
//!
//! This file holds a single test and no other, because the property only
//! exists in the window between `SpawnOptions::new()` and the setter. Tests
//! inside one binary run in parallel and the default is a process-wide
//! `OnceLock`, so a sibling test could install it first and leave this
//! assertion passing without having exercised anything.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rsactor::{
    message_handlers, set_default_mailbox_capacity, spawn_with_options, Actor, ActorRef,
    SpawnOptions,
};
use tokio::sync::Semaphore;

/// Differs from [`rsactor::DEFAULT_MAILBOX_CAPACITY`] (32) so the assertion
/// cannot be satisfied by the fallback.
const GLOBAL_CAPACITY: usize = 6;

const ADMISSION_PROBE: Duration = Duration::from_millis(200);

struct Blocker {
    started: Arc<AtomicU32>,
    gate: Arc<Semaphore>,
}

struct BlockMe;

impl Actor for Blocker {
    type Args = (Arc<AtomicU32>, Arc<Semaphore>);
    type Error = std::convert::Infallible;
    type IdleEvent = ();

    // Left at the default so the global value is what resolution reaches.
    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Blocker {
            started: args.0,
            gate: args.1,
        })
    }
}

#[message_handlers]
impl Blocker {
    #[handler]
    async fn handle_block(&mut self, _msg: BlockMe, _: &ActorRef<Self>) {
        self.started.fetch_add(1, Ordering::SeqCst);
        let _ = self.gate.acquire().await;
    }
}

#[tokio::test]
async fn global_default_is_read_at_spawn_not_at_options_construction() {
    // Built first, while no process-wide default exists.
    let opts = SpawnOptions::new();

    set_default_mailbox_capacity(GLOBAL_CAPACITY)
        .expect("this binary holds the only call to the setter");

    let started = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(Semaphore::new(0));
    let (actor_ref, handle) = spawn_with_options::<Blocker>((started.clone(), gate.clone()), opts);

    actor_ref.tell(BlockMe).await.expect("first tell");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while started.load(Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "actor never entered its handler"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let mut admitted = 0;
    while admitted < GLOBAL_CAPACITY + 4 {
        match actor_ref.tell_with_timeout(BlockMe, ADMISSION_PROBE).await {
            Ok(()) => admitted += 1,
            Err(_) => break,
        }
    }

    assert_eq!(
        admitted, GLOBAL_CAPACITY,
        "SpawnOptions built before the default was installed must still observe it at spawn"
    );

    gate.close();
    actor_ref.stop().await;
    handle.await.expect("actor task");
}
