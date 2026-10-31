// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for integration tests. Include with `mod common;`.

use rsactor::{Actor, ActorRef, ActorResult};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Holds a message handler inside its body until the test releases it.
///
/// Timeout tests need an actor that stays busy longer than the timeout. A
/// fixed `sleep` in the handler does that, but `kill()` is cooperative and
/// cannot interrupt a running handler, so the test then waits out the whole
/// sleep before the actor stops. A gated handler returns as soon as the test
/// calls [`release`](Self::release).
#[derive(Clone, Default)]
pub struct Gate {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

impl Gate {
    /// Call from the handler body: signals `started`, then parks until released.
    pub async fn pass(&self) {
        self.started.notify_one();
        self.release.notified().await;
    }

    /// Resolves once a handler has entered [`pass`](Self::pass), i.e. the
    /// message has left the mailbox and the handler is occupied.
    pub async fn wait_started(&self) {
        self.started.notified().await;
    }

    /// Lets one parked (or the next) handler return.
    pub fn release(&self) {
        self.release.notify_one();
    }
}

/// Kills the actor, then releases its gated handler, and returns the result.
///
/// The order matters. With Terminate already queued, the runtime's biased
/// `select!` observes it as soon as the gated handler returns, so no queued
/// gated message starts (it would park forever with no release left).
pub async fn kill_and_release<T: Actor>(
    actor_ref: &ActorRef<T>,
    handle: JoinHandle<ActorResult<T>>,
    gate: &Gate,
) -> ActorResult<T> {
    actor_ref.kill();
    gate.release();
    let result = handle.await.expect("actor task should not panic");
    assert!(result.was_killed(), "actor should report killed");
    result
}
