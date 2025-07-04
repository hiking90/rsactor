// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor-Task Communication Example
//!
//! This example demonstrates how to:
//! 1. Spawn a background task from an actor's on_start lifecycle method
//! 2. Send data from the actor to the background task using mpsc::channel
//! 3. Send data from the background task back to the actor using actor messages

use anyhow::Result;
use log::{debug, info};
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};
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

    async fn on_start(_args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!(
            "DataProcessorActor (id: {}) starting...",
            actor_ref.identity()
        );

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

    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        loop {
            self.interval.tick().await;

            // Generate a random value (simulating sensor data or similar)
            let raw_value = rand::random::<f64>() * 100.0;

            // Process the data directly (no need to send to self via message)
            let processed_value = raw_value * self.factor;

            // Update our state
            self.latest_value = Some(processed_value);
            self.latest_timestamp = Some(std::time::Instant::now());

            debug!("Generated data: original={raw_value:.2}, processed={processed_value:.2}");
        }
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
    async fn handle_send_task_command(
        &mut self,
        msg: SendTaskCommand,
        _actor_ref: &ActorRef<Self>,
    ) -> bool {
        match msg.0 {
            TaskCommand::ChangeInterval(new_interval) => {
                self.interval = tokio::time::interval(new_interval);
                info!("Successfully changed interval");
                true
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing if the feature is enabled
    #[cfg(feature = "tracing")]
    {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .init();
        println!("🚀 Actor Task Demo: Tracing is ENABLED");
        println!("You should see detailed trace logs for all actor operations\n");
    }

    #[cfg(not(feature = "tracing"))]
    {
        // Initialize logger with debug level for our example
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
        println!("📝 Actor Task Demo: Tracing is DISABLED\n");
    }

    info!("Starting actor-task communication example");

    // Create and spawn our actor
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
    let command_result = actor_ref
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

    // Stop the actor gracefully
    println!("Stopping actor...");
    actor_ref.stop().await?;

    // Wait for the actor to finish (this will also wait for the background task)
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed successfully. Killed: {killed}");
            println!(
                "Final state: factor={:.2}, latest_value={:?}",
                actor.factor, actor.latest_value
            );
        }
        rsactor::ActorResult::Failed {
            actor,
            error,
            killed,
            phase,
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
