// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Panic Handling Tests
//!
//! This module contains tests for panic handling in rsActor:
//! - Tests for panics in message handlers
//! - Tests for panics in on_run method
//! - Tests for proper error handling vs panicking
//! - Tests for supervision patterns

use anyhow::Result;
use log::info;
use rsactor::{impl_message_handler, spawn, Actor, ActorRef, ActorResult, Message};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

// Test messages
#[derive(Debug)]
struct SafeMessage(u32);

#[derive(Debug)]
struct PanicMessage;

#[derive(Debug)]
struct ErrorMessage;

#[derive(Debug)]
struct CounterResetMessage;

// Test actor for panic scenarios
#[derive(Debug)]
struct PanicTestActor {
    counter: u32,
    panic_threshold: Option<u32>,
}

impl Actor for PanicTestActor {
    type Args = Option<u32>; // Optional panic threshold for on_run
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("PanicTestActor started with ID: {}", actor_ref.identity());
        Ok(PanicTestActor {
            counter: 0,
            panic_threshold: args,
        })
    }

    async fn on_run(&mut self, _actor_ref: &ActorRef<Self>) -> Result<(), Self::Error> {
        if let Some(threshold) = self.panic_threshold {
            if self.counter >= threshold {
                panic!(
                    "Counter reached threshold {} - intentional panic in on_run!",
                    threshold
                );
            }
        }

        sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    async fn on_stop(
        &mut self,
        _actor_ref: &ActorRef<Self>,
        _killed: bool,
    ) -> Result<(), Self::Error> {
        info!("PanicTestActor stopping. Final counter: {}", self.counter);
        Ok(())
    }
}

impl Message<SafeMessage> for PanicTestActor {
    type Reply = u32;

    async fn handle(&mut self, msg: SafeMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.counter += msg.0;
        self.counter
    }
}

impl Message<PanicMessage> for PanicTestActor {
    type Reply = ();

    async fn handle(&mut self, _msg: PanicMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        panic!("Intentional panic in message handler!");
    }
}

impl Message<ErrorMessage> for PanicTestActor {
    type Reply = Result<(), anyhow::Error>;

    async fn handle(&mut self, _msg: ErrorMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        Err(anyhow::anyhow!("Intentional error in message handler"))
    }
}

impl Message<CounterResetMessage> for PanicTestActor {
    type Reply = u32;

    async fn handle(
        &mut self,
        _msg: CounterResetMessage,
        _actor_ref: &ActorRef<Self>,
    ) -> Self::Reply {
        let old_counter = self.counter;
        self.counter = 0;
        old_counter
    }
}

impl_message_handler!(
    PanicTestActor,
    [SafeMessage, PanicMessage, ErrorMessage, CounterResetMessage]
);

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_normal_actor_operation() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Test normal message handling
        let result = actor_ref.ask(SafeMessage(5)).await?;
        assert_eq!(result, 5);

        let result = actor_ref.ask(SafeMessage(3)).await?;
        assert_eq!(result, 8);

        // Stop gracefully
        actor_ref.stop().await?;
        let actor_result = join_handle.await.unwrap();

        match actor_result {
            ActorResult::Completed { actor, killed } => {
                assert!(!killed);
                assert_eq!(actor.counter, 8);
            }
            _ => panic!("Expected completed actor result"),
        }

        Ok(())
    }

    #[test]
    async fn test_panic_in_message_handler() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Send a message that will cause panic
        let result = actor_ref.ask(PanicMessage).await;
        assert!(result.is_err());

        // Actor should be dead now, so subsequent messages should fail
        let result = actor_ref.ask(SafeMessage(1)).await;
        assert!(result.is_err());

        // Check that the join handle indicates a panic
        let join_result = join_handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }

    #[test]
    async fn test_panic_in_on_run() -> Result<()> {
        // Set panic threshold to 3
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(Some(3));

        // Send messages to increase counter
        let result = actor_ref.ask(SafeMessage(1)).await?;
        assert_eq!(result, 1);

        let result = actor_ref.ask(SafeMessage(1)).await?;
        assert_eq!(result, 2);

        let result = actor_ref.ask(SafeMessage(1)).await?;
        assert_eq!(result, 3);

        // Give time for on_run to execute and panic
        sleep(Duration::from_millis(50)).await;

        // Actor should be dead now
        let result = actor_ref.ask(SafeMessage(1)).await;
        assert!(result.is_err());

        // Check that the join handle indicates a panic
        let join_result = join_handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }

    #[test]
    async fn test_error_handling_vs_panic() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Test error message (should return error, not panic)
        let result = actor_ref.ask(ErrorMessage).await?;
        assert!(result.is_err());

        // Actor should still be responsive
        let result = actor_ref.ask(SafeMessage(5)).await?;
        assert_eq!(result, 5);

        // Stop gracefully
        actor_ref.stop().await?;
        let actor_result = join_handle.await.unwrap();

        match actor_result {
            ActorResult::Completed { .. } => {}
            _ => panic!("Expected completed actor result"),
        }

        Ok(())
    }

    #[test]
    async fn test_actor_state_persistence_across_messages() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Send multiple messages and verify state persistence
        assert_eq!(actor_ref.ask(SafeMessage(2)).await?, 2);
        assert_eq!(actor_ref.ask(SafeMessage(3)).await?, 5);
        assert_eq!(actor_ref.ask(SafeMessage(1)).await?, 6);

        // Reset counter
        let old_counter = actor_ref.ask(CounterResetMessage).await?;
        assert_eq!(old_counter, 6);

        // Verify counter was reset
        assert_eq!(actor_ref.ask(SafeMessage(1)).await?, 1);

        actor_ref.stop().await?;
        join_handle.await.unwrap();

        Ok(())
    }

    #[test]
    async fn test_multiple_actors_isolation() -> Result<()> {
        // Spawn multiple actors
        let (actor_ref1, join_handle1) = spawn::<PanicTestActor>(None);
        let (actor_ref2, join_handle2) = spawn::<PanicTestActor>(None);
        let (actor_ref3, join_handle3) = spawn::<PanicTestActor>(None);

        // Set different states
        assert_eq!(actor_ref1.ask(SafeMessage(10)).await?, 10);
        assert_eq!(actor_ref2.ask(SafeMessage(20)).await?, 20);
        assert_eq!(actor_ref3.ask(SafeMessage(30)).await?, 30);

        // Cause panic in actor2
        let result = actor_ref2.ask(PanicMessage).await;
        assert!(result.is_err());

        // Actor1 and Actor3 should still be responsive
        assert_eq!(actor_ref1.ask(SafeMessage(1)).await?, 11);
        assert_eq!(actor_ref3.ask(SafeMessage(1)).await?, 31);

        // Verify actor2 is dead
        let result = actor_ref2.ask(SafeMessage(1)).await;
        assert!(result.is_err());

        // Clean up
        actor_ref1.stop().await?;
        actor_ref3.stop().await?;

        let result1 = join_handle1.await.unwrap();
        let result3 = join_handle3.await.unwrap();
        let result2 = join_handle2.await;

        assert!(matches!(result1, ActorResult::Completed { .. }));
        assert!(matches!(result3, ActorResult::Completed { .. }));
        assert!(result2.is_err() && result2.unwrap_err().is_panic());

        Ok(())
    }

    #[test]
    async fn test_actor_timeout_during_panic() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Test that ask operations timeout appropriately when actor panics
        let panic_future = actor_ref.ask(PanicMessage);
        let timeout_result = timeout(Duration::from_millis(100), panic_future).await;

        match timeout_result {
            Ok(Err(_)) => {} // Expected: ask failed due to panic
            Err(_) => panic!("Ask operation should fail quickly, not timeout"),
            Ok(Ok(_)) => panic!("Panic message should not succeed"),
        }

        // Verify join handle shows panic
        let join_result = join_handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }
}

// Supervisor tests
#[cfg(test)]
mod supervision_tests {
    use super::*;
    use tokio::test;

    struct SimpleSupervisor {
        max_restarts: u32,
        restart_count: Arc<AtomicU32>,
    }

    impl SimpleSupervisor {
        fn new(max_restarts: u32) -> Self {
            Self {
                max_restarts,
                restart_count: Arc::new(AtomicU32::new(0)),
            }
        }

        async fn supervise_actor(&mut self) -> Result<u32> {
            let mut total_restarts = 0;

            loop {
                let (actor_ref, join_handle) = spawn::<PanicTestActor>(Some(2)); // Panic at counter 2

                // Send messages until panic
                #[allow(clippy::redundant_pattern_matching)]
                while let Ok(_) = actor_ref.ask(SafeMessage(1)).await {
                    sleep(Duration::from_millis(20)).await;
                }

                let join_result = join_handle.await;

                if join_result.is_err() && join_result.unwrap_err().is_panic() {
                    total_restarts = self.restart_count.fetch_add(1, Ordering::SeqCst) + 1;

                    if total_restarts >= self.max_restarts {
                        break;
                    }

                    sleep(Duration::from_millis(100)).await; // Backoff
                    continue;
                } else {
                    break;
                }
            }

            Ok(total_restarts)
        }
    }

    #[test]
    async fn test_supervisor_restart_limit() -> Result<()> {
        let mut supervisor = SimpleSupervisor::new(3);
        let restart_count = supervisor.supervise_actor().await?;

        assert_eq!(restart_count, 3);

        Ok(())
    }

    #[test]
    async fn test_supervision_with_successful_completion() -> Result<()> {
        // Test supervisor when actor completes successfully (no panic threshold)
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

        // Send some messages
        assert_eq!(actor_ref.ask(SafeMessage(1)).await?, 1);
        assert_eq!(actor_ref.ask(SafeMessage(2)).await?, 3);

        // Stop gracefully
        actor_ref.stop().await?;
        let result = join_handle.await.unwrap();

        assert!(matches!(result, ActorResult::Completed { .. }));

        Ok(())
    }
}

// Advanced panic handling tests
#[cfg(test)]
mod advanced_tests {
    use super::*;
    use tokio::test;

    // Actor that can panic during on_start
    #[derive(Debug)]
    struct StartupPanicActor {
        _data: String,
    }

    impl Actor for StartupPanicActor {
        type Args = bool; // true to panic during on_start
        type Error = anyhow::Error;

        async fn on_start(
            should_panic: Self::Args,
            _actor_ref: &ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            if should_panic {
                panic!("Intentional panic during on_start!");
            }
            Ok(StartupPanicActor {
                _data: "initialized".to_string(),
            })
        }
    }

    impl_message_handler!(StartupPanicActor, []);

    #[test]
    async fn test_panic_during_on_start() {
        // This should cause spawn to panic
        let result = std::panic::catch_unwind(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let (_actor_ref, _join_handle) = spawn::<StartupPanicActor>(true);
            })
        });

        assert!(result.is_err());
    }

    #[test]
    async fn test_successful_on_start() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<StartupPanicActor>(false);

        actor_ref.stop().await?;
        let result = join_handle.await.unwrap();

        assert!(matches!(result, ActorResult::Completed { .. }));
        Ok(())
    }

    // Actor that can panic during on_stop
    #[derive(Debug)]
    struct StopPanicActor {
        panic_on_stop: bool,
    }

    impl Actor for StopPanicActor {
        type Args = bool; // true to panic during on_stop
        type Error = anyhow::Error;

        async fn on_start(
            panic_on_stop: Self::Args,
            _actor_ref: &ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            Ok(StopPanicActor { panic_on_stop })
        }

        async fn on_stop(
            &mut self,
            _actor_ref: &ActorRef<Self>,
            _killed: bool,
        ) -> Result<(), Self::Error> {
            if self.panic_on_stop {
                panic!("Intentional panic during on_stop!");
            }
            Ok(())
        }
    }

    impl_message_handler!(StopPanicActor, []);

    #[test]
    async fn test_panic_during_on_stop() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<StopPanicActor>(true);

        actor_ref.stop().await?;
        let join_result = join_handle.await;

        // on_stop panic should be caught
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }

    #[test]
    async fn test_normal_on_stop() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<StopPanicActor>(false);

        actor_ref.stop().await?;
        let result = join_handle.await.unwrap();

        assert!(matches!(result, ActorResult::Completed { .. }));
        Ok(())
    }

    #[derive(Debug)]
    struct KillMessage;

    // Actor for testing kill vs graceful stop during panic
    #[derive(Debug)]
    struct KillTestActor {
        should_panic_on_stop: bool,
    }

    impl Actor for KillTestActor {
        type Args = bool;
        type Error = anyhow::Error;

        async fn on_start(
            should_panic_on_stop: Self::Args,
            _actor_ref: &ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            Ok(KillTestActor {
                should_panic_on_stop,
            })
        }

        async fn on_stop(
            &mut self,
            _actor_ref: &ActorRef<Self>,
            killed: bool,
        ) -> Result<(), Self::Error> {
            if !killed && self.should_panic_on_stop {
                panic!("Panic during graceful stop!");
            }
            Ok(())
        }
    }

    impl Message<KillMessage> for KillTestActor {
        type Reply = ();

        async fn handle(&mut self, _msg: KillMessage, actor_ref: &ActorRef<Self>) -> Self::Reply {
            // Actor kills itself
            let _ = actor_ref.kill();
        }
    }

    impl_message_handler!(KillTestActor, [KillMessage]);

    #[test]
    async fn test_kill_vs_graceful_stop_with_panic() -> Result<()> {
        // Test graceful stop with panic
        let (actor_ref1, join_handle1) = spawn::<KillTestActor>(true);
        actor_ref1.stop().await?;
        let result1 = join_handle1.await;
        assert!(result1.is_err() && result1.unwrap_err().is_panic());

        // Test kill (should not trigger on_stop panic)
        let (actor_ref2, join_handle2) = spawn::<KillTestActor>(true);
        let _ = actor_ref2.kill();
        let result2 = join_handle2.await.unwrap();
        assert!(matches!(
            result2,
            ActorResult::Completed { killed: true, .. }
        ));

        Ok(())
    }

    #[test]
    async fn test_self_kill_during_message_handling() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<KillTestActor>(false);

        // Send kill message - actor should kill itself
        actor_ref.tell(KillMessage).await?;

        // Give time for the kill to process
        sleep(Duration::from_millis(50)).await;

        let result = join_handle.await.unwrap();
        assert!(matches!(
            result,
            ActorResult::Completed { killed: true, .. }
        ));

        Ok(())
    }
}

// Stress tests for panic scenarios
#[cfg(test)]
mod stress_tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_concurrent_panic_actors() -> Result<()> {
        let mut handles = Vec::new();

        // Spawn multiple actors that will panic
        for _ in 0..10 {
            let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);

            // Start a task that will cause the actor to panic
            let task_handle = tokio::spawn(async move {
                let _ = actor_ref.ask(PanicMessage).await;
            });

            handles.push((task_handle, join_handle));
        }

        // Wait for all actors and tasks to complete
        for (task_handle, join_handle) in handles {
            let _ = task_handle.await;
            let join_result = join_handle.await;
            assert!(join_result.is_err());
            assert!(join_result.unwrap_err().is_panic());
        }

        Ok(())
    }

    #[test]
    async fn test_rapid_message_sending_before_panic() -> Result<()> {
        let (actor_ref, join_handle) = spawn::<PanicTestActor>(Some(100)); // High threshold

        // Send many messages rapidly
        let mut tasks = Vec::new();
        for _ in 0..50 {
            let actor_ref_clone = actor_ref.clone();
            let task = tokio::spawn(async move { actor_ref_clone.ask(SafeMessage(1)).await });
            tasks.push(task);
        }

        // Now send panic message
        let panic_task = tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await; // Small delay
            let _ = actor_ref.ask(PanicMessage).await;
        });

        // Collect results - some may succeed, some may fail
        let mut success_count = 0;
        let mut error_count = 0;

        for task in tasks {
            match task.await.unwrap() {
                Ok(_) => success_count += 1,
                Err(_) => error_count += 1,
            }
        }

        let _ = panic_task.await;

        // At least some messages should have been processed
        assert!(success_count > 0);
        println!(
            "Processed {} messages before panic, {} failed",
            success_count, error_count
        );

        // Actor should have panicked
        let join_result = join_handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }

    #[test]
    async fn test_memory_cleanup_after_panic() -> Result<()> {
        // This test verifies that resources are properly cleaned up after panic
        let mut actor_refs = Vec::new();
        let mut join_handles = Vec::new();

        // Create multiple actors and cause them to panic
        for _ in 0..5 {
            let (actor_ref, join_handle) = spawn::<PanicTestActor>(None);
            actor_refs.push(actor_ref);
            join_handles.push(join_handle);
        }

        // Cause all actors to panic
        for actor_ref in &actor_refs {
            let _ = actor_ref.ask(PanicMessage).await;
        }

        // Wait for all to complete and verify they panicked
        for join_handle in join_handles {
            let result = join_handle.await;
            assert!(result.is_err());
            assert!(result.unwrap_err().is_panic());
        }

        // Try to use actor_refs after panic - should fail
        for actor_ref in actor_refs {
            let result = actor_ref.ask(SafeMessage(1)).await;
            assert!(result.is_err());
        }

        Ok(())
    }
}

// Integration tests
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio::test;

    /// Test that demonstrates realistic panic recovery scenario
    #[test]
    async fn test_realistic_service_with_panic_recovery() -> Result<()> {
        // Simulate a service that processes work items and can fail
        #[derive(Debug)]
        struct WorkItem(u32);

        #[derive(Debug)]
        struct GetStats;

        #[derive(Debug)]
        struct ServiceActor {
            processed_count: u32,
            error_count: u32,
        }

        impl Actor for ServiceActor {
            type Args = ();
            type Error = anyhow::Error;

            async fn on_start(
                _args: Self::Args,
                _actor_ref: &ActorRef<Self>,
            ) -> Result<Self, Self::Error> {
                Ok(ServiceActor {
                    processed_count: 0,
                    error_count: 0,
                })
            }
        }

        impl Message<WorkItem> for ServiceActor {
            type Reply = Result<String, String>;

            async fn handle(&mut self, msg: WorkItem, _actor_ref: &ActorRef<Self>) -> Self::Reply {
                // Simulate processing that might panic on certain values
                if msg.0 == 13 {
                    panic!("Unlucky number 13!");
                }

                if msg.0 % 10 == 9 {
                    self.error_count += 1;
                    return Err(format!("Simulated error for item {}", msg.0));
                }

                self.processed_count += 1;
                Ok(format!("Processed item {}", msg.0))
            }
        }

        impl Message<GetStats> for ServiceActor {
            type Reply = (u32, u32);

            async fn handle(&mut self, _msg: GetStats, _actor_ref: &ActorRef<Self>) -> Self::Reply {
                (self.processed_count, self.error_count)
            }
        }

        impl_message_handler!(ServiceActor, [WorkItem, GetStats]);

        let (actor_ref, join_handle) = spawn::<ServiceActor>(());

        // Process various work items
        for i in 1..=15 {
            if i == 13 {
                // This should cause panic
                let result = actor_ref.ask(WorkItem(i)).await;
                assert!(result.is_err());
                break;
            } else {
                let result = actor_ref.ask(WorkItem(i)).await?;
                match result {
                    Ok(msg) => println!("Success: {}", msg),
                    Err(err) => println!("Expected error: {}", err),
                }
            }
        }

        // Verify actor panicked
        let join_result = join_handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_panic());

        Ok(())
    }

    /// Test simple actor isolation during panic
    #[test]
    async fn test_actor_isolation_during_panic() -> Result<()> {
        #[derive(Debug)]
        struct SimpleMsg;

        #[derive(Debug)]
        struct SimpleActor {
            id: u32,
        }

        impl Actor for SimpleActor {
            type Args = u32;
            type Error = anyhow::Error;

            async fn on_start(
                id: Self::Args,
                _actor_ref: &ActorRef<Self>,
            ) -> Result<Self, Self::Error> {
                Ok(SimpleActor { id })
            }
        }

        impl Message<SimpleMsg> for SimpleActor {
            type Reply = u32;

            async fn handle(
                &mut self,
                _msg: SimpleMsg,
                _actor_ref: &ActorRef<Self>,
            ) -> Self::Reply {
                if self.id == 2 {
                    panic!("Actor {} panicked!", self.id);
                }
                self.id
            }
        }

        impl_message_handler!(SimpleActor, [SimpleMsg]);

        // Create multiple actors
        let (actor1_ref, actor1_handle) = spawn::<SimpleActor>(1);
        let (actor2_ref, actor2_handle) = spawn::<SimpleActor>(2);
        let (actor3_ref, actor3_handle) = spawn::<SimpleActor>(3);

        // Test normal operation
        assert_eq!(actor1_ref.ask(SimpleMsg).await?, 1);
        assert_eq!(actor3_ref.ask(SimpleMsg).await?, 3);

        // Cause actor 2 to panic
        let result = actor2_ref.ask(SimpleMsg).await;
        assert!(result.is_err());

        // Other actors should still work
        assert_eq!(actor1_ref.ask(SimpleMsg).await?, 1);
        assert_eq!(actor3_ref.ask(SimpleMsg).await?, 3);

        // Clean up
        actor1_ref.stop().await?;
        actor3_ref.stop().await?;

        let result1 = actor1_handle.await.unwrap();
        let result2 = actor2_handle.await;
        let result3 = actor3_handle.await.unwrap();

        assert!(matches!(result1, ActorResult::Completed { .. }));
        assert!(result2.is_err() && result2.unwrap_err().is_panic());
        assert!(matches!(result3, ActorResult::Completed { .. }));

        Ok(())
    }
}
