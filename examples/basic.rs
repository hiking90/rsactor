// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use futures::stream::StreamExt;
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};
use tokio::time::{interval, Duration};
use tokio_stream::wrappers::IntervalStream;
use tracing::info;

// Message types
struct Increment; // Message to increment the actor's counter
struct Decrement; // Message to decrement the actor's counter

/// Periodic idle events delivered by the streams subscribed in `on_start`.
#[derive(Debug, Clone, Copy)]
enum Tick {
    Fast, // every 300ms
    Slow, // every 1s
}

// Define the actor struct
struct MyActor {
    count: u32,                   // Internal state of the actor
    start_up: std::time::Instant, // Optional field to track the start time
}

// Implement the Actor trait for MyActor
impl Actor for MyActor {
    type Args = u32; // initial count
    type Error = anyhow::Error;
    type IdleEvent = Tick;

    // Called when the actor is started. Subscribes the two periodic streams that
    // drive the actor's idle work. Stream state (the next-tick instant) lives in
    // the runtime's `SelectAll`, so ticks are never cancelled by message arrivals.
    async fn on_start(
        initial: Self::Args,
        actor_ref: &ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        info!("MyActor started. Initial count: {}.", initial);

        actor_ref.subscribe_idle(
            IntervalStream::new(interval(Duration::from_millis(300))).map(|_| Tick::Fast),
        )?;
        actor_ref.subscribe_idle(
            IntervalStream::new(interval(Duration::from_secs(1))).map(|_| Tick::Slow),
        )?;

        Ok(MyActor {
            count: initial,
            start_up: std::time::Instant::now(),
        })
    }

    async fn on_idle(&mut self, event: Tick, _: &ActorWeak<Self>) -> Result<(), Self::Error> {
        match event {
            Tick::Fast => println!("300ms tick. Elapsed: {:?}", self.start_up.elapsed()),
            Tick::Slow => println!("1s tick. Elapsed: {:?}", self.start_up.elapsed()),
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("Spawning MyActor...");

    // Spawn the actor. This returns an ActorRef for sending messages
    // and a JoinHandle to await the actor's completion. The initial counter
    // value is passed via `Args`; the actor's idle streams are subscribed
    // inside `on_start` so they outlive `select!` cancellation.
    let (actor_ref, join_handle) = rsactor::spawn::<MyActor>(100);

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
    println!("Sending StopGracefully message to actor.",);
    actor_ref.stop().await?; // Corrected method name

    // Wait for the actor's task to complete.
    // `join_handle.await` returns a Result containing a tuple:
    // - The actor instance (allowing access to its final state).
    // - The ActorResult indicating completion state and returned actor.
    println!("Waiting for actor to stop...");
    let result = join_handle.await?;
    // Successfully retrieved the actor result.
    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!(
                "Actor stopped. Final count: {}. Killed: {}",
                actor.count, killed
            );
        }
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            println!(
                "Actor stop failed: {}. Phase: {}, Killed: {}. Final count: {}",
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
