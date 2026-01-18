// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tracing Feature Demonstration
//!
//! This example demonstrates the tracing capabilities of rsActor when the "tracing" feature is enabled.
//!
//! To run this example with tracing enabled:
//! ```bash
//! cargo run --example tracing_demo --features tracing
//! ```
//!
//! The example shows:
//! 1. Message sending with tell/ask operations
//! 2. Actor message processing
//! 3. Reply handling
//! 4. Error handling
//! 5. Actor lifecycle events
//!
//! When tracing is enabled, you'll see structured logs showing:
//! - Message type information
//! - Actor IDs
//! - Processing duration
//! - Success/error states
//! - Reply handling

use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::time::Duration;

// Message types for demonstration
#[derive(Debug)]
struct Ping;

#[derive(Debug)]
struct Echo(String);

#[derive(Debug)]
struct GetCounter;

#[derive(Debug)]
struct Increment;

#[derive(Debug)]
struct SlowOperation(u64); // Processing time in milliseconds

#[derive(Debug)]
struct FailingMessage;

// Actor implementation
#[derive(Actor)]
struct TracingDemoActor {
    counter: u64,
    name: String,
}

#[message_handlers]
impl TracingDemoActor {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) -> String {
        format!("Pong from {}", self.name)
    }

    #[handler]
    async fn handle_echo(&mut self, msg: Echo, _: &ActorRef<Self>) -> String {
        format!("{} echoes: {}", self.name, msg.0)
    }

    #[handler]
    async fn handle_get_counter(&mut self, _msg: GetCounter, _: &ActorRef<Self>) -> u64 {
        self.counter
    }

    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> String {
        self.counter += 1;
        format!("Counter incremented to {}", self.counter)
    }

    #[handler]
    async fn handle_slow_operation(&mut self, msg: SlowOperation, _: &ActorRef<Self>) -> String {
        // Simulate slow processing
        tokio::time::sleep(Duration::from_millis(msg.0)).await;
        self.counter += 1;
        format!(
            "Slow operation completed after {}ms, counter: {}",
            msg.0, self.counter
        )
    }

    #[handler]
    async fn handle_failing_message(
        &mut self,
        _msg: FailingMessage,
        _: &ActorRef<Self>,
    ) -> Result<String, String> {
        // Simulate an error
        Err("This message always fails for demonstration".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Create and spawn actor
    let actor = TracingDemoActor {
        counter: 0,
        name: "DemoActor".to_string(),
    };

    let (actor_ref, _handle) = spawn::<TracingDemoActor>(actor);

    println!("1. Testing ping message (ask)...");
    let response: String = actor_ref.ask(Ping).await?;
    println!("Response: {response}\n");

    println!("2. Testing echo message (ask)...");
    let response: String = actor_ref.ask(Echo("Hello World!".to_string())).await?;
    println!("Response: {response}\n");

    println!("3. Testing increment (ask for responses)...");
    let response1: String = actor_ref.ask(Increment).await?;
    let response2: String = actor_ref.ask(Increment).await?;
    let response3: String = actor_ref.ask(Increment).await?;
    println!("Increment responses: {response1}, {response2}, {response3}\n");

    println!("4. Getting counter value...");
    let counter: u64 = actor_ref.ask(GetCounter).await?;
    println!("Counter value: {counter}\n");

    println!("5. Testing slow operation...");
    let response: String = actor_ref.ask(SlowOperation(100)).await?;
    println!("Response: {response}\n");

    println!("6. Testing ask with timeout...");
    match actor_ref
        .ask_with_timeout(SlowOperation(200), Duration::from_millis(150))
        .await
    {
        Ok(response) => println!("Response: {response}"),
        Err(e) => println!("Timeout occurred as expected: {e}\n"),
    }

    println!("7. Testing error handling...");
    match actor_ref.ask(FailingMessage).await {
        Ok(response) => println!("Unexpected success: {response:?}"),
        Err(e) => println!("Error handled correctly: {e}\n"),
    }

    println!("8. Testing tell with timeout...");
    actor_ref
        .tell_with_timeout(Increment, Duration::from_millis(100))
        .await?;
    println!("Tell with timeout completed\n");

    println!("9. Final counter check...");
    let final_counter: u64 = actor_ref.ask(GetCounter).await?;
    println!("Final counter value: {final_counter}\n");

    println!("10. Gracefully stopping actor...");
    actor_ref.stop().await?;
    println!("Actor stopped successfully");

    #[cfg(feature = "tracing")]
    println!("\n✅ Demo completed! Check the trace logs above to see the complete message lifecycle tracking.");

    #[cfg(not(feature = "tracing"))]
    println!("\n✅ Demo completed! Run with --features tracing to see detailed message tracking.");

    Ok(())
}
