// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Metrics Demo - Actor Performance Monitoring
//!
//! This example demonstrates the metrics system for tracking actor performance:
//! - Message count
//! - Processing time (average and maximum)
//! - Error count
//! - Uptime
//! - Last activity timestamp
//!
//! Run with: `cargo run --example metrics_demo --features metrics`
//! With tracing: `RUST_LOG=debug cargo run --example metrics_demo --features "metrics tracing"`

use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::time::Duration;

#[derive(Actor)]
struct WorkerActor {
    name: String,
}

// Messages
struct FastTask;
struct SlowTask {
    delay_ms: u64,
}
struct GetName;

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_fast_task(&mut self, _msg: FastTask, _: &ActorRef<Self>) {
        // Quick task - minimal processing
    }

    #[handler]
    async fn handle_slow_task(&mut self, msg: SlowTask, _: &ActorRef<Self>) {
        // Simulate slow processing
        tokio::time::sleep(Duration::from_millis(msg.delay_ms)).await;
    }

    #[handler]
    async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> String {
        self.name.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing if enabled
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("=== rsactor Metrics Demo ===\n");

    // Spawn an actor
    let actor = WorkerActor {
        name: "Worker-1".to_string(),
    };
    let (actor_ref, handle) = spawn::<WorkerActor>(actor);

    // Check initial metrics
    #[cfg(feature = "metrics")]
    {
        let metrics = actor_ref.metrics();
        println!("Initial metrics:");
        println!("  Message count: {}", metrics.message_count);
        println!("  Uptime: {:?}", metrics.uptime);
        println!("  Last activity: {:?}\n", metrics.last_activity);
    }

    // Send some fast messages
    println!("Sending 10 fast tasks...");
    for _ in 0..10 {
        actor_ref.tell(FastTask).await?;
    }

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(50)).await;

    #[cfg(feature = "metrics")]
    {
        println!("\nAfter fast tasks:");
        println!("  Message count: {}", actor_ref.message_count());
        println!(
            "  Avg processing time: {:?}",
            actor_ref.avg_processing_time()
        );
        println!(
            "  Max processing time: {:?}",
            actor_ref.max_processing_time()
        );
    }

    // Send some slow messages
    println!("\nSending 3 slow tasks (50ms, 100ms, 150ms)...");
    actor_ref.tell(SlowTask { delay_ms: 50 }).await?;
    actor_ref.tell(SlowTask { delay_ms: 100 }).await?;
    actor_ref.tell(SlowTask { delay_ms: 150 }).await?;

    // Wait for slow tasks to complete
    tokio::time::sleep(Duration::from_millis(400)).await;

    #[cfg(feature = "metrics")]
    {
        let metrics = actor_ref.metrics();
        println!("\nFinal metrics snapshot:");
        println!("  Message count: {}", metrics.message_count);
        println!("  Avg processing time: {:?}", metrics.avg_processing_time);
        println!("  Max processing time: {:?}", metrics.max_processing_time);
        println!("  Error count: {}", metrics.error_count);
        println!("  Uptime: {:?}", metrics.uptime);
        println!("  Last activity: {:?}", metrics.last_activity);

        // Demonstrate individual accessors
        println!("\nUsing individual accessor methods:");
        println!(
            "  actor_ref.message_count() = {}",
            actor_ref.message_count()
        );
        println!("  actor_ref.uptime() = {:?}", actor_ref.uptime());
    }

    // Test ask pattern with metrics
    let name = actor_ref.ask(GetName).await?;
    println!("\nActor name (via ask): {}", name);

    #[cfg(feature = "metrics")]
    println!("Message count after ask: {}", actor_ref.message_count());

    // Graceful shutdown
    actor_ref.stop().await?;
    let result = handle.await?;

    println!("\nActor completed: {:?}", result.is_completed());
    println!("\n=== Demo Complete ===");

    #[cfg(not(feature = "metrics"))]
    println!("\nNote: Run with --features metrics to see metrics output");

    Ok(())
}
