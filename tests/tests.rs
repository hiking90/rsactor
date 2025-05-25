// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use tokio::sync::Mutex;
use std::sync::Arc;
use log::debug; // Ensure 'log' crate is a dev-dependency or available

use rsactor::{
    impl_message_handler, set_default_mailbox_capacity, spawn, spawn_with_mailbox_capacity, Actor, ActorRef, ActorResult, Error, Identity, Message
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

    async fn on_start(args: Self::Args, actor_ref: &ActorRef) -> Result<Self, Self::Error> {
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
    async fn handle(&mut self, msg: PingMsg, _: &ActorRef) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("PingMsg".to_string());
        format!("pong: {}", msg.0)
    }
}

impl Message<UpdateCounterMsg> for TestActor {
    type Reply = (); // tell type messages often use this.
    async fn handle(&mut self, msg: UpdateCounterMsg, _: &ActorRef) -> Self::Reply {
        let mut counter = self.counter.lock().await;
        *counter += msg.0;
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("UpdateCounterMsg".to_string());
    }
}

impl Message<GetCounterMsg> for TestActor {
    type Reply = i32;
    async fn handle(&mut self, _msg: GetCounterMsg, _: &ActorRef) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("GetCounterMsg".to_string());
        *self.counter.lock().await
    }
}

// Added for timeout tests
impl Message<SlowMsg> for TestActor {
    type Reply = ();
    async fn handle(&mut self, _msg: SlowMsg, _: &ActorRef) -> Self::Reply {
        let mut lpmt = self.last_processed_message_type.lock().await;
        *lpmt = Some("SlowMsg".to_string());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await // Sleep for 100ms
    }
}

impl_message_handler!(TestActor, [PingMsg, UpdateCounterMsg, GetCounterMsg, SlowMsg]);

async fn setup_actor() -> (
    ActorRef,
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
    (
        actor_ref,
        handle,
        counter,
        last_processed_message_type,
    )
}

#[tokio::test]
async fn test_spawn_and_actor_ref_id() {
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;
    assert_ne!(actor_ref.identity().id, 0, "Actor ID should be non-zero");

    actor_ref.stop().await.expect("Failed to stop actor");
    let result = handle.await.expect("Actor task failed");
    assert!(result.is_completed(), "Actor should have completed normally");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        assert_eq!(actor.id, actor_ref.identity(), "Actor state ID should match ActorRef ID");
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
    let (actor_ref, handle, counter, last_processed) =
        setup_actor().await;

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
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;

    actor_ref.tell(UpdateCounterMsg(100)).await.unwrap();

    let ask_future = actor_ref.ask::<_, i32>(GetCounterMsg); // Send before stop
    let count_val = ask_future.await.expect("ask sent before stop should succeed");
    assert_eq!(count_val, 100);

    actor_ref.stop().await.expect("stop command failed");

    let result = handle.await.expect("Actor task failed");
    assert!(result.is_completed(), "Actor should have completed normally");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        assert_eq!(actor.id, actor_ref.identity(), "Actor state ID should match ActorRef ID");
        assert_eq!(*actor.counter.lock().await, 100, "Counter should be 100 after stop");
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after stop
    assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to stopped actor should fail");
    assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to stopped actor should fail");
}

#[tokio::test]
async fn test_actor_ref_kill() {
    let (actor_ref, handle, _counter_arc_from_setup, _lpmt_arc_from_setup) =
        setup_actor().await;

    // Send a message that should ideally sit in the queue if kill is prioritized.
    // The initial value of counter in TestActor is 0.
    actor_ref.tell(UpdateCounterMsg(10)).await.expect("Tell UpdateCounterMsg failed");

    // Immediately send kill, without waiting for the previous message to be processed.
    // The dedicated terminate channel and biased select in Runtime should prioritize this.
    actor_ref.kill().expect("kill command failed");

    let result = handle.await.expect("Actor task failed to complete");

    assert!(result.was_killed(), "Actor should have been killed");
    assert!(result.is_completed(), "Actor should have completed normally (killed is a completion state)");

    let (actor, _) = result.into();
    if let Some(actor) = actor {
        println!("Actor value: {:?}", actor.counter);

        // Verify that the UpdateCounterMsg(10) was NOT processed because kill took priority.
        let final_counter = *actor.counter.lock().await;
        assert_eq!(final_counter, 0, "Counter should be 0, indicating UpdateCounterMsg was not processed due to kill priority. Got: {}", final_counter);

        let final_lpmt = actor.last_processed_message_type.lock().await.clone();
        assert_eq!(final_lpmt, None, "Last processed message type should be None, indicating UpdateCounterMsg was not processed. Got: {:?}", final_lpmt);
        assert_eq!(actor.id, actor_ref.identity(), "Actor ID should match ActorRef ID");
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after kill should still fail
    assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to killed actor should fail");
    assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to killed actor should fail");
}

#[tokio::test]
async fn test_ask_wrong_reply_type() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    let result = actor_ref.ask::<PingMsg, i32>(PingMsg("hello".to_string())).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Failed to downcast reply"));
    }

    actor_ref.stop().await.unwrap();
    let _result = handle.await.unwrap();
}

#[tokio::test]
async fn test_unhandled_message_type() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    struct UnhandledMsg; // Not in impl_message_handler! for TestActor

    let result = actor_ref.ask::<UnhandledMsg, ()>(UnhandledMsg).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("received an unhandled message type."));
    }

    actor_ref.stop().await.unwrap();
    // Actor panics with unhandled message, no need to call stop
    let join_result = handle.await;
    assert!(join_result.is_err(), "Expected actor to panic with unhandled message");
    if let Err(join_error) = join_result {
        assert!(join_error.is_panic(), "The join error should be caused by a panic");
    }
    // We don't assert on_stop_called since the actor panics before on_stop can be called
}

// Test actor lifecycle errors
struct LifecycleErrorArgs {
    fail_on_start: bool,
    fail_on_run: bool,
    on_start_attempted: Arc<Mutex<bool>>,
    on_run_attempted: Arc<Mutex<bool>>,
}

struct LifecycleErrorActor {
    _id: Identity,
    fail_on_run: bool,
    on_start_attempted: Arc<Mutex<bool>>,
    on_run_attempted: Arc<Mutex<bool>>,
}

impl Actor for LifecycleErrorActor {
    type Args = LifecycleErrorArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef) -> Result<Self, Self::Error> {
        let _id = actor_ref.identity();
        *args.on_start_attempted.lock().await = true;
        if args.fail_on_start {
            Err(anyhow::anyhow!("simulated on_start failure"))
        } else {
            Ok(LifecycleErrorActor {
                _id,
                fail_on_run: args.fail_on_run,
                on_start_attempted: args.on_start_attempted,
                on_run_attempted: args.on_run_attempted,
            })
        }
    }

    async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
        *self.on_run_attempted.lock().await = true;
        if self.fail_on_run {
            Err(anyhow::anyhow!("simulated on_run failure"))
        } else {
            loop {
                // Simulate some work
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

struct NoOpMsg; // Dummy message for LifecycleErrorActor
impl Message<NoOpMsg> for LifecycleErrorActor {
    type Reply = ();
    async fn handle(&mut self, _msg: NoOpMsg, _: &ActorRef) -> Self::Reply {}
}
impl_message_handler!(LifecycleErrorActor, [NoOpMsg]);

#[tokio::test]
async fn test_actor_fail_on_start() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: true,
        fail_on_run: false,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");
    assert!(result.is_startup_failed(), "Actor should have failed on startup");
    let (_, cause) = result.into();
    if let Some(cause) = cause {
        assert!(cause.to_string().contains("simulated on_start failure"));
    }

    // on_start failed, so no actor was created
    assert!(*on_start_attempted.lock().await);
    assert!(!*on_run_attempted.lock().await);
}

#[tokio::test]
async fn test_actor_fail_on_run() {
    let on_start_attempted = Arc::new(Mutex::new(false));
    let on_run_attempted = Arc::new(Mutex::new(false));

    let args = LifecycleErrorArgs {
        fail_on_start: false,
        fail_on_run: true,
        on_start_attempted: on_start_attempted.clone(),
        on_run_attempted: on_run_attempted.clone(),
    };
    let (_actor_ref, handle) = spawn::<LifecycleErrorActor>(args);

    let result = handle.await.expect("Join handle should not fail");
    assert!(result.is_runtime_failed(), "Actor should have failed at runtime");

    match result {
        rsactor::ActorResult::Failed { actor, error , ..} => {
            assert!(error.to_string().contains("simulated on_run failure"));
            if let Some(actor) = actor {
                assert!(*actor.on_start_attempted.lock().await);
                assert!(*actor.on_run_attempted.lock().await);
            }
        }
        _ => panic!("Expected RuntimeFailed result"),
    }
}

#[tokio::test]
async fn test_set_default_mailbox_capacity_to_zero() {
    // This test is independent of whether the capacity has been set before or not.
    let result = set_default_mailbox_capacity(0);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{}", err).contains("Mailbox capacity error:"));
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
struct PanicActor {
}
impl Actor for PanicActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef) -> Result<Self, Self::Error> {
        Ok(PanicActor {
        })
    }
}
#[derive(Debug)] // Added Debug for consistency and potential logging
struct PanicMsg; // Define PanicMsg

impl Message<PanicMsg> for PanicActor {
    type Reply = ();
    async fn handle(&mut self, _msg: PanicMsg, _: &ActorRef) -> Self::Reply {
        panic!("Simulated panic in message handler");
    }
}
impl_message_handler!(PanicActor, [PanicMsg]);

// Actor with String as Error type
struct StringErrorActor {
    _id: rsactor::Identity,
}

impl Actor for StringErrorActor {
    type Args = Arc<Mutex<bool>>;
    type Error = String; // Using String as the error type

    async fn on_start(args: Self::Args, actor_ref: &ActorRef) -> Result<Self, Self::Error> {
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
    async fn handle(&mut self, _msg: SimpleMsg, _actor_ref: &ActorRef) -> Self::Reply {
        "SimpleMsg processed".to_string()
    }
}

impl_message_handler!(StringErrorActor, [SimpleMsg]);

#[tokio::test]
async fn test_actor_panic_in_message_handler() {
    let (actor_ref, handle) = spawn::<PanicActor>(());

    // Sending a message that causes a panic in the handler.
    // The ask call itself will likely fail because the actor task panics and closes the reply channel.
    let ask_result = actor_ref.ask::<PanicMsg, ()>(PanicMsg).await;
    assert!(ask_result.is_err(), "Ask should fail when handler panics");
    if let Err(e) = ask_result {
        // Error could be "reply channel closed" or similar, as the actor task terminates.
        println!("Ask error after handler panic: {}", e);
        assert!(e.to_string().contains("Reply channel closed") || e.to_string().contains("Mailbox channel closed"));
    }

    // The JoinHandle should return Err because the underlying tokio task panicked.
    match handle.await {
        Ok(result) => {
            // This path should ideally not be taken if the task truly panics.
            // However, if the framework were to catch panics and convert them to runtime failures,
            // this would be the case. Current code does not do this for handler panics.
            panic!("Expected JoinHandle to return Err due to task panic, but got Ok with result: {:?}", result);
        }
        Err(join_error) => {
            assert!(join_error.is_panic(), "Expected a panic JoinError from actor task");
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
    assert!(*on_start_called.lock().await, "on_start should be called for StringErrorActor");

    // Send a message and check reply
    let reply: String = actor_ref.ask(SimpleMsg).await.expect("ask failed for SimpleMsg");
    assert_eq!(reply, "SimpleMsg processed");

    // Stop the actor
    actor_ref.stop().await.expect("Failed to stop StringErrorActor");
    let result = handle.await.expect("StringErrorActor task failed");

    assert!(result.is_completed(), "StringErrorActor should have completed normally");
    assert!(!result.was_killed(), "StringErrorActor should not have been killed");
    assert!(result.stopped_normally(), "StringErrorActor should have stopped normally");
}

// Test: Spawning multiple actors

// Dummy Actor for simple spawn and stop test
struct DummyActor;

impl Actor for DummyActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef) -> Result<Self, Self::Error> {
        Ok(DummyActor)
    }
}

// Even if the actor handles no messages, impl_message_handler is needed
impl_message_handler!(DummyActor, []); // Assuming this is valid for no messages

#[tokio::test]
async fn test_spawn_and_stop_dummy_actor() {
    let (actor_ref, handle) = spawn::<DummyActor>(());

    // Optionally, give a brief moment for the actor to fully start, though not strictly necessary
    // if on_start does nothing.
    // tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    actor_ref.stop().await.expect("Failed to stop dummy actor");
    let result = handle.await.expect("Dummy actor task failed");

    assert!(result.is_completed(), "Dummy actor should have completed normally");
    assert!(!result.was_killed(), "Dummy actor should not have been killed");
    assert!(result.stopped_normally(), "Dummy actor should have stopped normally");
}

#[tokio::test]
async fn test_actor_ref_tell_blocking() {
    let (actor_ref, handle, counter, last_processed) =
        setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    let counter_clone = counter.clone();
    let last_processed_clone = last_processed.clone();

    // Spawn a blocking task to call tell_blocking
    let join_handle = tokio::task::spawn_blocking(move || {
        actor_ref_clone
            .tell_blocking(UpdateCounterMsg(7), Some(std::time::Duration::from_millis(100)))
            .expect("tell_blocking failed");
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

#[tokio::test]
async fn test_actor_ref_ask_blocking() {
    let (actor_ref, handle, _counter, _lpmt) = setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    // Spawn a blocking task to call ask_blocking
    let join_handle = tokio::task::spawn_blocking(move || {
        let reply: String = actor_ref_clone
            .ask_blocking(PingMsg("hello_blocking".to_string()), Some(std::time::Duration::from_millis(100)))
            .expect("ask_blocking failed for PingMsg");
        assert_eq!(reply, "pong: hello_blocking");

        let count: i32 = actor_ref_clone
            .ask_blocking(GetCounterMsg, Some(std::time::Duration::from_millis(100)))
            .expect("ask_blocking failed for GetCounterMsg");
        assert_eq!(count, 0);

        let _: () = actor_ref_clone
            .ask_blocking(UpdateCounterMsg(15), Some(std::time::Duration::from_millis(100)))
            .expect("ask_blocking failed for UpdateCounterMsg");

        let count_after_update: i32 = actor_ref_clone
            .ask_blocking(GetCounterMsg, Some(std::time::Duration::from_millis(100)))
            .expect("ask_blocking failed for GetCounterMsg after update");
        assert_eq!(count_after_update, 15);
    });

    // Added to increase the test coverage
    let actor_ref_clone = actor_ref.clone();
    let thread_handle = std::thread::spawn(move || {
        // Explicitly specify the message type M=PingMsg and reply type R=String
        // PingMsg is defined to reply with String.
        assert!(actor_ref_clone.ask_blocking::<PingMsg, String>(PingMsg("hello_blocking".to_string()), None).is_err());
        assert!(actor_ref_clone.tell_blocking(PingMsg("hello_blocking".to_string()), None).is_err());
    });

    thread_handle.join().expect("Thread panicked");

    join_handle.await.expect("Blocking task panicked");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_ask_blocking_timeout() {
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;

    // SlowMsg handler sleeps for 100ms. ask_blocking timeout is 10ms.
    let actor_ref_clone = actor_ref.clone();
    let join_handle = tokio::task::spawn_blocking(move || {
        let result: Result<(), _> = actor_ref_clone
            .ask_blocking(SlowMsg, Some(std::time::Duration::from_millis(10))); // Timeout 10ms
        assert!(result.is_err(), "ask_blocking should have timed out");
        if let Err(e) = result {
            // The error message format includes actor ID and timeout duration
            // Just check that it contains "timed out" which is what we care about
            assert!(e.to_string().contains("timed out"), "Error should indicate a timeout: {}", e);
        }
    });

    join_handle.await.expect("Blocking task panicked for ask timeout test");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_tell_blocking_timeout_when_mailbox_full() {
    // Spawn an actor with a mailbox capacity of 1.
    let counter = Arc::new(Mutex::new(0));
    let last_processed_message_type = Arc::new(Mutex::new(None));

    let actor_args = TestArgs {
        counter: counter.clone(),
        last_processed_message_type: last_processed_message_type.clone(),
    };
    // Spawn with capacity 1
    let (actor_ref, handle) = spawn_with_mailbox_capacity::<TestActor>(actor_args, 1);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await; // on_start

    // 1. Send SlowMsg to make the actor busy. Handler sleeps for 100ms.
    actor_ref.tell(SlowMsg).await.expect("Tell SlowMsg failed");
    // Give a moment for the actor to pick up SlowMsg and start sleeping.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // 2. Send UpdateCounterMsg(1). This will fill the mailbox (capacity 1)
    //    because the actor is busy with SlowMsg.
    actor_ref.tell(UpdateCounterMsg(1)).await.expect("Tell UpdateCounterMsg(1) to fill mailbox failed");

    // 3. Attempt tell_blocking with another message. This should timeout.
    let actor_ref_clone = actor_ref.clone();
    let join_handle_blocking_task = tokio::task::spawn_blocking(move || {
        let result = actor_ref_clone
            .tell_blocking(UpdateCounterMsg(2), Some(std::time::Duration::from_millis(10))); // Timeout 10ms
        assert!(result.is_err(), "tell_blocking should have timed out");
        if let Err(e) = result {
            // The error message might include more details like actor ID and timeout duration
            // Just check that it contains "timed out" which is what we care about
            assert!(e.to_string().contains("timed out"), "Error should indicate a timeout: {}", e);
        }
    });

    join_handle_blocking_task.await.expect("Blocking task for tell_blocking timeout panicked");

    // Allow the actor to process messages (SlowMsg, then UpdateCounterMsg(1))
    // The UpdateCounterMsg(2) from tell_blocking should have failed and not be in the queue.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await; // Wait for SlowMsg (100ms) + UpdateCounterMsg

    actor_ref.stop().await.expect("Failed to stop actor");
    let result = handle.await.expect("Actor task failed");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        // Verify that only UpdateCounterMsg(1) was processed.
        assert_eq!(*actor.counter.lock().await, 1, "Counter should be 1 after SlowMsg and UpdateCounterMsg(1)");
    } else {
        panic!("Actor state should not be None");
    }
}

#[tokio::test]
async fn test_actor_ref_ask_blocking_no_timeout() {
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;

    let actor_ref_clone = actor_ref.clone();
    // Spawn a blocking task to call ask_blocking with None timeout
    let join_handle_blocking_task = tokio::task::spawn_blocking(move || {
        let reply: String = actor_ref_clone
            .ask_blocking(PingMsg("hello_no_timeout".to_string()), None)
            .expect("ask_blocking with None timeout failed for PingMsg");
        assert_eq!(reply, "pong: hello_no_timeout");

        let count: i32 = actor_ref_clone
            .ask_blocking(GetCounterMsg, None)
            .expect("ask_blocking with None timeout failed for GetCounterMsg");
        assert_eq!(count, 0);
    });

    join_handle_blocking_task
        .await
        .expect("Blocking task for ask_blocking with None timeout panicked");

    actor_ref.stop().await.expect("Failed to stop actor");
    handle.await.expect("Actor task failed");
}

#[tokio::test]
async fn test_actor_ref_kill_multiple_times() {
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;

    // Call kill multiple times
    actor_ref.kill().expect("First kill command failed");
    actor_ref.kill().expect("Second kill command should also succeed (idempotent)");
    actor_ref.kill().expect("Third kill command should also succeed (idempotent)");

    let result = handle.await.expect("Actor task failed to complete");

    assert!(result.is_completed(), "Actor should have completed");
    assert!(result.was_killed(), "Actor should have been killed");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        // Verify that messages sent before or after kill are not processed if kill is effective.
        // (This part is similar to test_actor_ref_kill, ensuring state consistency)
        let final_counter = *actor.counter.lock().await;
        assert_eq!(final_counter, 0, "Counter should be 0, indicating no messages processed due to kill. Got: {}", final_counter);
    } else {
        panic!("Actor state should not be None");
    }

    // Interactions after kill should still fail
    assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to killed actor should fail");
    assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to killed actor should fail");
}

#[tokio::test]
async fn test_actor_ref_is_alive() {
    // Test 1: Actor is alive after spawn, and dead after stop
    let (actor_ref_stop_test, handle_stop_test, _counter_stop, _lpmt_stop) =
        setup_actor().await;

    assert!(actor_ref_stop_test.is_alive(), "Actor should be alive after spawn (stop test)");

    actor_ref_stop_test.stop().await.expect("Failed to stop actor (stop test)");
    let result_stop = handle_stop_test.await.expect("Actor task failed after stop (stop test)");
    assert!(result_stop.is_completed(), "Actor should have completed normally (stop test)");
    assert!(!result_stop.was_killed(), "Actor should not have been killed (stop test)");
    assert!(result_stop.stopped_normally(), "Actor should have stopped normally (stop test)");

    assert!(!actor_ref_stop_test.is_alive(), "Actor should not be alive after stop (stop test)");

    // Test 2: Actor is alive after spawn, and dead after kill
    let (actor_ref_kill_test, handle_kill_test, _counter_kill, _lpmt_kill) =
        setup_actor().await;

    assert!(actor_ref_kill_test.is_alive(), "Actor should be alive before kill (kill test)");

    actor_ref_kill_test.kill().expect("kill command failed (kill test)");
    let result_kill = handle_kill_test.await.expect("Actor task failed after kill (kill test)");
    assert!(result_kill.is_completed(), "Actor should have completed (kill test)");
    assert!(result_kill.was_killed(), "Actor should have been killed (kill test)");

    assert!(!actor_ref_kill_test.is_alive(), "Actor should not be alive after kill (kill test)");
}

#[tokio::test]
async fn test_ask_with_timeout() {
    let (actor_ref, handle, _counter, _lpmt) =
        setup_actor().await;

    // Test a successful case (timeout is long enough)
    let reply: String = actor_ref
        .ask_with_timeout(PingMsg("hello_timeout".to_string()), std::time::Duration::from_millis(500))
        .await
        .expect("ask_with_timeout failed with sufficient timeout");
    assert_eq!(reply, "pong: hello_timeout");

    // Test timeout case - SlowMsg handler sleeps for 100ms, but we set a 10ms timeout
    let result: Result<(), _> = actor_ref
        .ask_with_timeout(SlowMsg, std::time::Duration::from_millis(10))
        .await;
    assert!(result.is_err(), "ask_with_timeout should have timed out");
    if let Err(e) = result {
        assert!(e.to_string().contains("timed out"), "Error message should mention timeout: {}", e);
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
    let (actor_ref, handle, counter, last_processed) =
        setup_actor().await;

    // Test a successful case (timeout is sufficient)
    let result = actor_ref
        .tell_with_timeout(UpdateCounterMsg(5), std::time::Duration::from_millis(500))
        .await;
    assert!(result.is_ok(), "tell_with_timeout with sufficient timeout should succeed");

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

#[test]
fn test_runtime_error_outside_tokio() {
    // Setup an actor in tokio runtime
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let actor_ref = runtime.block_on(async {
        let (actor_ref, _handle, _, _) = setup_actor().await;
        actor_ref
    });

    // Now drop the runtime to ensure we're outside a tokio context
    drop(runtime);

    // Attempt to use ask_blocking outside tokio runtime context
    let result = actor_ref.ask_blocking::<PingMsg, String>(
        PingMsg("hello".to_string()),
        Some(std::time::Duration::from_millis(100))
    );

    // This should result in Error::Runtime
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, Error::Runtime { .. }), "Expected Error::Runtime, got: {:?}", e);
        assert!(format!("{}", e).contains("Runtime error in"));

        // Extract and validate error details
        if let Error::Runtime { identity, details } = e {
            assert_ne!(identity.id, 0, "Actor ID should be non-zero");
            assert!(details.contains("No tokio runtime available"),
                "Error details should mention tokio runtime unavailability: {}", details);
        }
    }

    // Similarly test tell_blocking
    let tell_result = actor_ref.tell_blocking(
        UpdateCounterMsg(5),
        Some(std::time::Duration::from_millis(100))
    );

    assert!(tell_result.is_err());
    if let Err(e) = tell_result {
        assert!(matches!(e, Error::Runtime { .. }), "Expected Error::Runtime, got: {:?}", e);

        if let Error::Runtime { identity, details } = e {
            assert_ne!(identity.id, 0, "Actor ID should be non-zero");
            assert!(details.contains("No tokio runtime available"),
                "Error details should mention tokio runtime unavailability: {}", details);
        }
    }
}

#[tokio::test]
#[should_panic(expected = "Mailbox capacity must be greater than 0")]
async fn test_spawn_with_zero_mailbox_capacity() {
    // Prepare an actor instance
    let counter = Arc::new(Mutex::new(0));
    let last_processed_message_type = Arc::new(Mutex::new(None::<String>));

    let actor_args = TestArgs {
        counter,
        last_processed_message_type,
    };

    // This should panic with message "Mailbox capacity must be greater than 0"
    let (_actor_ref, _handle) = spawn_with_mailbox_capacity::<TestActor>(actor_args, 0);
}

#[tokio::test]
async fn test_mailbox_capacity_error_when_full() {
    // Create an actor with a very small mailbox capacity (1)
    let counter = Arc::new(Mutex::new(0));
    let last_processed_message_type = Arc::new(Mutex::new(None::<String>));

    let actor_args = TestArgs {
        counter: counter.clone(),
        last_processed_message_type: last_processed_message_type.clone(),
    };

    // Spawn with capacity 1 to make it easy to fill the mailbox
    let (actor_ref, handle) = spawn_with_mailbox_capacity::<TestActor>(actor_args, 1);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await; // Give time for on_start

    // Send a SlowMsg that will make the actor busy for 100ms
    actor_ref.tell(SlowMsg).await.expect("Tell SlowMsg failed");
    tokio::time::sleep(std::time::Duration::from_millis(10)).await; // Ensure message is being processed

    // Fill the mailbox with one message (capacity is 1)
    actor_ref.tell(UpdateCounterMsg(1)).await.expect("Tell UpdateCounterMsg(1) to fill mailbox failed");

    // Now try to send another message with a timeout - this should fail with MailboxCapacity error
    let result = actor_ref
        .tell_with_timeout(UpdateCounterMsg(2), std::time::Duration::from_millis(50))
        .await;

    assert!(result.is_err(), "Expected tell_with_timeout to fail with mailbox full");
    assert!(matches!(result, Err(Error::Timeout { .. })),
        "Expected timeout error when mailbox is full, got: {:?}", result);

    // Allow the actor to process the queued messages
    tokio::time::sleep(std::time::Duration::from_millis(150)).await; // Wait for SlowMsg (100ms) + UpdateCounterMsg

    actor_ref.stop().await.expect("Failed to stop actor");
    let result = handle.await.expect("Actor task failed");
    let (actor, _) = result.into();
    if let Some(actor) = actor {
        // Verify that only UpdateCounterMsg(1) was processed.
        assert_eq!(*actor.counter.lock().await, 1, "Counter should be 1 after SlowMsg and UpdateCounterMsg(1)");
    } else {
        panic!("Actor state should not be None");
    }
}

// === Generic Actor Test ===
mod generic_actor {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Generic actor that can hold any type T
    #[derive(Debug)]
    pub(crate) struct GenericActor<T: Send + 'static> {
        value: Arc<Mutex<T>>,
    }

    impl<T: Send + 'static> GenericActor<T> {
        pub(crate) fn new(value: T) -> Self {
            Self { value: Arc::new(Mutex::new(value)) }
        }
    }

    impl<T: Send + 'static> Actor for GenericActor<T> {
        type Args = T;
        type Error = anyhow::Error;

        async fn on_start(args: Self::Args, _actor_ref: &ActorRef) -> Result<Self, Self::Error> {
            Ok(GenericActor::new(args))
        }
    }

    #[derive(Debug)]
    pub(crate) struct GetValueMsg;

    impl<T: Send + Clone + 'static> Message<GetValueMsg> for GenericActor<T> {
        type Reply = T;
        async fn handle(&mut self, _msg: GetValueMsg, _: &ActorRef) -> Self::Reply {
            self.value.lock().await.clone()
        }
    }

}

// The impl_message_handler! macro must be invoked for concrete types
impl_message_handler!(generic_actor::GenericActor<u32>, [generic_actor::GetValueMsg]);

#[tokio::test]
async fn test_generic_actor() {
    let (actor_ref, handle) = spawn::<generic_actor::GenericActor<u32>>(123u32);
    let expected_type_name = std::any::type_name::<generic_actor::GenericActor<u32>>();
    let identity = actor_ref.identity();
    assert_eq!(identity.type_name, expected_type_name);
    assert_eq!(identity.name(), format!("{}#{}", expected_type_name, actor_ref.identity().id));
    let reply: u32 = actor_ref.ask(generic_actor::GetValueMsg).await.expect("ask failed for GetValueMsg");
    assert_eq!(reply, 123);
    actor_ref.stop().await.expect("Failed to stop generic actor");
    handle.await.expect("Generic actor task failed");
}

impl_message_handler!(generic_actor::GenericActor<u64>, [generic_actor::GetValueMsg]);

#[tokio::test]
async fn test_generic_actor_u64() {
    let (actor_ref, handle) = spawn::<generic_actor::GenericActor<u64>>(9223372036854775808u64); // 2^63, large number to test u64 specifically
    let reply: u64 = actor_ref.ask(generic_actor::GetValueMsg).await.expect("ask failed for GetValueMsg");
    assert_eq!(reply, 9223372036854775808u64);
    actor_ref.stop().await.expect("Failed to stop generic actor");
    handle.await.expect("Generic actor task failed");
}
