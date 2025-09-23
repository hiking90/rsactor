// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for blocking methods when no tokio runtime is active.
//! These tests verify that tell_blocking and ask_blocking can create
//! their own runtime when called outside of any tokio context.

use std::time::Duration;

use rsactor::{spawn, Actor, ActorRef, Message};

// Test Actor for runtime-less environment tests
struct RuntimelessTestActor {
    counter: i32,
}

impl Actor for RuntimelessTestActor {
    type Args = i32;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Self { counter: args })
    }
}

// Test messages
#[derive(Debug)]
struct IncrementMsg(i32);

#[derive(Debug)]
struct GetCounterMsg;

impl Message<IncrementMsg> for RuntimelessTestActor {
    type Reply = ();
    async fn handle(&mut self, msg: IncrementMsg, _: &ActorRef<Self>) -> Self::Reply {
        self.counter += msg.0;
    }
}

impl Message<GetCounterMsg> for RuntimelessTestActor {
    type Reply = i32;
    async fn handle(&mut self, _msg: GetCounterMsg, _: &ActorRef<Self>) -> Self::Reply {
        self.counter
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tell_blocking_without_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test blocking call from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Calling blocking_tell from std::thread without runtime context...");
        let result = actor_ref_clone.blocking_tell(IncrementMsg(42));
        println!("blocking_tell result: {:?}", result);
        result
    })
    .join()
    .expect("Thread should not panic");

    assert!(
        thread_result.is_ok(),
        "blocking_tell should succeed without existing runtime: {:?}",
        thread_result
    );

    // Allow time for message processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify the message was processed
    let final_counter = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("Failed to get counter");
    println!("Final counter value: {}", final_counter);
    assert_eq!(final_counter, 42);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_ask_blocking_without_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(100);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test blocking call from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Calling ask_blocking from std::thread without runtime context...");
        // let result = actor_ref_clone.blocking_tell(IncrementMsg(100));
        let result = actor_ref_clone.blocking_ask(GetCounterMsg);
        println!("ask_blocking result: {:?}", result);
        result
    })
    .join()
    .expect("Thread should not panic");

    match thread_result {
        Ok(value) => {
            assert_eq!(value, 100, "Should get initial counter value");
        }
        Err(e) => {
            panic!(
                "ask_blocking should succeed without existing runtime: {:?}",
                e
            );
        }
    }

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_blocking_calls_without_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test multiple blocking calls in sequence from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Testing multiple blocking calls from std::thread without runtime...");

        // First increment
        let result1 = actor_ref_clone.blocking_tell(IncrementMsg(10));
        if result1.is_err() {
            return Err(format!("First blocking_tell failed: {:?}", result1));
        }

        // Get current value (this should work even though we just did tell)
        let result2 = actor_ref_clone.blocking_ask(GetCounterMsg);
        if result2.is_err() {
            return Err(format!("blocking_ask failed: {:?}", result2));
        }

        // Second increment
        let result3 = actor_ref_clone.blocking_tell(IncrementMsg(5));
        if result3.is_err() {
            return Err(format!("Second blocking_tell failed: {:?}", result3));
        }

        // Final value check
        let final_result = actor_ref_clone.blocking_ask(GetCounterMsg);
        match final_result {
            Ok(value) => {
                if value == 15 {
                    Ok(value)
                } else {
                    Err(format!(
                        "Final counter should be 15 (10 + 5), got: {}",
                        value
                    ))
                }
            }
            Err(e) => Err(format!("Final blocking_ask failed: {:?}", e)),
        }
    })
    .join()
    .expect("Thread should not panic");

    match thread_result {
        Ok(final_value) => {
            assert_eq!(final_value, 15, "Final counter should be 15 (10 + 5)");
        }
        Err(error_msg) => {
            panic!("{}", error_msg);
        }
    }

    // Verify final state from the tokio context as well
    let final_counter = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("Failed to get counter");
    assert_eq!(final_counter, 15);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_calls_without_timeout_and_without_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(50);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test blocking calls without timeout from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Testing blocking calls without timeout from std::thread without runtime...");

        // blocking_tell without timeout
        let result1 = actor_ref_clone.blocking_tell(IncrementMsg(25));
        if result1.is_err() {
            return Err(format!("blocking_tell failed: {:?}", result1));
        }

        // blocking_ask without timeout
        let result2 = actor_ref_clone.blocking_ask(GetCounterMsg);
        match result2 {
            Ok(value) => {
                if value == 75 {
                    Ok(value)
                } else {
                    Err(format!("Counter should be 75 (50 + 25), got: {}", value))
                }
            }
            Err(e) => Err(format!("blocking_ask failed: {:?}", e)),
        }
    })
    .join()
    .expect("Thread should not panic");

    match thread_result {
        Ok(final_value) => {
            assert_eq!(final_value, 75, "Counter should be 75 (50 + 25)");
        }
        Err(error_msg) => {
            panic!("{}", error_msg);
        }
    }

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}
