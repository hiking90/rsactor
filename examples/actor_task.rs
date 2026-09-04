// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor-Task Communication Example
//!
//! This example demonstrates how to:
//! 1. Spawn an async background task from an actor's on_start lifecycle method
//! 2. Send commands from the actor to the background task using tokio's mpsc::channel
//! 3. Send data from the background task back to the actor using actor messages (`tell`)
//! 4. Shut the task down from `on_stop` so it never outlives the actor
//!
//! This is the async counterpart of `actor_blocking_task.rs`, which drives the same
//! pattern from a synchronous `spawn_blocking` task. If you only need periodic work
//! inside the actor itself — with no separate task and no channel — use the
//! stream-based idle handler instead (see `basic.rs`).

use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{debug, info};

// Define message types for our actor

/// Message to get the current state of the actor
struct GetState;

/// Message to change the processing factor
struct SetFactor(f64);

/// Message sent from the background task to the actor with generated data
struct ProcessedData {
    value: f64,
    timestamp: std::time::Instant,
}

/// Commands that the actor can send to the background task
enum TaskCommand {
    /// Change the interval between data generations
    ChangeInterval(Duration),
    /// Stop the background task
    Stop,
}

/// Message asking the actor to relay a command to its background task
struct SendTaskCommand(TaskCommand);

/// Define our actor, which owns an async background task spawned in `on_start`.
struct DataProcessorActor {
    /// Current processing factor (multiplier for incoming values)
    factor: f64,
    /// Latest processed value received from the task
    latest_value: Option<f64>,
    /// Latest timestamp when data was received
    latest_timestamp: Option<std::time::Instant>,
    /// Sender used to command the background task
    task_sender: mpsc::Sender<TaskCommand>,
    /// Handle of the background task, so callers can await its completion
    task_handle: task::JoinHandle<()>,
}

impl Actor for DataProcessorActor {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!(
            "DataProcessorActor (id: {}) starting...",
            actor_ref.identity()
        );

        // Channel for actor -> task communication.
        let (task_tx, mut task_rx) = mpsc::channel::<TaskCommand>(32);

        // Clone the actor_ref so the task can send messages back to the actor.
        let task_actor_ref = actor_ref.clone();

        // Spawn the async background task.
        let task_handle = task::spawn(async move {
            info!("Background task started");

            let mut interval = tokio::time::interval(Duration::from_millis(500));

            loop {
                tokio::select! {
                    // Periodically generate a value and send it to the actor.
                    _ = interval.tick() => {
                        let raw_value = rand::random::<f64>() * 100.0;
                        debug!("Task sending value {raw_value:.2} to actor");

                        if let Err(e) = task_actor_ref
                            .tell(ProcessedData {
                                value: raw_value,
                                timestamp: std::time::Instant::now(),
                            })
                            .await
                        {
                            info!("Failed to send data to actor: {e}");
                            break;
                        }
                    }

                    // Handle commands coming from the actor.
                    cmd = task_rx.recv() => match cmd {
                        Some(TaskCommand::ChangeInterval(new_interval)) => {
                            info!("Task changing interval to {new_interval:?}");
                            interval = tokio::time::interval(new_interval);
                        }
                        Some(TaskCommand::Stop) => {
                            info!("Task received stop command");
                            break;
                        }
                        None => {
                            info!("Task command channel closed, stopping task");
                            break;
                        }
                    },
                }
            }

            info!("Background task stopping");
        });

        info!("DataProcessorActor started and background task spawned");
        Ok(Self {
            factor: 1.0,
            latest_value: None,
            latest_timestamp: None,
            task_sender: task_tx,
            task_handle,
        })
    }

    async fn on_stop(&mut self, _actor_weak: &ActorWeak<Self>, _killed: bool) -> Result<()> {
        // Ask the task to stop. It also exits on its own if this channel is
        // dropped, which is what happens when the actor is killed before
        // `on_stop` can run to completion.
        let _ = self.task_sender.send(TaskCommand::Stop).await;
        Ok(())
    }
}

// Implement message handlers for our actor
#[message_handlers]
impl DataProcessorActor {
    #[handler]
    async fn handle_get_state(
        &mut self,
        _msg: GetState,
        _: &ActorRef<Self>,
    ) -> (f64, Option<f64>, Option<std::time::Instant>) {
        (self.factor, self.latest_value, self.latest_timestamp)
    }

    #[handler]
    async fn handle_set_factor(&mut self, msg: SetFactor, _: &ActorRef<Self>) -> f64 {
        let old_factor = self.factor;
        self.factor = msg.0;
        info!(
            "Changed factor from {:.2} to {:.2}",
            old_factor, self.factor
        );
        self.factor
    }

    #[handler]
    async fn handle_processed_data(&mut self, msg: ProcessedData, _: &ActorRef<Self>) {
        // Apply our processing factor to the incoming value
        let processed_value = msg.value * self.factor;

        // Update our state
        self.latest_value = Some(processed_value);
        self.latest_timestamp = Some(msg.timestamp);

        debug!(
            "Received data from task: original={:.2}, processed={:.2}, age={:?}",
            msg.value,
            processed_value,
            msg.timestamp.elapsed()
        );
    }

    #[handler]
    async fn handle_send_task_command(&mut self, msg: SendTaskCommand, _: &ActorRef<Self>) -> bool {
        match self.task_sender.send(msg.0).await {
            Ok(()) => {
                info!("Sent command to background task");
                true
            }
            Err(_) => {
                info!("Failed to send command to background task");
                false
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    info!("Starting actor-task communication example");

    // Create and spawn our actor. The background task is spawned inside `on_start`.
    let (actor_ref, join_handle) = rsactor::spawn::<DataProcessorActor>(());

    // Wait a bit to get some initial data
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get the current state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) =
        actor_ref.ask(GetState).await?;
    println!("Current state: factor={factor:.2}, latest_value={latest_value:?}");

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    // Change the processing factor
    println!("Changing processing factor to 2.5...");
    let new_factor: f64 = actor_ref.ask(SetFactor(2.5)).await?;
    println!("Factor changed to: {new_factor:.2}");

    // Change the task's data generation interval
    println!("Changing the task's data generation interval...");

    // Now we can send our command via actor messaging
    let command_result: bool = actor_ref
        .ask(SendTaskCommand(TaskCommand::ChangeInterval(
            Duration::from_millis(200),
        )))
        .await?;

    if command_result {
        println!("Successfully changed task interval");
    } else {
        println!("Failed to change task interval");
    }

    // Wait a bit more to collect data with the new parameters
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Get the updated state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) =
        actor_ref.ask(GetState).await?;
    println!("Updated state: factor={factor:.2}, latest_value={latest_value:?}");

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    // Stop the actor gracefully. `on_stop` tells the background task to stop.
    println!("Stopping actor...");
    actor_ref.stop().await;

    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed successfully. Killed: {killed}");
            println!(
                "Final state: factor={:.2}, latest_value={:?}",
                actor.factor, actor.latest_value
            );
            // The actor is returned by value, so we can await its task here.
            actor.task_handle.await.expect("Failed to join task handle");
        }
        rsactor::ActorResult::Failed { failure, killed } => {
            println!(
                "Actor stop failed: {}. Phase: {}, Killed: {killed}",
                failure.error(),
                failure.phase()
            );
            if let Some(actor) = failure.actor() {
                println!(
                    "Final state: factor={:.2}, latest_value={:?}",
                    actor.factor, actor.latest_value
                );
            }
        }
    }

    Ok(())
}
