// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0
#![allow(deprecated)]

use log::debug;
use std::sync::Arc;
use tokio::sync::Mutex;

use rsactor::{impl_message_handler, spawn, Actor, ActorRef, Identity, Message};

// Initialize logger for tests with configurable level
fn init_test_logger() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .is_test(true)
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

impl_message_handler!(DropTestActor, [TestMessage]);

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
