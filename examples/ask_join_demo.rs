// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # ask_join Demo
//!
//! This example demonstrates the `ask_join` method pattern where:
//! 1. Message handlers spawn long-running tasks using `tokio::spawn`
//! 2. Handlers return `JoinHandle<T>` to avoid blocking the actor
//! 3. Callers use `ask_join` to automatically await task completion
//!
//! This pattern is useful for CPU-intensive or I/O-bound operations that
//! shouldn't block the actor's message processing loop.

use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef};
use std::time::Duration;
use tokio::task::JoinHandle;

// -------------------------------------------------------------------
// Worker Actor - Processes heavy tasks in spawned tasks
// -------------------------------------------------------------------

#[derive(Actor)]
struct WorkerActor {
    task_counter: u32,
}

// Demonstrate error handling - create a task that will panic
struct PanicTask;

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_panic_task(&mut self, _: PanicTask, _: &ActorRef<Self>) -> JoinHandle<String> {
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            panic!("This task intentionally panics!");
        })
    }
}

// Message types
struct HeavyComputationTask {
    id: u32,
    duration_secs: u64,
    multiplier: u32,
}

struct FileProcessingTask {
    filename: String,
    content: String,
}

struct GetTaskCount;

#[message_handlers]
impl WorkerActor {
    /// Handler that spawns a CPU-intensive task
    #[handler]
    async fn handle_heavy_computation(
        &mut self,
        msg: HeavyComputationTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<u64> {
        self.task_counter += 1;
        let task_id = msg.id;
        let duration = Duration::from_secs(msg.duration_secs);
        let multiplier = msg.multiplier;

        println!(
            "WorkerActor: Spawning heavy computation task {} (duration: {}s, multiplier: {})",
            task_id, msg.duration_secs, multiplier
        );

        // Spawn a long-running task to avoid blocking the actor
        tokio::spawn(async move {
            println!("Task {}: Starting computation...", task_id);

            // Simulate heavy computation
            tokio::time::sleep(duration).await;
            let result = (task_id as u64) * (multiplier as u64);

            println!(
                "Task {}: Computation completed, result: {}",
                task_id, result
            );
            result
        })
    }

    /// Handler that spawns an I/O-bound task
    #[handler]
    async fn handle_file_processing(
        &mut self,
        msg: FileProcessingTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<String> {
        self.task_counter += 1;
        let filename = msg.filename.clone();
        let content = msg.content.clone();

        println!(
            "WorkerActor: Spawning file processing task for '{}' ({} bytes)",
            filename,
            content.len()
        );

        // Spawn an I/O-bound task
        tokio::spawn(async move {
            println!("File task: Processing file '{}'...", filename);

            // Simulate file I/O operations
            tokio::time::sleep(Duration::from_millis(800)).await;

            let processed_content = format!(
                "PROCESSED[{}]: {} (length: {})",
                filename,
                content,
                content.len()
            );

            println!("File task: Processing completed for '{}'", filename);
            processed_content
        })
    }

    /// Regular handler that doesn't spawn tasks
    #[handler]
    async fn handle_get_task_count(&mut self, _: GetTaskCount, _: &ActorRef<Self>) -> u32 {
        self.task_counter
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
        println!("🚀 ask_join Demo: Tracing is ENABLED");
        println!("You should see detailed trace logs for all actor operations\n");
    }

    #[cfg(not(feature = "tracing"))]
    {
        env_logger::init();
        println!("📝 ask_join Demo: Tracing is DISABLED\n");
    }

    // Create the worker actor
    let (worker_ref, worker_handle) =
        rsactor::spawn::<WorkerActor>(WorkerActor { task_counter: 0 });

    println!("=== ask_join Demo: Heavy Computation Tasks ===");

    // Spawn multiple heavy computation tasks concurrently
    let computation_tasks = vec![
        HeavyComputationTask {
            id: 1,
            duration_secs: 2,
            multiplier: 10,
        },
        HeavyComputationTask {
            id: 2,
            duration_secs: 1,
            multiplier: 20,
        },
        HeavyComputationTask {
            id: 3,
            duration_secs: 3,
            multiplier: 5,
        },
    ];

    // Start all computation tasks concurrently using ask_join
    let mut computation_handles = Vec::new();
    for task in computation_tasks {
        let worker_ref_clone = worker_ref.clone();
        let handle = tokio::spawn(async move {
            let task_id = task.id;
            println!(
                "Client: Sending computation task {} using ask_join",
                task_id
            );
            let result = worker_ref_clone.ask_join(task).await;
            (task_id, result)
        });
        computation_handles.push(handle);
    }

    // Wait for all computation tasks to complete
    for handle in computation_handles {
        let (task_id, result) = handle.await?;
        match result {
            Ok(value) => println!(
                "✅ Computation task {} completed with result: {}",
                task_id, value
            ),
            Err(e) => println!("❌ Computation task {} failed: {}", task_id, e),
        }
    }

    println!("\n=== ask_join Demo: File Processing Tasks ===");

    // Process multiple files concurrently
    let file_tasks = vec![
        FileProcessingTask {
            filename: "document1.txt".to_string(),
            content: "Hello world from document 1".to_string(),
        },
        FileProcessingTask {
            filename: "data.json".to_string(),
            content: r#"{"name": "example", "value": 42}"#.to_string(),
        },
        FileProcessingTask {
            filename: "config.yaml".to_string(),
            content: "server:\n  port: 8080\n  host: localhost".to_string(),
        },
    ];

    // Process files concurrently using ask_join
    let mut file_handles = Vec::new();
    for task in file_tasks {
        let worker_ref_clone = worker_ref.clone();
        let filename = task.filename.clone();
        let handle = tokio::spawn(async move {
            println!("Client: Processing file '{}' using ask_join", filename);
            let result = worker_ref_clone.ask_join(task).await;
            (filename, result)
        });
        file_handles.push(handle);
    }

    // Wait for all file processing tasks to complete
    for handle in file_handles {
        let (filename, result) = handle.await?;
        match result {
            Ok(content) => println!("✅ File '{}' processed: {}", filename, content),
            Err(e) => println!("❌ File '{}' processing failed: {}", filename, e),
        }
    }

    println!("\n=== ask_join Demo: Error Handling ===");

    println!("Client: Sending a task that will panic to demonstrate error handling");
    match worker_ref.ask_join(PanicTask).await {
        Ok(result) => println!("Unexpected success: {}", result),
        Err(rsactor::Error::Join { identity, source }) => {
            println!(
                "✅ Correctly caught join error from actor {}: {}",
                identity, source
            );
            if source.is_panic() {
                println!("   The task panicked as expected");
            }
        }
        Err(e) => println!("Unexpected error type: {}", e),
    }

    println!("\n=== ask_join Demo: Comparison with Regular ask ===");

    // Demonstrate the difference between ask and ask_join
    println!("Using regular ask (returns JoinHandle):");
    let join_handle: JoinHandle<u64> = worker_ref
        .ask(HeavyComputationTask {
            id: 999,
            duration_secs: 1,
            multiplier: 100,
        })
        .await?;

    println!("Got JoinHandle, now manually awaiting...");
    let result = join_handle.await?;
    println!("Manual await result: {}", result);

    println!("\nUsing ask_join (automatically awaits):");
    let result: u64 = worker_ref
        .ask_join(HeavyComputationTask {
            id: 1000,
            duration_secs: 1,
            multiplier: 100,
        })
        .await?;
    println!("ask_join result: {}", result);

    // Get final task count
    let task_count = worker_ref.ask(GetTaskCount).await?;
    println!("\nTotal tasks processed by the actor: {}", task_count);

    // Gracefully stop the actor
    println!("\nStopping actor...");
    worker_ref.stop().await?;

    // Wait for actor to complete
    let _result = worker_handle.await?;
    println!("ask_join demo completed successfully!");

    Ok(())
}
