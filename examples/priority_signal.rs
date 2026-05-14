// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Demonstrates the opt-in priority channel.
//!
//! The actor below simulates a worker that processes "long" jobs sequentially
//! from its regular mailbox. We use the priority channel for two control-plane
//! messages that must not be queued behind those long jobs:
//!
//! - `HealthCheck` (ask): fetches a snapshot of state without waiting in line.
//! - `PauseProcessing` / `ResumeProcessing` (tell): toggles a flag that the
//!   long-job handler observes.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example priority_signal
//! ```

use anyhow::Result;
use rsactor::{message_handlers, spawn_with_options, Actor, ActorRef, SpawnOptions};
use std::time::Duration;

#[derive(Debug)]
struct WorkerActor {
    jobs_done: u32,
    paused: bool,
}

#[derive(Debug)]
struct ProcessJob {
    id: u32,
}

#[derive(Debug)]
struct HealthCheck;

#[derive(Debug)]
struct HealthSnapshot {
    jobs_done: u32,
    paused: bool,
}

#[derive(Debug)]
struct PauseProcessing;

#[derive(Debug)]
struct ResumeProcessing;

impl Actor for WorkerActor {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self> {
        Ok(WorkerActor {
            jobs_done: 0,
            paused: false,
        })
    }
}

#[message_handlers]
impl WorkerActor {
    // Long-running regular handler. Notice how it observes `self.paused`,
    // which the priority-channel handler can flip mid-stream.
    #[handler]
    async fn handle_job(&mut self, msg: ProcessJob, _: &ActorRef<Self>) {
        if self.paused {
            // Drop the job in this demo so we can clearly see the pause take effect.
            println!("[worker] paused — skipping job #{}", msg.id);
            return;
        }
        println!("[worker] starting job #{}", msg.id);
        // Simulate work that occupies the regular mailbox for a while.
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.jobs_done += 1;
        println!("[worker] finished job #{}", msg.id);
    }

    #[handler]
    async fn handle_health(&mut self, _: HealthCheck, _: &ActorRef<Self>) -> HealthSnapshot {
        HealthSnapshot {
            jobs_done: self.jobs_done,
            paused: self.paused,
        }
    }

    #[handler]
    async fn handle_pause(&mut self, _: PauseProcessing, _: &ActorRef<Self>) {
        println!("[worker] *** PAUSE signal received ***");
        self.paused = true;
    }

    #[handler]
    async fn handle_resume(&mut self, _: ResumeProcessing, _: &ActorRef<Self>) {
        println!("[worker] *** RESUME signal received ***");
        self.paused = false;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_target(false)
        .init();

    println!("=== Priority Channel Demo ===\n");

    // Spawn with the priority channel enabled.
    let opts = SpawnOptions::new().mailbox_capacity(8).with_priority();
    let (actor, join) = spawn_with_options::<WorkerActor>((), opts);
    assert!(actor.has_priority_channel());

    // Queue several long jobs into the regular mailbox.
    for id in 1..=5 {
        actor.tell(ProcessJob { id }).await?;
    }

    // While jobs are being processed, ask for a health snapshot via the
    // priority channel. The reply arrives without waiting in line behind the
    // pending jobs.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let snap = actor
        .ask_priority(HealthCheck, Duration::from_secs(1))
        .await?;
    println!(
        "[main]  health snapshot mid-flight: jobs_done={}, paused={}",
        snap.jobs_done, snap.paused
    );

    // Send a pause signal mid-stream.
    actor
        .tell_priority(PauseProcessing, Duration::from_secs(1))
        .await?;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let snap = actor
        .ask_priority(HealthCheck, Duration::from_secs(1))
        .await?;
    println!(
        "[main]  health snapshot after pause: jobs_done={}, paused={}",
        snap.jobs_done, snap.paused
    );

    // Resume and let the remaining queued jobs drain.
    actor
        .tell_priority(ResumeProcessing, Duration::from_secs(1))
        .await?;

    actor.stop().await;
    let result = join.await?;
    if let Some(final_state) = result.actor() {
        println!(
            "\n[main]  worker stopped. final jobs_done={}, paused={}",
            final_state.jobs_done, final_state.paused
        );
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}
