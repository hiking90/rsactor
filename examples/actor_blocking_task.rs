// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor Blocking Task Communication Example
//!
//! This example demonstrates how to:
//! 1. Spawn a synchronous background task from an actor's on_start lifecycle method using spawn_blocking
//! 2. Send data from the actor to the sync task using tokio channels
//! 3. Send data from the sync task back to the actor using actor messages with the blocking API
//!
//! IMPORTANT: The blocking functions (ask_blocking and tell_blocking) are specifically designed
//! to be used within tokio::task::spawn_blocking tasks where a Tokio runtime is accessible.
//! They are NOT intended for use in general synchronous code or threads created with std::thread::spawn.

use anyhow::Result;
use log::{debug, info};
use rsactor::{Actor, ActorRef, Message};
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc; // Using tokio channels for communication
use tokio::task;

// Define message types for our actor

/// Message to get the current state of the actor
struct GetState;

/// Message to change the processing factor
struct SetFactor(f64);

/// Message sent from the sync background task to the actor with processed data
struct ProcessedData {
    value: f64,
    timestamp: std::time::Instant,
}

/// Commands that the actor can send to the sync background task
enum TaskCommand {
    /// Change the processing interval
    ChangeInterval(Duration),
    /// Stop the background task
    Stop,
}

/// Define our actor that will spawn a sync background task
struct SyncDataProcessorActor {
    /// Current processing factor (multiplier for incoming values)
    factor: f64,
    /// Latest processed value received from the task
    latest_value: Option<f64>,
    /// Latest timestamp when data was received
    latest_timestamp: Option<std::time::Instant>,
    /// Sender to communicate with the background task
    task_sender: mpsc::Sender<TaskCommand>,
    /// Task handle to await the background task
    task_handle: task::JoinHandle<()>,
}

impl Actor for SyncDataProcessorActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!(
            "SyncDataProcessorActor (id: {}) starting...",
            actor_ref.identity()
        );

        // Create a tokio channel for actor -> task communication
        // We use a buffer size of 32 for the channel
        let (task_tx, mut task_rx) = mpsc::channel::<TaskCommand>(32);

        // Clone the actor_ref for the task to send messages back
        let task_actor_ref = actor_ref.clone();

        // Spawn a BLOCKING background task using tokio's spawn_blocking
        // This will run in the tokio threadpool for blocking tasks
        // IMPORTANT: The _blocking methods of ActorRef are specifically designed to be used
        // within tokio::task::spawn_blocking tasks, where a tokio runtime is available
        // but we're in a blocking context
        let handle = task::spawn_blocking(move || {
            info!("Synchronous background task started");

            // Initial data generation interval
            let mut interval = Duration::from_millis(500);

            // Task loop
            let mut running = true;
            while running {
                // In a sync context, we use std::thread::sleep instead of tokio::time::sleep
                thread::sleep(interval);

                // Generate a random value (simulating sensor data or similar)
                let raw_value = rand::random::<f64>() * 100.0;

                // Send the data to our actor using tell_blocking
                debug!("Sync task sending value {:.2} to actor", raw_value);

                // Use tell_blocking which is designed for tokio blocking contexts
                // Note: This requires access to a tokio runtime, which is available inside spawn_blocking
                if let Err(e) = task_actor_ref.tell_blocking(
                    ProcessedData {
                        value: raw_value,
                        timestamp: std::time::Instant::now(),
                    },
                    None,
                ) {
                    info!("Failed to send data to actor: {}", e);
                    running = false;
                }

                // Check for commands from the actor using non-blocking try_recv
                // With tokio channels in a blocking context, we use try_recv for non-blocking behavior
                match task_rx.try_recv() {
                    // Command received
                    Ok(cmd) => match cmd {
                        TaskCommand::ChangeInterval(new_interval) => {
                            info!("Sync task changing interval to {:?}", new_interval);
                            interval = new_interval;
                        }
                        TaskCommand::Stop => {
                            info!("Sync task received stop command");
                            running = false;
                        }
                    },
                    // No command available
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // This is the normal case when no commands are available
                    }
                    // Channel closed
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        info!("Task command channel closed, stopping task");
                        running = false;
                    }
                }
            }

            info!("Synchronous background task stopping");
        });

        let actor = Self {
            factor: 1.0,
            latest_value: None,
            latest_timestamp: None,
            task_sender: task_tx,
            task_handle: handle,
        };

        info!("SyncDataProcessorActor started and sync background task spawned");
        Ok(actor)
    }
}

// Implement message handlers for our actor

impl Message<GetState> for SyncDataProcessorActor {
    type Reply = (f64, Option<f64>, Option<std::time::Instant>);

    async fn handle(&mut self, _msg: GetState, _: &ActorRef<Self>) -> Self::Reply {
        (self.factor, self.latest_value, self.latest_timestamp)
    }
}

impl Message<SetFactor> for SyncDataProcessorActor {
    type Reply = f64; // Return the new factor

    async fn handle(&mut self, msg: SetFactor, _: &ActorRef<Self>) -> Self::Reply {
        let old_factor = self.factor;
        self.factor = msg.0;
        info!(
            "Changed factor from {:.2} to {:.2}",
            old_factor, self.factor
        );
        self.factor
    }
}

impl Message<ProcessedData> for SyncDataProcessorActor {
    type Reply = (); // No reply needed for data coming from the task

    async fn handle(&mut self, msg: ProcessedData, _: &ActorRef<Self>) -> Self::Reply {
        // Apply our processing factor to the incoming value
        let processed_value = msg.value * self.factor;

        // Update our state
        self.latest_value = Some(processed_value);
        self.latest_timestamp = Some(msg.timestamp);

        debug!(
            "Received data from sync task: original={:.2}, processed={:.2}, age={:?}",
            msg.value,
            processed_value,
            msg.timestamp.elapsed()
        );
    }
}

// Handler for sending commands to the background task
impl Message<TaskCommand> for SyncDataProcessorActor {
    type Reply = bool;

    async fn handle(&mut self, msg: TaskCommand, _: &ActorRef<Self>) -> Self::Reply {
        // With tokio channels, send is asynchronous
        match self.task_sender.send(msg).await {
            Ok(_) => {
                info!("Sent command to sync task");
                true
            }
            Err(_) => {
                info!("Failed to send command to sync task");
                false
            }
        }
    }
}

// Implement the message handler trait for our actor
rsactor::impl_message_handler!(
    SyncDataProcessorActor,
    [GetState, SetFactor, ProcessedData, TaskCommand]
);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with debug level for our example
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    info!("Starting actor-sync-task communication example");

    // Create and spawn our actor
    let (actor_ref, join_handle) = rsactor::spawn::<SyncDataProcessorActor>(());

    // Wait a bit to get some initial data
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get the current state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) =
        actor_ref.ask(GetState).await?;
    println!(
        "Current state: factor={:.2}, latest_value={:?}",
        factor, latest_value
    );

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    // Change the processing factor
    println!("Changing processing factor to 2.5...");
    let new_factor: f64 = actor_ref.ask(SetFactor(2.5)).await?;
    println!("Factor changed to: {:.2}", new_factor);

    // Change the task's data generation interval
    println!("Changing the sync task's data generation interval...");

    let command_result = actor_ref
        .ask(TaskCommand::ChangeInterval(Duration::from_millis(200)))
        .await?;

    if command_result {
        println!("Successfully changed sync task interval");
    } else {
        println!("Failed to change sync task interval");
    }

    // Wait a bit more to collect data with the new parameters
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Get the updated state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) =
        actor_ref.ask(GetState).await?;
    println!(
        "Updated state: factor={:.2}, latest_value={:?}",
        factor, latest_value
    );

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    actor_ref.ask(TaskCommand::Stop).await?;

    // Stop the actor gracefully
    println!("Stopping actor...");
    actor_ref.stop().await?;

    // Wait for the actor to finish (this will also wait for the background task)
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed successfully. Killed: {}", killed);
            println!(
                "Final state: factor={:.2}, latest_value={:?}",
                actor.factor, actor.latest_value
            );
            actor.task_handle.await.expect("Failed to join task handle");
        }
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            println!("Actor stop failed: {error}. Phase: {phase}, Killed: {killed}");
            if let Some(actor) = actor {
                println!(
                    "Final state: factor={:.2}, latest_value={:?}",
                    actor.factor, actor.latest_value
                );
            }
        }
    }

    Ok(())
}
