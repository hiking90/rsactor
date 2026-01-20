// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use rsactor::{
    set_default_mailbox_capacity, spawn, Actor, ActorRef, ActorResult, ActorWeak, Error, Identity,
    Message,
};

// Test Actor Setup
struct TestActor {
    id: Identity,
    counter: Arc<Mutex<i32>>,
    last_processed_message_type: Arc<Mutex<Option<String>>>,
    // Declaration to check if the Send trait is sufficient
    marker: std::marker::PhantomData<std::cell::Cell<()>>,
}

struct TestArgs {
    counter: Arc<Mutex<i32>>,
    last_processed_message_type: Arc<Mutex<Option<String>>>,
}

impl Actor for TestActor {
    type Args = TestArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        debug!("TestActor (id: {}) started.", actor_ref.identity());
        Ok(Self {
            id: actor_ref.identity(),
            counter: args.counter,
            last_processed_message_type: args.last_processed_message_type.clone(),
            marker: std::marker::PhantomData,
        })
    }
}

// Messages
#[derive(Debug)] // Added for logging if needed
struct PingMsg(String);
#[derive(Debug)]
struct UpdateCounterMsg(i32);
#[derive(Debug)]
struct GetCounterMsg;
struct SlowMsg; // Added for timeout tests

impl Message<PingMsg> for TestActor {
    type Reply = String;
    async fn handle(&mut self, msg: PingMsg, _: &ActorRef<Self>) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("PingMsg".to_string());
        format!("pong: {}", msg.0)
    }
}

impl Message<UpdateCounterMsg> for TestActor {
    type Reply = (); // tell type messages often use this.
    async fn handle(&mut self, msg: UpdateCounterMsg, _: &ActorRef<Self>) -> Self::Reply {
        let mut counter = self.counter.lock().await;
        *counter += msg.0;
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("UpdateCounterMsg".to_string());
    }
}

impl Message<GetCounterMsg> for TestActor {
    type Reply = i32;
    async fn handle(&mut self, _msg: GetCounterMsg, _: &ActorRef<Self>) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("GetCounterMsg".to_string());
        *self.counter.lock().await
    }
}

// Added for timeout tests
impl Message<SlowMsg> for TestActor {
    type Reply = ();
    async fn handle(&mut self, _msg: SlowMsg, _: &ActorRef<Self>) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("SlowMsg".to_string());
        tokio::time::sleep(std::time::Duration::from_millis(200)).await // Sleep for 100ms
    }
}

async fn setup_actor() -> (
    ActorRef<TestActor>,
    tokio::task::JoinHandle<ActorResult<TestActor>>,
    Arc<Mutex<i32>>,
    Arc<Mutex<Option<String>>>,
) {
    // It's good practice to initialize logger for tests, e.g. using a static Once.
    // For simplicity here, we assume it's handled or not strictly needed for output.
    // let _ = env_logger::builder().is_test(true).try_init();

    let counter = Arc::new(Mutex::new(0));
    let last_processed_message_type = Arc::new(Mutex::new(None::<String>));

    let args = TestArgs {
        counter: counter.clone(),
        last_processed_message_type: last_processed_message_type.clone(),
    };
    let (actor_ref, handle) = spawn::<TestActor>(args);
    // Give a moment for on_start to potentially run
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (actor_ref, handle, counter, last_processed_message_type)
}

#[tokio::test]
async fn test_spawn_and_actor_ref_id() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    actor_ref.stop().await.expect("Failed to stop actor");
    let result = handle.await.expect("Actor task failed");
    assert!(
        result.is_completed(),
        "Actor should have completed normally"
    );
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        assert_eq!(
            actor.id,
            actor_ref.identity(),
            "Actor state ID should match ActorRef ID"
        );
    } else {
        panic!("Actor state should not be None");
    }
}

#[tokio::test]
async fn test_actor_ref_ask() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    let reply: String = actor_ref
        .ask(PingMsg("hello".to_string()))
        .await
        .expect("ask failed for PingMsg");
    assert_eq!(reply, "pong: hello");

    let count: i32 = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("ask failed for GetCounterMsg");
    assert_eq!(count, 0);

    // ask can also be used for messages that don't conceptually return a value,
    // by expecting a unit type `()` if the handler is defined to return it.
    // Here UpdateCounterMsg returns ()
    let _: () = actor_ref
        .ask(UpdateCounterMsg(10))
        .await
        .expect("ask failed for UpdateCounterMsg");

    let count_after_update: i32 = actor_ref
        .ask(GetCounterMsg)
        .await
        .expect("ask failed for GetCounterMsg after update");
    assert_eq!(count_after_update, 10);

    actor_ref.stop().await.expect("Failed to stop actor");
    let _result = handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_tell() {
    let (actor_ref, handle, counter, last_processed) = setup_actor().await;

    actor_ref
        .tell(UpdateCounterMsg(5))
        .await
        .expect("tell failed");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await; // Allow time for processing

    assert_eq!(*counter.lock().await, 5);
    assert_eq!(
        *last_processed.lock().await,
        Some("UpdateCounterMsg".to_string())
    );

    actor_ref.stop().await.expect("Failed to stop actor");
    let _result = handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_stop() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    actor_ref.tell(UpdateCounterMsg(100)).await.unwrap();

    let ask_future = actor_ref.ask(GetCounterMsg); // Send before stop
    let count_val = ask_future
        .await
        .expect("ask sent before stop should succeed");
    assert_eq!(count_val, 100);

    actor_ref.stop().await.expect("stop command failed");

    let result = handle.await.expect("Actor task failed");
    assert!(
        result.is_completed(),
        "Actor should have completed normally"
    );
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        assert_eq!(
            actor.id,
            actor_ref.identity(),
            "Actor state ID should match ActorRef ID"
        );
        assert_eq!(
            *actor.counter.lock().await,
            100,
            "Counter should be 100 after stop"
        );
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after stop
    assert!(
        actor_ref.tell(UpdateCounterMsg(1)).await.is_err(),
        "Tell to stopped actor should fail"
    );
    assert!(
        actor_ref.ask(PingMsg("test".to_string())).await.is_err(),
        "Ask to stopped actor should fail"
    );
}

#[tokio::test]
async fn test_actor_ref_kill() {
    let (actor_ref, handle, _counter_arc_from_setup, _lpmt_arc_from_setup) = setup_actor().await;

    // Send a message that should ideally sit in the queue if kill is prioritized.
    // The initial value of counter in TestActor is 0.
    actor_ref
        .tell(UpdateCounterMsg(10))
        .await
        .expect("Tell UpdateCounterMsg failed");

    // Immediately send kill, without waiting for the previous message to be processed.
    // The dedicated terminate channel and biased select in Runtime should prioritize this.
    actor_ref.kill().expect("kill command failed");

    let result = handle.await.expect("Actor task failed to complete");

    assert!(result.was_killed(), "Actor should have been killed");
    assert!(
        result.is_completed(),
        "Actor should have completed normally (killed is a completion state)"
    );

    let (actor, _) = result.into();
    if let Some(actor) = actor {
        println!("Actor value: {:?}", actor.counter);

        // Verify that the UpdateCounterMsg(10) was NOT processed because kill took priority.
        let final_counter = *actor.counter.lock().await;
        assert_eq!(final_counter, 0, "Counter should be 0, indicating UpdateCounterMsg was not processed due to kill priority. Got: {final_counter}");

        let final_lpmt = actor.last_processed_message_type.lock().await.clone();
        assert_eq!(final_lpmt, None, "Last processed message type should be None, indicating UpdateCounterMsg was not processed. Got: {final_lpmt:?}");
        assert_eq!(
            actor.id,
            actor_ref.identity(),
            "Actor ID should match ActorRef ID"
        );
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after kill should still fail
    assert!(
        actor_ref.tell(UpdateCounterMsg(1)).await.is_err(),
        "Tell to killed actor should fail"
    );
    assert!(
        actor_ref.ask(PingMsg("test".to_string())).await.is_err(),
        "Ask to killed actor should fail"
    );
}

// Test actor lifecycle errors
struct LifecycleErrorArgs {
    fail_on_start: bool,
    fail_on_run: bool,
    fail_on_stop: bool,
    on_start_attempted: Arc<Mutex<bool>>,
    on_run_attempted: Arc<Mutex<bool>>,
    on_stop_attempted: Arc<Mutex<bool>>,
}

struct LifecycleErrorActor {
    _id: Identity,
    fail_on_run: bool,
    fail_on_stop: bool,
    on_start_attempted: Arc<Mutex<bool>>,
    on_run_attempted: Arc<Mutex<bool>>,
    on_stop_attempted: Arc<Mutex<bool>>,
}

impl Actor for LifecycleErrorActor {
    type Args = LifecycleErrorArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let _id = actor_ref.identity();
        *args.on_start_attempted.lock().await = true;
        if args.fail_on_start {
            Err(anyhow::anyhow!("simulated on_start failure"))
        } else {
            Ok(LifecycleErrorActor {
                _id,
                fail_on_run: args.fail_on_run,
                fail_on_stop: args.fail_on_stop,
                on_start_attempted: args.on_start_attempted,
                on_run_attempted: args.on_run_attempted,
                on_stop_attempted: args.on_stop_attempted,
            })
        }
    }

    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        *self.on_run_attempted.lock().await = true;
        if self.fail_on_run {
            Err(anyhow::anyhow!("simulated on_run failure"))
        } else {
            // Continue idle processing
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(true)
        }
    }

    async fn on_stop(
        &mut self,
        _actor_ref: &ActorWeak<Self>,
        _killed: bool,
    ) -> Result<(), Self::Error> {
        *self.on_stop_attempted.lock().await = true;
        if self.fail_on_stop {
            Err(anyhow::anyhow!("simulated on_stop failure"))
        } else {
            Ok(())
        }
    }
}

struct NoOpMsg; // Dummy message for LifecycleErrorActor
impl Message<NoOpMsg> for LifecycleErrorActor {
    type Reply = ();
    async fn handle(&mut self, _msg: NoOpMsg, _: &ActorRef<Self>) -> Self::Reply {}
}

#[tokio::test]
async fn test_actor_fail_on_start() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: true,
        fail_on_run: false,
        fail_on_stop: false,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_startup_failed(),
        "Actor should have failed on startup"
    );
    let (_, cause) = result.into();
    if let Some(cause) = cause {
        assert!(cause.to_string().contains("simulated on_start failure"));
    }

    // on_start failed, so no actor was created
    assert!(*on_start_attempted.lock().await);
    assert!(!*on_run_attempted.lock().await);
    assert!(!*on_stop_attempted.lock().await);
}

#[tokio::test]
async fn test_actor_fail_on_run() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: true,
        fail_on_stop: false,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_runtime_failed(),
        "Actor should have failed at runtime"
    );

    match result {
        rsactor::ActorResult::Failed { actor, error, .. } => {
            assert!(error.to_string().contains("simulated on_run failure"));
            if let Some(actor) = actor {
                assert!(*actor.on_start_attempted.lock().await);
                assert!(*actor.on_run_attempted.lock().await);
                // on_stop is now called even when on_run fails (for cleanup)
                assert!(*actor.on_stop_attempted.lock().await);
            }
        }
        _ => panic!("Expected Failed result"),
    }
}

#[tokio::test]
async fn test_set_default_mailbox_capacity_to_zero() {
    // This test is independent of whether the capacity has been set before or not.
    let result = set_default_mailbox_capacity(0);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{err}").contains("Mailbox capacity error:"));
    assert!(
        matches!(err, Error::MailboxCapacity { message } if message == "Global default mailbox capacity must be greater than 0"),
        "Error message for zero capacity didn't match"
    );
}

#[tokio::test]
async fn test_set_default_mailbox_capacity_already_set() {
    // Use a higher value (e.g., 1000) to avoid conflicts with other tests
    // that might have set a smaller value
    let capacity = 1000;

    // First attempt should succeed
    let first_result = set_default_mailbox_capacity(capacity);

    // If this is the first test to run that sets default capacity, it should succeed
    // If another test has already set it, this will fail with "already been set" error
    if first_result.is_err() {
        assert!(
            matches!(first_result, Err(Error::MailboxCapacity { message }) if message == "Global default mailbox capacity has already been set"),
            "Error message didn't match"
        );

        // In this case, we've already verified the error works as expected
        return;
    }

    // If the first call succeeded, then the second call should definitely fail
    let second_result = set_default_mailbox_capacity(capacity + 1);
    assert!(second_result.is_err(), "Expected second call to fail");

    assert!(
        matches!(second_result, Err(Error::MailboxCapacity { message }) if message == "Global default mailbox capacity has already been set"),
        "Error message for second attempt didn't match"
    );
}

// Test actor panic in message handler
#[derive(Debug)] // Added Debug for consistency and potential logging
struct PanicActor {}
impl Actor for PanicActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(PanicActor {})
    }
}
#[derive(Debug)] // Added Debug for consistency and potential logging
struct PanicMsg; // Define PanicMsg

impl Message<PanicMsg> for PanicActor {
    type Reply = ();
    async fn handle(&mut self, _msg: PanicMsg, _: &ActorRef<Self>) -> Self::Reply {
        panic!("Simulated panic in message handler");
    }
}

// Actor with String as Error type
struct StringErrorActor {
    _id: rsactor::Identity,
}

impl Actor for StringErrorActor {
    type Args = Arc<Mutex<bool>>;
    type Error = String; // Using String as the error type

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let on_start_called = args;
        *on_start_called.lock().await = true;
        debug!("StringErrorActor (id: {}) started.", actor_ref.identity());
        Ok(Self {
            _id: actor_ref.identity(),
        })
    }
    // run_loop will use the default implementation which returns Ok(true)
}

// Message for StringErrorActor
#[derive(Debug)]
struct SimpleMsg;

impl Message<SimpleMsg> for StringErrorActor {
    type Reply = String;
    async fn handle(&mut self, _msg: SimpleMsg, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        "SimpleMsg processed".to_string()
    }
}

#[tokio::test]
async fn test_actor_panic_in_message_handler() {
    let (actor_ref, handle) = spawn::<PanicActor>(());

    // Sending a message that causes a panic in the handler.
    // The ask call itself will likely fail because the actor task panics and closes the reply channel.
    let ask_result = actor_ref.ask(PanicMsg).await;
    assert!(ask_result.is_err(), "Ask should fail when handler panics");
    if let Err(e) = ask_result {
        // Error could be "reply channel closed" or similar, as the actor task terminates.
        println!("Ask error after handler panic: {e}");
        assert!(
            e.to_string().contains("Reply channel closed")
                || e.to_string().contains("Mailbox channel closed")
        );
    }

    // The JoinHandle should return Err because the underlying tokio task panicked.
    match handle.await {
        Ok(result) => {
            // This path should ideally not be taken if the task truly panics.
            // However, if the framework were to catch panics and convert them to runtime failures,
            // this would be the case. Current code does not do this for handler panics.
            panic!(
                "Expected JoinHandle to return Err due to task panic, but got Ok with result: {result:?}"
            );
        }
        Err(join_error) => {
            assert!(
                join_error.is_panic(),
                "Expected a panic JoinError from actor task"
            );
        }
    }
}

#[tokio::test]
async fn test_actor_with_string_error_type() {
    let on_start_called = Arc::new(Mutex::new(false));

    let actor_args = on_start_called.clone();
    let (actor_ref, handle) = spawn::<StringErrorActor>(actor_args);

    // Give a moment for on_start to potentially run
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        *on_start_called.lock().await,
        "on_start should be called for StringErrorActor"
    );

    // Send a message and check reply
    let reply: String = actor_ref
        .ask(SimpleMsg)
        .await
        .expect("ask failed for SimpleMsg");
    assert_eq!(reply, "SimpleMsg processed");

    // Stop the actor
    actor_ref
        .stop()
        .await
        .expect("Failed to stop StringErrorActor");
    let result = handle.await.expect("StringErrorActor task failed");

    assert!(
        result.is_completed(),
        "StringErrorActor should have completed normally"
    );
    assert!(
        !result.was_killed(),
        "StringErrorActor should not have been killed"
    );
    assert!(
        result.stopped_normally(),
        "StringErrorActor should have stopped normally"
    );
}

// Test: Spawning multiple actors

// Dummy Actor for simple spawn and stop test
struct DummyActor;

impl Actor for DummyActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(DummyActor)
    }
}

#[tokio::test]
async fn test_spawn_and_stop_dummy_actor() {
    let (actor_ref, handle) = spawn::<DummyActor>(());

    // Optionally, give a brief moment for the actor to fully start, though not strictly necessary
    // if on_start does nothing.
    // tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    actor_ref.stop().await.expect("Failed to stop dummy actor");
    let result = handle.await.expect("Dummy actor task failed");

    assert!(
        result.is_completed(),
        "Dummy actor should have completed normally"
    );
    assert!(
        !result.was_killed(),
        "Dummy actor should not have been killed"
    );
    assert!(
        result.stopped_normally(),
        "Dummy actor should have stopped normally"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_actor_ref_blocking_tell() {
    let (actor_ref, handle, counter, last_processed) = setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    let counter_clone = counter.clone();
    let last_processed_clone = last_processed.clone();

    // Spawn a blocking task to call blocking_tell
    let join_handle = tokio::task::spawn_blocking(move || {
        actor_ref_clone
            .blocking_tell(UpdateCounterMsg(7), None)
            .expect("blocking_tell failed");
    });

    join_handle.await.expect("Blocking task panicked");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await; // Allow time for processing

    assert_eq!(*counter_clone.lock().await, 7);
    assert_eq!(
        *last_processed_clone.lock().await,
        Some("UpdateCounterMsg".to_string())
    );

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_actor_ref_blocking_ask() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    // Spawn a blocking task to call blocking_ask
    let join_handle = tokio::task::spawn_blocking(move || {
        let reply: String = actor_ref_clone
            .blocking_ask(PingMsg("hello_blocking".to_string()), None)
            .expect("blocking_ask failed for PingMsg");
        assert_eq!(reply, "pong: hello_blocking");

        let count: i32 = actor_ref_clone
            .blocking_ask(GetCounterMsg, None)
            .expect("blocking_ask failed for GetCounterMsg");
        assert_eq!(count, 0);

        let _: () = actor_ref_clone
            .blocking_ask(UpdateCounterMsg(15), None)
            .expect("blocking_ask failed for UpdateCounterMsg");

        let count_after_update: i32 = actor_ref_clone
            .blocking_ask(GetCounterMsg, None)
            .expect("blocking_ask failed for GetCounterMsg after update");
        assert_eq!(count_after_update, 15);
    });

    // Added to increase the test coverage
    let actor_ref_clone = actor_ref.clone();
    let thread_handle = std::thread::spawn(move || {
        // With the new blocking implementation, these should succeed
        // because they don't require a Tokio runtime context
        assert!(actor_ref_clone
            .blocking_ask(PingMsg("hello_blocking".to_string()), None)
            .is_ok());
        assert!(actor_ref_clone
            .blocking_tell(PingMsg("hello_blocking".to_string()), None)
            .is_ok());
    });

    thread_handle.join().expect("Thread panicked");

    join_handle.await.expect("Blocking task panicked");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

// NOTE: Timeout test disabled because new blocking_ask doesn't support timeout
// #[tokio::test]
// async fn test_actor_ref_blocking_ask_timeout() {
//     // This test is no longer applicable since blocking_ask doesn't support timeout
// }

// NOTE: Timeout test disabled because new blocking_tell doesn't support timeout
// #[tokio::test]
// async fn test_actor_ref_blocking_tell_timeout_when_mailbox_full() {
//     // This test is no longer applicable since blocking_tell doesn't support timeout
// }

#[tokio::test(flavor = "multi_thread")]
async fn test_actor_ref_blocking_ask_no_timeout() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    // Spawn a blocking task to call blocking_ask with None timeout
    let join_handle_blocking_task = tokio::task::spawn_blocking(move || {
        let reply: String = actor_ref_clone
            .blocking_ask(PingMsg("hello_no_timeout".to_string()), None)
            .expect("blocking_ask with None timeout failed for PingMsg");
        assert_eq!(reply, "pong: hello_no_timeout");

        let count: i32 = actor_ref_clone
            .blocking_ask(GetCounterMsg, None)
            .expect("blocking_ask with None timeout failed for GetCounterMsg");
        assert_eq!(count, 0);
    });

    join_handle_blocking_task
        .await
        .expect("Blocking task for blocking_ask with None timeout panicked");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_kill_multiple_times() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    // Call kill multiple times
    actor_ref.kill().expect("First kill command failed");
    actor_ref
        .kill()
        .expect("Second kill command should also succeed (idempotent)");
    actor_ref
        .kill()
        .expect("Third kill command should also succeed (idempotent)");

    let result = handle.await.expect("Actor task failed to complete");

    assert!(result.is_completed(), "Actor should have completed");
    assert!(result.was_killed(), "Actor should have been killed");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        // Verify that messages sent before or after kill are not processed if kill is effective.
        // (This part is similar to test_actor_ref_kill, ensuring state consistency)
        let final_counter = *actor.counter.lock().await;
        assert_eq!(
            final_counter, 0,
            "Counter should be 0, indicating no messages processed due to kill. Got: {final_counter}"
        );
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after kill should still fail
    assert!(
        actor_ref.tell(UpdateCounterMsg(1)).await.is_err(),
        "Tell to killed actor should fail"
    );
    assert!(
        actor_ref.ask(PingMsg("test".to_string())).await.is_err(),
        "Ask to killed actor should fail"
    );
}

#[tokio::test]
async fn test_actor_ref_is_alive() {
    // Test 1: Actor is alive after spawn, and dead after stop
    let (actor_ref_stop_test, handle_stop_test, _counter_stop, _lpmt_stop) = setup_actor().await;

    assert!(
        actor_ref_stop_test.is_alive(),
        "Actor should be alive after spawn (stop test)"
    );

    actor_ref_stop_test
        .stop()
        .await
        .expect("Failed to stop actor (stop test)");
    let result_stop = handle_stop_test
        .await
        .expect("Actor task failed after stop (stop test)");
    assert!(
        result_stop.is_completed(),
        "Actor should have completed normally (stop test)"
    );
    assert!(
        !result_stop.was_killed(),
        "Actor should not have been killed (stop test)"
    );
    assert!(
        result_stop.stopped_normally(),
        "Actor should have stopped normally (stop test)"
    );

    assert!(
        !actor_ref_stop_test.is_alive(),
        "Actor should not be alive after stop (stop test)"
    );

    // Test 2: Actor is alive after spawn, and dead after kill
    let (actor_ref_kill_test, handle_kill_test, _counter_kill, _lpmt_kill) = setup_actor().await;

    assert!(
        actor_ref_kill_test.is_alive(),
        "Actor should be alive before kill (kill test)"
    );

    actor_ref_kill_test
        .kill()
        .expect("kill command failed (kill test)");
    let result_kill = handle_kill_test
        .await
        .expect("Actor task failed after kill (kill test)");
    assert!(
        result_kill.is_completed(),
        "Actor should have completed (kill test)"
    );
    assert!(
        result_kill.was_killed(),
        "Actor should have been killed (kill test)"
    );

    assert!(
        !actor_ref_kill_test.is_alive(),
        "Actor should not be alive after kill (kill test)"
    );
}

#[tokio::test]
async fn test_ask_with_timeout() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    // Test a successful case (timeout is long enough)
    let reply: String = actor_ref
        .ask_with_timeout(
            PingMsg("hello_timeout".to_string()),
            std::time::Duration::from_millis(500),
        )
        .await
        .expect("ask_with_timeout failed with sufficient timeout");
    assert_eq!(reply, "pong: hello_timeout");

    // Test timeout case - SlowMsg handler sleeps for 100ms, but we set a 10ms timeout
    let result: Result<(), _> = actor_ref
        .ask_with_timeout(SlowMsg, std::time::Duration::from_millis(10))
        .await;
    assert!(result.is_err(), "ask_with_timeout should have timed out");
    if let Err(e) = result {
        assert!(
            e.to_string().contains("timed out"),
            "Error message should mention timeout: {e}"
        );
    }

    // Verify regular operation works after timeout
    let count: i32 = actor_ref
        .ask_with_timeout(GetCounterMsg, std::time::Duration::from_millis(500))
        .await
        .expect("ask_with_timeout for GetCounterMsg should succeed");
    assert_eq!(count, 0);

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_tell_with_timeout() {
    let (actor_ref, handle, counter, last_processed) = setup_actor().await;

    // Test a successful case (timeout is sufficient)
    let result = actor_ref
        .tell_with_timeout(UpdateCounterMsg(5), std::time::Duration::from_millis(500))
        .await;
    assert!(
        result.is_ok(),
        "tell_with_timeout with sufficient timeout should succeed"
    );

    // Allow time for processing
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify the message was processed
    assert_eq!(*counter.lock().await, 5);
    assert_eq!(
        *last_processed.lock().await,
        Some("UpdateCounterMsg".to_string())
    );

    // Since the mailbox channel immediately accepts messages in most test scenarios,
    // it's hard to create a realistic timeout situation for tell_with_timeout
    // Without introducing artificial delays or mocks

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

// === UntypedActorRef Incompatible Message Tests ===

// === on_stop Error Handling Tests ===
#[tokio::test]
async fn test_actor_fail_on_stop_during_graceful_stop() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: false,
        fail_on_stop: true,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Gracefully stop the actor - this should trigger on_stop failure
    actor_ref.stop().await.expect("stop command should succeed");

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_stop_failed(),
        "Actor should have failed during stop"
    );
    assert!(!result.was_killed(), "Actor should not have been killed");

    match result {
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            assert_eq!(
                phase,
                rsactor::FailurePhase::OnStop,
                "Should fail in OnStop phase"
            );
            assert!(!killed, "Should not be marked as killed for graceful stop");
            assert!(error.to_string().contains("simulated on_stop failure"));

            if let Some(actor) = actor {
                assert!(
                    *actor.on_start_attempted.lock().await,
                    "on_start should have been called"
                );
                assert!(
                    *actor.on_run_attempted.lock().await,
                    "on_run should have been called"
                );
                assert!(
                    *actor.on_stop_attempted.lock().await,
                    "on_stop should have been called"
                );
            }
        }
        _ => panic!("Expected Failed result"),
    }
}

#[tokio::test]
async fn test_actor_fail_on_stop_during_kill() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: false,
        fail_on_stop: true,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Kill the actor - this should trigger on_stop failure
    actor_ref.kill().expect("kill command should succeed");

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_stop_failed(),
        "Actor should have failed during stop"
    );
    assert!(result.was_killed(), "Actor should be marked as killed");

    match result {
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            assert_eq!(
                phase,
                rsactor::FailurePhase::OnStop,
                "Should fail in OnStop phase"
            );
            assert!(killed, "Should be marked as killed for kill operation");
            assert!(error.to_string().contains("simulated on_stop failure"));

            if let Some(actor) = actor {
                assert!(
                    *actor.on_start_attempted.lock().await,
                    "on_start should have been called"
                );
                assert!(
                    *actor.on_run_attempted.lock().await,
                    "on_run should have been called"
                );
                assert!(
                    *actor.on_stop_attempted.lock().await,
                    "on_stop should have been called"
                );
            }
        }
        _ => panic!("Expected Failed result"),
    }
}

#[tokio::test]
async fn test_actor_fail_on_stop_after_on_run_failure() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: true,
        fail_on_stop: true, // on_stop will be called but its error is logged (not propagated)
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");

    // When on_run fails, on_stop is called for cleanup.
    // The original on_run error is preserved; on_stop error is only logged.
    assert!(
        result.is_runtime_failed(),
        "Actor should have failed during runtime (on_run)"
    );

    match result {
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            assert_eq!(
                phase,
                rsactor::FailurePhase::OnRun,
                "Should fail in OnRun phase (original error preserved)"
            );
            assert!(
                !killed,
                "Should not be marked as killed for on_run failure scenario"
            );
            // The error should be from on_run, not on_stop
            assert!(error.to_string().contains("simulated on_run failure"));

            if let Some(actor) = actor {
                assert!(
                    *actor.on_start_attempted.lock().await,
                    "on_start should have been called"
                );
                assert!(
                    *actor.on_run_attempted.lock().await,
                    "on_run should have been called"
                );
                // on_stop is now called for cleanup even when on_run fails
                assert!(
                    *actor.on_stop_attempted.lock().await,
                    "on_stop should be called for cleanup when on_run fails"
                );
            }
        }
        _ => panic!("Expected Failed result with OnRun phase"),
    }
}

#[tokio::test]
async fn test_actor_on_stop_success_after_on_run_failure() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: true,
        fail_on_stop: false, // on_stop will succeed
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");

    // When on_run fails, on_stop is called for cleanup.
    // The result is still Failed with OnRun phase.
    assert!(
        result.is_runtime_failed(),
        "Actor should have failed during runtime (on_run)"
    );

    match result {
        rsactor::ActorResult::Failed {
            actor,
            error,
            phase,
            killed,
        } => {
            assert_eq!(
                phase,
                rsactor::FailurePhase::OnRun,
                "Should fail in OnRun phase"
            );
            assert!(
                !killed,
                "Should not be marked as killed for on_run failure scenario"
            );
            assert!(error.to_string().contains("simulated on_run failure"));

            if let Some(actor) = actor {
                assert!(
                    *actor.on_start_attempted.lock().await,
                    "on_start should have been called"
                );
                assert!(
                    *actor.on_run_attempted.lock().await,
                    "on_run should have been called"
                );
                // on_stop is now called for cleanup even when on_run fails
                assert!(
                    *actor.on_stop_attempted.lock().await,
                    "on_stop should be called for cleanup when on_run fails"
                );
            }
        }
        _ => panic!("Expected Failed result with OnRun phase"),
    }
}

#[tokio::test]
async fn test_actor_on_stop_success_during_graceful_stop() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: false,
        fail_on_stop: false, // All lifecycle methods should succeed
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Gracefully stop the actor - this should succeed
    actor_ref.stop().await.expect("stop command should succeed");

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_completed(),
        "Actor should have completed successfully"
    );
    assert!(
        result.stopped_normally(),
        "Actor should have stopped normally"
    );
    assert!(!result.was_killed(), "Actor should not have been killed");

    if let rsactor::ActorResult::Completed { actor, killed } = result {
        assert!(!killed, "Should not be marked as killed for graceful stop");
        assert!(
            *actor.on_start_attempted.lock().await,
            "on_start should have been called"
        );
        assert!(
            *actor.on_run_attempted.lock().await,
            "on_run should have been called"
        );
        assert!(
            *actor.on_stop_attempted.lock().await,
            "on_stop should have been called"
        );
    } else {
        panic!("Expected Completed result");
    }
}

#[tokio::test]
async fn test_actor_on_stop_success_during_kill() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));
    let on_stop_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: false,
        fail_on_stop: false, // All lifecycle methods should succeed
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
        on_stop_attempted: on_stop_attempted.clone(),
    };
    let (actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    // Wait for actor to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Kill the actor - this should succeed
    actor_ref.kill().expect("kill command should succeed");

    let result = handle.await.expect("Join handle should not fail");
    assert!(
        result.is_completed(),
        "Actor should have completed successfully"
    );
    assert!(result.was_killed(), "Actor should be marked as killed");

    if let rsactor::ActorResult::Completed { actor, killed } = result {
        assert!(killed, "Should be marked as killed for kill operation");
        assert!(
            *actor.on_start_attempted.lock().await,
            "on_start should have been called"
        );
        assert!(
            *actor.on_run_attempted.lock().await,
            "on_run should have been called"
        );
        assert!(
            *actor.on_stop_attempted.lock().await,
            "on_stop should have been called"
        );
    } else {
        panic!("Expected Completed result");
    }
}
