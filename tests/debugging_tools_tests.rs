// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for Phase 1 debugging tools: Enhanced Error Messages and Dead Letter Tracking
//!
//! These tests use delta comparison pattern (comparing before/after counts) for parallel test safety.
//! Run with: `cargo test --features test-utils`

use rsactor::{spawn, Actor, ActorRef, Error, Identity};
use std::time::Duration;

// ============ Error Method Tests ============

#[test]
fn test_is_retryable_for_all_variants() {
    let identity = Identity::new(1, "TestActor");

    // Timeout is retryable
    let timeout_err = Error::Timeout {
        identity,
        timeout: Duration::from_secs(1),
        operation: "ask".into(),
    };
    assert!(timeout_err.is_retryable());

    // All others are NOT retryable
    let send_err = Error::Send {
        identity,
        details: "channel closed".into(),
    };
    assert!(!send_err.is_retryable());

    let receive_err = Error::Receive {
        identity,
        details: "channel closed".into(),
    };
    assert!(!receive_err.is_retryable());

    let downcast_err = Error::Downcast {
        identity,
        expected_type: "String".into(),
    };
    assert!(!downcast_err.is_retryable());

    let runtime_err = Error::Runtime {
        identity,
        details: "test error".into(),
    };
    assert!(!runtime_err.is_retryable());

    let mailbox_err = Error::MailboxCapacity {
        message: "invalid capacity".into(),
    };
    assert!(!mailbox_err.is_retryable());
}

#[test]
fn test_all_errors_have_debugging_tips() {
    let identity = Identity::new(1, "TestActor");

    // Test all 6 variants that don't require JoinError
    let errors: Vec<Error> = vec![
        Error::Send {
            identity,
            details: "test".into(),
        },
        Error::Receive {
            identity,
            details: "test".into(),
        },
        Error::Timeout {
            identity,
            timeout: Duration::from_secs(1),
            operation: "ask".into(),
        },
        Error::Downcast {
            identity,
            expected_type: "String".into(),
        },
        Error::Runtime {
            identity,
            details: "test".into(),
        },
        Error::MailboxCapacity {
            message: "test".into(),
        },
    ];

    for err in &errors {
        let tips = err.debugging_tips();
        assert!(!tips.is_empty(), "Missing tips for: {:?}", err);
        // Verify tips are actionable (contain verbs or specific guidance)
        for tip in tips {
            assert!(tip.len() > 10, "Tip too short to be useful: {}", tip);
        }
    }
}

#[test]
fn test_runtime_error_tips_are_specific() {
    let identity = Identity::new(1, "TestActor");
    let err = Error::Runtime {
        identity,
        details: "test".into(),
    };
    let tips = err.debugging_tips();

    // Verify tips mention specific lifecycle methods
    let tips_text = tips.join(" ");
    assert!(
        tips_text.contains("on_start") || tips_text.contains("on_run"),
        "Runtime tips should mention lifecycle methods"
    );
    assert!(
        tips_text.contains("ActorResult"),
        "Runtime tips should mention ActorResult"
    );
}

#[test]
fn test_downcast_error_debugging_tips() {
    let identity = Identity::new(1, "TestActor");
    let err = Error::Downcast {
        identity,
        expected_type: "String".into(),
    };

    let tips = err.debugging_tips();
    assert!(!tips.is_empty(), "Downcast error should have tips");

    // Verify tips mention Message trait
    let tips_text = tips.join(" ");
    assert!(
        tips_text.contains("Message") || tips_text.contains("handler"),
        "Downcast tips should mention Message trait or handler"
    );
}

#[test]
fn test_mailbox_capacity_error_tips() {
    // Test: MailboxCapacity error has useful debugging tips
    let err = Error::MailboxCapacity {
        message: "capacity must be greater than 0".into(),
    };

    let tips = err.debugging_tips();
    assert!(
        !tips.is_empty(),
        "MailboxCapacity should have debugging tips"
    );

    let tips_text = tips.join(" ");
    assert!(
        tips_text.contains("greater than 0") || tips_text.contains("capacity"),
        "Tips should mention capacity requirements"
    );
    assert!(
        tips_text.contains("set_default_mailbox_capacity") || tips_text.contains("once"),
        "Tips should mention set_default_mailbox_capacity behavior"
    );
}

#[test]
fn test_error_display_all_variants() {
    let identity = Identity::new(1, "TestActor");

    // Verify Display impl produces useful output
    let errors = vec![
        Error::Send {
            identity,
            details: "channel closed".into(),
        },
        Error::Receive {
            identity,
            details: "reply dropped".into(),
        },
        Error::Timeout {
            identity,
            timeout: Duration::from_secs(5),
            operation: "ask".into(),
        },
        Error::Downcast {
            identity,
            expected_type: "String".into(),
        },
        Error::Runtime {
            identity,
            details: "panic in handler".into(),
        },
        Error::MailboxCapacity {
            message: "capacity must be > 0".into(),
        },
    ];

    for err in &errors {
        let display = format!("{}", err);
        assert!(!display.is_empty(), "Display should not be empty");
        assert!(display.len() > 5, "Display should be descriptive");
    }
}

// ============ Dead Letter Tests ============

#[derive(Actor)]
struct TestActor;

struct Ping;

#[rsactor::message_handlers]
impl TestActor {
    #[handler]
    async fn handle(&mut self, _: Ping, _: &ActorRef<Self>) {}
}

#[tokio::test]
async fn test_dead_letter_on_stopped_actor() {
    let initial = rsactor::dead_letter_count();

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // This should trigger a dead letter
    let result = actor_ref.tell(Ping).await;
    assert!(result.is_err());
    assert!(
        rsactor::dead_letter_count() > initial,
        "Dead letter should have been recorded"
    );
}

#[tokio::test]
async fn test_dead_letter_count_increments() {
    // Record initial count before our operations
    let initial = rsactor::dead_letter_count();

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Multiple failed sends should increment counter
    for _ in 0..3 {
        let _ = actor_ref.tell(Ping).await;
    }

    // Use >= comparison for parallel test safety (other tests may also increment)
    assert!(
        rsactor::dead_letter_count() - initial >= 3,
        "Should have recorded at least 3 dead letters"
    );
}

#[tokio::test]
async fn test_dead_letter_after_full_stop() {
    // Test: send message after actor has fully stopped
    let initial = rsactor::dead_letter_count();

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);

    // Stop and wait for handle to ensure actor is fully stopped
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Now try to send - this should definitely fail
    let result = actor_ref.tell(Ping).await;

    // Should be error since actor is stopped
    assert!(result.is_err());

    // Dead letter should be recorded
    assert!(
        rsactor::dead_letter_count() > initial,
        "Dead letter should be recorded after actor stopped"
    );
}

#[tokio::test]
async fn test_dead_letter_multiple_actors_isolation() {
    // Test: dead letters from one actor don't affect another (running actor should succeed)
    let initial = rsactor::dead_letter_count();

    let (actor1, handle1) = spawn::<TestActor>(TestActor);
    let (actor2, handle2) = spawn::<TestActor>(TestActor);

    // Stop only actor1
    actor1.stop().await.unwrap();
    handle1.await.unwrap();

    // Record count after actor1 stopped but before our sends
    let before_sends = rsactor::dead_letter_count();

    // Send to stopped actor1 (dead letter)
    let _ = actor1.tell(Ping).await;

    // Send to running actor2 (should succeed)
    let result = actor2.tell(Ping).await;
    assert!(result.is_ok(), "Message to running actor should succeed");

    // At least 1 dead letter from actor1 (our send)
    // Use >= because other parallel tests might also be generating dead letters
    assert!(
        rsactor::dead_letter_count() - before_sends >= 1,
        "Stopped actor should generate at least 1 dead letter"
    );

    // Cleanup: stop actor2
    actor2.stop().await.unwrap();
    handle2.await.unwrap();

    // Overall: we should have at least 1 more dead letter than when we started
    assert!(
        rsactor::dead_letter_count() > initial,
        "Should have recorded dead letters"
    );
}

#[tokio::test]
async fn test_dead_letter_with_timeout() {
    let initial = rsactor::dead_letter_count();

    // Create an actor with a full mailbox to force timeout
    // The actor is slow to process, so the second message should timeout waiting to enter mailbox
    #[derive(Actor)]
    struct SlowActor;

    struct SlowPing;

    #[rsactor::message_handlers]
    impl SlowActor {
        #[handler]
        async fn handle(&mut self, _: SlowPing, _: &ActorRef<Self>) {
            // Sleep long enough to cause timeout
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    // Spawn with small mailbox capacity
    let (actor_ref, handle) = rsactor::spawn_with_mailbox_capacity::<SlowActor>(SlowActor, 1);

    // First message will start processing (and sleep)
    let _ = actor_ref.tell(SlowPing).await;

    // Second message should timeout trying to send (mailbox is full while first is processing)
    let result = actor_ref
        .tell_with_timeout(SlowPing, Duration::from_millis(10))
        .await;

    // Either timeout or success is acceptable (depends on timing)
    // What we care about is that IF it times out, a dead letter is recorded
    if result.is_err() {
        assert!(
            rsactor::dead_letter_count() > initial,
            "Timeout should record dead letter"
        );
    }

    // Cleanup
    actor_ref.kill().unwrap();
    let _ = handle.await;
}

#[tokio::test]
async fn test_dead_letter_blocking_tell() {
    let initial = rsactor::dead_letter_count();

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Clone for blocking context
    let actor_ref_clone = actor_ref.clone();

    // Use blocking_tell on stopped actor
    let result = tokio::task::spawn_blocking(move || actor_ref_clone.blocking_tell(Ping, None))
        .await
        .unwrap();

    assert!(result.is_err());
    assert!(
        rsactor::dead_letter_count() > initial,
        "blocking_tell should record dead letter"
    );
}

#[tokio::test]
async fn test_dead_letter_ask_with_timeout() {
    let initial = rsactor::dead_letter_count();

    // Create a slow actor for timeout scenario
    #[derive(Actor)]
    struct SlowAskActor;

    struct SlowAskPing;

    #[rsactor::message_handlers]
    impl SlowAskActor {
        #[handler]
        async fn handle(&mut self, _: SlowAskPing, _: &ActorRef<Self>) -> String {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "pong".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<SlowAskActor>(SlowAskActor);

    // ask_with_timeout should timeout and record dead letter
    let result: Result<String, _> = actor_ref
        .ask_with_timeout(SlowAskPing, Duration::from_millis(10))
        .await;

    assert!(result.is_err());
    assert!(
        rsactor::dead_letter_count() > initial,
        "ask_with_timeout should record dead letter on timeout"
    );

    // Cleanup
    actor_ref.kill().unwrap();
    let _ = handle.await;
}

#[tokio::test]
async fn test_dead_letter_blocking_ask() {
    let initial = rsactor::dead_letter_count();

    #[derive(Actor)]
    struct BlockingAskActor;

    struct BlockingPing;

    #[rsactor::message_handlers]
    impl BlockingAskActor {
        #[handler]
        async fn handle(&mut self, _: BlockingPing, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<BlockingAskActor>(BlockingAskActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Clone for blocking context
    let actor_ref_clone = actor_ref.clone();

    // Use blocking_ask on stopped actor
    let result: Result<String, _> =
        tokio::task::spawn_blocking(move || actor_ref_clone.blocking_ask(BlockingPing, None))
            .await
            .unwrap();

    assert!(result.is_err());
    assert!(
        rsactor::dead_letter_count() > initial,
        "blocking_ask should record dead letter"
    );
}

#[tokio::test]
async fn test_dead_letter_ask_on_stopped_actor() {
    // Test: ask() on stopped actor triggers ActorStopped dead letter
    let initial = rsactor::dead_letter_count();

    #[derive(Actor)]
    struct AskActor;

    struct AskPing;

    #[rsactor::message_handlers]
    impl AskActor {
        #[handler]
        async fn handle(&mut self, _: AskPing, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<AskActor>(AskActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // ask() on stopped actor should trigger ActorStopped dead letter
    let result: Result<String, _> = actor_ref.ask(AskPing).await;
    assert!(result.is_err());
    assert!(
        rsactor::dead_letter_count() > initial,
        "ask on stopped actor should record dead letter"
    );
}

#[tokio::test]
async fn test_dead_letter_concurrent_asks_to_stopped_actor() {
    // Test: multiple concurrent asks to stopped actor should all record dead letters

    #[derive(Actor)]
    struct ConcurrentActor;

    struct ConcurrentPing;

    #[rsactor::message_handlers]
    impl ConcurrentActor {
        #[handler]
        async fn handle(&mut self, _: ConcurrentPing, _: &ActorRef<Self>) -> String {
            "pong".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<ConcurrentActor>(ConcurrentActor);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Record count right before our concurrent sends
    let before_sends = rsactor::dead_letter_count();

    // Fire 5 concurrent asks to stopped actor
    let mut handles = vec![];
    for _ in 0..5 {
        let actor = actor_ref.clone();
        handles.push(tokio::spawn(async move {
            let _: Result<String, _> = actor.ask(ConcurrentPing).await;
        }));
    }

    // Wait for all to complete
    for h in handles {
        h.await.unwrap();
    }

    // All 5 should generate dead letters (use >= for parallel test safety)
    assert!(
        rsactor::dead_letter_count() - before_sends >= 5,
        "All 5 concurrent asks should record dead letters"
    );
}

#[tokio::test]
async fn test_retry_pattern_with_is_retryable() {
    // Integration test: verify is_retryable() works in actual retry logic

    #[derive(Actor)]
    struct RetryActor {
        call_count: u32,
    }

    struct RetryPing;

    #[rsactor::message_handlers]
    impl RetryActor {
        #[handler]
        async fn handle(&mut self, _: RetryPing, _: &ActorRef<Self>) -> u32 {
            self.call_count += 1;
            self.call_count
        }
    }

    let (actor_ref, handle) = spawn::<RetryActor>(RetryActor { call_count: 0 });

    // Simulate retry pattern
    async fn send_with_retry(
        actor: &ActorRef<RetryActor>,
        max_attempts: usize,
    ) -> Result<u32, rsactor::Error> {
        let mut attempts = 0;
        loop {
            match actor
                .ask_with_timeout(RetryPing, Duration::from_millis(100))
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() && attempts < max_attempts => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    let result = send_with_retry(&actor_ref, 3).await;
    assert!(result.is_ok());

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

// ============ Additional Edge Case Tests ============

#[tokio::test]
async fn test_dead_letter_race_condition() {
    // Test: send message immediately after stop (race condition scenario)
    // Note: Due to race conditions, the message may succeed or fail depending on timing
    let initial = rsactor::dead_letter_count();

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);

    // Stop and immediately try to send (don't wait for handle)
    actor_ref.stop().await.unwrap();
    let result = actor_ref.tell(Ping).await;

    // Wait for handle to complete
    handle.await.unwrap();

    // If the send failed, a dead letter should have been recorded
    if result.is_err() {
        assert!(
            rsactor::dead_letter_count() > initial,
            "Dead letter should be recorded when send fails"
        );
    }
    // If the send succeeded (race condition), that's also acceptable behavior
}

#[tokio::test]
async fn test_dead_letter_reply_dropped() {
    // Test scenario: handler panics before sending reply
    let initial = rsactor::dead_letter_count();

    #[derive(Actor)]
    struct PanicActor;

    struct PanicPing;

    #[rsactor::message_handlers]
    impl PanicActor {
        #[handler]
        async fn handle(&mut self, _: PanicPing, _: &ActorRef<Self>) -> String {
            panic!("intentional panic before reply");
        }
    }

    let (actor_ref, handle) = spawn::<PanicActor>(PanicActor);

    // This should trigger ReplyDropped dead letter
    let result: Result<String, _> = actor_ref.ask(PanicPing).await;

    assert!(result.is_err());
    // Note: The actor may have terminated due to panic
    let _ = handle.await;

    // Dead letter should be recorded (either ReplyDropped or ActorStopped)
    assert!(
        rsactor::dead_letter_count() > initial,
        "Reply dropped scenario should record dead letter"
    );
}

#[tokio::test]
async fn test_dead_letter_timeout_while_stopping() {
    // Test: race condition between timeout and actor stopping
    // Note: Due to race conditions, the operation may succeed or fail depending on timing
    let initial = rsactor::dead_letter_count();

    #[derive(Actor)]
    struct RaceActor;

    struct RacePing;

    #[rsactor::message_handlers]
    impl RaceActor {
        #[handler]
        async fn handle(&mut self, _: RacePing, _: &ActorRef<Self>) {
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    let (actor_ref, handle) = spawn::<RaceActor>(RaceActor);

    // Start a timeout operation, then stop actor while it's pending
    let actor_clone = actor_ref.clone();
    let timeout_handle = tokio::spawn(async move {
        actor_clone
            .tell_with_timeout(RacePing, Duration::from_millis(50))
            .await
    });

    // Brief delay then stop actor
    tokio::time::sleep(Duration::from_millis(10)).await;
    actor_ref.stop().await.unwrap();

    let result = timeout_handle.await.unwrap();
    let _ = handle.await;

    // If the operation failed (timeout or actor stop), dead letter should be recorded
    if result.is_err() {
        assert!(
            rsactor::dead_letter_count() > initial,
            "Dead letter should be recorded when operation fails"
        );
    }
    // If the operation succeeded (race condition), that's also acceptable behavior
}

#[tokio::test]
async fn test_dead_letter_blocking_ask_reply_dropped() {
    // Test: blocking_ask when handler panics triggers ReplyDropped dead letter
    let initial = rsactor::dead_letter_count();

    #[derive(Actor)]
    struct PanicBlockingActor;

    struct PanicBlockingPing;

    #[rsactor::message_handlers]
    impl PanicBlockingActor {
        #[handler]
        async fn handle(&mut self, _: PanicBlockingPing, _: &ActorRef<Self>) -> String {
            panic!("intentional panic in blocking_ask test");
        }
    }

    let (actor_ref, handle) = spawn::<PanicBlockingActor>(PanicBlockingActor);
    let actor_ref_clone = actor_ref.clone();

    // blocking_ask with panicking handler should trigger dead letter
    let result: Result<String, _> =
        tokio::task::spawn_blocking(move || actor_ref_clone.blocking_ask(PanicBlockingPing, None))
            .await
            .unwrap();

    assert!(result.is_err());
    let _ = handle.await;

    assert!(
        rsactor::dead_letter_count() > initial,
        "blocking_ask with panic should record dead letter"
    );
}

// ============ Additional Coverage Tests ============

#[test]
fn test_dead_letter_reason_display() {
    use rsactor::DeadLetterReason;

    // Test Display impl for all DeadLetterReason variants
    assert_eq!(
        format!("{}", DeadLetterReason::ActorStopped),
        "actor stopped"
    );
    assert_eq!(format!("{}", DeadLetterReason::Timeout), "timeout");
    assert_eq!(
        format!("{}", DeadLetterReason::ReplyDropped),
        "reply dropped"
    );
}

// NOTE: test_reset_dead_letter_count was moved to tests/dead_letter_reset_test.rs
// to run in a separate process — reset_dead_letter_count() sets the global counter to 0,
// which interferes with delta-based assertions in parallel tests.

#[tokio::test]
async fn test_error_join_display() {
    use std::error::Error as StdError;

    // Create a JoinError by spawning a task that panics
    let handle = tokio::spawn(async {
        panic!("test panic for JoinError");
    });

    let join_error = handle.await.unwrap_err();
    let identity = Identity::new(1, "TestActor");

    let error = Error::Join {
        identity,
        source: join_error,
    };

    let display = format!("{}", error);
    assert!(display.contains("Failed to join"));
    assert!(display.contains("TestActor"));

    // Also test that source() returns Some for Join errors
    assert!(error.source().is_some(), "Join error should have a source");
}

#[test]
fn test_error_source_returns_none_for_non_join() {
    use std::error::Error as StdError;

    let identity = Identity::new(1, "TestActor");

    // Test that source() returns None for all non-Join error variants
    let errors: Vec<Error> = vec![
        Error::Send {
            identity,
            details: "test".into(),
        },
        Error::Receive {
            identity,
            details: "test".into(),
        },
        Error::Timeout {
            identity,
            timeout: Duration::from_secs(1),
            operation: "ask".into(),
        },
        Error::Downcast {
            identity,
            expected_type: "String".into(),
        },
        Error::Runtime {
            identity,
            details: "test".into(),
        },
        Error::MailboxCapacity {
            message: "test".into(),
        },
    ];

    for err in &errors {
        assert!(
            err.source().is_none(),
            "Non-Join error {:?} should have no source",
            err
        );
    }
}

#[tokio::test]
async fn test_error_join_debugging_tips() {
    // Create a JoinError by spawning a task that panics
    let handle = tokio::spawn(async {
        panic!("test panic for debugging tips");
    });

    let join_error = handle.await.unwrap_err();
    let identity = Identity::new(1, "TestActor");

    let error = Error::Join {
        identity,
        source: join_error,
    };

    let tips = error.debugging_tips();
    assert!(!tips.is_empty(), "Join error should have debugging tips");

    let tips_text = tips.join(" ");
    assert!(
        tips_text.contains("panic") || tips_text.contains("cancelled"),
        "Join tips should mention panic or cancellation"
    );
    assert!(
        tips_text.contains("RUST_BACKTRACE"),
        "Join tips should mention RUST_BACKTRACE"
    );
}

#[tokio::test]
async fn test_error_join_is_not_retryable() {
    // Create a JoinError by spawning a task that panics
    let handle = tokio::spawn(async {
        panic!("test panic for retryable check");
    });

    let join_error = handle.await.unwrap_err();
    let identity = Identity::new(1, "TestActor");

    let error = Error::Join {
        identity,
        source: join_error,
    };

    assert!(!error.is_retryable(), "Join error should not be retryable");
}

// ============ Actor Lifecycle Coverage Tests ============

#[tokio::test]
async fn test_kill_when_terminate_channel_full() {
    // Test: calling kill() twice quickly should trigger the "Full" branch
    // The terminate channel has capacity of 1, so second kill should hit Full case

    #[derive(Actor)]
    struct SlowStopActor;

    struct SlowMessage;

    #[rsactor::message_handlers]
    impl SlowStopActor {
        #[handler]
        async fn handle(&mut self, _: SlowMessage, _: &ActorRef<Self>) {
            // Sleep to keep the actor busy
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    let (actor_ref, handle) = spawn::<SlowStopActor>(SlowStopActor);

    // Send a message to keep the actor busy
    let _ = actor_ref.tell(SlowMessage).await;

    // First kill should succeed
    let result1 = actor_ref.kill();
    assert!(result1.is_ok(), "First kill should succeed");

    // Second kill should also succeed (hits Full branch - returns Ok)
    let result2 = actor_ref.kill();
    assert!(
        result2.is_ok(),
        "Second kill should succeed (terminate channel full case)"
    );

    // Clean up
    let _ = handle.await;
}

#[tokio::test]
async fn test_kill_when_actor_already_stopped() {
    // Test: calling kill() after actor has stopped should trigger the "Closed" branch

    let (actor_ref, handle) = spawn::<TestActor>(TestActor);

    // Stop the actor gracefully and wait for it to finish
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    // Now kill should hit the Closed branch (channel is closed)
    let result = actor_ref.kill();
    assert!(
        result.is_ok(),
        "Kill on stopped actor should succeed (terminate channel closed case)"
    );
}

#[tokio::test]
async fn test_default_on_run_behavior() {
    // Test: actor with default on_run() should work correctly
    // The default on_run() sleeps for 1 second in a loop

    #[derive(Actor)]
    struct DefaultRunActor {
        received: bool,
    }

    struct CheckMessage;

    #[rsactor::message_handlers]
    impl DefaultRunActor {
        #[handler]
        async fn handle(&mut self, _: CheckMessage, _: &ActorRef<Self>) -> bool {
            self.received = true;
            self.received
        }
    }

    let (actor_ref, handle) = spawn::<DefaultRunActor>(DefaultRunActor { received: false });

    // Actor should be able to process messages while on_run is sleeping
    let result: bool = actor_ref.ask(CheckMessage).await.unwrap();
    assert!(result, "Actor with default on_run should process messages");

    // Stop the actor
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_deprecated_tell_blocking_method() {
    // Test: deprecated tell_blocking method should still work
    // This covers the deprecated method lines

    #[derive(Actor)]
    struct DeprecatedTestActor {
        count: u32,
    }

    struct DeprecatedMessage;

    #[rsactor::message_handlers]
    impl DeprecatedTestActor {
        #[handler]
        async fn handle(&mut self, _: DeprecatedMessage, _: &ActorRef<Self>) {
            self.count += 1;
        }
    }

    let (actor_ref, handle) = spawn::<DeprecatedTestActor>(DeprecatedTestActor { count: 0 });

    let actor_ref_clone = actor_ref.clone();

    // Use deprecated tell_blocking (with #[allow(deprecated)] to suppress warning)
    #[allow(deprecated)]
    let result = tokio::task::spawn_blocking(move || {
        actor_ref_clone.tell_blocking(DeprecatedMessage, Some(Duration::from_secs(1)))
    })
    .await
    .unwrap();

    assert!(result.is_ok(), "Deprecated tell_blocking should work");

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_deprecated_ask_blocking_method() {
    // Test: deprecated ask_blocking method should still work

    #[derive(Actor)]
    struct DeprecatedAskActor {
        value: String,
    }

    struct GetValue;

    #[rsactor::message_handlers]
    impl DeprecatedAskActor {
        #[handler]
        async fn handle(&mut self, _: GetValue, _: &ActorRef<Self>) -> String {
            self.value.clone()
        }
    }

    let (actor_ref, handle) = spawn::<DeprecatedAskActor>(DeprecatedAskActor {
        value: "test_value".to_string(),
    });

    let actor_ref_clone = actor_ref.clone();

    // Use deprecated ask_blocking
    #[allow(deprecated)]
    let result: Result<String, _> = tokio::task::spawn_blocking(move || {
        actor_ref_clone.ask_blocking(GetValue, Some(Duration::from_secs(1)))
    })
    .await
    .unwrap();

    assert!(result.is_ok(), "Deprecated ask_blocking should work");
    assert_eq!(result.unwrap(), "test_value");

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[test]
#[should_panic(expected = "Mailbox capacity must be greater than 0")]
fn test_spawn_with_zero_mailbox_capacity_panics() {
    // Test: spawn_with_mailbox_capacity(0) should panic
    // This covers lib.rs line 479

    #[derive(Actor)]
    struct ZeroCapacityActor;

    // This should panic
    let _ = rsactor::spawn_with_mailbox_capacity::<ZeroCapacityActor>(ZeroCapacityActor, 0);
}

#[tokio::test]
async fn test_weak_handler_clone() {
    // Test: WeakTellHandler and WeakAskHandler clone implementations
    // This covers handler.rs clone implementations

    use rsactor::{AskHandler, TellHandler};

    #[derive(Actor)]
    struct CloneTestActor;

    struct CloneTestMessage;

    #[rsactor::message_handlers]
    impl CloneTestActor {
        #[handler]
        async fn handle(&mut self, _: CloneTestMessage, _: &ActorRef<Self>) -> String {
            "response".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<CloneTestActor>(CloneTestActor);

    // Get a TellHandler trait object
    let tell_handler: Box<dyn TellHandler<CloneTestMessage>> = Box::new(actor_ref.clone());

    // Create a weak handler and clone it
    let weak_tell = tell_handler.downgrade();
    let weak_tell_clone = weak_tell.clone();

    // Upgrade should work for both
    assert!(
        weak_tell.upgrade().is_some(),
        "Original weak tell should upgrade"
    );
    assert!(
        weak_tell_clone.upgrade().is_some(),
        "Cloned weak tell should upgrade"
    );

    // Get an AskHandler trait object
    let ask_handler: Box<dyn AskHandler<CloneTestMessage, String>> = Box::new(actor_ref.clone());

    // Create a weak handler and clone it
    let weak_ask = ask_handler.downgrade();
    let weak_ask_clone = weak_ask.clone();

    // Upgrade should work for both
    assert!(
        weak_ask.upgrade().is_some(),
        "Original weak ask should upgrade"
    );
    assert!(
        weak_ask_clone.upgrade().is_some(),
        "Cloned weak ask should upgrade"
    );

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_tell_handler_blocking_tell() {
    // Test: TellHandler::blocking_tell via trait object
    // This covers handler.rs line 291-293

    use rsactor::TellHandler;

    #[derive(Actor)]
    struct BlockingTellActor {
        received: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    struct BlockingTellMessage;

    #[rsactor::message_handlers]
    impl BlockingTellActor {
        #[handler]
        async fn handle(&mut self, _: BlockingTellMessage, _: &ActorRef<Self>) {
            self.received
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let received = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (actor_ref, handle) = spawn::<BlockingTellActor>(BlockingTellActor {
        received: received.clone(),
    });

    // Get a TellHandler trait object
    let tell_handler: Box<dyn TellHandler<BlockingTellMessage>> = Box::new(actor_ref.clone());

    // Use blocking_tell via the trait object
    let result =
        tokio::task::spawn_blocking(move || tell_handler.blocking_tell(BlockingTellMessage, None))
            .await
            .unwrap();

    assert!(result.is_ok(), "blocking_tell via trait should succeed");

    // Give the actor time to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        received.load(std::sync::atomic::Ordering::SeqCst),
        "Message should have been received"
    );

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_ask_handler_blocking_ask() {
    // Test: AskHandler::blocking_ask via trait object
    // This covers handler.rs lines 333-335

    use rsactor::AskHandler;

    #[derive(Actor)]
    struct BlockingAskTraitActor;

    struct BlockingAskTraitMessage;

    #[rsactor::message_handlers]
    impl BlockingAskTraitActor {
        #[handler]
        async fn handle(&mut self, _: BlockingAskTraitMessage, _: &ActorRef<Self>) -> String {
            "blocking_ask_response".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<BlockingAskTraitActor>(BlockingAskTraitActor);

    // Get an AskHandler trait object
    let ask_handler: Box<dyn AskHandler<BlockingAskTraitMessage, String>> =
        Box::new(actor_ref.clone());

    // Use blocking_ask via the trait object
    let result: Result<String, _> = tokio::task::spawn_blocking(move || {
        ask_handler.blocking_ask(BlockingAskTraitMessage, None)
    })
    .await
    .unwrap();

    assert!(result.is_ok(), "blocking_ask via trait should succeed");
    assert_eq!(result.unwrap(), "blocking_ask_response");

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_actor_control_from_conversions() {
    // Test: From<ActorRef<T>> for Box<dyn ActorControl>
    // This covers actor_control.rs lines 234-236

    use rsactor::ActorControl;

    #[derive(Actor)]
    struct FromConversionActor;

    let (actor_ref, handle) = spawn::<FromConversionActor>(FromConversionActor);

    // Test From<ActorRef<T>> for Box<dyn ActorControl>
    let control: Box<dyn ActorControl> = actor_ref.clone().into();

    // Verify the control works
    assert!(control.is_alive(), "Actor should be alive");

    // Stop via control
    control.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_weak_actor_control_from_reference() {
    // Test: From<&ActorWeak<T>> for Box<dyn WeakActorControl>
    // This covers actor_control.rs lines 255-257

    use rsactor::WeakActorControl;

    #[derive(Actor)]
    struct WeakFromRefActor;

    let (actor_ref, handle) = spawn::<WeakFromRefActor>(WeakFromRefActor);

    // Create a weak reference
    let weak_ref = ActorRef::downgrade(&actor_ref);

    // Test From<&ActorWeak<T>> for Box<dyn WeakActorControl>
    let weak_control: Box<dyn WeakActorControl> = (&weak_ref).into();

    // Verify it can be upgraded
    assert!(
        weak_control.upgrade().is_some(),
        "Weak control should upgrade"
    );

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_ask_handler_from_actor_ref() {
    // Test: From<ActorRef<T>> for Box<dyn AskHandler<M, R>>
    // This covers handler.rs lines 448-450

    use rsactor::AskHandler;

    #[derive(Actor)]
    struct AskHandlerFromActor;

    struct AskHandlerFromMessage;

    #[rsactor::message_handlers]
    impl AskHandlerFromActor {
        #[handler]
        async fn handle(&mut self, _: AskHandlerFromMessage, _: &ActorRef<Self>) -> i32 {
            42
        }
    }

    let (actor_ref, handle) = spawn::<AskHandlerFromActor>(AskHandlerFromActor);

    // Test From<ActorRef<T>> for Box<dyn AskHandler<M, R>>
    let ask_handler: Box<dyn AskHandler<AskHandlerFromMessage, i32>> = actor_ref.clone().into();

    // Verify the handler works
    let result: i32 = ask_handler.ask(AskHandlerFromMessage).await.unwrap();
    assert_eq!(result, 42);

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_weak_ask_handler_from_reference() {
    // Test: From<&ActorWeak<T>> for Box<dyn WeakAskHandler<M, R>>
    // This covers handler.rs lines 508-510

    use rsactor::WeakAskHandler;

    #[derive(Actor)]
    struct WeakAskFromRefActor;

    struct WeakAskFromRefMessage;

    #[rsactor::message_handlers]
    impl WeakAskFromRefActor {
        #[handler]
        async fn handle(&mut self, _: WeakAskFromRefMessage, _: &ActorRef<Self>) -> String {
            "weak_response".to_string()
        }
    }

    let (actor_ref, handle) = spawn::<WeakAskFromRefActor>(WeakAskFromRefActor);

    // Create a weak reference
    let weak_ref = ActorRef::downgrade(&actor_ref);

    // Test From<&ActorWeak<T>> for Box<dyn WeakAskHandler<M, R>>
    let weak_handler: Box<dyn WeakAskHandler<WeakAskFromRefMessage, String>> = (&weak_ref).into();

    // Verify it can be upgraded and used
    if let Some(strong) = weak_handler.upgrade() {
        let result: String = strong.ask(WeakAskFromRefMessage).await.unwrap();
        assert_eq!(result, "weak_response");
    } else {
        panic!("Weak handler should be upgradeable");
    }

    actor_ref.stop().await.unwrap();
    handle.await.unwrap();
}
