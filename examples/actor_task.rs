// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor-Task Communication Example
//!
//! This example demonstrates how to:
//! 1. Spawn a background task from an actor's on_start lifecycle method
//! 2. Send data from the actor to the background task using mpsc::channel
//! 3. Send data from the background task back to the actor using actor messages

use rsactor::{Actor, ActorRef, ActorStopReason, Message};
use anyhow::Result;
use log::{info, debug};
use tokio::sync::mpsc;
use std::time::Duration;

// Define message types for our actor

/// Message to get the current state of the actor
struct GetState;

/// Message to change the processing factor
struct SetFactor(f64);

/// Message sent from the background task to the actor with processed data
struct ProcessedData {
    value: f64,
    timestamp: std::time::Instant,
}

/// Commands that the actor can send to the background task
enum TaskCommand {
    /// Change the interval between data generations
    ChangeInterval(Duration),
    /// Request to stop the task (we'll use this for a clean shutdown)
    Stop,
}

/// Message to send a command to the background task
struct SendTaskCommand(TaskCommand);

/// Define our actor that will spawn a background task
struct DataProcessorActor {
    /// Current processing factor (multiplier for incoming values)
    factor: f64,
    /// Latest processed value received from the task
    latest_value: Option<f64>,
    /// Latest timestamp when data was received
    latest_timestamp: Option<std::time::Instant>,
    /// Sender to communicate with the background task
    task_sender: Option<mpsc::Sender<TaskCommand>>,
    /// Task handle to monitor/join the background task
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl DataProcessorActor {
    fn new() -> Self {
        Self {
            factor: 1.0,
            latest_value: None,
            latest_timestamp: None,
            task_sender: None,
            task_handle: None,
        }
    }
}

impl Actor for DataProcessorActor {
    type Error = anyhow::Error;

    async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
        info!("DataProcessorActor (id: {}) starting...", actor_ref.id());

        // Create a channel for actor -> task communication
        let (task_tx, mut task_rx) = mpsc::channel::<TaskCommand>(32);

        // Store the sender in our actor state
        self.task_sender = Some(task_tx);

        // Clone the actor_ref for the task to send messages back
        let task_actor_ref = actor_ref.clone();

        // Spawn a background task
        let handle = tokio::spawn(async move {
            info!("Background task started");

            // Initial data generation interval
            let mut interval = tokio::time::interval(Duration::from_millis(500));

            // Task loop
            let mut running = true;
            while running {
                tokio::select! {
                    // Time to generate a new data point
                    _ = interval.tick() => {
                        // Generate a random value (simulating sensor data or similar)
                        let raw_value = rand::random::<f64>() * 100.0;

                        // Send the data to our actor
                        debug!("Task sending value {:.2} to actor", raw_value);
                        if let Err(e) = task_actor_ref.tell(ProcessedData {
                            value: raw_value,
                            timestamp: std::time::Instant::now(),
                        }).await {
                            info!("Failed to send data to actor: {}", e);
                            running = false;
                        }
                    }

                    // Check for commands from the actor
                    Some(cmd) = task_rx.recv() => {
                        match cmd {
                            TaskCommand::ChangeInterval(new_interval) => {
                                info!("Task changing interval to {:?}", new_interval);
                                // Recreate the interval with the new duration
                                interval = tokio::time::interval(new_interval);
                            }
                            TaskCommand::Stop => {
                                info!("Task received stop command");
                                running = false;
                            }
                        }
                    }
                }
            }

            info!("Background task stopping");
        });

        // Store the task handle
        self.task_handle = Some(handle);

        info!("DataProcessorActor started and background task spawned");
        Ok(())
    }

    async fn on_stop(&mut self, _actor_ref: ActorRef, stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
        info!("DataProcessorActor stopping: {:?}", stop_reason);

        // Signal the background task to stop if we still have a sender
        if let Some(sender) = self.task_sender.take() {
            info!("Sending stop command to background task");
            // Use try_send as we're in shutdown and don't want to await
            let _ = sender.try_send(TaskCommand::Stop);
        }

        // Wait for the task to complete if we have a handle
        // This ensures clean shutdown of the background task
        if let Some(handle) = self.task_handle.take() {
            info!("Waiting for background task to complete");
            // Use a timeout to avoid blocking forever if the task is stuck
            match tokio::time::timeout(Duration::from_secs(2), handle).await {
                Ok(result) => {
                    if let Err(e) = result {
                        info!("Background task ended with error: {}", e);
                    } else {
                        info!("Background task completed successfully");
                    }
                }
                Err(_) => {
                    info!("Timeout waiting for background task to complete");
                }
            }
        }

        info!("DataProcessorActor stopped with latest value: {:?}", self.latest_value);
        Ok(())
    }
}

// Implement message handlers for our actor

impl Message<GetState> for DataProcessorActor {
    type Reply = (f64, Option<f64>, Option<std::time::Instant>);

    async fn handle(&mut self, _msg: GetState) -> Self::Reply {
        (self.factor, self.latest_value, self.latest_timestamp)
    }
}

impl Message<SetFactor> for DataProcessorActor {
    type Reply = f64; // Return the new factor

    async fn handle(&mut self, msg: SetFactor) -> Self::Reply {
        let old_factor = self.factor;
        self.factor = msg.0;
        info!("Changed factor from {:.2} to {:.2}", old_factor, self.factor);
        self.factor
    }
}

impl Message<ProcessedData> for DataProcessorActor {
    type Reply = (); // No reply needed for data coming from the task

    async fn handle(&mut self, msg: ProcessedData) -> Self::Reply {
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
}

// Handler for sending commands to the background task
impl Message<SendTaskCommand> for DataProcessorActor {
    type Reply = bool;

    async fn handle(&mut self, msg: SendTaskCommand) -> Self::Reply {
        if let Some(sender) = &self.task_sender {
            match sender.send(msg.0).await {
                Ok(_) => {
                    info!("Sent command to task");
                    true
                }
                Err(_) => {
                    info!("Failed to send command to task");
                    false
                }
            }
        } else {
            info!("No task sender available");
            false
        }
    }
}

// Implement the message handler trait for our actor
rsactor::impl_message_handler!(DataProcessorActor, [GetState, SetFactor, ProcessedData, SendTaskCommand]);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with debug level for our example
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    info!("Starting actor-task communication example");

    // Create and spawn our actor
    let actor = DataProcessorActor::new();
    let (actor_ref, join_handle) = rsactor::spawn(actor);

    // Wait a bit to get some initial data
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get the current state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) = actor_ref.ask(GetState).await?;
    println!("Current state: factor={:.2}, latest_value={:?}",
             factor, latest_value);

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    // Change the processing factor
    println!("Changing processing factor to 2.5...");
    let new_factor: f64 = actor_ref.ask(SetFactor(2.5)).await?;
    println!("Factor changed to: {:.2}", new_factor);

    // Change the task's data generation interval
    println!("Changing the task's data generation interval...");

    // Now we can send our command via actor messaging
    let command_result = actor_ref.ask(SendTaskCommand(
        TaskCommand::ChangeInterval(Duration::from_millis(200))
    )).await?;

    if command_result {
        println!("Successfully changed task interval");
    } else {
        println!("Failed to change task interval");
    }

    // Wait a bit more to collect data with the new parameters
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Get the updated state
    let (factor, latest_value, timestamp): (f64, Option<f64>, Option<std::time::Instant>) = actor_ref.ask(GetState).await?;
    println!("Updated state: factor={:.2}, latest_value={:?}",
             factor, latest_value);

    if let Some(ts) = timestamp {
        println!("Data age: {:?}", ts.elapsed());
    }

    // Stop the actor gracefully
    println!("Stopping actor...");
    actor_ref.stop().await?;

    // Wait for the actor to finish (this will also wait for the background task)
    let (actor, stop_reason) = join_handle.await?;
    println!("Actor stopped with reason: {:?}", stop_reason);

    // We can access the final state of the actor
    println!("Final state: factor={:.2}, latest_value={:?}",
             actor.factor, actor.latest_value);

    Ok(())
}
