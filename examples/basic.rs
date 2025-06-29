// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use log::info; // ADDED
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak}; // MODIFIED: Added message macro, removed Message
use tokio::time::{interval, Duration}; // MODIFIED: Added MissedTickBehavior

// Message types
struct Increment; // Message to increment the actor's counter
struct Decrement; // Message to decrement the actor's counter

// Define the actor struct
struct MyActor {
    count: u32,                        // Internal state of the actor
    start_up: std::time::Instant,      // Optional field to track the start time
    tick_300ms: tokio::time::Interval, // Interval for 300ms ticks
    tick_1s: tokio::time::Interval,    // Interval for 1s ticks
}

// Implement the Actor trait for MyActor
impl Actor for MyActor {
    type Args = Self;
    type Error = anyhow::Error; // Define the error type for actor operations

    // Called when the actor is started
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!(
            "MyActor (id: {}) started. Initial count: {}.",
            actor_ref.identity(),
            args.count
        );
        Ok(args)
    }

    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // Use the tokio::select! macro to handle the first completed asynchronous operation among several.
        tokio::select! {
            // Executes when the 300ms interval timer ticks.
            _ = self.tick_300ms.tick() => {
                println!("300ms tick. Elapsed: {:?}",
                    self.start_up.elapsed()); // Print the current count
            }
            // Executes when the 1s interval timer ticks. (Currently no specific action)
            _ = self.tick_1s.tick() => {
                println!("1s tick. Elapsed: {:?} ",
                    self.start_up.elapsed()); // Print the current count
            }
        }

        Ok(())
    }
}

// A dummy message type for demonstration
struct DummyMessage;

// Message handling using the #[message_handlers] macro with #[handler] attributes
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> u32 {
        self.count += 1;
        println!("MyActor handled Increment. Count is now {}.", self.count);
        self.count
    }

    #[handler]
    async fn handle_decrement(&mut self, _msg: Decrement, _: &ActorRef<Self>) -> u32 {
        self.count -= 1;
        println!("MyActor handled Decrement. Count is now {}.", self.count);
        self.count
    }

    #[handler]
    async fn handle_dummy_message(&mut self, _msg: DummyMessage, _: &ActorRef<Self>) -> u32 {
        println!("MyActor handled DummyMessage. Count is now {}.", self.count);
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize the logger

    println!("Spawning MyActor...");

    let my_actor = MyActor {
        count: 100,
        start_up: std::time::Instant::now(),
        tick_300ms: interval(Duration::from_millis(300)),
        tick_1s: interval(Duration::from_secs(1)),
    };

    // Spawn the actor. This returns an ActorRef for sending messages
    // and a JoinHandle to await the actor's completion.
    let (actor_ref, join_handle) = rsactor::spawn::<MyActor>(my_actor); // MODIFIED: use system.spawn and await
    println!("MyActor spawned with ref: {:?}", actor_ref.identity());

    tokio::time::sleep(Duration::from_millis(700)).await;

    println!("Sending Increment message...");
    // Send an Increment message and await the reply using `ask`.
    let count_after_inc: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment: {count_after_inc}");

    println!("Sending Decrement message...");
    // Send a Decrement message and await the reply.
    let count_after_dec: u32 = actor_ref.ask(Decrement).await?;
    println!("Reply after Decrement: {count_after_dec}");

    println!("Sending Increment message again...");
    // Send another Increment message.
    let count_after_inc_2: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment again: {count_after_inc_2}");

    tokio::time::sleep(Duration::from_millis(700)).await;

    // Signal the actor to stop gracefully.
    // The actor will process any remaining messages in its mailbox before stopping.
    println!(
        "Sending StopGracefully message to actor {}.",
        actor_ref.identity()
    );
    actor_ref.stop().await?; // Corrected method name

    // Wait for the actor's task to complete.
    // `join_handle.await` returns a Result containing a tuple:
    // - The actor instance (allowing access to its final state).
    // - The ActorResult indicating completion state and returned actor.
    println!("Waiting for actor {} to stop...", actor_ref.identity());
    let result = join_handle.await?;
    // Successfully retrieved the actor result.
    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!(
                "Actor {} stopped. Final count: {}. Killed: {}",
                actor_ref.identity(),
                actor.count,
                killed
            );
        }
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            println!(
                "Actor {} stop failed: {}. Phase: {}, Killed: {}. Final count: {}",
                actor_ref.identity(),
                error,
                phase,
                killed,
                actor.as_ref().map(|a| a.count).unwrap_or(0)
            );
        }
    }

    println!("Main function finished.");
    Ok(())
}
