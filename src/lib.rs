use std::{
    any::Any,
    fmt::Debug,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering}
};

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use log::{info, error, warn};

#[macro_export]
macro_rules! impl_message_handler {
    ($actor_type:ty, [$($msg_type:ty),+ $(,)?]) => {
        impl $crate::MessageHandler for $actor_type {
            async fn handle(
                &mut self,
                msg_any: Box<dyn std::any::Any + Send>,
            ) -> anyhow::Result<Box<dyn std::any::Any + Send>> {
                $(
                    if msg_any.is::<$msg_type>() {
                        match msg_any.downcast::<$msg_type>() {
                            Ok(msg) => {
                                let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *msg).await;
                                return Ok(Box::new(reply) as Box<dyn std::any::Any + Send>);
                            }
                            Err(_) => {
                                return Err(anyhow::anyhow!(concat!("Internal error: Downcast to ", stringify!($msg_type), " failed after type check.")));
                            }
                        }
                    }
                )+
                Err(anyhow::anyhow!(concat!(stringify!($actor_type), ": ErasedMessageHandler received unknown message type.")))
            }
        }
    };
}

// Enum to represent messages within the actor system, including control messages.
pub enum MailboxMessage {
    Envelope {
        payload: Box<dyn Any + Send>,
        reply_channel: oneshot::Sender<Result<Box<dyn Any + Send>>>,
    },
    Terminate, // Signal to terminate the actor
    StopGracefully, // Signal to stop after processing existing messages
}

// Type alias for the sender part of the actor's mailbox channel.
type MailboxSender = mpsc::Sender<MailboxMessage>;

// Counter for generating unique actor IDs.
static ACTOR_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Debug)]
pub struct ActorRef {
    id: usize,
    sender: MailboxSender,
}

impl ActorRef {
    // Creates a new ActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    fn new_internal(id: usize, sender: MailboxSender) -> Self {
        ActorRef {
            id,
            sender,
        }
    }

    pub const fn id(&self) -> usize {
        self.id
    }

    // Sends a message to the actor without awaiting a reply (fire and forget).
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
    {
        // We still need to create a oneshot channel because MailboxMessage expects it.
        // However, the sender part (reply_tx) will be dropped immediately by this method,
        // and the actor's run loop should handle the case where sending a reply fails.
        let (reply_tx, _reply_rx) = oneshot::channel::<Result<Box<dyn Any + Send>>>();
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: reply_tx,
        };

        if self.sender.send(envelope).await.is_err() {
            Err(anyhow::anyhow!(
                "Failed to send message to actor {}: mailbox channel closed",
                self.id
            ))
        } else {
            Ok(())
        }
    }

    // Sends a message to the actor and awaits a reply.
    // M is the message type, R is the expected reply type.
    pub async fn ask<M, R>(&self, msg: M) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: reply_tx,
        };

        // Use the ActorRef's own sender
        if self.sender.send(envelope).await.is_err() {
            return Err(anyhow::anyhow!(
                "Failed to send message to actor {}: mailbox channel closed",
                self.id
            ));
        }

        match reply_rx.await {
            Ok(Ok(reply_any)) => reply_any
                .downcast::<R>()
                .map(|r| *r)
                .map_err(|_| anyhow::anyhow!("Failed to downcast reply to the expected type")),
            Ok(Err(e)) => Err(e), // Error from the actor's handler
            Err(_) => Err(anyhow::anyhow!(
                "Failed to receive reply from actor {}: reply channel closed",
                self.id
            )),
        }
    }

    // New method to send a termination signal to the actor.
    pub async fn kill(&self) -> Result<()> {
        info!("Sending Terminate message to actor {}", self.id);
        match self.sender.send(MailboxMessage::Terminate).await {
            Ok(_) => Ok(()),
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send Terminate to actor {}: mailbox closed. Actor might already be stopped.", self.id);
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }

    // New method to send a graceful stop signal to the actor.
    // The actor will process all messages currently in its mailbox and then stop.
    // New messages sent after this call might be ignored or fail.
    pub async fn stop(&self) -> Result<()> {
        info!("Sending StopGracefully message to actor {}", self.id);
        match self.sender.send(MailboxMessage::StopGracefully).await {
            Ok(_) => Ok(()),
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.id);
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub enum ActorStopReason {
    /// Actor stopped normally.
    Normal,
    /// Actor was killed.
    Killed,
    /// Actor panicked or a lifecycle hook (on_start, on_stop) failed.
    Error(anyhow::Error),
}

/// Trait for actors
pub trait Actor: Send + 'static {
    type Error: Send + Debug + 'static; // Error type for the actor

    fn on_start(&mut self, _actor_ref: ActorRef) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }
}

// Trait for messages that an actor can handle.
// The actor struct (e.g., MyActor) implements this for each message type it handles.
pub trait Message<T: Send + 'static>: Actor {
    type Reply: Send + 'static; // Reply type must be Send and 'static for oneshot channel

    // Handles the message. This is an async method.
    fn handle(&mut self, msg: T) -> impl Future<Output = Self::Reply> + Send;
}

// New trait for type-erased message handling within the Runtime.
// Actors that can be managed by the system must implement this.
pub trait MessageHandler: Send + Sync + 'static {
    fn handle(
        &mut self,
        msg_any: Box<dyn Any + Send>,
    ) -> impl Future<Output = Result<Box<dyn Any + Send>>> + Send;
}

// Manages the lifecycle and message loop for a single actor instance.
pub struct Runtime<T: Actor + MessageHandler> {
    actor_ref: ActorRef,
    actor: T, // Actor instance is now owned by Runtime
    receiver: mpsc::Receiver<MailboxMessage>, // Receives messages for this actor
}

impl<T: Actor + MessageHandler> Runtime<T> {
    pub fn new(
        actor: T, // Actor is moved into Runtime
        actor_ref: ActorRef,
        receiver: mpsc::Receiver<MailboxMessage>,
    ) -> Self {
        Runtime {
            actor_ref,
            actor,
            receiver,
        }
    }

    // This method encapsulates the actor's entire lifecycle within its spawned task.
    // It handles on_start, message processing, and on_stop, then returns the actor
    // instance and the reason for stopping. Consumes self to return the actor.
    async fn run_actor_lifecycle(mut self) -> (T, ActorStopReason) {
        let actor_id = self.actor_ref.id();

        // Call on_start
        if let Err(e) = self.actor.on_start(self.actor_ref.clone()).await {
            error!("Actor {} on_start error: {:?}", actor_id, e);
            let err_msg = format!("on_start failed for actor {}: {:?}", actor_id, e);
            let mut reason = ActorStopReason::Error(anyhow::Error::msg(err_msg));
            if let Err(e_on_stop) = self.actor.on_stop(self.actor_ref.clone(), &reason).await {
                error!("Actor {} on_stop error: {:?}", actor_id, e_on_stop);

                if let ActorStopReason::Error(original_start_err) = reason {
                    let new_err = original_start_err.context("on_stop failed after on_start error");
                    reason = ActorStopReason::Error(new_err); // Corrected this line
                }
            }
            info!("Actor {} task finishing prematurely due to on_start error.", actor_id);
            return (self.actor, reason); // Return actor instance and reason
        }

        info!("Runtime for actor {} is running.", actor_id);

        let mut gracefully_stopping = false;
        let mut final_reason = ActorStopReason::Normal; // Default reason

        // Message processing loop
        while let Some(actor_message) = self.receiver.recv().await {
            match actor_message {
                MailboxMessage::Envelope { payload, reply_channel } => {
                    if gracefully_stopping {
                        warn!("Actor {} is stopping gracefully, ignoring Envelope message.", actor_id);
                        continue;
                    }
                    let result = self.actor.handle(payload).await;
                    if reply_channel.send(result).is_err() {
                        warn!(
                            "Failed to send reply: reply channel closed for actor {}",
                            actor_id
                        );
                    }
                }
                MailboxMessage::Terminate => {
                    info!("Actor {} received Terminate message. Initiating immediate shutdown.", actor_id);
                    final_reason = ActorStopReason::Killed;
                    break; // Exit the loop to proceed to on_stop
                }
                MailboxMessage::StopGracefully => {
                    info!("Actor {} received StopGracefully message. Will process remaining messages then shut down. New messages will be ignored.", actor_id);
                    gracefully_stopping = true;
                    self.receiver.close(); // Close the receiver. recv() will drain existing messages then return None.
                    final_reason = ActorStopReason::Normal; // Will stop normally after queue drains
                }
            }
        }

        info!("Runtime for actor {} is shutting down.", actor_id);

        // Call on_stop
        if let Err(e) = self.actor.on_stop(self.actor_ref.clone(), &final_reason).await {
            error!("Actor {} on_stop error: {:?}", actor_id, e);
            if matches!(final_reason, ActorStopReason::Normal) {
                let err_msg = format!("on_stop failed for actor {}: {:?}", actor_id, e);
                final_reason = ActorStopReason::Error(anyhow::Error::msg(err_msg));
            }
        }

        info!("Actor {} task finished with reason: {:?}.", actor_id, final_reason);
        (self.actor, final_reason) // Return actor instance and final reason
    }
}

// Spawns a new actor and returns an ActorRef to it, and a JoinHandle to get the actor and stop reason.
pub fn spawn<T: Actor + MessageHandler + 'static>(
    actor: T, // Actor instance is taken by value
) -> (ActorRef, tokio::task::JoinHandle<(T, ActorStopReason)>) { // Updated return type
    let actor_id = ACTOR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel::<MailboxMessage>(32);

    let actor_ref = ActorRef::new_internal(actor_id, tx);

    let runtime = Runtime::new(actor, actor_ref.clone(), rx);
    let id_for_log = runtime.actor_ref.id(); // Capture ID for logging before move

    let handle = tokio::spawn(async move { // runtime is moved into the spawned task
        info!("Spawning task for actor {}.", id_for_log);
        // run_actor_lifecycle consumes runtime and returns (T, ActorStopReason)
        // This tuple becomes the result of the JoinHandle on success.
        runtime.run_actor_lifecycle().await
    });

    (actor_ref, handle) // Return ActorRef and JoinHandle
}

// ---------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;
    use std::sync::Arc;
    use log::debug; // Ensure 'log' crate is a dev-dependency or available

    // Test Actor Setup
    struct TestActor {
        id: usize,
        counter: Arc<Mutex<i32>>,
        last_processed_message_type: Arc<Mutex<Option<String>>>,
        on_start_called: Arc<Mutex<bool>>,
        on_stop_called: Arc<Mutex<bool>>,
    }

    impl TestActor {
        fn new(
            counter: Arc<Mutex<i32>>,
            last_processed_message_type: Arc<Mutex<Option<String>>>,
            on_start_called: Arc<Mutex<bool>>,
            on_stop_called: Arc<Mutex<bool>>,
        ) -> Self {
            TestActor {
                id: 0, // Will be set by on_start or if read from ActorRef
                counter,
                last_processed_message_type,
                on_start_called,
                on_stop_called,
            }
        }
    }

    impl Actor for TestActor {
        type Error = anyhow::Error;

        async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
            self.id = actor_ref.id();
            let mut called = self.on_start_called.lock().await;
            *called = true;
            debug!("TestActor (id: {}) started.", self.id);
            Ok(())
        }

        async fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
            let mut called = self.on_stop_called.lock().await;
            *called = true;
            debug!("TestActor (id: {}) stopped. Final count: {}", self.id, *self.counter.lock().await);
            Ok(())
        }
    }

    // Messages
    #[derive(Debug)] // Added for logging if needed
    struct PingMsg(String);
    #[derive(Debug)]
    struct UpdateCounterMsg(i32);
    #[derive(Debug)]
    struct GetCounterMsg;

    impl Message<PingMsg> for TestActor {
        type Reply = String;
        async fn handle(&mut self, msg: PingMsg) -> Self::Reply {
            let mut lpmt = self.last_processed_message_type.lock().await;
            *lpmt = Some("PingMsg".to_string());
            format!("pong: {}", msg.0)
        }
    }

    impl Message<UpdateCounterMsg> for TestActor {
        type Reply = (); // tell type messages often use this.
        async fn handle(&mut self, msg: UpdateCounterMsg) -> Self::Reply {
            let mut counter = self.counter.lock().await;
            *counter += msg.0;
            let mut lpmt = self.last_processed_message_type.lock().await;
            *lpmt = Some("UpdateCounterMsg".to_string());
        }
    }

    impl Message<GetCounterMsg> for TestActor {
        type Reply = i32;
        async fn handle(&mut self, _msg: GetCounterMsg) -> Self::Reply {
            let mut lpmt = self.last_processed_message_type.lock().await;
            *lpmt = Some("GetCounterMsg".to_string());
            *self.counter.lock().await
        }
    }

    impl_message_handler!(TestActor, [PingMsg, UpdateCounterMsg, GetCounterMsg]);

    async fn setup_actor() -> (
        ActorRef,
        tokio::task::JoinHandle<(TestActor, ActorStopReason)>,
        Arc<Mutex<i32>>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<bool>>,
        Arc<Mutex<bool>>,
    ) {
        // It's good practice to initialize logger for tests, e.g. using a static Once.
        // For simplicity here, we assume it's handled or not strictly needed for output.
        // let _ = env_logger::builder().is_test(true).try_init();

        let counter = Arc::new(Mutex::new(0));
        let last_processed_message_type = Arc::new(Mutex::new(None::<String>));
        let on_start_called = Arc::new(Mutex::new(false));
        let on_stop_called = Arc::new(Mutex::new(false));

        let actor_instance = TestActor::new(
            counter.clone(),
            last_processed_message_type.clone(),
            on_start_called.clone(),
            on_stop_called.clone(),
        );
        let (actor_ref, handle) = spawn(actor_instance);
        // Give a moment for on_start to potentially run
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (
            actor_ref,
            handle,
            counter,
            last_processed_message_type,
            on_start_called,
            on_stop_called,
        )
    }

    #[tokio::test]
    async fn test_spawn_and_actor_ref_id() {
        let (actor_ref, handle, _counter, _lpmt, on_start_called, on_stop_called) =
            setup_actor().await;
        assert!(*on_start_called.lock().await, "on_start should be called");
        assert_ne!(actor_ref.id(), 0, "Actor ID should be non-zero");

        actor_ref.stop().await.expect("Failed to stop actor");
        let (actor_state, reason) = handle.await.expect("Actor task failed");
        assert!(matches!(reason, ActorStopReason::Normal));
        assert!(*on_stop_called.lock().await, "on_stop should be called");
        assert_eq!(actor_state.id, actor_ref.id());
    }

    #[tokio::test]
    async fn test_actor_ref_ask() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) = setup_actor().await;

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
        handle.await.expect("Actor task failed");
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_tell() {
        let (actor_ref, handle, counter, last_processed, _on_start, on_stop_called) =
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
        handle.await.expect("Actor task failed");
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_stop() {
        let (actor_ref, handle, _counter, _lpmt, on_start_called, on_stop_called) =
            setup_actor().await;
        assert!(*on_start_called.lock().await);

        actor_ref.tell(UpdateCounterMsg(100)).await.unwrap();

        let ask_future = actor_ref.ask::<_, i32>(GetCounterMsg); // Send before stop
        let count_val = ask_future.await.expect("ask sent before stop should succeed");
        assert_eq!(count_val, 100);

        actor_ref.stop().await.expect("stop command failed");

        let (actor_state, reason) = handle.await.expect("Actor task failed");
        assert!(matches!(reason, ActorStopReason::Normal), "Reason: {:?}", reason);
        assert!(*on_stop_called.lock().await, "on_stop was not called");
        assert_eq!(*actor_state.counter.lock().await, 100);

        // Interactions after stop
        assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to stopped actor should fail");
        assert!(actor_ref.ask::<_, String>(PingMsg("test".to_string())).await.is_err(), "Ask to stopped actor should fail");
    }

    #[tokio::test]
    async fn test_actor_ref_kill() {
        let (actor_ref, handle, _counter, _lpmt, on_start_called, on_stop_called) =
            setup_actor().await;
        assert!(*on_start_called.lock().await);

        // Send a message that might be in flight
        actor_ref.tell(UpdateCounterMsg(10)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await; // Small delay

        actor_ref.kill().await.expect("kill command failed");

        let (_actor_state, reason) = handle.await.expect("Actor task failed");
        assert!(matches!(reason, ActorStopReason::Killed), "Reason: {:?}", reason);
        // Current implementation calls on_stop even for Killed
        assert!(*on_stop_called.lock().await, "on_stop should be called on kill");

        // Interactions after kill
        assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to killed actor should fail");
        assert!(actor_ref.ask::<_, String>(PingMsg("test".to_string())).await.is_err(), "Ask to killed actor should fail");
    }

    #[tokio::test]
    async fn test_ask_wrong_reply_type() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) = setup_actor().await;

        let result = actor_ref.ask::<PingMsg, i32>(PingMsg("hello".to_string())).await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Failed to downcast reply"));
        }

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_unhandled_message_type() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) = setup_actor().await;

        struct UnhandledMsg; // Not in impl_message_handler! for TestActor

        let result = actor_ref.ask::<UnhandledMsg, ()>(UnhandledMsg).await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("ErasedMessageHandler received unknown message type"));
        }

        actor_ref.stop().await.unwrap();
        handle.await.unwrap();
        assert!(*on_stop_called.lock().await);
    }

    // Test actor lifecycle errors
    struct LifecycleErrorActor {
        id: usize,
        fail_on_start: bool,
        fail_on_stop: bool,
        on_start_attempted: Arc<Mutex<bool>>,
        on_stop_attempted: Arc<Mutex<bool>>,
    }
    impl Actor for LifecycleErrorActor {
        type Error = anyhow::Error;
        async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
            self.id = actor_ref.id();
            *self.on_start_attempted.lock().await = true;
            if self.fail_on_start { Err(anyhow::anyhow!("simulated on_start failure")) } else { Ok(()) }
        }
        async fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
            *self.on_stop_attempted.lock().await = true;
            if self.fail_on_stop { Err(anyhow::anyhow!("simulated on_stop failure")) } else { Ok(()) }
        }
    }
    struct NoOpMsg; // Dummy message for LifecycleErrorActor
    impl Message<NoOpMsg> for LifecycleErrorActor {
        type Reply = ();
        async fn handle(&mut self, _msg: NoOpMsg) -> Self::Reply {}
    }
    impl_message_handler!(LifecycleErrorActor, [NoOpMsg]);

    #[tokio::test]
    async fn test_actor_fail_on_start() {
        let on_start_attempted = Arc::new(Mutex::new(false));
        let on_stop_attempted = Arc::new(Mutex::new(false));
        let actor = LifecycleErrorActor {
            id: 0,
            fail_on_start: true,
            fail_on_stop: false,
            on_start_attempted: on_start_attempted.clone(),
            on_stop_attempted: on_stop_attempted.clone(),
        };
        let (_actor_ref, handle) = spawn(actor);

        match handle.await {
            Ok((returned_actor, reason)) => {
                assert!(matches!(reason, ActorStopReason::Error(_)), "Expected Panicked, got {:?}", reason);
                if let ActorStopReason::Error(e) = reason {
                    assert!(e.to_string().contains("on_start failed"));
                }
                assert!(*returned_actor.on_start_attempted.lock().await);
                assert!(!*returned_actor.on_stop_attempted.lock().await); // on_stop should not be called if on_start fails
            }
            Err(e) => panic!("Expected Ok with Panicked reason, got JoinError: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_actor_fail_on_stop() {
        let on_start_attempted = Arc::new(Mutex::new(false));
        let on_stop_attempted = Arc::new(Mutex::new(false));
        let actor = LifecycleErrorActor {
            id: 0,
            fail_on_start: false,
            fail_on_stop: true,
            on_start_attempted: on_start_attempted.clone(),
            on_stop_attempted: on_stop_attempted.clone(),
        };
        let (actor_ref, handle) = spawn(actor);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await; // Ensure on_start runs
        assert!(*on_start_attempted.lock().await);

        actor_ref.stop().await.expect("Stop command should succeed");

        match handle.await {
            Ok((returned_actor, reason)) => {
                assert!(matches!(reason, ActorStopReason::Error(_)), "Expected Panicked, got {:?}", reason);
                if let ActorStopReason::Error(e) = reason {
                    assert!(e.to_string().contains("on_stop failed"));
                }
                assert!(*returned_actor.on_start_attempted.lock().await);
                assert!(*returned_actor.on_stop_attempted.lock().await);
            }
            Err(e) => panic!("Expected Ok with Panicked reason, got JoinError: {:?}", e),
        }
    }

    // Test for panic within a message handler
    struct PanicActor { on_stop_called: Arc<Mutex<bool>> }
    struct PanicMsg;

    impl Actor for PanicActor {
        type Error = anyhow::Error;
        async fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> Result<(), Self::Error> { *self.on_stop_called.lock().await = true; Ok(()) }
    }
    impl Message<PanicMsg> for PanicActor {
        type Reply = ();
        async fn handle(&mut self, _msg: PanicMsg) -> Self::Reply {
            panic!("Simulated panic in message handler");
        }
    }
    impl_message_handler!(PanicActor, [PanicMsg]);

    #[tokio::test]
    async fn test_actor_panic_in_message_handler() {
        let on_stop_called_arc = Arc::new(Mutex::new(false));
        let actor = PanicActor { on_stop_called: on_stop_called_arc.clone() };
        let (actor_ref, handle) = spawn(actor);

        // Sending a message that causes a panic in the handler.
        // The ask call itself will likely fail because the actor task panics and closes the reply channel.
        let ask_result = actor_ref.ask::<PanicMsg, ()>(PanicMsg).await;
        assert!(ask_result.is_err(), "Ask should fail when handler panics");
        if let Err(e) = ask_result {
            // Error could be "reply channel closed" or similar, as the actor task terminates.
            debug!("Ask error after handler panic: {}", e);
            assert!(e.to_string().contains("reply channel closed") || e.to_string().contains("mailbox channel closed"));
        }

        // The JoinHandle should return Err because the underlying tokio task panicked.
        match handle.await {
            Ok((_actor_state, reason)) => {
                // This path should ideally not be taken if the task truly panics.
                // However, if the framework were to catch panics and convert them to ActorStopReason::Panicked,
                // this would be the case. Current code does not do this for handler panics.
                panic!("Expected JoinHandle to return Err due to task panic, but got Ok with reason: {:?}", reason);
            }
            Err(join_error) => {
                assert!(join_error.is_panic(), "Expected a panic JoinError from actor task");
            }
        }
        // Check if on_stop was called. If the task panics, on_stop in run_actor_lifecycle might not be reached.
        // The current run_actor_lifecycle does not have a catch_unwind around the message handling loop.
        // So, a panic in `self.actor.handle()` will propagate and terminate the task before `on_stop` is called by the loop.
        assert!(!*on_stop_called_arc.lock().await, "on_stop should not be called if handler panics and task terminates abruptly");
    }
}

