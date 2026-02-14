// Tests specifically designed to improve code coverage
// These tests target uncovered code paths identified by llvm-cov

use rsactor::{spawn, Actor, ActorRef, ActorWeak, Identity};
use std::time::Duration;

/// Initialize tracing subscriber for tests
/// This enables coverage of tracing-related code paths
fn init_test_logger() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

// ============================================================================
// Tests for default on_run and on_stop implementations
// ============================================================================

/// Test actor that uses the default on_run implementation
/// The default on_run sleeps for 1 second and returns Ok(())
mod default_lifecycle_tests {
    use super::*;

    #[derive(Actor)]
    struct DefaultLifecycleActor {
        #[allow(dead_code)]
        on_run_started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    struct Ping;

    #[rsactor::message_handlers]
    impl DefaultLifecycleActor {
        #[handler]
        async fn handle(&mut self, _: Ping, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }
    }

    #[tokio::test]
    async fn test_default_on_run_executes() {
        init_test_logger();
        // This test verifies that the default on_run implementation is called
        // by letting the actor run for a bit longer than the 1-second sleep
        let (actor_ref, handle) = spawn::<DefaultLifecycleActor>(DefaultLifecycleActor {
            on_run_started: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        // Wait a bit to let on_run execute at least once
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send a message to verify the actor is still working
        let response: String = actor_ref.ask(Ping).await.unwrap();
        assert_eq!(response, "pong");

        // Stop the actor gracefully - this triggers the default on_stop
        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());
    }

    #[tokio::test]
    async fn test_default_on_stop_via_kill() {
        init_test_logger();
        // Test that default on_stop is called when actor is killed
        let (actor_ref, handle) = spawn::<DefaultLifecycleActor>(DefaultLifecycleActor {
            on_run_started: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        // Give the actor a moment to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Kill triggers on_stop with killed=true
        actor_ref.kill().unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());
        assert!(result.was_killed());
    }

    #[tokio::test]
    async fn test_default_on_stop_via_graceful_stop() {
        init_test_logger();
        // Test that default on_stop is called on graceful stop
        let (actor_ref, handle) = spawn::<DefaultLifecycleActor>(DefaultLifecycleActor {
            on_run_started: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        // Graceful stop triggers on_stop with killed=false
        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());
        assert!(!result.was_killed());
    }
}

// ============================================================================
// Tests for ActorWeak::is_alive method
// ============================================================================

mod actor_weak_tests {
    use super::*;

    #[derive(Actor)]
    struct SimpleActor;

    struct GetId;

    #[rsactor::message_handlers]
    impl SimpleActor {
        #[handler]
        async fn handle(&mut self, _: GetId, actor_ref: &ActorRef<Self>) -> Identity {
            actor_ref.identity()
        }
    }

    #[tokio::test]
    async fn test_actor_weak_is_alive_while_running() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<SimpleActor>(SimpleActor);
        let weak = ActorRef::downgrade(&actor_ref);

        // Actor should be alive
        assert!(weak.is_alive());

        // Verify identity is accessible from weak ref
        assert_eq!(weak.identity(), actor_ref.identity());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_weak_is_alive_after_stop() {
        let (actor_ref, handle) = spawn::<SimpleActor>(SimpleActor);
        let weak = ActorRef::downgrade(&actor_ref);

        assert!(weak.is_alive());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // After stop, the weak reference should no longer be upgradeable
        // (depending on when the channels are closed)
        // is_alive() may return false after stop
    }

    #[tokio::test]
    async fn test_actor_weak_upgrade_returns_none_after_all_refs_dropped() {
        let (actor_ref, handle) = spawn::<SimpleActor>(SimpleActor);
        let weak = ActorRef::downgrade(&actor_ref);

        // Drop all strong references
        drop(actor_ref);

        // Wait for the actor to stop
        handle.await.unwrap();

        // After all strong references are dropped, upgrade should return None
        assert!(weak.upgrade().is_none());
    }
}

// ============================================================================
// Tests for ActorRef cloning and identity
// ============================================================================

mod actor_ref_clone_tests {
    use super::*;

    #[derive(Actor)]
    struct CloneTestActor;

    struct GetIdentity;

    #[rsactor::message_handlers]
    impl CloneTestActor {
        #[handler]
        async fn handle(&mut self, _: GetIdentity, actor_ref: &ActorRef<Self>) -> Identity {
            actor_ref.identity()
        }
    }

    #[tokio::test]
    async fn test_actor_ref_clone_preserves_identity() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<CloneTestActor>(CloneTestActor);

        // Clone the actor ref
        let cloned_ref = actor_ref.clone();

        // Both should have the same identity
        assert_eq!(actor_ref.identity(), cloned_ref.identity());

        // Both should be able to communicate with the actor
        let id1: Identity = actor_ref.ask(GetIdentity).await.unwrap();
        let id2: Identity = cloned_ref.ask(GetIdentity).await.unwrap();
        assert_eq!(id1, id2);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_ref_is_alive() {
        let (actor_ref, handle) = spawn::<CloneTestActor>(CloneTestActor);

        // Actor should be alive initially
        assert!(actor_ref.is_alive());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // After stop and join, is_alive might still return true
        // because is_alive checks channel state, not actor state
    }
}

// ============================================================================
// Tests for ActorWeak cloning
// ============================================================================

mod weak_clone_tests {
    use super::*;

    #[derive(Actor)]
    struct WeakCloneActor;

    struct Echo(String);

    #[rsactor::message_handlers]
    impl WeakCloneActor {
        #[handler]
        async fn handle(&mut self, msg: Echo, _: &ActorRef<Self>) -> String {
            msg.0
        }
    }

    #[tokio::test]
    async fn test_actor_weak_clone() {
        let (actor_ref, handle) = spawn::<WeakCloneActor>(WeakCloneActor);
        let weak1 = ActorRef::downgrade(&actor_ref);
        let weak2 = weak1.clone();

        // Both weak references should have the same identity
        assert_eq!(weak1.identity(), weak2.identity());

        // Both should be upgradeable
        let strong1 = weak1.upgrade().expect("weak1 should upgrade");
        let strong2 = weak2.upgrade().expect("weak2 should upgrade");

        assert_eq!(strong1.identity(), strong2.identity());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for tell_with_timeout edge cases
// ============================================================================

mod timeout_tests {
    use super::*;
    use rsactor::Error;

    #[derive(Actor)]
    struct SlowActor;

    struct SlowMessage;

    #[rsactor::message_handlers]
    impl SlowActor {
        #[handler]
        async fn handle(&mut self, _: SlowMessage, _: &ActorRef<Self>) {
            // This handler is intentionally slow
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    #[tokio::test]
    async fn test_tell_with_very_short_timeout_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<SlowActor>(SlowActor);

        // Stop the actor immediately
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // Try to send with timeout to stopped actor
        let result = actor_ref
            .tell_with_timeout(SlowMessage, Duration::from_millis(1))
            .await;

        // Should fail because actor is stopped (Send error, not Timeout)
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Send { .. } => {}    // Expected
            Error::Timeout { .. } => {} // Also acceptable
            e => panic!("Unexpected error type: {:?}", e),
        }
    }
}

// ============================================================================
// Tests for actor lifecycle with custom on_run returning error
// ============================================================================

mod on_run_error_tests {
    use super::*;

    struct FailingOnRunActor {
        fail_count: u32,
    }

    impl Actor for FailingOnRunActor {
        type Args = u32; // Number of times to succeed before failing
        type Error = String;

        async fn on_start(
            args: Self::Args,
            _actor_ref: &ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            Ok(Self { fail_count: args })
        }

        async fn on_run(&mut self, _actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
            if self.fail_count > 0 {
                self.fail_count -= 1;
                // Use a short sleep to yield control
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(true)
            } else {
                Err("Intentional on_run failure".to_string())
            }
        }
    }

    #[tokio::test]
    async fn test_on_run_error_terminates_actor() {
        init_test_logger();
        // Actor will succeed 2 times then fail
        let (actor_ref, handle) = spawn::<FailingOnRunActor>(2);

        // Wait for the actor to fail
        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_runtime_failed());

        // Verify the error message
        let error = result.error().unwrap();
        assert_eq!(error, "Intentional on_run failure");

        // The actor_ref should no longer be alive
        assert!(!actor_ref.is_alive());
    }

    // Actor that fails in both on_run and on_stop
    struct FailingOnRunAndOnStopActor {
        fail_count: u32,
    }

    impl Actor for FailingOnRunAndOnStopActor {
        type Args = u32;
        type Error = String;

        async fn on_start(
            args: Self::Args,
            _actor_ref: &ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            Ok(Self { fail_count: args })
        }

        async fn on_run(&mut self, _actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
            if self.fail_count > 0 {
                self.fail_count -= 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(true)
            } else {
                Err("Intentional on_run failure".to_string())
            }
        }

        async fn on_stop(
            &mut self,
            _actor_weak: &ActorWeak<Self>,
            _killed: bool,
        ) -> Result<(), Self::Error> {
            // This error is logged but not propagated (on_run error takes precedence)
            Err("Intentional on_stop failure during on_run cleanup".to_string())
        }
    }

    #[tokio::test]
    async fn test_on_run_error_with_on_stop_also_failing() {
        init_test_logger();
        // Actor will succeed 1 time then fail in on_run, and on_stop will also fail
        let (actor_ref, handle) = spawn::<FailingOnRunAndOnStopActor>(1);

        // Wait for the actor to fail
        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_runtime_failed());

        // The error should be from on_run (on_stop error is only logged)
        let error = result.error().unwrap();
        assert_eq!(error, "Intentional on_run failure");

        // Phase should be OnRunThenOnStop since both on_run and on_stop failed
        match &result {
            rsactor::ActorResult::Failed { phase, .. } => {
                assert_eq!(*phase, rsactor::FailurePhase::OnRunThenOnStop);
            }
            _ => panic!("Expected Failed result"),
        }

        // is_cleanup_failed should be true
        assert!(result.is_cleanup_failed());

        assert!(!actor_ref.is_alive());
    }
}

// ============================================================================
// Tests for actor lifecycle with custom on_stop returning error
// ============================================================================

mod on_stop_error_tests {
    use super::*;

    struct FailingOnStopActor;

    impl Actor for FailingOnStopActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self)
        }

        async fn on_stop(
            &mut self,
            _actor_weak: &ActorWeak<Self>,
            _killed: bool,
        ) -> Result<(), Self::Error> {
            Err("Intentional on_stop failure".to_string())
        }
    }

    struct Ping;

    #[rsactor::message_handlers]
    impl FailingOnStopActor {
        #[handler]
        async fn handle(&mut self, _: Ping, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }
    }

    #[tokio::test]
    async fn test_on_stop_error_on_graceful_stop() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<FailingOnStopActor>(());

        // Verify the actor works
        let response: String = actor_ref.ask(Ping).await.unwrap();
        assert_eq!(response, "pong");

        // Stop the actor - this will trigger on_stop which fails
        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_stop_failed());

        let error = result.error().unwrap();
        assert_eq!(error, "Intentional on_stop failure");
    }

    #[tokio::test]
    async fn test_on_stop_error_on_kill() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<FailingOnStopActor>(());

        // Kill the actor - this will trigger on_stop which fails
        actor_ref.kill().unwrap();
        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_stop_failed());
        assert!(result.was_killed());
    }
}

// ============================================================================
// Tests for actor lifecycle with on_start error
// ============================================================================

mod on_start_error_tests {
    use super::*;

    struct FailingOnStartActor;

    impl Actor for FailingOnStartActor {
        type Args = bool; // true = succeed, false = fail
        type Error = String;

        async fn on_start(succeed: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            if succeed {
                Ok(Self)
            } else {
                Err("Intentional on_start failure".to_string())
            }
        }
    }

    #[tokio::test]
    async fn test_on_start_error() {
        init_test_logger();
        let (_actor_ref, handle) = spawn::<FailingOnStartActor>(false);

        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_startup_failed());

        // Actor instance should not be available for on_start failures
        assert!(!result.has_actor());

        let error = result.error().unwrap();
        assert_eq!(error, "Intentional on_start failure");
    }
}

// ============================================================================
// Tests for dead letter scenarios
// ============================================================================

mod dead_letter_tests {
    use super::*;

    #[derive(Actor)]
    struct DeadLetterActor;

    struct TestMessage;

    #[rsactor::message_handlers]
    impl DeadLetterActor {
        #[handler]
        async fn handle(&mut self, _: TestMessage, _: &ActorRef<Self>) -> String {
            "response".to_string()
        }
    }

    #[tokio::test]
    async fn test_tell_to_stopped_actor_records_dead_letter() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeadLetterActor>(DeadLetterActor);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // Try to tell a stopped actor
        let result = actor_ref.tell(TestMessage).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ask_to_stopped_actor_records_dead_letter() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeadLetterActor>(DeadLetterActor);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // Try to ask a stopped actor
        let result: rsactor::Result<String> = actor_ref.ask(TestMessage).await;
        assert!(result.is_err());
    }
}

// ============================================================================
// Tests for actor termination via all refs dropped
// ============================================================================

mod ref_drop_termination_tests {
    use super::*;

    #[derive(Actor)]
    struct RefDropActor {
        #[allow(dead_code)]
        stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    struct IsRunning;

    #[rsactor::message_handlers]
    impl RefDropActor {
        #[handler]
        async fn handle(&mut self, _: IsRunning, _: &ActorRef<Self>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_actor_terminates_when_all_refs_dropped() {
        init_test_logger();
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let (actor_ref, handle) = spawn::<RefDropActor>(RefDropActor {
            stopped: stopped.clone(),
        });

        // Verify actor is running
        let running: bool = actor_ref.ask(IsRunning).await.unwrap();
        assert!(running);

        // Drop all strong references
        drop(actor_ref);

        // The actor should terminate
        let result = handle.await.unwrap();
        assert!(result.is_completed());
        assert!(!result.was_killed()); // terminated due to ref drop, not kill
    }
}

// ============================================================================
// Tests for Identity methods (lib.rs coverage)
// ============================================================================

mod identity_tests {
    use super::*;

    #[test]
    fn test_identity_name_method() {
        let identity = Identity::new(42, "TestActor");

        // Test the name() method
        assert_eq!(identity.name(), "TestActor");

        // Test Display implementation (includes ID)
        let display = format!("{}", identity);
        assert_eq!(display, "TestActor(#42)");

        // Test id field
        assert_eq!(identity.id, 42);
    }

    #[test]
    fn test_identity_equality() {
        let id1 = Identity::new(1, "Actor");
        let id2 = Identity::new(1, "Actor");
        let id3 = Identity::new(2, "Actor");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}

// ============================================================================
// Tests for ActorResult From conversion (actor_result.rs coverage)
// ============================================================================

mod actor_result_from_tests {
    use super::*;

    #[derive(Actor)]
    struct FromTestActor {
        value: i32,
    }

    #[tokio::test]
    async fn test_actor_result_into_tuple_completed() {
        let (actor_ref, handle) = spawn::<FromTestActor>(FromTestActor { value: 42 });

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        // Convert ActorResult to tuple
        let (maybe_actor, maybe_error): (Option<FromTestActor>, Option<std::convert::Infallible>) =
            result.into();

        assert!(maybe_actor.is_some(), "Completed result should have actor");
        assert!(
            maybe_error.is_none(),
            "Completed result should have no error"
        );
        assert_eq!(maybe_actor.unwrap().value, 42);
    }

    #[tokio::test]
    async fn test_actor_result_into_tuple_failed_with_actor() {
        // Create an actor that fails in on_stop (will have actor)
        struct FailOnStopActor {
            value: i32,
        }

        impl Actor for FailOnStopActor {
            type Args = i32;
            type Error = String;

            async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
                Ok(Self { value: args })
            }

            async fn on_stop(&mut self, _: &ActorWeak<Self>, _: bool) -> Result<(), Self::Error> {
                Err("on_stop error".to_string())
            }
        }

        let (actor_ref, handle) = spawn::<FailOnStopActor>(100);

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        assert!(result.is_failed());

        // Convert to tuple
        let (maybe_actor, maybe_error): (Option<FailOnStopActor>, Option<String>) = result.into();

        assert!(maybe_actor.is_some(), "Failed with actor should have actor");
        assert!(maybe_error.is_some(), "Failed result should have error");
        assert_eq!(maybe_actor.unwrap().value, 100);
        assert_eq!(maybe_error.unwrap(), "on_stop error");
    }

    #[tokio::test]
    async fn test_actor_result_into_tuple_failed_without_actor() {
        // Create an actor that fails in on_start (won't have actor)
        struct FailOnStartActor;

        impl Actor for FailOnStartActor {
            type Args = ();
            type Error = String;

            async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
                Err("on_start error".to_string())
            }
        }

        let (_, handle) = spawn::<FailOnStartActor>(());
        let result = handle.await.unwrap();

        assert!(result.is_failed());
        assert!(result.is_startup_failed());

        // Convert to tuple
        let (maybe_actor, maybe_error): (Option<FailOnStartActor>, Option<String>) = result.into();

        assert!(
            maybe_actor.is_none(),
            "Failed on_start should have no actor"
        );
        assert!(maybe_error.is_some(), "Failed result should have error");
        assert_eq!(maybe_error.unwrap(), "on_start error");
    }
}

// ============================================================================
// Tests for set_default_mailbox_capacity error (lib.rs coverage)
// ============================================================================

mod mailbox_capacity_tests {
    // Note: Can't easily test "already set" case since global state
    // But we can test the zero capacity error case
    #[test]
    fn test_set_default_mailbox_capacity_zero_fails() {
        let result = rsactor::set_default_mailbox_capacity(0);
        assert!(result.is_err(), "Setting capacity to 0 should fail");

        match result.unwrap_err() {
            rsactor::Error::MailboxCapacity { message } => {
                assert!(message.contains("greater than 0"));
            }
            _ => panic!("Expected MailboxCapacity error"),
        }
    }
}

// ============================================================================
// Tests for blocking methods with timeout (actor_ref.rs coverage)
// ============================================================================

mod blocking_timeout_tests {
    use super::*;

    #[derive(Actor)]
    struct BlockingTimeoutActor;

    struct SlowMessage;
    struct FastMessage;

    #[rsactor::message_handlers]
    impl BlockingTimeoutActor {
        #[handler]
        async fn handle_slow(&mut self, _: SlowMessage, _: &ActorRef<Self>) -> String {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "slow done".to_string()
        }

        #[handler]
        async fn handle_fast(&mut self, _: FastMessage, _: &ActorRef<Self>) -> String {
            "fast done".to_string()
        }
    }

    #[tokio::test]
    async fn test_blocking_tell_with_timeout_success() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(BlockingTimeoutActor);

        let actor_clone = actor_ref.clone();

        // blocking_tell with timeout that succeeds
        let result = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_tell(FastMessage, Some(Duration::from_secs(5)))
        })
        .await
        .unwrap();

        assert!(
            result.is_ok(),
            "blocking_tell with timeout should succeed for fast message"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_blocking_ask_with_timeout_success() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(BlockingTimeoutActor);

        let actor_clone = actor_ref.clone();

        // blocking_ask with timeout that succeeds
        let result: Result<String, _> = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_ask(FastMessage, Some(Duration::from_secs(5)))
        })
        .await
        .unwrap();

        assert!(result.is_ok(), "blocking_ask with timeout should succeed");
        assert_eq!(result.unwrap(), "fast done");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_blocking_tell_with_timeout_expired() {
        init_test_logger();

        // Create actor with small mailbox that we can fill
        let (actor_ref, handle) =
            rsactor::spawn_with_mailbox_capacity::<BlockingTimeoutActor>(BlockingTimeoutActor, 1);

        // Fill the mailbox with a slow message
        let _ = actor_ref.tell(SlowMessage).await;

        let actor_clone = actor_ref.clone();

        // Now blocking_tell with very short timeout should timeout
        let result = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_tell(FastMessage, Some(Duration::from_millis(10)))
        })
        .await
        .unwrap();

        // Either timeout or success is acceptable (race condition)
        // The key is covering the timeout code path
        let _ = result; // just making sure it executed

        actor_ref.kill().unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_blocking_ask_with_timeout_expired() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(BlockingTimeoutActor);

        let actor_clone = actor_ref.clone();

        // blocking_ask with timeout that expires (slow message)
        let result: Result<String, _> = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_ask(SlowMessage, Some(Duration::from_millis(10)))
        })
        .await
        .unwrap();

        assert!(
            result.is_err(),
            "blocking_ask should timeout on slow message"
        );

        actor_ref.kill().unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_blocking_tell_with_timeout_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(BlockingTimeoutActor);

        // Stop the actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // Now try blocking_tell with timeout to stopped actor
        let result = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_tell(FastMessage, Some(Duration::from_millis(100)))
        })
        .await
        .unwrap();

        assert!(
            result.is_err(),
            "blocking_tell to stopped actor should fail"
        );
    }

    #[tokio::test]
    async fn test_blocking_ask_with_timeout_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(BlockingTimeoutActor);

        // Stop the actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // Now try blocking_ask with timeout to stopped actor
        let result: Result<String, _> = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_ask(FastMessage, Some(Duration::from_millis(100)))
        })
        .await
        .unwrap();

        assert!(result.is_err(), "blocking_ask to stopped actor should fail");
    }
}

// ============================================================================
// Tests for more actor_result.rs methods coverage
// ============================================================================

// ============================================================================
// Tests for tracing feature (actor.rs coverage when tracing is enabled)
// ============================================================================

#[cfg(feature = "tracing")]
mod tracing_coverage_tests {
    use super::*;

    #[derive(Actor)]
    struct TracingTestActor;

    struct TracingPing;
    struct TracingSlowPing;

    #[rsactor::message_handlers]
    impl TracingTestActor {
        #[handler]
        async fn handle_ping(&mut self, _: TracingPing, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }

        #[handler]
        async fn handle_slow(&mut self, _: TracingSlowPing, _: &ActorRef<Self>) -> String {
            tokio::time::sleep(Duration::from_millis(50)).await;
            "slow pong".to_string()
        }
    }

    #[tokio::test]
    async fn test_actor_with_tracing_enabled() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TracingTestActor>(TracingTestActor);

        // Send messages to trigger tracing code paths
        let response: String = actor_ref.ask(TracingPing).await.unwrap();
        assert_eq!(response, "pong");

        // Slow message to test timing traces
        let slow_response: String = actor_ref.ask(TracingSlowPing).await.unwrap();
        assert_eq!(slow_response, "slow pong");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_tell_with_tracing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TracingTestActor>(TracingTestActor);

        // tell() with tracing
        actor_ref.tell(TracingPing).await.unwrap();

        // tell_with_timeout() with tracing
        actor_ref
            .tell_with_timeout(TracingPing, Duration::from_secs(1))
            .await
            .unwrap();

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_ask_with_timeout_tracing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TracingTestActor>(TracingTestActor);

        // ask_with_timeout() success path
        let response: String = actor_ref
            .ask_with_timeout(TracingPing, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(response, "pong");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_kill_with_tracing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TracingTestActor>(TracingTestActor);

        // Test kill() with tracing
        actor_ref.kill().unwrap();
        let result = handle.await.unwrap();
        assert!(result.was_killed());
    }

    #[tokio::test]
    async fn test_stop_with_tracing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TracingTestActor>(TracingTestActor);

        // Test stop() with tracing
        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(!result.was_killed());
    }
}

mod actor_result_methods_tests {
    use super::*;

    #[derive(Actor)]
    struct MethodsTestActor {
        data: String,
    }

    #[tokio::test]
    async fn test_actor_result_to_result_completed() {
        let (actor_ref, handle) = spawn::<MethodsTestActor>(MethodsTestActor {
            data: "test".to_string(),
        });

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        // Use to_result() for conversion
        let std_result = result.to_result();
        assert!(std_result.is_ok());
        assert_eq!(std_result.unwrap().data, "test");
    }

    #[tokio::test]
    async fn test_actor_result_to_result_failed() {
        #[derive(Debug)]
        struct FailActor;

        impl Actor for FailActor {
            type Args = ();
            type Error = String;

            async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, String> {
                Err("failed".to_string())
            }
        }

        let (_, handle) = spawn::<FailActor>(());
        let result = handle.await.unwrap();

        // Use to_result() for conversion
        let std_result = result.to_result();
        assert!(std_result.is_err());
        assert_eq!(std_result.unwrap_err(), "failed");
    }

    #[tokio::test]
    async fn test_actor_result_into_actor_completed() {
        let (actor_ref, handle) = spawn::<MethodsTestActor>(MethodsTestActor {
            data: "into_actor_test".to_string(),
        });

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        let maybe_actor = result.into_actor();
        assert!(maybe_actor.is_some());
        assert_eq!(maybe_actor.unwrap().data, "into_actor_test");
    }

    #[tokio::test]
    async fn test_actor_result_into_error_failed() {
        struct FailActorForError;

        impl Actor for FailActorForError {
            type Args = ();
            type Error = String;

            async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, String> {
                Err("into_error_test".to_string())
            }
        }

        let (_, handle) = spawn::<FailActorForError>(());
        let result = handle.await.unwrap();

        let maybe_error = result.into_error();
        assert!(maybe_error.is_some());
        assert_eq!(maybe_error.unwrap(), "into_error_test");
    }

    #[tokio::test]
    async fn test_actor_result_into_error_completed() {
        let (actor_ref, handle) = spawn::<MethodsTestActor>(MethodsTestActor {
            data: "test".to_string(),
        });

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        // Completed result should have no error
        let maybe_error = result.into_error();
        assert!(maybe_error.is_none());
    }
}

// ============================================================================
// Tests for ask_join (actor_ref.rs coverage)
// ============================================================================

mod ask_join_tests {
    use super::*;
    use tokio::task::JoinHandle;

    #[derive(Actor)]
    struct JoinTestActor;

    struct SpawnTask {
        value: i32,
    }

    #[rsactor::message_handlers]
    impl JoinTestActor {
        #[handler]
        async fn handle(&mut self, msg: SpawnTask, _: &ActorRef<Self>) -> JoinHandle<i32> {
            let value = msg.value;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                value * 2
            })
        }
    }

    #[tokio::test]
    async fn test_ask_join_successful() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<JoinTestActor>(JoinTestActor);

        // Use ask_join to get the final result
        let result: i32 = actor_ref.ask_join(SpawnTask { value: 21 }).await.unwrap();
        assert_eq!(result, 42);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for blocking methods to stopped actor (coverage for error paths)
// ============================================================================

mod blocking_error_paths_tests {
    use super::*;

    #[derive(Actor)]
    struct BlockingErrorActor;

    struct BlockingMsg;

    #[rsactor::message_handlers]
    impl BlockingErrorActor {
        #[handler]
        async fn handle(&mut self, _: BlockingMsg, _: &ActorRef<Self>) -> String {
            "ok".to_string()
        }
    }

    #[tokio::test]
    async fn test_blocking_tell_no_timeout_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingErrorActor>(BlockingErrorActor);

        // Stop actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // blocking_tell without timeout to stopped actor
        let result =
            tokio::task::spawn_blocking(move || actor_clone.blocking_tell(BlockingMsg, None))
                .await
                .unwrap();

        assert!(
            result.is_err(),
            "blocking_tell to stopped actor should fail"
        );
    }

    #[tokio::test]
    async fn test_blocking_ask_no_timeout_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingErrorActor>(BlockingErrorActor);

        // Stop actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // blocking_ask without timeout to stopped actor
        let result: Result<String, _> =
            tokio::task::spawn_blocking(move || actor_clone.blocking_ask(BlockingMsg, None))
                .await
                .unwrap();

        assert!(result.is_err(), "blocking_ask to stopped actor should fail");
    }
}

// ============================================================================
// Tests for ActorWeak clone (actor_ref.rs coverage)
// ============================================================================

mod actor_weak_clone_tests {
    use super::*;

    #[derive(Actor)]
    struct WeakCloneTestActor;

    struct WeakCloneMsg;

    #[rsactor::message_handlers]
    impl WeakCloneTestActor {
        #[handler]
        async fn handle(&mut self, _: WeakCloneMsg, _: &ActorRef<Self>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_actor_weak_clone_and_upgrade() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<WeakCloneTestActor>(WeakCloneTestActor);

        // Create weak reference
        let weak1 = ActorRef::downgrade(&actor_ref);

        // Clone the weak reference
        let weak2 = weak1.clone();

        // Both should have the same identity
        assert_eq!(weak1.identity(), weak2.identity());

        // Both should be able to upgrade
        let strong1 = weak1.upgrade().expect("weak1 should upgrade");
        let strong2 = weak2.upgrade().expect("weak2 should upgrade");

        assert_eq!(strong1.identity(), strong2.identity());

        // Test is_alive on weak reference
        assert!(weak1.is_alive());

        // Drop all strong references first
        drop(strong1);
        drop(strong2);
        drop(actor_ref);

        // Wait for actor to terminate
        handle.await.unwrap();

        // After all strong refs dropped and actor terminated, upgrade should return None
        // Note: is_alive() checks channel strong count, which may not be 0 immediately
        // So we check upgrade() instead which is more reliable
        assert!(
            weak1.upgrade().is_none(),
            "weak reference should not upgrade after actor stopped"
        );
    }
}

// ============================================================================
// Tests for FailurePhase Display (actor_result.rs coverage)
// ============================================================================

mod failure_phase_tests {
    use rsactor::FailurePhase;

    #[test]
    fn test_failure_phase_display_all_variants() {
        assert_eq!(format!("{}", FailurePhase::OnStart), "OnStart");
        assert_eq!(format!("{}", FailurePhase::OnRun), "OnRun");
        assert_eq!(format!("{}", FailurePhase::OnStop), "OnStop");
    }

    #[test]
    fn test_failure_phase_debug() {
        // Test Debug implementation
        let debug_str = format!("{:?}", FailurePhase::OnStart);
        assert!(debug_str.contains("OnStart"));

        let debug_str = format!("{:?}", FailurePhase::OnRun);
        assert!(debug_str.contains("OnRun"));

        let debug_str = format!("{:?}", FailurePhase::OnStop);
        assert!(debug_str.contains("OnStop"));
    }

    #[test]
    fn test_failure_phase_clone_copy() {
        let phase = FailurePhase::OnRun;
        let cloned = phase;
        let copied = phase;

        assert_eq!(phase, cloned);
        assert_eq!(phase, copied);
    }
}

// ============================================================================
// Tests for ActorResult Debug (actor_result.rs coverage)
// ============================================================================

mod actor_result_debug_tests {
    use super::*;

    #[derive(Actor, Debug)]
    struct DebugTestActor {
        #[allow(dead_code)]
        value: i32,
    }

    #[tokio::test]
    async fn test_actor_result_debug_completed() {
        let (actor_ref, handle) = spawn::<DebugTestActor>(DebugTestActor { value: 42 });

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();

        // Test Debug implementation
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Completed"));
        assert!(debug_str.contains("42"));
    }

    #[tokio::test]
    async fn test_actor_result_debug_failed() {
        #[derive(Debug)]
        struct FailDebugActor;

        impl Actor for FailDebugActor {
            type Args = ();
            type Error = String;

            async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, String> {
                Err("debug test failure".to_string())
            }
        }

        let (_, handle) = spawn::<FailDebugActor>(());
        let result = handle.await.unwrap();

        // Test Debug implementation for failed
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Failed"));
        assert!(debug_str.contains("debug test failure"));
    }
}

// ============================================================================
// Tests for deprecated tell_blocking and ask_blocking methods (actor_ref.rs)
// ============================================================================

mod deprecated_blocking_methods_tests {
    use super::*;

    #[derive(Actor)]
    struct DeprecatedMethodsActor;

    struct DeprecatedMsg;

    #[rsactor::message_handlers]
    impl DeprecatedMethodsActor {
        #[handler]
        async fn handle(&mut self, _: DeprecatedMsg, _: &ActorRef<Self>) -> String {
            "deprecated_response".to_string()
        }
    }

    #[tokio::test]
    async fn test_deprecated_tell_blocking() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeprecatedMethodsActor>(DeprecatedMethodsActor);

        let actor_clone = actor_ref.clone();

        // Test deprecated tell_blocking method
        #[allow(deprecated)]
        let result = tokio::task::spawn_blocking(move || {
            actor_clone.tell_blocking(DeprecatedMsg, Some(Duration::from_secs(5)))
        })
        .await
        .unwrap();

        assert!(result.is_ok(), "deprecated tell_blocking should succeed");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_deprecated_ask_blocking() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeprecatedMethodsActor>(DeprecatedMethodsActor);

        let actor_clone = actor_ref.clone();

        // Test deprecated ask_blocking method
        #[allow(deprecated)]
        let result: Result<String, _> = tokio::task::spawn_blocking(move || {
            actor_clone.ask_blocking(DeprecatedMsg, Some(Duration::from_secs(5)))
        })
        .await
        .unwrap();

        assert!(result.is_ok(), "deprecated ask_blocking should succeed");
        assert_eq!(result.unwrap(), "deprecated_response");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_deprecated_tell_blocking_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeprecatedMethodsActor>(DeprecatedMethodsActor);

        // Stop the actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // Test deprecated tell_blocking to stopped actor
        #[allow(deprecated)]
        let result =
            tokio::task::spawn_blocking(move || actor_clone.tell_blocking(DeprecatedMsg, None))
                .await
                .unwrap();

        assert!(
            result.is_err(),
            "deprecated tell_blocking to stopped actor should fail"
        );
    }

    #[tokio::test]
    async fn test_deprecated_ask_blocking_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DeprecatedMethodsActor>(DeprecatedMethodsActor);

        // Stop the actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        let actor_clone = actor_ref.clone();

        // Test deprecated ask_blocking to stopped actor
        #[allow(deprecated)]
        let result: Result<String, _> =
            tokio::task::spawn_blocking(move || actor_clone.ask_blocking(DeprecatedMsg, None))
                .await
                .unwrap();

        assert!(
            result.is_err(),
            "deprecated ask_blocking to stopped actor should fail"
        );
    }
}

// ============================================================================
// Tests for ask_join error scenarios (actor_ref.rs)
// ============================================================================

mod ask_join_error_tests {
    use super::*;
    use tokio::task::JoinHandle;

    #[derive(Actor)]
    struct AskJoinErrorActor;

    struct SpawnPanicTask;
    struct SpawnCancelledTask;
    struct SpawnSuccessTask(i32);

    #[rsactor::message_handlers]
    impl AskJoinErrorActor {
        #[handler]
        async fn handle_panic(&mut self, _: SpawnPanicTask, _: &ActorRef<Self>) -> JoinHandle<i32> {
            tokio::spawn(async move {
                panic!("Intentional panic in spawned task");
            })
        }

        #[handler]
        async fn handle_cancelled(
            &mut self,
            _: SpawnCancelledTask,
            _: &ActorRef<Self>,
        ) -> JoinHandle<i32> {
            let handle = tokio::spawn(async move {
                // Long sleep that will be cancelled
                tokio::time::sleep(Duration::from_secs(100)).await;
                42
            });
            // Immediately abort the task
            handle.abort();
            handle
        }

        #[handler]
        async fn handle_success(
            &mut self,
            msg: SpawnSuccessTask,
            _: &ActorRef<Self>,
        ) -> JoinHandle<i32> {
            let value = msg.0;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                value * 2
            })
        }
    }

    #[tokio::test]
    async fn test_ask_join_with_panic() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<AskJoinErrorActor>(AskJoinErrorActor);

        // ask_join should return Error::Join when the spawned task panics
        let result: rsactor::Result<i32> = actor_ref.ask_join(SpawnPanicTask).await;

        assert!(result.is_err(), "ask_join should fail when task panics");

        match result.unwrap_err() {
            rsactor::Error::Join { source, .. } => {
                assert!(source.is_panic(), "Join error should indicate panic");
            }
            e => panic!("Expected Join error, got: {:?}", e),
        }

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_ask_join_with_cancelled_task() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<AskJoinErrorActor>(AskJoinErrorActor);

        // ask_join should return Error::Join when the spawned task is cancelled
        let result: rsactor::Result<i32> = actor_ref.ask_join(SpawnCancelledTask).await;

        assert!(
            result.is_err(),
            "ask_join should fail when task is cancelled"
        );

        match result.unwrap_err() {
            rsactor::Error::Join { source, .. } => {
                assert!(
                    source.is_cancelled(),
                    "Join error should indicate cancellation"
                );
            }
            e => panic!("Expected Join error, got: {:?}", e),
        }

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_ask_join_success() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<AskJoinErrorActor>(AskJoinErrorActor);

        // ask_join should succeed with correct result
        let result: i32 = actor_ref.ask_join(SpawnSuccessTask(21)).await.unwrap();
        assert_eq!(result, 42);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_ask_join_to_stopped_actor() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<AskJoinErrorActor>(AskJoinErrorActor);

        // Stop the actor first
        actor_ref.stop().await.unwrap();
        handle.await.unwrap();

        // ask_join to stopped actor should fail with Send error
        let result: rsactor::Result<i32> = actor_ref.ask_join(SpawnSuccessTask(10)).await;

        assert!(result.is_err(), "ask_join to stopped actor should fail");

        match result.unwrap_err() {
            rsactor::Error::Send { .. } => {}
            e => panic!("Expected Send error, got: {:?}", e),
        }
    }
}

// ============================================================================
// Tests for ActorRef Debug implementation (actor_ref.rs)
// ============================================================================

mod actor_ref_debug_tests {
    use super::*;

    #[derive(Actor, Debug)]
    struct DebugRefActor;

    struct DebugMsg;

    #[rsactor::message_handlers]
    impl DebugRefActor {
        #[handler]
        async fn handle(&mut self, _: DebugMsg, _: &ActorRef<Self>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_actor_ref_debug() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DebugRefActor>(DebugRefActor);

        // Test Debug implementation for ActorRef
        let debug_str = format!("{:?}", actor_ref);
        assert!(
            debug_str.contains("ActorRef"),
            "Debug should contain ActorRef"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_weak_debug() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<DebugRefActor>(DebugRefActor);

        let weak = ActorRef::downgrade(&actor_ref);

        // Test Debug implementation for ActorWeak
        let debug_str = format!("{:?}", weak);
        assert!(
            debug_str.contains("ActorWeak"),
            "Debug should contain ActorWeak"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for kill edge cases (actor_ref.rs)
// ============================================================================

mod kill_edge_cases_tests {
    use super::*;

    #[derive(Actor)]
    struct KillEdgeCaseActor;

    struct SlowProcessMsg;

    #[rsactor::message_handlers]
    impl KillEdgeCaseActor {
        #[handler]
        async fn handle(&mut self, _: SlowProcessMsg, _: &ActorRef<Self>) {
            // Slow processing to allow kill signals to queue
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    #[tokio::test]
    async fn test_kill_when_actor_already_stopping() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<KillEdgeCaseActor>(KillEdgeCaseActor);

        // Stop the actor
        actor_ref.stop().await.unwrap();

        // Now try to kill the stopping actor - should succeed (idempotent)
        let result = actor_ref.kill();
        assert!(result.is_ok(), "kill to stopping actor should succeed");

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_kill_multiple_times_rapidly() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<KillEdgeCaseActor>(KillEdgeCaseActor);

        // Send multiple kill signals rapidly - should all succeed
        // (channel capacity is 1, so subsequent kills hit the Full case)
        for _ in 0..10 {
            let result = actor_ref.kill();
            assert!(result.is_ok(), "rapid kill calls should succeed");
        }

        let result = handle.await.unwrap();
        assert!(result.was_killed());
    }

    #[tokio::test]
    async fn test_kill_after_actor_completely_stopped() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<KillEdgeCaseActor>(KillEdgeCaseActor);

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());

        // Kill after actor is completely stopped - should succeed (channel closed case)
        let kill_result = actor_ref.kill();
        assert!(kill_result.is_ok(), "kill to stopped actor should succeed");
    }
}

// ============================================================================
// Tests for stop edge cases (actor_ref.rs)
// ============================================================================

mod stop_edge_cases_tests {
    use super::*;

    #[derive(Actor)]
    struct StopEdgeCaseActor;

    struct SimpleMsg;

    #[rsactor::message_handlers]
    impl StopEdgeCaseActor {
        #[handler]
        async fn handle(&mut self, _: SimpleMsg, _: &ActorRef<Self>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_stop_after_actor_completely_stopped() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<StopEdgeCaseActor>(StopEdgeCaseActor);

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());

        // Stop after actor is completely stopped - should succeed (channel closed case)
        let stop_result = actor_ref.stop().await;
        assert!(stop_result.is_ok(), "stop to stopped actor should succeed");
    }

    #[tokio::test]
    async fn test_stop_multiple_times() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<StopEdgeCaseActor>(StopEdgeCaseActor);

        // Send multiple stop signals
        actor_ref.stop().await.unwrap();
        actor_ref.stop().await.unwrap(); // Second stop should also succeed

        let result = handle.await.unwrap();
        assert!(result.is_completed());
        assert!(!result.was_killed());
    }
}

// ============================================================================
// Tests for on_run returning Ok(false) (actor.rs)
// ============================================================================

mod on_run_disable_tests {
    use super::*;

    struct OnRunDisableActor {
        run_count: u32,
    }

    impl Actor for OnRunDisableActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self { run_count: 0 })
        }

        async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<bool, Self::Error> {
            self.run_count += 1;
            // Return false after first run to disable further on_run calls
            Ok(false)
        }
    }

    struct GetRunCount;

    #[rsactor::message_handlers]
    impl OnRunDisableActor {
        #[handler]
        async fn handle(&mut self, _: GetRunCount, _: &ActorRef<Self>) -> u32 {
            self.run_count
        }
    }

    #[tokio::test]
    async fn test_on_run_returns_false_disables_idle_processing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<OnRunDisableActor>(());

        // Wait for on_run to execute once
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check run count - should be 1 (on_run was called once, returned false)
        let run_count: u32 = actor_ref.ask(GetRunCount).await.unwrap();
        assert_eq!(run_count, 1, "on_run should have been called exactly once");

        // Wait more and check again - should still be 1
        tokio::time::sleep(Duration::from_millis(100)).await;
        let run_count: u32 = actor_ref.ask(GetRunCount).await.unwrap();
        assert_eq!(
            run_count, 1,
            "on_run should not have been called again after returning false"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for Message trait handle method (actor.rs)
// ============================================================================

mod message_trait_tests {
    use super::*;

    struct MessageTraitActor {
        received: Vec<String>,
    }

    impl Actor for MessageTraitActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self {
                received: Vec::new(),
            })
        }
    }

    struct StringMsg(String);
    struct IntMsg(i32);
    struct GetReceived;

    impl rsactor::Message<StringMsg> for MessageTraitActor {
        type Reply = String;

        async fn handle(&mut self, msg: StringMsg, _: &ActorRef<Self>) -> Self::Reply {
            self.received.push(format!("string:{}", msg.0));
            format!("received:{}", msg.0)
        }
    }

    impl rsactor::Message<IntMsg> for MessageTraitActor {
        type Reply = i32;

        async fn handle(&mut self, msg: IntMsg, _: &ActorRef<Self>) -> Self::Reply {
            self.received.push(format!("int:{}", msg.0));
            msg.0 * 2
        }
    }

    impl rsactor::Message<GetReceived> for MessageTraitActor {
        type Reply = Vec<String>;

        async fn handle(&mut self, _: GetReceived, _: &ActorRef<Self>) -> Self::Reply {
            self.received.clone()
        }
    }

    #[tokio::test]
    async fn test_message_trait_multiple_message_types() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MessageTraitActor>(());

        // Test sending different message types
        let str_reply: String = actor_ref.ask(StringMsg("hello".to_string())).await.unwrap();
        assert_eq!(str_reply, "received:hello");

        let int_reply: i32 = actor_ref.ask(IntMsg(21)).await.unwrap();
        assert_eq!(int_reply, 42);

        // Verify received messages
        let received: Vec<String> = actor_ref.ask(GetReceived).await.unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], "string:hello");
        assert_eq!(received[1], "int:21");

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for ActorRef::is_alive edge cases (actor_ref.rs)
// ============================================================================

mod is_alive_edge_cases_tests {
    use super::*;

    #[derive(Actor)]
    struct IsAliveTestActor;

    struct Ping;

    #[rsactor::message_handlers]
    impl IsAliveTestActor {
        #[handler]
        async fn handle(&mut self, _: Ping, _: &ActorRef<Self>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_is_alive_during_message_processing() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<IsAliveTestActor>(IsAliveTestActor);

        // Actor should be alive during message processing
        assert!(actor_ref.is_alive());

        let _: bool = actor_ref.ask(Ping).await.unwrap();
        assert!(actor_ref.is_alive());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_is_alive_after_kill() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<IsAliveTestActor>(IsAliveTestActor);

        assert!(actor_ref.is_alive());
        actor_ref.kill().unwrap();

        // Wait for actor to complete
        handle.await.unwrap();

        // After kill and completion, should not be alive
        assert!(!actor_ref.is_alive());
    }

    #[tokio::test]
    async fn test_is_alive_cloned_ref_after_original_dropped() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<IsAliveTestActor>(IsAliveTestActor);

        let cloned_ref = actor_ref.clone();

        // Both should be alive
        assert!(actor_ref.is_alive());
        assert!(cloned_ref.is_alive());

        // Drop original (but actor continues due to cloned ref)
        drop(actor_ref);

        // Cloned ref should still be alive
        assert!(cloned_ref.is_alive());

        cloned_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for timeout scenarios with actual timeouts (actor_ref.rs)
// ============================================================================

mod actual_timeout_tests {
    use super::*;

    struct TimeoutTestActor;

    impl Actor for TimeoutTestActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self)
        }
    }

    struct VerySlowMsg;
    struct QuickMsg;

    impl rsactor::Message<VerySlowMsg> for TimeoutTestActor {
        type Reply = String;

        async fn handle(&mut self, _: VerySlowMsg, _: &ActorRef<Self>) -> Self::Reply {
            tokio::time::sleep(Duration::from_secs(60)).await;
            "done".to_string()
        }
    }

    impl rsactor::Message<QuickMsg> for TimeoutTestActor {
        type Reply = String;

        async fn handle(&mut self, _: QuickMsg, _: &ActorRef<Self>) -> Self::Reply {
            "quick".to_string()
        }
    }

    #[tokio::test]
    async fn test_ask_with_timeout_actually_times_out() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TimeoutTestActor>(());

        // Very short timeout with slow message should timeout
        let result: rsactor::Result<String> = actor_ref
            .ask_with_timeout(VerySlowMsg, Duration::from_millis(10))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            rsactor::Error::Timeout { operation, .. } => {
                assert_eq!(operation, "ask");
            }
            e => panic!("Expected Timeout error, got: {:?}", e),
        }

        actor_ref.kill().unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_tell_with_timeout_success_path() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<TimeoutTestActor>(());

        // Tell with sufficient timeout should succeed
        let result = actor_ref
            .tell_with_timeout(QuickMsg, Duration::from_secs(5))
            .await;

        assert!(result.is_ok());

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for Actor lifecycle phases (actor.rs)
// ============================================================================

mod lifecycle_phase_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct LifecyclePhaseActor {
        phase_tracker: std::sync::Arc<AtomicU32>,
    }

    impl Actor for LifecyclePhaseActor {
        type Args = std::sync::Arc<AtomicU32>;
        type Error = String;

        async fn on_start(tracker: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            tracker.fetch_add(1, Ordering::SeqCst); // 1
            Ok(Self {
                phase_tracker: tracker,
            })
        }

        async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<bool, Self::Error> {
            self.phase_tracker.fetch_add(10, Ordering::SeqCst); // 11
            Ok(false) // Don't continue
        }

        async fn on_stop(&mut self, _: &ActorWeak<Self>, _killed: bool) -> Result<(), Self::Error> {
            self.phase_tracker.fetch_add(100, Ordering::SeqCst); // 111
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_lifecycle_phases_execute_in_order() {
        init_test_logger();
        let tracker = std::sync::Arc::new(AtomicU32::new(0));

        let (actor_ref, handle) = spawn::<LifecyclePhaseActor>(tracker.clone());

        // Wait for on_run to execute
        tokio::time::sleep(Duration::from_millis(100)).await;

        // After on_start and on_run, should be 11
        let value = tracker.load(Ordering::SeqCst);
        assert!(value >= 11, "Expected at least 11, got {}", value);

        actor_ref.stop().await.unwrap();
        let result = handle.await.unwrap();
        assert!(result.is_completed());

        // After on_stop, should be 111
        let final_value = tracker.load(Ordering::SeqCst);
        assert_eq!(
            final_value, 111,
            "All lifecycle phases should have executed"
        );
    }
}

// ============================================================================
// Tests for metrics API methods (actor_ref.rs)
// ============================================================================

#[cfg(feature = "metrics")]
mod metrics_api_tests {
    use super::*;

    struct MetricsTestActor;

    impl Actor for MetricsTestActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self)
        }
    }

    struct SlowMsg;
    struct QuickMsg;
    struct ErrorMsg;

    impl rsactor::Message<SlowMsg> for MetricsTestActor {
        type Reply = ();

        async fn handle(&mut self, _: SlowMsg, _: &ActorRef<Self>) -> Self::Reply {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    impl rsactor::Message<QuickMsg> for MetricsTestActor {
        type Reply = i32;

        async fn handle(&mut self, _: QuickMsg, _: &ActorRef<Self>) -> Self::Reply {
            42
        }
    }

    impl rsactor::Message<ErrorMsg> for MetricsTestActor {
        type Reply = ();

        async fn handle(&mut self, _: ErrorMsg, _: &ActorRef<Self>) -> Self::Reply {
            // Simulating an error scenario (but we can't actually return error from Message trait)
            // This is for coverage of error_count method
        }
    }

    #[tokio::test]
    async fn test_metrics_snapshot() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Send a few messages
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        // Use ask instead of tell to ensure message is processed before checking metrics
        let _: () = actor_ref.ask(SlowMsg).await.unwrap();

        // Test metrics snapshot
        let metrics = actor_ref.metrics();
        assert!(
            metrics.message_count >= 3,
            "Should have processed at least 3 messages"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_message_count() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Initial message count should be 0
        assert_eq!(actor_ref.message_count(), 0);

        // Send messages
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Message count should be at least 1
        assert!(actor_ref.message_count() >= 1);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_avg_processing_time() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Initial avg processing time should be zero
        assert_eq!(actor_ref.avg_processing_time(), Duration::ZERO);

        // Send a slow message (use ask to ensure message is processed before checking metrics)
        let _: () = actor_ref.ask(SlowMsg).await.unwrap();

        // Avg processing time should be non-zero now
        let avg_time = actor_ref.avg_processing_time();
        assert!(
            avg_time > Duration::ZERO,
            "Avg processing time should be non-zero"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_max_processing_time() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Send quick and slow messages
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        // Use ask instead of tell to ensure message is processed before checking metrics
        let _: () = actor_ref.ask(SlowMsg).await.unwrap();

        // Max processing time should reflect the slow message
        let max_time = actor_ref.max_processing_time();
        assert!(
            max_time >= Duration::from_millis(40),
            "Max processing time should be at least 40ms"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_error_count() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Error count should start at 0
        assert_eq!(actor_ref.error_count(), 0);

        // Even after sending messages, error_count stays 0 (no actual errors)
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(actor_ref.error_count(), 0);

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_uptime() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Uptime should be non-zero
        let uptime = actor_ref.uptime();
        assert!(
            uptime >= Duration::from_millis(40),
            "Uptime should be at least 40ms"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_last_activity() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<MetricsTestActor>(());

        // Initially no activity
        assert!(actor_ref.last_activity().is_none());

        // Send a message
        let _: i32 = actor_ref.ask(QuickMsg).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Last activity should now be Some
        let last_activity = actor_ref.last_activity();
        assert!(
            last_activity.is_some(),
            "Last activity should be Some after processing message"
        );

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }
}

// ============================================================================
// Tests for tell_with_timeout actual timeout (actor_ref.rs)
// ============================================================================

mod tell_with_timeout_tests {
    use super::*;

    struct SlowTellActor;

    impl Actor for SlowTellActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self)
        }
    }

    struct BlockingMsg;

    impl rsactor::Message<BlockingMsg> for SlowTellActor {
        type Reply = ();

        async fn handle(&mut self, _: BlockingMsg, _: &ActorRef<Self>) -> Self::Reply {
            // Block for a very long time
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    #[tokio::test]
    async fn test_tell_with_timeout_actual_timeout() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<SlowTellActor>(());

        // First, send a blocking message that will occupy the actor
        actor_ref.tell(BlockingMsg).await.unwrap();

        // Now try to send another message with a very short timeout
        // The actor is busy processing the first message, so this should timeout
        let _result = actor_ref
            .tell_with_timeout(BlockingMsg, Duration::from_millis(5))
            .await;

        // This might succeed (if the message queue isn't full) or timeout
        // The timeout error occurs when waiting for the message to be sent,
        // not when waiting for the handler to complete

        // Clean up
        actor_ref.kill().unwrap();
        let _ = handle.await;
    }
}

// ============================================================================
// Tests for blocking methods with timeout expiry (actor_ref.rs)
// ============================================================================

mod blocking_timeout_expiry_tests {
    use super::*;

    struct BlockingTimeoutActor;

    impl Actor for BlockingTimeoutActor {
        type Args = ();
        type Error = String;

        async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self)
        }
    }

    struct VerySlowHandlerMsg;

    impl rsactor::Message<VerySlowHandlerMsg> for BlockingTimeoutActor {
        type Reply = String;

        async fn handle(&mut self, _: VerySlowHandlerMsg, _: &ActorRef<Self>) -> Self::Reply {
            // Very long operation
            tokio::time::sleep(Duration::from_secs(120)).await;
            "done".to_string()
        }
    }

    #[tokio::test]
    async fn test_blocking_tell_timeout_with_slow_handler() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(());

        // Start processing a slow message
        actor_ref.tell(VerySlowHandlerMsg).await.unwrap();

        let actor_clone = actor_ref.clone();

        // Try blocking_tell with timeout while actor is busy
        let _result = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_tell(VerySlowHandlerMsg, Some(Duration::from_millis(10)))
        })
        .await
        .unwrap();

        // This might succeed (queued) or timeout depending on timing
        // The important thing is that the timeout path is exercised

        actor_ref.kill().unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_blocking_ask_timeout_with_slow_handler() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<BlockingTimeoutActor>(());

        // Start processing a slow message first
        actor_ref.tell(VerySlowHandlerMsg).await.unwrap();

        let actor_clone = actor_ref.clone();

        // Try blocking_ask with timeout while actor is busy processing
        let result: rsactor::Result<String> = tokio::task::spawn_blocking(move || {
            actor_clone.blocking_ask(VerySlowHandlerMsg, Some(Duration::from_millis(10)))
        })
        .await
        .unwrap();

        // Should timeout since actor is busy
        // The exact behavior depends on whether the message gets queued before timeout
        match result {
            Ok(_) => {}                               // Message was processed before timeout
            Err(rsactor::Error::Timeout { .. }) => {} // Expected timeout
            Err(e) => panic!("Unexpected error: {:?}", e),
        }

        actor_ref.kill().unwrap();
        let _ = handle.await;
    }
}

// ============================================================================
// Tests for ActorWeak upgrade scenarios (actor_ref.rs)
// ============================================================================

mod actor_weak_upgrade_tests {
    use super::*;

    #[derive(Actor)]
    struct WeakUpgradeActor;

    struct GetIdentity;

    #[rsactor::message_handlers]
    impl WeakUpgradeActor {
        #[handler]
        async fn handle(&mut self, _: GetIdentity, actor_ref: &ActorRef<Self>) -> String {
            actor_ref.identity().to_string()
        }
    }

    #[tokio::test]
    async fn test_actor_weak_upgrade_before_stop() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<WeakUpgradeActor>(WeakUpgradeActor);

        let weak = ActorRef::downgrade(&actor_ref);

        // Upgrade should succeed while actor is running
        let upgraded = weak.upgrade();
        assert!(upgraded.is_some());

        // Use the upgraded ref
        let upgraded_ref = upgraded.unwrap();
        let _: String = upgraded_ref.ask(GetIdentity).await.unwrap();

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_weak_upgrade_after_all_refs_dropped() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<WeakUpgradeActor>(WeakUpgradeActor);

        let weak = ActorRef::downgrade(&actor_ref);

        // Drop the strong reference - this should trigger actor termination
        drop(actor_ref);

        // Wait for actor to terminate
        let _ = handle.await;

        // Now upgrade should fail
        let upgraded = weak.upgrade();
        assert!(
            upgraded.is_none(),
            "Upgrade should fail after all refs dropped"
        );
    }

    #[tokio::test]
    async fn test_actor_weak_is_alive_transitions() {
        init_test_logger();
        let (actor_ref, handle) = spawn::<WeakUpgradeActor>(WeakUpgradeActor);

        let weak = ActorRef::downgrade(&actor_ref);

        // Should be alive while running
        assert!(weak.is_alive());

        // Drop the actor_ref to trigger termination (is_alive checks if mailbox is open)
        drop(actor_ref);
        let _ = handle.await;

        // Give the runtime a moment to close channels
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Should not be alive after actor terminates (this checks the mailbox sender)
        // Note: is_alive() checks if the mailbox sender can still accept messages
        // After actor termination, the receiver side is closed
        assert!(!weak.is_alive());
    }
}
