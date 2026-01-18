// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Example demonstrating the different termination scenarios with tracing.

use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};
use tracing::info;

// Simple actor for demonstration
#[derive(Debug)]
struct DemoActor {
    name: String,
}

impl Actor for DemoActor {
    type Args = String;
    type Error = anyhow::Error;

    async fn on_start(
        args: Self::Args,
        actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        info!(
            "DemoActor '{}' (id: {}) started!",
            args,
            actor_ref.identity()
        );
        Ok(DemoActor { name: args })
    }

    async fn on_stop(
        &mut self,
        actor_ref: &ActorWeak<Self>,
        killed: bool,
    ) -> std::result::Result<(), Self::Error> {
        let status = if killed { "killed" } else { "stopped" };
        info!(
            "DemoActor '{}' (id: {}) {} gracefully",
            self.name,
            actor_ref.identity(),
            status
        );
        Ok(())
    }
}

// Message types
#[derive(Debug)]
struct Ping;

// Message implementations
#[message_handlers]
impl DemoActor {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) -> String {
        format!("{} pong!", self.name)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("=== Kill Termination Demo ===\n");

    // Spawn an actor
    let (actor_ref, join_handle) = rsactor::spawn::<DemoActor>("KillTestActor".to_string());

    // Send a message first
    let response: String = actor_ref.ask(Ping).await?;
    println!("Response: {response}");

    println!("Calling kill() on the actor...");

    // Kill the actor (this should trigger the kill scenario)
    actor_ref.kill()?;

    // Wait for the actor to complete
    println!("Waiting for actor to terminate...");
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor '{}' completed. Killed: {}", actor.name, killed);
        }
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            println!(
                "Actor failed: {}. Phase: {:?}, Killed: {}, Final name: {}",
                error,
                phase,
                killed,
                actor
                    .as_ref()
                    .map(|a| &a.name)
                    .unwrap_or(&"Unknown".to_string())
            );
        }
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}
