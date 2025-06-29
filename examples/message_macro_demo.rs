// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Demo of the #[message_handlers] macro with #[handler] attributes that reduces boilerplate code

use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef};

// Message types
struct GetCount;
struct SetCount(u32);
struct Add(u32);
struct Multiply(u32);
struct Reset;

// Define the actor struct
#[derive(Actor)]
struct Calculator {
    value: u32,
}

// Message handling using the #[message_handlers] macro with #[handler] attributes
// This automatically generates the Message trait implementations
#[message_handlers]
impl Calculator {
    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
        self.value
    }

    #[handler]
    async fn handle_set_count(&mut self, msg: SetCount, _: &ActorRef<Self>) -> u32 {
        self.value = msg.0;
        self.value
    }

    #[handler]
    async fn handle_add(&mut self, msg: Add, _: &ActorRef<Self>) -> u32 {
        self.value += msg.0;
        self.value
    }

    #[handler]
    async fn handle_multiply(&mut self, msg: Multiply, _: &ActorRef<Self>) -> u32 {
        self.value *= msg.0;
        self.value
    }

    #[handler]
    async fn handle_reset(&mut self, _msg: Reset, _: &ActorRef<Self>) -> String {
        let old_value = self.value;
        self.value = 0;
        format!("Reset from {} to {}", old_value, self.value)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧮 Calculator Actor Demo with #[message_handlers] macro");

    let calculator = Calculator { value: 0 };
    let (actor_ref, join_handle) = rsactor::spawn::<Calculator>(calculator);

    println!("📊 Initial count: {}", actor_ref.ask(GetCount).await?);

    println!("➕ Adding 10...");
    let result = actor_ref.ask(Add(10)).await?;
    println!("   Result: {result}");

    println!("✖️  Multiplying by 3...");
    let result = actor_ref.ask(Multiply(3)).await?;
    println!("   Result: {result}");

    println!("📝 Setting to 100...");
    let result = actor_ref.ask(SetCount(100)).await?;
    println!("   Result: {result}");

    println!("📊 Current count: {}", actor_ref.ask(GetCount).await?);

    println!("🔄 Resetting...");
    let reset_msg: String = actor_ref.ask(Reset).await?;
    println!("   {reset_msg}");

    println!("📊 Final count: {}", actor_ref.ask(GetCount).await?);

    actor_ref.stop().await?;
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!(
                "✅ Calculator stopped. Final value: {}. Killed: {}",
                actor.value, killed
            );
        }
        rsactor::ActorResult::Failed { error, .. } => {
            println!("❌ Calculator failed: {error}");
        }
    }

    println!("🎉 Demo completed!");
    Ok(())
}
