// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Example demonstrating the usage of UntypedActorWeak for weak references to actors.
//!
//! This example shows:
//! - Creating weak references from strong actor references
//! - Upgrading weak references back to strong references
//! - Checking if actors are still alive through weak references
//! - Proper cleanup when actors are dropped

use anyhow::Error as AnyError;
use rsactor::{
    impl_message_handler, spawn, Actor, ActorRef, ActorWeak, Message, Result, UntypedActorRef,
};
use std::time::Duration;
use tokio::time::sleep;

// Simple actor that responds to ping messages
#[derive(Debug)]
struct PingActor {
    name: String,
    ping_count: usize,
}

impl Actor for PingActor {
    type Args = String;
    type Error = AnyError;

    async fn on_start(
        args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        println!("PingActor '{}' started!", args);
        Ok(PingActor {
            name: args,
            ping_count: 0,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: &ActorWeak<Self>,
        killed: bool,
    ) -> std::result::Result<(), Self::Error> {
        let status = if killed { "killed" } else { "stopped" };
        println!(
            "PingActor '{}' {} after {} pings",
            self.name, status, self.ping_count
        );
        Ok(())
    }
}

// Message types
#[derive(Debug)]
struct Ping;

#[derive(Debug)]
struct GetStatus;

#[derive(Debug)]
struct Status {
    name: String,
    ping_count: usize,
}

// Message implementations
impl Message<Ping> for PingActor {
    type Reply = String;

    async fn handle(&mut self, _msg: Ping, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.ping_count += 1;
        format!("{} pong! (count: {})", self.name, self.ping_count)
    }
}

impl Message<GetStatus> for PingActor {
    type Reply = Status;

    async fn handle(&mut self, _msg: GetStatus, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        Status {
            name: self.name.clone(),
            ping_count: self.ping_count,
        }
    }
}

// Wire up message handlers
impl_message_handler!(PingActor, [Ping, GetStatus]);

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== UntypedActorWeak Demo ===\n");

    // Spawn an actor
    let (actor_ref, join_handle) = spawn::<PingActor>("TestActor".to_string());

    // Create a weak reference
    let weak_ref = UntypedActorRef::downgrade(actor_ref.untyped_actor_ref());
    println!(
        "1. Created weak reference to actor: {}",
        weak_ref.identity()
    );
    println!("   Weak reference is_alive: {}", weak_ref.is_alive());

    // Test upgrade and usage
    if let Some(strong_ref) = weak_ref.upgrade() {
        println!("2. Successfully upgraded weak reference to strong reference");

        // Use the strong reference
        let response: String = strong_ref.ask(Ping).await?;
        println!("   Response: {}", response);
    } else {
        println!("2. Failed to upgrade weak reference - actor is dead");
    }

    // Send more messages using the original reference
    let response1: String = actor_ref.ask(Ping).await?;
    let response2: String = actor_ref.ask(Ping).await?;
    println!("3. Sent more pings: '{}', '{}'", response1, response2);

    // Test weak reference after some activity
    if let Some(strong_ref) = weak_ref.upgrade() {
        let status: Status = strong_ref.ask(GetStatus).await?;
        println!(
            "4. Actor status via weak ref: {} has {} pings",
            status.name, status.ping_count
        );
    }

    println!("5. Dropping strong actor reference...");
    drop(actor_ref);

    // Give some time for cleanup
    sleep(Duration::from_millis(100)).await;

    // Check if weak reference can still be upgraded
    println!("6. Checking weak reference after dropping strong reference:");
    println!("   Weak reference is_alive: {}", weak_ref.is_alive());

    if let Some(strong_ref) = weak_ref.upgrade() {
        println!("   Successfully upgraded weak reference after drop");

        // Try to use it
        match strong_ref.ask::<Ping, String>(Ping).await {
            Ok(response) => println!("   Response: {}", response),
            Err(e) => println!("   Error sending message: {}", e),
        }
    } else {
        println!("   Failed to upgrade weak reference - actor is no longer available");
    }

    // Stop the actor via join handle
    println!("7. Stopping actor...");
    if let Ok(actor_result) = join_handle.await {
        println!("   Actor stopped with result: {:?}", actor_result);
    }

    // Final check of weak reference
    println!("8. Final weak reference check:");
    println!("   Weak reference is_alive: {}", weak_ref.is_alive());

    if let Some(_strong_ref) = weak_ref.upgrade() {
        println!("   Unexpected: weak reference can still be upgraded after actor termination");
    } else {
        println!("   Expected: weak reference cannot be upgraded after actor termination");
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}
