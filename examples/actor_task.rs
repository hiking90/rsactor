// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor-Task Communication Example
//!
//! This example demonstrates how to:
//! 1. Spawn a background task from an actor's on_start lifecycle method
//! 2. Send data from the actor to the background task using mpsc::channel
//! 3. Send data from the background task back to the actor using actor messages

use rsactor::{Actor, ActorRef, Message};
use anyhow::Result;
use log::{info, debug};
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

/// Commands that the actor can send to update its event processing
enum TaskCommand {
    /// Change the interval between data generations
    ChangeInterval(Duration),
}

/// Message to send a command to the background task
struct SendTaskCommand(TaskCommand);

/// Define our actor that will use await_next_event/on_event pattern
struct DataProcessorActor {
    /// Current processing factor (multiplier for incoming values)
    factor: f64,
    /// Latest processed value received from the task
    latest_value: Option<f64>,
    /// Latest timestamp when data was received
    latest_timestamp: Option<std::time::Instant>,
    /// Interval for generating data
    interval: tokio::time::Interval,
    /// Whether the actor is running
    running: bool,
}

impl Actor for DataProcessorActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, actor_ref: &ActorRef) -> Result<Self, Self::Error> {
        info!("DataProcessorActor (id: {}) starting...", actor_ref.identity());

        let mut actor = Self {
            factor: 1.0,
            latest_value: None,
            latest_timestamp: None,
            interval: tokio::time::interval(Duration::from_millis(500)),
            running: true,
        };

        // Reset the interval to ensure it starts ticking from now
        actor.interval = tokio::time::interval(Duration::from_millis(500));
        actor.running = true;

        info!("DataProcessorActor started with event-based processing");
        Ok(actor)
    }

    async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
        loop {
            self.interval.tick().await;

            // Generate a random value (simulating sensor data or similar)
            let raw_value = rand::random::<f64>() * 100.0;

            // Process the data directly (no need to send to self via message)
            let processed_value = raw_value * self.factor;

            // Update our state
            self.latest_value = Some(processed_value);
            self.latest_timestamp = Some(std::time::Instant::now());

            debug!(
                "Generated data: original={:.2}, processed={:.2}",
                raw_value,
                processed_value
            );
        }
    }
}

// Implement message handlers for our actor

impl Message<GetState> for DataProcessorActor {
    type Reply = (f64, Option<f64>, Option<std::time::Instant>);

    async fn handle(&mut self, _msg: GetState, _: &ActorRef) -> Self::Reply {
        (self.factor, self.latest_value, self.latest_timestamp)
    }
}

impl Message<SetFactor> for DataProcessorActor {
    type Reply = f64; // Return the new factor

    async fn handle(&mut self, msg: SetFactor, _: &ActorRef) -> Self::Reply {
        let old_factor = self.factor;
        self.factor = msg.0;
        info!("Changed factor from {:.2} to {:.2}", old_factor, self.factor);
        self.factor
    }
}

impl Message<ProcessedData> for DataProcessorActor {
    type Reply = (); // No reply needed for data coming from the task

    async fn handle(&mut self, msg: ProcessedData, _: &ActorRef) -> Self::Reply {
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

// Handler for sending commands to the actor's event system
impl Message<SendTaskCommand> for DataProcessorActor {
    type Reply = bool;

    async fn handle(&mut self, msg: SendTaskCommand, _actor_ref: &ActorRef) -> Self::Reply {
        match msg.0 {
            TaskCommand::ChangeInterval(new_interval) => {
                self.interval = tokio::time::interval(new_interval);
                info!("Successfully changed interval");
                true
            }
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
    let (actor_ref, join_handle) = rsactor::spawn::<DataProcessorActor>(());

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
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed successfully. Killed: {}", killed);
            println!("Final state: factor={:.2}, latest_value={:?}",
                     actor.factor, actor.latest_value);
        }
        rsactor::ActorResult::StartupFailed { cause } => {
            println!("Actor failed to start: {}", cause);
        }
        rsactor::ActorResult::RuntimeFailed { actor, cause } => {
            println!("Actor failed at runtime: {}", cause);
            if let Some(actor) = actor {
                println!("Final state: factor={:.2}, latest_value={:?}",
                         actor.factor, actor.latest_value);
            }
        }
    }

    Ok(())
}
