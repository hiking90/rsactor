// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Handler Traits Demo
//!
//! This example demonstrates how to use the handler traits (`TellHandler`, `AskHandler`,
//! `WeakTellHandler`, `WeakAskHandler`) to manage different actor types that handle
//! the same message in a unified collection.
//!
//! Run with: `cargo run --example handler_demo`
//! With tracing: `RUST_LOG=debug cargo run --example handler_demo --features tracing`

use anyhow::Result;
use rsactor::{
    message_handlers, spawn, Actor, ActorRef, AskHandler, TellHandler, WeakAskHandler,
    WeakTellHandler,
};

// ============================================================================
// Message Types
// ============================================================================

/// A simple ping message for fire-and-forget communication
struct Ping {
    timestamp: u64,
}

/// A message to query the actor's status
struct GetStatus;

/// Status response type shared by all actors
#[derive(Debug, Clone)]
struct Status {
    name: String,
    message_count: u32,
}

// ============================================================================
// Actor A: A simple counter actor
// ============================================================================

#[derive(Actor)]
struct CounterActor {
    name: String,
    count: u32,
}

#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_ping(&mut self, msg: Ping, _: &ActorRef<Self>) {
        self.count += 1;
        println!(
            "[{}] Received ping with timestamp: {} (total: {})",
            self.name, msg.timestamp, self.count
        );
    }

    #[handler]
    async fn handle_get_status(&mut self, _msg: GetStatus, _: &ActorRef<Self>) -> Status {
        Status {
            name: self.name.clone(),
            message_count: self.count,
        }
    }
}

// ============================================================================
// Actor B: A logger actor that also handles the same messages
// ============================================================================

#[derive(Actor)]
struct LoggerActor {
    prefix: String,
    log_count: u32,
}

#[message_handlers]
impl LoggerActor {
    #[handler]
    async fn handle_ping(&mut self, msg: Ping, _: &ActorRef<Self>) {
        self.log_count += 1;
        println!(
            "[{}] LOG: Ping received at {} (logged {} times)",
            self.prefix, msg.timestamp, self.log_count
        );
    }

    #[handler]
    async fn handle_get_status(&mut self, _msg: GetStatus, _: &ActorRef<Self>) -> Status {
        Status {
            name: format!("{}-logger", self.prefix),
            message_count: self.log_count,
        }
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("\n=== Handler Traits Demo ===\n");

    // Spawn different actor types
    let (counter_actor, counter_handle) = spawn::<CounterActor>(CounterActor {
        name: "Counter-1".to_string(),
        count: 0,
    });

    let (logger_actor, logger_handle) = spawn::<LoggerActor>(LoggerActor {
        prefix: "App".to_string(),
        log_count: 0,
    });

    // =========================================================================
    // Demo 1: TellHandler - Fire-and-forget with multiple actor types
    // =========================================================================
    println!("--- Demo 1: TellHandler (Fire-and-forget) ---\n");

    // Create a collection of handlers for the same message type
    let tell_handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
        (&counter_actor).into(), // From &ActorRef<CounterActor>
        (&logger_actor).into(),  // From &ActorRef<LoggerActor>
    ];

    // Send messages to all handlers
    for (i, handler) in tell_handlers.iter().enumerate() {
        println!(
            "Sending ping to handler {} (identity: {})",
            i,
            handler.as_control().identity()
        );
        handler
            .tell(Ping {
                timestamp: 1000 + i as u64,
            })
            .await?;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // =========================================================================
    // Demo 2: AskHandler - Request-response with same reply type
    // =========================================================================
    println!("\n--- Demo 2: AskHandler (Request-Response) ---\n");

    let ask_handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> =
        vec![(&counter_actor).into(), (&logger_actor).into()];

    // Collect status from all handlers
    println!("Querying status from all handlers:");
    for handler in &ask_handlers {
        let status = handler.ask(GetStatus).await?;
        println!("  - {}: {} messages", status.name, status.message_count);
    }

    // =========================================================================
    // Demo 3: Clone handlers
    // =========================================================================
    println!("\n--- Demo 3: Clone Handlers ---\n");

    let handler: Box<dyn TellHandler<Ping>> = (&counter_actor).into();

    // Clone using clone_boxed
    let handler_clone = handler.clone_boxed();

    // Or using Clone trait on Vec
    let handlers_vec: Vec<Box<dyn TellHandler<Ping>>> = vec![handler];
    let handlers_vec_clone = handlers_vec.clone();

    println!(
        "Original handler identity: {}",
        handlers_vec[0].as_control().identity()
    );
    println!(
        "Cloned handler identity: {}",
        handlers_vec_clone[0].as_control().identity()
    );

    handler_clone.tell(Ping { timestamp: 2000 }).await?;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // =========================================================================
    // Demo 4: WeakTellHandler - Weak references
    // =========================================================================
    println!("\n--- Demo 4: WeakTellHandler (Weak References) ---\n");

    let weak_handlers: Vec<Box<dyn WeakTellHandler<Ping>>> = vec![
        ActorRef::downgrade(&counter_actor).into(),
        ActorRef::downgrade(&logger_actor).into(),
    ];

    // Must upgrade before use
    println!("Sending pings via weak handlers (upgrade pattern):");
    for (i, weak_handler) in weak_handlers.iter().enumerate() {
        if let Some(strong) = weak_handler.upgrade() {
            println!("  Handler {} upgraded successfully, sending ping...", i);
            strong
                .tell(Ping {
                    timestamp: 3000 + i as u64,
                })
                .await?;
        } else {
            println!("  Handler {} failed to upgrade (actor dead)", i);
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // =========================================================================
    // Demo 5: Batch operations with single upgrade
    // =========================================================================
    println!("\n--- Demo 5: Batch Operations with Single Upgrade ---\n");

    let weak_handler: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&counter_actor).into();

    if let Some(strong) = weak_handler.upgrade() {
        println!("Upgraded weak handler, sending batch of 3 pings:");
        for i in 0..3 {
            strong
                .tell(Ping {
                    timestamp: 4000 + i,
                })
                .await?;
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // =========================================================================
    // Demo 6: Downgrade strong to weak
    // =========================================================================
    println!("\n--- Demo 6: Downgrade Strong to Weak ---\n");

    let strong_handler: Box<dyn TellHandler<Ping>> = (&counter_actor).into();
    println!(
        "Strong handler identity: {}",
        strong_handler.as_control().identity()
    );
    println!(
        "Strong handler is_alive: {}",
        strong_handler.as_control().is_alive()
    );

    // Downgrade to weak
    let weak_from_strong: Box<dyn WeakTellHandler<Ping>> = strong_handler.downgrade();
    println!(
        "Weak handler identity: {}",
        weak_from_strong.as_weak_control().identity()
    );
    println!(
        "Weak handler is_alive: {}",
        weak_from_strong.as_weak_control().is_alive()
    );

    // Round-trip: upgrade back to strong
    if let Some(upgraded) = weak_from_strong.upgrade() {
        println!("Successfully upgraded back to strong handler");
        upgraded.tell(Ping { timestamp: 5000 }).await?;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // =========================================================================
    // Demo 7: WeakAskHandler
    // =========================================================================
    println!("\n--- Demo 7: WeakAskHandler ---\n");

    let weak_ask_handlers: Vec<Box<dyn WeakAskHandler<GetStatus, Status>>> = vec![
        ActorRef::downgrade(&counter_actor).into(),
        ActorRef::downgrade(&logger_actor).into(),
    ];

    println!("Querying status via weak ask handlers:");
    for weak_handler in &weak_ask_handlers {
        if let Some(strong) = weak_handler.upgrade() {
            let status = strong.ask(GetStatus).await?;
            println!("  - {}: {} messages", status.name, status.message_count);
        }
    }

    // =========================================================================
    // Demo 8: Debug formatting
    // =========================================================================
    println!("\n--- Demo 8: Debug Formatting ---\n");

    let tell_handler: Box<dyn TellHandler<Ping>> = (&counter_actor).into();
    let weak_tell_handler: Box<dyn WeakTellHandler<Ping>> =
        ActorRef::downgrade(&counter_actor).into();

    println!("TellHandler debug: {:?}", tell_handler);
    println!("WeakTellHandler debug: {:?}", weak_tell_handler);

    // =========================================================================
    // Cleanup
    // =========================================================================
    println!("\n--- Cleanup ---\n");

    // Stop all actors
    counter_actor.stop().await?;
    logger_actor.stop().await?;

    // Wait for completion
    let counter_result = counter_handle.await?;
    let logger_result = logger_handle.await?;

    println!(
        "Counter actor stopped: {:?}",
        counter_result.stopped_normally()
    );
    println!(
        "Logger actor stopped: {:?}",
        logger_result.stopped_normally()
    );

    // Demonstrate weak handler after actor stop
    println!("\nWeak handler after actor stop:");
    for weak_handler in &weak_handlers {
        println!(
            "  - identity: {}, is_alive: {}, can_upgrade: {}",
            weak_handler.as_weak_control().identity(),
            weak_handler.as_weak_control().is_alive(),
            weak_handler.upgrade().is_some()
        );
    }

    println!("\n=== Handler Demo Complete ===");

    Ok(())
}
