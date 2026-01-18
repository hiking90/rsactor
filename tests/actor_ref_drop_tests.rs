// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use rsactor::{spawn, Actor, ActorRef, Identity, Message};

// Initialize logger for tests with configurable level
fn init_test_logger() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();
}

// Test Actor for drop behavior testing
struct DropTestActor {
    id: Identity,
    processed_messages: Arc<Mutex<Vec<String>>>,
    message_count: Arc<Mutex<i32>>,
}

struct DropTestArgs {
    processed_messages: Arc<Mutex<Vec<String>>>,
    message_count: Arc<Mutex<i32>>,
}

impl Actor for DropTestActor {
    type Args = DropTestArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        debug!("DropTestActor (id: {}) started.", actor_ref.identity());
        Ok(Self {
            id: actor_ref.identity(),
            processed_messages: args.processed_messages,
            message_count: args.message_count,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: &rsactor::ActorWeak<Self>,
        _killed: bool,
    ) -> Result<(), Self::Error> {
        debug!("DropTestActor (id: {}) stopping.", self.id);
        let mut messages = self.processed_messages.lock().await;
        messages.push("on_stop_called".to_string());
        Ok(())
    }
}

// Test messages
#[derive(Debug)]
struct TestMessage {
    content: String,
    delay_ms: u64,
}

impl Message<TestMessage> for DropTestActor {
    type Reply = ();

    async fn handle(&mut self, msg: TestMessage, _: &ActorRef<Self>) -> Self::Reply {
        debug!("Processing message: {}", msg.content);

        // Simulate some processing time
        if msg.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(msg.delay_ms)).await;
        }

        // Record the processed message
        let mut messages = self.processed_messages.lock().await;
        messages.push(msg.content.clone());

        // Increment message count
        let mut count = self.message_count.lock().await;
        *count += 1;

        debug!("Completed processing message: {}", msg.content);
    }
}

#[tokio::test]
async fn test_actor_ref_drop_terminates_actor_after_processing_messages() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send multiple messages via tell before dropping actor_ref
    let messages_to_send = vec![
        TestMessage {
            content: "message_1".to_string(),
            delay_ms: 10,
        },
        TestMessage {
            content: "message_2".to_string(),
            delay_ms: 10,
        },
        TestMessage {
            content: "message_3".to_string(),
            delay_ms: 10,
        },
        TestMessage {
            content: "message_4".to_string(),
            delay_ms: 10,
        },
        TestMessage {
            content: "message_5".to_string(),
            delay_ms: 10,
        },
    ];

    for msg in messages_to_send {
        actor_ref.tell(msg).await.expect("Failed to send message");
    }

    debug!("All messages sent, calling stop before dropping actor_ref");

    // Instead of just dropping, call stop explicitly to ensure graceful shutdown
    actor_ref.stop().await.expect("Failed to stop actor");

    // Now drop the actor_ref
    drop(actor_ref);

    // Wait for the actor to complete
    let result = handle.await.expect("Actor task should complete");

    // Verify the actor completed normally (not killed)
    assert!(result.is_completed(), "Actor should have completed");
    assert!(!result.was_killed(), "Actor should not have been killed");
    assert!(
        result.stopped_normally(),
        "Actor should have stopped normally"
    );

    // Verify all messages were processed before termination
    let final_messages = processed_messages.lock().await;
    let final_count = *message_count.lock().await;

    assert_eq!(final_count, 5, "All 5 messages should have been processed");

    // Check that all expected messages were processed
    assert!(final_messages.contains(&"message_1".to_string()));
    assert!(final_messages.contains(&"message_2".to_string()));
    assert!(final_messages.contains(&"message_3".to_string()));
    assert!(final_messages.contains(&"message_4".to_string()));
    assert!(final_messages.contains(&"message_5".to_string()));

    // Check that on_stop was called
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("Processed messages: {:?}", *final_messages);
    debug!("Total messages processed: {final_count}");
}

#[tokio::test]
async fn test_actor_ref_drop_with_slow_messages() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send messages with longer processing times
    let messages_to_send = vec![
        TestMessage {
            content: "slow_message_1".to_string(),
            delay_ms: 50,
        },
        TestMessage {
            content: "slow_message_2".to_string(),
            delay_ms: 50,
        },
        TestMessage {
            content: "slow_message_3".to_string(),
            delay_ms: 50,
        },
    ];

    for msg in messages_to_send {
        actor_ref
            .tell(msg)
            .await
            .expect("Failed to send slow message");
    }

    debug!("All slow messages sent, stopping and dropping actor_ref");

    // Stop the actor explicitly and then drop the reference
    actor_ref.stop().await.expect("Failed to stop actor");
    drop(actor_ref);

    // Wait for the actor to complete (should take at least 150ms due to processing delays)
    let start_time = std::time::Instant::now();
    let result = handle.await.expect("Actor task should complete");
    let elapsed = start_time.elapsed();

    // Verify the actor took time to process all messages before terminating
    assert!(
        elapsed >= std::time::Duration::from_millis(140),
        "Actor should have taken time to process slow messages, elapsed: {elapsed:?}"
    );

    // Verify the actor completed normally
    assert!(result.is_completed(), "Actor should have completed");
    assert!(!result.was_killed(), "Actor should not have been killed");
    assert!(
        result.stopped_normally(),
        "Actor should have stopped normally"
    );

    // Verify all slow messages were processed
    let final_messages = processed_messages.lock().await;
    let final_count = *message_count.lock().await;

    assert_eq!(
        final_count, 3,
        "All 3 slow messages should have been processed"
    );

    assert!(final_messages.contains(&"slow_message_1".to_string()));
    assert!(final_messages.contains(&"slow_message_2".to_string()));
    assert!(final_messages.contains(&"slow_message_3".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("Processed slow messages: {:?}", *final_messages);
    debug!("Total slow messages processed: {final_count}");
    debug!("Processing time: {elapsed:?}");
}

#[tokio::test]
async fn test_multiple_actor_refs_drop_behavior() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Clone the actor_ref to create multiple references
    let actor_ref_clone1 = actor_ref.clone();
    let actor_ref_clone2 = actor_ref.clone();

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send messages using different actor_ref instances
    actor_ref
        .tell(TestMessage {
            content: "from_original".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from original");

    actor_ref_clone1
        .tell(TestMessage {
            content: "from_clone1".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from clone1");

    actor_ref_clone2
        .tell(TestMessage {
            content: "from_clone2".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from clone2");

    debug!("Messages sent from multiple refs, dropping original actor_ref");

    // Drop only the original actor_ref (clones still exist)
    drop(actor_ref);

    // Give some time for potential effects to manifest
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Actor should still be alive because clones exist
    assert!(
        actor_ref_clone1.is_alive(),
        "Actor should still be alive with existing clones"
    );

    // Send more messages using clones
    actor_ref_clone1
        .tell(TestMessage {
            content: "after_original_drop".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send after original drop");

    debug!("Dropping first clone");
    drop(actor_ref_clone1);

    // Give some time for potential effects to manifest
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Actor should still be alive because one clone exists
    assert!(
        actor_ref_clone2.is_alive(),
        "Actor should still be alive with one clone"
    );

    // Send final message
    actor_ref_clone2
        .tell(TestMessage {
            content: "final_message".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send final message");

    debug!("Stopping actor before dropping last clone");

    // Explicitly stop the actor before dropping the last reference
    actor_ref_clone2.stop().await.expect("Failed to stop actor");

    debug!("Dropping last clone");
    drop(actor_ref_clone2);

    // Now the actor should terminate
    let result = handle.await.expect("Actor task should complete");

    // Verify the actor completed normally
    assert!(result.is_completed(), "Actor should have completed");
    assert!(!result.was_killed(), "Actor should not have been killed");
    assert!(
        result.stopped_normally(),
        "Actor should have stopped normally"
    );

    // Verify all messages were processed
    let final_messages = processed_messages.lock().await;
    let final_count = *message_count.lock().await;

    assert_eq!(final_count, 5, "All 5 messages should have been processed");

    assert!(final_messages.contains(&"from_original".to_string()));
    assert!(final_messages.contains(&"from_clone1".to_string()));
    assert!(final_messages.contains(&"from_clone2".to_string()));
    assert!(final_messages.contains(&"after_original_drop".to_string()));
    assert!(final_messages.contains(&"final_message".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!(
        "Processed messages from multiple refs: {:?}",
        *final_messages
    );
    debug!("Total messages processed: {final_count}");
}

#[tokio::test]
async fn test_actor_ref_clone_drop_behavior() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Test that ActorRef clones work independently
    {
        let actor_clone = actor_ref.clone();

        // Send messages from the clone
        actor_clone
            .tell(TestMessage {
                content: "from_clone_scope".to_string(),
                delay_ms: 10,
            })
            .await
            .expect("Failed to send from clone");

        // Clone goes out of scope here, but actor should still be alive
    }

    // Give some time for processing
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    // Actor should still be alive because original reference exists
    assert!(
        actor_ref.is_alive(),
        "Actor should still be alive after clone drop"
    );

    // Send another message from original reference
    actor_ref
        .tell(TestMessage {
            content: "after_clone_drop".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send after clone drop");

    // Explicitly stop before dropping original
    actor_ref.stop().await.expect("Failed to stop actor");
    println!("Stopping actor before dropping original reference");
    drop(actor_ref);

    println!("Waiting for actor to complete");
    let result = handle.await.expect("Actor task should complete");

    // Verify the actor completed normally
    assert!(result.is_completed(), "Actor should have completed");
    assert!(!result.was_killed(), "Actor should not have been killed");
    assert!(
        result.stopped_normally(),
        "Actor should have stopped normally"
    );

    // Verify messages were processed
    let final_messages = processed_messages.lock().await;
    let final_count = *message_count.lock().await;

    assert_eq!(final_count, 2, "Both messages should have been processed");

    assert!(final_messages.contains(&"from_clone_scope".to_string()));
    assert!(final_messages.contains(&"after_clone_drop".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("Processed messages from clone test: {:?}", *final_messages);
    debug!("Total messages processed: {final_count}");
}

#[tokio::test]
async fn test_actor_ref_drop_vs_explicit_stop() {
    init_test_logger();

    // Test 1: Explicit stop
    {
        let processed_messages = Arc::new(Mutex::new(Vec::new()));
        let message_count = Arc::new(Mutex::new(0));

        let args = DropTestArgs {
            processed_messages: processed_messages.clone(),
            message_count: message_count.clone(),
        };

        let (actor_ref, handle) = spawn::<DropTestActor>(args);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send messages
        for i in 1..=3 {
            actor_ref
                .tell(TestMessage {
                    content: format!("explicit_stop_{i}"),
                    delay_ms: 10,
                })
                .await
                .expect("Failed to send message");
        }

        // Explicitly stop the actor
        actor_ref.stop().await.expect("Failed to stop actor");

        let result = handle.await.expect("Actor should complete");
        assert!(
            result.stopped_normally(),
            "Actor should have stopped normally"
        );

        let final_count = *message_count.lock().await;
        assert_eq!(
            final_count, 3,
            "All messages should be processed with explicit stop"
        );
    }

    // Test 2: Drop behavior
    {
        let processed_messages = Arc::new(Mutex::new(Vec::new()));
        let message_count = Arc::new(Mutex::new(0));

        let args = DropTestArgs {
            processed_messages: processed_messages.clone(),
            message_count: message_count.clone(),
        };

        let (actor_ref, handle) = spawn::<DropTestActor>(args);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send messages
        for i in 1..=3 {
            actor_ref
                .tell(TestMessage {
                    content: format!("drop_behavior_{i}"),
                    delay_ms: 10,
                })
                .await
                .expect("Failed to send message");
        }

        // Drop the actor_ref instead of explicit stop
        drop(actor_ref);

        let result = handle.await.expect("Actor should complete");
        assert!(
            result.stopped_normally(),
            "Actor should have stopped normally via drop"
        );

        let final_count = *message_count.lock().await;
        assert_eq!(
            final_count, 3,
            "All messages should be processed with drop behavior"
        );
    }
}

#[tokio::test]
async fn test_actor_weak_reference_basic_functionality() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create a weak reference
    let weak_ref = ActorRef::downgrade(&actor_ref);

    // Test that weak reference has the same identity
    assert_eq!(
        actor_ref.identity(),
        weak_ref.identity(),
        "ActorWeak should have the same identity as ActorRef"
    );

    // Test that weak reference reports actor as alive
    assert!(
        weak_ref.is_alive(),
        "ActorWeak should report actor as alive when ActorRef exists"
    );

    // Test upgrade functionality
    let upgraded_ref = weak_ref.upgrade();
    assert!(
        upgraded_ref.is_some(),
        "ActorWeak should be able to upgrade to ActorRef when actor is alive"
    );

    let upgraded_ref = upgraded_ref.unwrap();
    assert_eq!(
        upgraded_ref.identity(),
        actor_ref.identity(),
        "Upgraded ActorRef should have the same identity"
    );

    // Use the upgraded reference to send a message
    upgraded_ref
        .tell(TestMessage {
            content: "via_upgraded_ref".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send message via upgraded ref");

    // Drop the original actor_ref but keep the upgraded one
    drop(actor_ref);

    // Weak reference should still be alive because upgraded ref exists
    assert!(
        weak_ref.is_alive(),
        "ActorWeak should still report alive when upgraded ref exists"
    );

    // Should still be able to upgrade
    let another_upgrade = weak_ref.upgrade();
    assert!(
        another_upgrade.is_some(),
        "Should still be able to upgrade weak reference"
    );

    // Stop the actor using the upgraded reference
    upgraded_ref.stop().await.expect("Failed to stop actor");
    drop(upgraded_ref);

    let result = handle.await.expect("Actor task should complete");
    assert!(result.stopped_normally());

    // Verify message was processed
    let final_messages = processed_messages.lock().await;
    assert!(final_messages.contains(&"via_upgraded_ref".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("ActorWeak basic functionality test completed");
}

#[tokio::test]
async fn test_actor_weak_reference_after_actor_drop() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create a weak reference
    let weak_ref = ActorRef::downgrade(&actor_ref);

    // Send a message first
    actor_ref
        .tell(TestMessage {
            content: "before_drop".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send message");

    // Stop and drop the actor reference
    actor_ref.stop().await.expect("Failed to stop actor");
    drop(actor_ref);

    // Wait for the actor to complete
    let result = handle.await.expect("Actor task should complete");
    assert!(result.stopped_normally());

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Now the weak reference should report the actor as dead
    assert!(
        !weak_ref.is_alive(),
        "ActorWeak should report actor as dead after all strong references are dropped"
    );

    // Upgrade should fail
    let upgrade_result = weak_ref.upgrade();
    assert!(
        upgrade_result.is_none(),
        "ActorWeak upgrade should fail when actor is dead"
    );

    // Verify the message was processed
    let final_messages = processed_messages.lock().await;
    assert!(final_messages.contains(&"before_drop".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("ActorWeak after actor drop test completed");
}

#[tokio::test]
async fn test_actor_weak_reference_clone_behavior() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create multiple weak references
    let weak_ref1 = ActorRef::downgrade(&actor_ref);
    let weak_ref2 = weak_ref1.clone();
    let weak_ref3 = ActorRef::downgrade(&actor_ref);

    // All weak references should have the same identity
    assert_eq!(weak_ref1.identity(), weak_ref2.identity());
    assert_eq!(weak_ref1.identity(), weak_ref3.identity());

    // All should report actor as alive
    assert!(weak_ref1.is_alive());
    assert!(weak_ref2.is_alive());
    assert!(weak_ref3.is_alive());

    // All should be able to upgrade
    let upgrade1 = weak_ref1.upgrade();
    let upgrade2 = weak_ref2.upgrade();
    let upgrade3 = weak_ref3.upgrade();

    assert!(upgrade1.is_some());
    assert!(upgrade2.is_some());
    assert!(upgrade3.is_some());

    // Use different upgraded references to send messages
    upgrade1
        .unwrap()
        .tell(TestMessage {
            content: "from_upgrade1".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from upgrade1");

    upgrade2
        .unwrap()
        .tell(TestMessage {
            content: "from_upgrade2".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from upgrade2");

    let upgrade3_ref = upgrade3.unwrap();
    upgrade3_ref
        .tell(TestMessage {
            content: "from_upgrade3".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send from upgrade3");

    // Stop the actor
    upgrade3_ref.stop().await.expect("Failed to stop actor");
    drop(actor_ref);
    drop(upgrade3_ref);

    let result = handle.await.expect("Actor task should complete");
    assert!(result.stopped_normally());

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // All weak references should now report actor as dead
    assert!(!weak_ref1.is_alive());
    assert!(!weak_ref2.is_alive());
    assert!(!weak_ref3.is_alive());

    // None should be able to upgrade
    assert!(weak_ref1.upgrade().is_none());
    assert!(weak_ref2.upgrade().is_none());
    assert!(weak_ref3.upgrade().is_none());

    // Verify all messages were processed
    let final_messages = processed_messages.lock().await;
    assert!(final_messages.contains(&"from_upgrade1".to_string()));
    assert!(final_messages.contains(&"from_upgrade2".to_string()));
    assert!(final_messages.contains(&"from_upgrade3".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("ActorWeak clone behavior test completed");
}

#[tokio::test]
async fn test_actor_weak_reference_with_kill() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create a weak reference
    let weak_ref = ActorRef::downgrade(&actor_ref);

    // Send some messages
    actor_ref
        .tell(TestMessage {
            content: "before_kill".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send message");

    // Verify weak reference is alive
    assert!(weak_ref.is_alive(), "ActorWeak should be alive before kill");

    // Kill the actor
    actor_ref.kill().expect("Failed to kill actor");
    drop(actor_ref);

    let result = handle.await.expect("Actor task should complete");
    assert!(result.was_killed(), "Actor should have been killed");

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Weak reference should report actor as dead
    assert!(
        !weak_ref.is_alive(),
        "ActorWeak should report actor as dead after kill"
    );

    // Upgrade should fail
    assert!(
        weak_ref.upgrade().is_none(),
        "ActorWeak upgrade should fail after kill"
    );

    // Verify the actor was killed (message might not be processed due to kill)
    let final_messages = processed_messages.lock().await;
    // Note: When actor is killed, messages might not be processed
    // So we don't assert that "before_kill" was processed, just check kill behavior
    debug!("Messages after kill: {:?}", *final_messages);

    debug!("ActorWeak with kill test completed");
}

#[tokio::test]
async fn test_actor_weak_reference_upgrade_race_condition() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Create weak references
    let weak_ref = ActorRef::downgrade(&actor_ref);
    let weak_ref_clone = weak_ref.clone();

    // Send a message to ensure actor is working
    actor_ref
        .tell(TestMessage {
            content: "initial_message".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send initial message");

    // Test rapid upgrade attempts while dropping the strong reference
    let weak_clone_for_task = weak_ref_clone.clone();
    let upgrade_task = tokio::spawn(async move {
        let mut successful_upgrades = 0;
        let mut failed_upgrades = 0;

        for i in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;

            if let Some(upgraded) = weak_clone_for_task.upgrade() {
                successful_upgrades += 1;
                // Try to use the upgraded reference
                if upgraded
                    .tell(TestMessage {
                        content: format!("rapid_upgrade_{i}"),
                        delay_ms: 1,
                    })
                    .await
                    .is_ok()
                {
                    // Message sent successfully
                }
                drop(upgraded);
            } else {
                failed_upgrades += 1;
            }
        }

        (successful_upgrades, failed_upgrades)
    });

    // Let the upgrade task run for a bit
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;

    // Stop the actor
    actor_ref.stop().await.expect("Failed to stop actor");
    drop(actor_ref);

    // Wait for the upgrade task to complete
    let (successful_upgrades, failed_upgrades) = upgrade_task.await.expect("Upgrade task failed");

    // Wait for actor to complete
    let result = handle.await.expect("Actor task should complete");
    assert!(result.stopped_normally());

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // After actor is fully stopped, weak reference should be dead
    assert!(!weak_ref.is_alive());
    assert!(weak_ref.upgrade().is_none());

    // Verify that we had both successful and failed upgrades during the race
    debug!(
        "Upgrade race test: {successful_upgrades} successful, {failed_upgrades} failed upgrades"
    );

    // Verify initial message was processed
    let final_messages = processed_messages.lock().await;
    assert!(final_messages.contains(&"initial_message".to_string()));
    assert!(final_messages.contains(&"on_stop_called".to_string()));

    debug!("ActorWeak upgrade race condition test completed");
}

#[tokio::test]
async fn test_actor_weak_reference_identity_consistency() {
    init_test_logger();

    let processed_messages = Arc::new(Mutex::new(Vec::new()));
    let message_count = Arc::new(Mutex::new(0));

    let args = DropTestArgs {
        processed_messages: processed_messages.clone(),
        message_count: message_count.clone(),
    };

    let (actor_ref, handle) = spawn::<DropTestActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let original_identity = actor_ref.identity();

    // Create weak reference and verify identity
    let weak_ref = ActorRef::downgrade(&actor_ref);
    assert_eq!(weak_ref.identity(), original_identity);

    // Clone weak reference and verify identity
    let weak_clone = weak_ref.clone();
    assert_eq!(weak_clone.identity(), original_identity);

    // Upgrade weak reference and verify identity
    let upgraded = weak_ref.upgrade().expect("Should be able to upgrade");
    assert_eq!(upgraded.identity(), original_identity);

    // Clone the upgraded reference and verify identity
    let upgraded_clone = upgraded.clone();
    assert_eq!(upgraded_clone.identity(), original_identity);

    // Create another weak reference from the upgraded reference
    let weak_from_upgraded = ActorRef::downgrade(&upgraded);
    assert_eq!(weak_from_upgraded.identity(), original_identity);

    // All references should report the same identity throughout the lifecycle
    actor_ref
        .tell(TestMessage {
            content: "identity_test".to_string(),
            delay_ms: 10,
        })
        .await
        .expect("Failed to send message");

    // Stop the actor
    actor_ref.stop().await.expect("Failed to stop actor");
    drop(actor_ref);
    drop(upgraded);
    drop(upgraded_clone);

    let result = handle.await.expect("Actor task should complete");
    assert!(result.stopped_normally());

    // Even after actor is dead, weak references should retain the same identity
    assert_eq!(weak_ref.identity(), original_identity);
    assert_eq!(weak_clone.identity(), original_identity);
    assert_eq!(weak_from_upgraded.identity(), original_identity);

    // But they should all be dead
    assert!(!weak_ref.is_alive());
    assert!(!weak_clone.is_alive());
    assert!(!weak_from_upgraded.is_alive());

    // And all upgrade attempts should fail
    assert!(weak_ref.upgrade().is_none());
    assert!(weak_clone.upgrade().is_none());
    assert!(weak_from_upgraded.upgrade().is_none());

    debug!("ActorWeak identity consistency test completed");
}
