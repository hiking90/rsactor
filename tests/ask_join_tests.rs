// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for the ask_join functionality that handles JoinHandle patterns

use anyhow::Result;
use rsactor::{message_handlers, spawn, Actor, ActorRef, Error};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

// Test actor that spawns tasks and returns JoinHandles
#[derive(Actor)]
struct TaskSpawnerActor {
    task_counter: Arc<AtomicU32>,
}

// Test messages
struct ComputeTask {
    value: u32,
    delay_ms: u64,
}

struct PanicTask {
    delay_ms: u64,
}

struct CancelledTask;

struct GetTaskCount;

#[message_handlers]
impl TaskSpawnerActor {
    #[handler]
    async fn handle_compute_task(
        &mut self,
        msg: ComputeTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<u64> {
        let counter = self.task_counter.fetch_add(1, Ordering::SeqCst);
        let value = msg.value;
        let delay = Duration::from_millis(msg.delay_ms);

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            (value as u64) * 2 + (counter as u64)
        })
    }

    #[handler]
    async fn handle_panic_task(
        &mut self,
        msg: PanicTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<String> {
        let delay = Duration::from_millis(msg.delay_ms);

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            panic!("Intentional panic for testing");
        })
    }

    #[handler]
    async fn handle_cancelled_task(
        &mut self,
        _: CancelledTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<String> {
        let handle = tokio::spawn(async {
            // This task will be cancelled before it completes
            tokio::time::sleep(Duration::from_secs(10)).await;
            "This should never complete".to_string()
        });

        // Cancel the task immediately
        handle.abort();
        handle
    }

    #[handler]
    async fn handle_get_task_count(&mut self, _: GetTaskCount, _: &ActorRef<Self>) -> u32 {
        self.task_counter.load(Ordering::SeqCst)
    }
}

#[tokio::test]
async fn test_ask_join_successful_computation() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor {
        task_counter: task_counter.clone(),
    });

    // Test successful computation with ask_join
    let result: u64 = actor_ref
        .ask_join(ComputeTask {
            value: 10,
            delay_ms: 100,
        })
        .await?;

    // Expected: 10 * 2 + 0 (first task, counter starts at 0) = 20
    assert_eq!(result, 20);

    // Verify task counter was incremented
    let task_count = actor_ref.ask(GetTaskCount).await?;
    assert_eq!(task_count, 1);

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_multiple_concurrent_tasks() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor {
        task_counter: task_counter.clone(),
    });

    // Spawn multiple tasks concurrently
    let tasks = vec![
        (5u32, 50u64),  // value: 5, delay: 50ms
        (10u32, 30u64), // value: 10, delay: 30ms
        (15u32, 70u64), // value: 15, delay: 70ms
    ];

    let mut handles = Vec::new();
    for (value, delay) in tasks {
        let actor_ref_clone = actor_ref.clone();
        let handle = tokio::spawn(async move {
            actor_ref_clone
                .ask_join(ComputeTask {
                    value,
                    delay_ms: delay,
                })
                .await
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.await??;
        results.push(result);
    }

    // Results should be computed as: value * 2 + counter
    // The exact counter values depend on execution order, but we know the pattern
    assert_eq!(results.len(), 3);

    // All results should be different due to different counter values
    results.sort();
    assert!(results.windows(2).all(|w| w[0] != w[1]));

    // Verify all tasks were counted
    let task_count = actor_ref.ask(GetTaskCount).await?;
    assert_eq!(task_count, 3);

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_panicked_task() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Test that panicked task is properly handled
    let result = actor_ref.ask_join(PanicTask { delay_ms: 50 }).await;

    match result {
        Ok(_) => panic!("Expected join error for panicked task"),
        Err(Error::Join { identity, source }) => {
            assert!(identity.name().contains("TaskSpawnerActor"));
            assert!(source.is_panic());
        }
        Err(e) => panic!("Expected Join error, got: {:?}", e),
    }

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_cancelled_task() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Test that cancelled task is properly handled
    let result = actor_ref.ask_join(CancelledTask).await;

    match result {
        Ok(_) => panic!("Expected join error for cancelled task"),
        Err(Error::Join { identity, source }) => {
            assert!(identity.name().contains("TaskSpawnerActor"));
            assert!(source.is_cancelled());
        }
        Err(e) => panic!("Expected Join error, got: {:?}", e),
    }

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_vs_regular_ask() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Test regular ask (returns JoinHandle)
    let join_handle: JoinHandle<u64> = actor_ref
        .ask(ComputeTask {
            value: 20,
            delay_ms: 50,
        })
        .await?;

    // Manually await the JoinHandle
    let manual_result = join_handle.await?;

    // Test ask_join (automatically awaits)
    let auto_result: u64 = actor_ref
        .ask_join(ComputeTask {
            value: 20,
            delay_ms: 50,
        })
        .await?;

    // Both should give similar results (different counter values)
    // Expected pattern: 20 * 2 + counter
    assert!(manual_result >= 40); // At least 40 (20*2 + 0)
    assert!(auto_result >= 40); // At least 40 (20*2 + counter)

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_with_actor_stopped() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Stop the actor first
    actor_ref.stop().await?;
    actor_handle.await?;

    // Try to use ask_join on stopped actor
    let result = actor_ref
        .ask_join(ComputeTask {
            value: 5,
            delay_ms: 10,
        })
        .await;

    match result {
        Ok(_) => panic!("Expected error when using ask_join on stopped actor"),
        Err(Error::Send { identity, .. }) => {
            assert!(identity.name().contains("TaskSpawnerActor"));
        }
        Err(e) => panic!("Expected Send error, got: {:?}", e),
    }

    Ok(())
}

#[tokio::test]
async fn test_ask_join_timeout_behavior() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Test that ask_join waits for long-running tasks
    let start_time = std::time::Instant::now();

    let result: u64 = actor_ref
        .ask_join(ComputeTask {
            value: 1,
            delay_ms: 200, // 200ms delay
        })
        .await?;

    let elapsed = start_time.elapsed();

    // Should have waited at least 200ms
    assert!(elapsed >= Duration::from_millis(190)); // Allow some tolerance
    assert_eq!(result, 2); // 1 * 2 + 0

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_ask_join_error_source() -> Result<()> {
    let task_counter = Arc::new(AtomicU32::new(0));
    let (actor_ref, actor_handle) = spawn::<TaskSpawnerActor>(TaskSpawnerActor { task_counter });

    // Test that we can access the source error
    let result = actor_ref.ask_join(PanicTask { delay_ms: 10 }).await;

    match result {
        Err(Error::Join { source, .. }) => {
            // Test that we can access the source error properly
            assert!(source.is_panic());

            // Test that the error chain works
            let error_as_std_error: &dyn std::error::Error = &Error::Join {
                identity: actor_ref.identity(),
                source,
            };
            assert!(error_as_std_error.source().is_some());
        }
        _ => panic!("Expected Join error"),
    }

    actor_ref.stop().await?;
    actor_handle.await?;
    Ok(())
}
