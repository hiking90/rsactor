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
    type IdleEvent = ();

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
        let result = actor_ref_clone.blocking_tell(IncrementMsg(42), None);
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
        let result = actor_ref_clone.blocking_ask(GetCounterMsg, None);
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
        let result1 = actor_ref_clone.blocking_tell(IncrementMsg(10), None);
        if result1.is_err() {
            return Err(format!("First blocking_tell failed: {:?}", result1));
        }

        // Get current value (this should work even though we just did tell)
        let result2 = actor_ref_clone.blocking_ask(GetCounterMsg, None);
        if result2.is_err() {
            return Err(format!("blocking_ask failed: {:?}", result2));
        }

        // Second increment
        let result3 = actor_ref_clone.blocking_tell(IncrementMsg(5), None);
        if result3.is_err() {
            return Err(format!("Second blocking_tell failed: {:?}", result3));
        }

        // Final value check
        let final_result = actor_ref_clone.blocking_ask(GetCounterMsg, None);
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
        let result1 = actor_ref_clone.blocking_tell(IncrementMsg(25), None);
        if result1.is_err() {
            return Err(format!("blocking_tell failed: {:?}", result1));
        }

        // blocking_ask without timeout
        let result2 = actor_ref_clone.blocking_ask(GetCounterMsg, None);
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

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_tell_with_timeout() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test blocking_tell with timeout from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Calling blocking_tell with timeout from std::thread without runtime context...");
        let result = actor_ref_clone.blocking_tell(IncrementMsg(42), Some(Duration::from_secs(5)));
        println!("blocking_tell with timeout result: {:?}", result);
        result
    })
    .join()
    .expect("Thread should not panic");

    assert!(
        thread_result.is_ok(),
        "blocking_tell with timeout should succeed: {:?}",
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
async fn test_blocking_ask_with_timeout() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(100);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test blocking_ask with timeout from std::thread (no tokio context)
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        println!("Calling blocking_ask with timeout from std::thread without runtime context...");
        let result = actor_ref_clone.blocking_ask(GetCounterMsg, Some(Duration::from_secs(5)));
        println!("blocking_ask with timeout result: {:?}", result);
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
                "blocking_ask with timeout should succeed without existing runtime: {:?}",
                e
            );
        }
    }

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_with_timeout_inside_tokio_context() {
    // Test that blocking methods with timeout work even inside tokio spawn_blocking
    // This tests the separate thread + runtime approach
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Give actor time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    let actor_ref_clone = actor_ref.clone();
    let join_handle = tokio::task::spawn_blocking(move || {
        // blocking_tell with timeout inside spawn_blocking
        let tell_result =
            actor_ref_clone.blocking_tell(IncrementMsg(10), Some(Duration::from_secs(5)));
        assert!(
            tell_result.is_ok(),
            "blocking_tell with timeout should succeed: {:?}",
            tell_result
        );

        // blocking_ask with timeout inside spawn_blocking
        let ask_result = actor_ref_clone.blocking_ask(GetCounterMsg, Some(Duration::from_secs(5)));
        assert!(
            ask_result.is_ok(),
            "blocking_ask with timeout should succeed: {:?}",
            ask_result
        );

        ask_result.unwrap()
    });

    let counter_value = join_handle.await.expect("Blocking task panicked");
    assert_eq!(counter_value, 10, "Counter should be 10 after increment");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_ask_timeout_on_stopped_actor() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Stop the actor first
    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");

    // Test blocking_ask with timeout on stopped actor
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        actor_ref_clone.blocking_ask(GetCounterMsg, Some(Duration::from_secs(1)))
    })
    .join()
    .expect("Thread should not panic");

    assert!(
        thread_result.is_err(),
        "blocking_ask on stopped actor should fail"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_tell_timeout_on_stopped_actor() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // Stop the actor first
    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");

    // Test blocking_tell with timeout on stopped actor
    let actor_ref_clone = actor_ref.clone();
    let thread_result = std::thread::spawn(move || {
        actor_ref_clone.blocking_tell(IncrementMsg(1), Some(Duration::from_secs(1)))
    })
    .join()
    .expect("Thread should not panic");

    assert!(
        thread_result.is_err(),
        "blocking_tell on stopped actor should fail"
    );
}

// The following tests cover the `async fn -> sync fn -> blocking_*` bridge.
// On a multi-thread runtime the blocking_* APIs detect the current runtime
// and use `block_in_place` + `Handle::block_on` to reuse it. Before that
// change, calling `blocking_tell(_, None)` directly from an async context
// would panic because tokio's `blocking_send` rejects async contexts.

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_tell_from_async_multi_thread_context() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);

    // No timeout: this would panic on the old code path because it called
    // `blocking_send` directly. The block_in_place fast path makes it safe.
    actor_ref
        .blocking_tell(IncrementMsg(7), None)
        .expect("blocking_tell (no timeout) from async ctx should succeed");

    // With timeout: takes the same fast path; verify it still works.
    actor_ref
        .blocking_tell(IncrementMsg(3), Some(Duration::from_secs(5)))
        .expect("blocking_tell (timeout) from async ctx should succeed");

    let counter = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("Failed to get counter");
    assert_eq!(counter, 10);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blocking_ask_from_async_multi_thread_context() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(42);

    // No timeout: previously would panic from async ctx. Fast path makes it safe.
    let val = actor_ref
        .blocking_ask(GetCounterMsg, None)
        .expect("blocking_ask (no timeout) from async ctx should succeed");
    assert_eq!(val, 42);

    let val = actor_ref
        .blocking_ask(GetCounterMsg, Some(Duration::from_secs(5)))
        .expect("blocking_ask (timeout) from async ctx should succeed");
    assert_eq!(val, 42);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

// On a current_thread runtime, the block_in_place fast path is NOT eligible
// (block_in_place requires multi_thread). The blocking_* APIs must therefore
// fall through to the spawn-thread + new-runtime path. These tests pin that
// behaviour by invoking from `tokio::task::spawn_blocking` so that the test
// task yields and the runtime's sole thread is free to drive the actor.
// Calling the blocking_* APIs directly from `async fn` (or via a synchronous
// `std::thread::spawn(...).join()`) on a current_thread runtime would
// deadlock — the single runtime thread would be parked while the actor
// needed it to make progress. This is a fundamental property of
// current_thread runtimes, not a regression.
// `timeout: None` is omitted here because the fallback uses `blocking_send`
// directly which still panics in async context.

#[tokio::test(flavor = "current_thread")]
async fn test_blocking_tell_with_timeout_on_current_thread_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(0);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let actor_ref_clone = actor_ref.clone();
    tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_tell(IncrementMsg(11), Some(Duration::from_secs(5)))
    })
    .await
    .expect("spawn_blocking task should not panic")
    .expect("blocking_tell with timeout on current_thread runtime should succeed");

    tokio::time::sleep(Duration::from_millis(50)).await;
    let counter = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("Failed to get counter");
    assert_eq!(counter, 11);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "current_thread")]
async fn test_blocking_ask_with_timeout_on_current_thread_runtime() {
    let (actor_ref, handle) = spawn::<RuntimelessTestActor>(13);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let actor_ref_clone = actor_ref.clone();
    let val = tokio::task::spawn_blocking(move || {
        actor_ref_clone.blocking_ask(GetCounterMsg, Some(Duration::from_secs(5)))
    })
    .await
    .expect("spawn_blocking task should not panic")
    .expect("blocking_ask with timeout on current_thread runtime should succeed");
    assert_eq!(val, 13);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}
