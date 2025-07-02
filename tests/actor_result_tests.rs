// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0
#![allow(deprecated)]

use rsactor::{spawn, Actor, ActorRef, ActorResult, ActorWeak, FailurePhase, Message};

// Dummy message for test actors
#[derive(Debug)]
struct NoOpMsg;

// Test actor for ActorResult testing
#[derive(Debug)]
struct TestActor {
    id: String,
    value: i32,
}

impl Actor for TestActor {
    type Args = (String, i32);
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let (id, value) = args;
        Ok(TestActor { id, value })
    }

    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // Simple run loop that sleeps
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

impl Message<NoOpMsg> for TestActor {
    type Reply = ();
    async fn handle(&mut self, _msg: NoOpMsg, _: &ActorRef<Self>) -> Self::Reply {}
}

// Test actor that can be configured to fail in different phases
#[derive(Debug)]
struct FailureTestActor {
    id: String,
    value: i32,
}

#[derive(Debug)]
struct FailureTestArgs {
    id: String,
    value: i32,
    fail_on_start: bool,
}

impl Actor for FailureTestActor {
    type Args = FailureTestArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        if args.fail_on_start {
            return Err(anyhow::anyhow!("Test failure in on_start"));
        }
        Ok(FailureTestActor {
            id: args.id,
            value: args.value,
        })
    }

    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        if self.id.contains("fail_on_run") {
            return Err(anyhow::anyhow!("Test failure in on_run"));
        }
        // Simple run loop that sleeps
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn on_stop(
        &mut self,
        _actor_ref: &ActorWeak<Self>,
        _killed: bool,
    ) -> Result<(), Self::Error> {
        if self.id.contains("fail_on_stop") {
            return Err(anyhow::anyhow!("Test failure in on_stop"));
        }
        Ok(())
    }
}

impl Message<NoOpMsg> for FailureTestActor {
    type Reply = ();
    async fn handle(&mut self, _msg: NoOpMsg, _: &ActorRef<Self>) -> Self::Reply {}
}

// === ActorResult::Completed Tests ===

#[tokio::test]
async fn test_actor_result_completed_stopped_normally() {
    let (actor_ref, handle) = spawn::<TestActor>(("test_normal".to_string(), 42));

    // Wait for actor to start and then stop it gracefully
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    actor_ref.stop().await.expect("Stop should succeed");

    let result = handle.await.expect("Actor should complete");

    // Test all methods for Completed { killed: false }
    assert!(result.is_completed(), "Should be completed");
    assert!(!result.was_killed(), "Should not be killed");
    assert!(result.stopped_normally(), "Should have stopped normally");
    assert!(!result.is_startup_failed(), "Should not be startup failed");
    assert!(!result.is_runtime_failed(), "Should not be runtime failed");
    assert!(!result.is_stop_failed(), "Should not be stop failed");
    assert!(!result.is_failed(), "Should not be failed");
    assert!(result.has_actor(), "Should have actor");

    // Test actor access
    assert!(result.actor().is_some(), "Should have actor reference");
    if let Some(actor) = result.actor() {
        assert_eq!(actor.id, "test_normal");
        assert_eq!(actor.value, 42);
    }

    // Test error access
    assert!(result.error().is_none(), "Should not have error");

    // Test to_result conversion
    let result_copy = ActorResult::Completed {
        actor: TestActor {
            id: "test_normal".to_string(),
            value: 42,
        },
        killed: false,
    };
    let std_result = result_copy.to_result();
    assert!(std_result.is_ok(), "to_result should be Ok");
    if let Ok(actor) = std_result {
        assert_eq!(actor.id, "test_normal");
        assert_eq!(actor.value, 42);
    }

    // Test into_actor
    let actor_option = result.into_actor();
    assert!(actor_option.is_some(), "into_actor should return Some");
    if let Some(actor) = actor_option {
        assert_eq!(actor.id, "test_normal");
        assert_eq!(actor.value, 42);
    }
}

#[tokio::test]
async fn test_actor_result_completed_killed() {
    let (actor_ref, handle) = spawn::<TestActor>(("test_killed".to_string(), 100));

    // Wait for actor to start and then kill it
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    actor_ref.kill().expect("Kill should succeed");

    let result = handle.await.expect("Actor should complete");

    // Test all methods for Completed { killed: true }
    assert!(result.is_completed(), "Should be completed");
    assert!(result.was_killed(), "Should be killed");
    assert!(
        !result.stopped_normally(),
        "Should not have stopped normally"
    );
    assert!(!result.is_startup_failed(), "Should not be startup failed");
    assert!(!result.is_runtime_failed(), "Should not be runtime failed");
    assert!(!result.is_stop_failed(), "Should not be stop failed");
    assert!(!result.is_failed(), "Should not be failed");
    assert!(result.has_actor(), "Should have actor");

    // Test actor access
    assert!(result.actor().is_some(), "Should have actor reference");
    if let Some(actor) = result.actor() {
        assert_eq!(actor.id, "test_killed");
        assert_eq!(actor.value, 100);
    }

    // Test error access
    assert!(result.error().is_none(), "Should not have error");
}

// === ActorResult::Failed Tests ===

#[tokio::test]
async fn test_actor_result_failed_on_start() {
    let args = FailureTestArgs {
        id: "test_start_fail".to_string(),
        value: 999,
        fail_on_start: true,
    };
    let (_actor_ref, handle) = spawn::<FailureTestActor>(args);

    let result = handle.await.expect("Handle should not fail");

    // Test all methods for Failed { phase: OnStart }
    assert!(!result.is_completed(), "Should not be completed");
    assert!(!result.was_killed(), "Should not be killed");
    assert!(
        !result.stopped_normally(),
        "Should not have stopped normally"
    );
    assert!(result.is_startup_failed(), "Should be startup failed");
    assert!(!result.is_runtime_failed(), "Should not be runtime failed");
    assert!(!result.is_stop_failed(), "Should not be stop failed");
    assert!(result.is_failed(), "Should be failed");
    assert!(
        !result.has_actor(),
        "Should not have actor (failed on_start)"
    );

    // Test actor access (should be None for on_start failure)
    assert!(result.actor().is_none(), "Should not have actor reference");

    // Test error access
    assert!(result.error().is_some(), "Should have error");
    if let Some(error) = result.error() {
        assert!(error.to_string().contains("Test failure in on_start"));
    }

    // Test to_result conversion
    let result_copy = ActorResult::Failed::<FailureTestActor> {
        actor: None,
        error: anyhow::anyhow!("Test failure in on_start"),
        phase: FailurePhase::OnStart,
        killed: false,
    };
    let std_result = result_copy.to_result();
    assert!(std_result.is_err(), "to_result should be Err");
    if let Err(error) = std_result {
        assert!(error.to_string().contains("Test failure in on_start"));
    }

    // Test into_actor and into_error
    let actor_option = result.into_actor();
    assert!(
        actor_option.is_none(),
        "into_actor should return None for on_start failure"
    );
}

#[tokio::test]
async fn test_actor_result_failed_on_run() {
    let args = FailureTestArgs {
        id: "test_fail_on_run".to_string(),
        value: 777,
        fail_on_start: false,
    };
    let (_actor_ref, handle) = spawn::<FailureTestActor>(args);

    let result = handle.await.expect("Handle should not fail");

    // Test all methods for Failed { phase: OnRun }
    assert!(!result.is_completed(), "Should not be completed");
    assert!(!result.was_killed(), "Should not be killed");
    assert!(
        !result.stopped_normally(),
        "Should not have stopped normally"
    );
    assert!(!result.is_startup_failed(), "Should not be startup failed");
    assert!(result.is_runtime_failed(), "Should be runtime failed");
    assert!(!result.is_stop_failed(), "Should not be stop failed");
    assert!(result.is_failed(), "Should be failed");
    assert!(
        result.has_actor(),
        "Should have actor (failed after on_start)"
    );

    // Test actor access (should be Some for on_run failure)
    assert!(result.actor().is_some(), "Should have actor reference");
    if let Some(actor) = result.actor() {
        assert_eq!(actor.id, "test_fail_on_run");
        assert_eq!(actor.value, 777);
    }

    // Test error access
    assert!(result.error().is_some(), "Should have error");
    if let Some(error) = result.error() {
        assert!(error.to_string().contains("Test failure in on_run"));
    }
}

#[tokio::test]
async fn test_actor_result_failed_on_stop_graceful() {
    let args = FailureTestArgs {
        id: "test_fail_on_stop".to_string(),
        value: 555,
        fail_on_start: false,
    };
    let (actor_ref, handle) = spawn::<FailureTestActor>(args);

    // Wait for actor to start and then stop it gracefully
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    actor_ref.stop().await.expect("Stop should succeed");

    let result = handle.await.expect("Handle should not fail");

    // Test all methods for Failed { phase: OnStop, killed: false }
    assert!(!result.is_completed(), "Should not be completed");
    assert!(!result.was_killed(), "Should not be killed (graceful stop)");
    assert!(
        !result.stopped_normally(),
        "Should not have stopped normally"
    );
    assert!(!result.is_startup_failed(), "Should not be startup failed");
    assert!(!result.is_runtime_failed(), "Should not be runtime failed");
    assert!(result.is_stop_failed(), "Should be stop failed");
    assert!(result.is_failed(), "Should be failed");
    assert!(result.has_actor(), "Should have actor");

    // Test actor access
    assert!(result.actor().is_some(), "Should have actor reference");
    if let Some(actor) = result.actor() {
        assert_eq!(actor.id, "test_fail_on_stop");
        assert_eq!(actor.value, 555);
    }

    // Test error access
    assert!(result.error().is_some(), "Should have error");
    if let Some(error) = result.error() {
        assert!(error.to_string().contains("Test failure in on_stop"));
    }
}

#[tokio::test]
async fn test_actor_result_failed_on_stop_killed() {
    let args = FailureTestArgs {
        id: "test_fail_on_stop".to_string(),
        value: 333,
        fail_on_start: false,
    };
    let (actor_ref, handle) = spawn::<FailureTestActor>(args);

    // Wait for actor to start and then kill it
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    actor_ref.kill().expect("Kill should succeed");

    let result = handle.await.expect("Handle should not fail");

    // Test all methods for Failed { phase: OnStop, killed: true }
    assert!(!result.is_completed(), "Should not be completed");
    assert!(result.was_killed(), "Should be killed");
    assert!(
        !result.stopped_normally(),
        "Should not have stopped normally"
    );
    assert!(!result.is_startup_failed(), "Should not be startup failed");
    assert!(!result.is_runtime_failed(), "Should not be runtime failed");
    assert!(result.is_stop_failed(), "Should be stop failed");
    assert!(result.is_failed(), "Should be failed");
    assert!(result.has_actor(), "Should have actor");

    // Test actor access
    assert!(result.actor().is_some(), "Should have actor reference");
    if let Some(actor) = result.actor() {
        assert_eq!(actor.id, "test_fail_on_stop");
        assert_eq!(actor.value, 333);
    }

    // Test error access
    assert!(result.error().is_some(), "Should have error");
    if let Some(error) = result.error() {
        assert!(error.to_string().contains("Test failure in on_stop"));
    }
}

// === FailurePhase Tests ===

#[tokio::test]
async fn test_failure_phase_display() {
    assert_eq!(FailurePhase::OnStart.to_string(), "OnStart");
    assert_eq!(FailurePhase::OnRun.to_string(), "OnRun");
    assert_eq!(FailurePhase::OnStop.to_string(), "OnStop");
}

#[tokio::test]
async fn test_failure_phase_equality() {
    assert_eq!(FailurePhase::OnStart, FailurePhase::OnStart);
    assert_eq!(FailurePhase::OnRun, FailurePhase::OnRun);
    assert_eq!(FailurePhase::OnStop, FailurePhase::OnStop);

    assert_ne!(FailurePhase::OnStart, FailurePhase::OnRun);
    assert_ne!(FailurePhase::OnRun, FailurePhase::OnStop);
    assert_ne!(FailurePhase::OnStart, FailurePhase::OnStop);
}

// === From conversion Tests ===

#[tokio::test]
async fn test_actor_result_from_completed() {
    let result = ActorResult::Completed {
        actor: TestActor {
            id: "test".to_string(),
            value: 123,
        },
        killed: false,
    };

    let (actor_option, error_option): (Option<TestActor>, Option<anyhow::Error>) = result.into();

    assert!(actor_option.is_some(), "Should have actor");
    assert!(error_option.is_none(), "Should not have error");

    if let Some(actor) = actor_option {
        assert_eq!(actor.id, "test");
        assert_eq!(actor.value, 123);
    }
}

#[tokio::test]
async fn test_actor_result_from_failed_with_actor() {
    let result = ActorResult::Failed {
        actor: Some(TestActor {
            id: "test".to_string(),
            value: 456,
        }),
        error: anyhow::anyhow!("test error"),
        phase: FailurePhase::OnRun,
        killed: false,
    };

    let (actor_option, error_option): (Option<TestActor>, Option<anyhow::Error>) = result.into();

    assert!(actor_option.is_some(), "Should have actor");
    assert!(error_option.is_some(), "Should have error");

    if let Some(actor) = actor_option {
        assert_eq!(actor.id, "test");
        assert_eq!(actor.value, 456);
    }

    if let Some(error) = error_option {
        assert_eq!(error.to_string(), "test error");
    }
}

#[tokio::test]
async fn test_actor_result_from_failed_without_actor() {
    let result = ActorResult::Failed::<TestActor> {
        actor: None,
        error: anyhow::anyhow!("startup error"),
        phase: FailurePhase::OnStart,
        killed: false,
    };

    let (actor_option, error_option): (Option<TestActor>, Option<anyhow::Error>) = result.into();

    assert!(actor_option.is_none(), "Should not have actor");
    assert!(error_option.is_some(), "Should have error");

    if let Some(error) = error_option {
        assert_eq!(error.to_string(), "startup error");
    }
}

// === into_error Tests ===

#[tokio::test]
async fn test_actor_result_into_error_completed() {
    let result = ActorResult::Completed {
        actor: TestActor {
            id: "test".to_string(),
            value: 789,
        },
        killed: false,
    };

    let error_option = result.into_error();
    assert!(
        error_option.is_none(),
        "Completed result should not have error"
    );
}

#[tokio::test]
async fn test_actor_result_into_error_failed() {
    let result = ActorResult::Failed::<TestActor> {
        actor: None,
        error: anyhow::anyhow!("test into_error"),
        phase: FailurePhase::OnStart,
        killed: false,
    };

    let error_option = result.into_error();
    assert!(error_option.is_some(), "Failed result should have error");

    if let Some(error) = error_option {
        assert_eq!(error.to_string(), "test into_error");
    }
}

// === Edge cases and comprehensive coverage ===

#[tokio::test]
async fn test_actor_result_debug_format() {
    // Test that Debug is implemented and doesn't panic
    let completed_result = ActorResult::Completed {
        actor: TestActor {
            id: "debug_test".to_string(),
            value: 42,
        },
        killed: false,
    };
    let debug_str = format!("{completed_result:?}");
    assert!(
        debug_str.contains("Completed"),
        "Debug should contain variant name"
    );

    let failed_result = ActorResult::Failed::<TestActor> {
        actor: None,
        error: anyhow::anyhow!("debug error"),
        phase: FailurePhase::OnStart,
        killed: false,
    };
    let debug_str = format!("{failed_result:?}");
    assert!(
        debug_str.contains("Failed"),
        "Debug should contain variant name"
    );
}

#[tokio::test]
async fn test_all_boolean_combinations() {
    // Test all possible boolean combinations for completeness

    // Completed, not killed
    let result1 = ActorResult::Completed {
        actor: TestActor {
            id: "test1".to_string(),
            value: 1,
        },
        killed: false,
    };
    assert!(result1.is_completed() && !result1.was_killed() && result1.stopped_normally());

    // Completed, killed
    let result2 = ActorResult::Completed {
        actor: TestActor {
            id: "test2".to_string(),
            value: 2,
        },
        killed: true,
    };
    assert!(result2.is_completed() && result2.was_killed() && !result2.stopped_normally());

    // Failed OnStart, not killed
    let result3 = ActorResult::Failed::<TestActor> {
        actor: None,
        error: anyhow::anyhow!("error3"),
        phase: FailurePhase::OnStart,
        killed: false,
    };
    assert!(!result3.is_completed() && !result3.was_killed() && result3.is_startup_failed());

    // Failed OnRun, not killed
    let result4 = ActorResult::Failed {
        actor: Some(TestActor {
            id: "test4".to_string(),
            value: 4,
        }),
        error: anyhow::anyhow!("error4"),
        phase: FailurePhase::OnRun,
        killed: false,
    };
    assert!(!result4.is_completed() && !result4.was_killed() && result4.is_runtime_failed());

    // Failed OnStop, not killed
    let result5 = ActorResult::Failed {
        actor: Some(TestActor {
            id: "test5".to_string(),
            value: 5,
        }),
        error: anyhow::anyhow!("error5"),
        phase: FailurePhase::OnStop,
        killed: false,
    };
    assert!(!result5.is_completed() && !result5.was_killed() && result5.is_stop_failed());

    // Failed OnStop, killed
    let result6 = ActorResult::Failed {
        actor: Some(TestActor {
            id: "test6".to_string(),
            value: 6,
        }),
        error: anyhow::anyhow!("error6"),
        phase: FailurePhase::OnStop,
        killed: true,
    };
    assert!(!result6.is_completed() && result6.was_killed() && result6.is_stop_failed());
}

// === Error::Runtime Tests ===

#[tokio::test]
async fn test_error_runtime_tell_blocking_outside_runtime() {
    // Spawn an actor within a Tokio runtime
    let args = (String::from("runtime_test"), 42);
    let (actor_ref, handle) = spawn::<TestActor>(args);

    // Give the actor time to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create a thread that has no Tokio runtime context
    let actor_ref_clone = actor_ref.clone();
    let thread_handle = std::thread::spawn(move || {
        // This should trigger Error::Runtime because we're calling tell_blocking
        // from a thread that doesn't have a Tokio runtime handle
        let result =
            actor_ref_clone.tell_blocking(NoOpMsg, Some(std::time::Duration::from_millis(100)));

        // Verify that we get Error::Runtime
        assert!(
            result.is_err(),
            "tell_blocking should fail outside runtime context"
        );
        if let Err(rsactor::Error::Runtime { identity, details }) = result {
            assert_eq!(
                identity,
                actor_ref_clone.identity(),
                "Identity should match"
            );
            assert!(
                details.contains("Failed to get Tokio runtime handle for tell_blocking"),
                "Error should mention runtime handle failure, got: {details}"
            );
        } else {
            panic!("Expected Error::Runtime, got: {result:?}");
        }
    });

    // Wait for the thread to complete
    thread_handle.join().expect("Thread should not panic");

    // Clean up
    actor_ref.stop().await.expect("Failed to stop actor");
    let _result = handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_error_runtime_ask_blocking_outside_runtime() {
    // Spawn an actor within a Tokio runtime
    let args = (String::from("runtime_test"), 42);
    let (actor_ref, handle) = spawn::<TestActor>(args);

    // Give the actor time to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create a thread that has no Tokio runtime context
    let actor_ref_clone = actor_ref.clone();
    let thread_handle = std::thread::spawn(move || {
        // This should trigger Error::Runtime because we're calling ask_blocking
        // from a thread that doesn't have a Tokio runtime handle
        let result: Result<(), rsactor::Error> =
            actor_ref_clone.ask_blocking(NoOpMsg, Some(std::time::Duration::from_millis(100)));

        // Verify that we get Error::Runtime
        assert!(
            result.is_err(),
            "ask_blocking should fail outside runtime context"
        );
        if let Err(rsactor::Error::Runtime { identity, details }) = result {
            assert_eq!(
                identity,
                actor_ref_clone.identity(),
                "Identity should match"
            );
            assert!(
                details.contains("Failed to get Tokio runtime handle for ask_blocking"),
                "Error should mention runtime handle failure, got: {details}"
            );
        } else {
            panic!("Expected Error::Runtime, got: {result:?}");
        }
    });

    // Wait for the thread to complete
    thread_handle.join().expect("Thread should not panic");

    // Clean up
    actor_ref.stop().await.expect("Failed to stop actor");
    let _result = handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_error_runtime_display_format() {
    // Create a sample Error::Runtime for testing Display implementation
    let identity = rsactor::Identity::new(1, "TestActor");
    let error = rsactor::Error::Runtime {
        identity,
        details: "Test runtime error details".to_string(),
    };

    let display_str = format!("{error}");
    assert!(
        display_str.contains("Runtime error in actor"),
        "Display should mention runtime error"
    );
    assert!(
        display_str.contains("TestActor"),
        "Display should contain actor name"
    );
    assert!(
        display_str.contains("Test runtime error details"),
        "Display should contain error details"
    );
}
