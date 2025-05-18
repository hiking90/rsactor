// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # rsactor: A Rust Actor Framework
//!
//! `rsactor` provides a simple and lightweight actor framework for building concurrent
//! applications in Rust. It is built on top of `tokio` for asynchronous message
//! passing and task management.
//!
//! ## Features
//!
//! - **Asynchronous Actors**: Actors run in their own asynchronous tasks.
//! - **Message Passing**: Actors communicate by sending and receiving messages.
//!   - `tell`: Send a message without waiting for a reply (fire-and-forget).
//!   - `ask`: Send a message and await a reply.
//!   - `tell_blocking`: Blocking version of `tell` for use in `tokio::task::spawn_blocking` tasks.
//!   - `ask_blocking`: Blocking version of `ask` for use in `tokio::task::spawn_blocking` tasks.
//! - **Actor Lifecycle**: Actors have `on_start` and `on_stop` lifecycle hooks.
//! - **Graceful Shutdown & Kill**: Actors can be stopped gracefully or killed immediately.
//! - **Typed Messages**: Messages are strongly typed, and replies are also typed.
//! - **Macro for Message Handling**: The `impl_message_handler!` macro simplifies
//!   handling multiple message types.
//!
//! ## Core Concepts
//!
//! - **`Actor`**: Trait defining actor behavior and lifecycle hooks.
//! - **`Message<M>`**: Trait for handling a message type `M` and defining its reply type.
//! - **`ActorRef`**: Handle for sending messages to an actor.
//! - **`spawn`**: Function to create and start an actor, returning an `ActorRef` and a `JoinHandle`.
//! - **`MessageHandler`**: Trait for type-erased message handling. This is typically implemented automatically by the `impl_message_handler!` macro.
//! - **`MailboxMessage(Internal)`**: Enum for messages in an actor's mailbox (user messages and control signals).
//! - **`Runtime(Internal)`**: Manages an actor's internal lifecycle and message loop.
//!
//! ## Getting Started
//!
//! Define an actor struct, implement `Actor` and `Message<M>` for each message type.
//! Use `impl_message_handler!` to wire up message handling.
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
//! use anyhow::Result;
//!
//! // 1. Define your actor struct
//! struct MyActor {
//!     data: String,
//! }
//!
//! impl MyActor {
//!     fn new(data: &str) -> Self {
//!         MyActor { data: data.to_string() }
//!     }
//! }
//!
//! // 2. Implement the Actor trait
//! impl Actor for MyActor {
//!     type Error = anyhow::Error; // Define an error type
//!
//!     async fn on_start(&mut self, _actor_ref: ActorRef) -> Result<(), Self::Error> {
//!         println!("MyActor (data: '{}') started!", self.data);
//!         Ok(())
//!     }
//!
//!     async fn on_stop(&mut self, _actor_ref: ActorRef, _reason: &rsactor::ActorStopReason) -> Result<(), Self::Error> {
//!         println!("MyActor (data: '{}') stopped!", self.data);
//!         Ok(())
//!     }
//! }
//!
//! // 3. Define your message types
//! struct GetData; // A message to get the actor's data
//! struct UpdateData(String); // A message to update the actor's data
//!
//! // 4. Implement Message<M> for each message type
//! impl Message<GetData> for MyActor {
//!     type Reply = String; // This message will return a String
//!
//!     async fn handle(&mut self, _msg: GetData, _actor_ref: &ActorRef) -> Self::Reply {
//!         self.data.clone()
//!     }
//! }
//!
//! impl Message<UpdateData> for MyActor {
//!     type Reply = (); // This message does not return a value
//!
//!     async fn handle(&mut self, msg: UpdateData, _actor_ref: &ActorRef) -> Self::Reply {
//!         self.data = msg.0;
//!         println!("MyActor data updated!");
//!     }
//! }
//!
//! // 5. Use the macro to implement the MessageHandler trait
//! impl_message_handler!(MyActor, [GetData, UpdateData]);
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let my_actor = MyActor::new("initial data");
//!     let (actor_ref, join_handle) = spawn(my_actor);
//!
//!     // Send an "ask" message and wait for a reply
//!     let current_data: String = actor_ref.ask(GetData).await?;
//!     println!("Received data: {}", current_data);
//!
//!     // Send a "tell" message (fire-and-forget)
//!     actor_ref.tell(UpdateData("new data".to_string())).await?;
//!
//!     // Verify the update
//!     let updated_data: String = actor_ref.ask(GetData).await?;
//!     println!("Updated data: {}", updated_data);
//!
//!     // Stop the actor gracefully
//!     actor_ref.stop().await?;
//!
//!     // Wait for the actor to terminate
//!     let (_actor_instance, stop_reason) = join_handle.await?;
//!     println!("Actor stopped with reason: {:?}", stop_reason);
//!
//!     Ok(())
//! }
//! ```
//!
//! This crate-level documentation provides an overview of `rsactor`.
//! For more details on specific components, please refer to their individual
//! documentation.

use core::error;
use std::{
    any::Any,
    fmt::Debug,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

use anyhow::Result;
use tokio::sync::{mpsc, oneshot}; // Mutex might be from tests, ensure it's not needed here. std::sync::Mutex if for global. No, OnceLock is fine.
use log::{info, error, warn, debug, trace};

/// Implements the `MessageHandler` trait for a given actor type.
///
/// This macro simplifies the process of handling multiple message types within an actor.
/// It generates the necessary boilerplate code to downcast a `Box<dyn Any + Send>`
/// message to its concrete type and then calls the appropriate `Message::handle`
/// implementation on the actor.
///
/// # Usage
///
/// ```rust,ignore
/// struct MyActor;
///
/// impl Actor for MyActor { /* ... */ }
///
/// struct Msg1;
/// struct Msg2;
///
/// impl Message<Msg1> for MyActor {
///     type Reply = ();
///     async fn handle(&mut self, msg: Msg1) -> Self::Reply { /* ... */ }
/// }
///
/// impl Message<Msg2> for MyActor {
///     type Reply = String;
///     async fn handle(&mut self, msg: Msg2) -> Self::Reply { /* ... */ "response".to_string() }
/// }
///
/// // This will implement `MessageHandler` for `MyActor`, allowing it to handle `Msg1` and `Msg2`.
/// impl_message_handler!(MyActor, [Msg1, Msg2]);
/// ```
///
/// # Arguments
///
/// * `$actor_type`: The type of the actor for which to implement `MessageHandler`.
/// * `[$($msg_type:ty),+]`: A list of message types that the actor can handle.
///
/// # Internals
/// This macro facilitates dynamic message dispatch by downcasting `Box<dyn std::any::Any + Send>`
/// message payloads to their concrete types at runtime.
#[macro_export]
macro_rules! impl_message_handler {
    ($actor_type:ty, [$($msg_type:ty),* $(,)?]) => {
        impl $crate::MessageHandler for $actor_type {
            async fn handle(
                &mut self,
                _msg_any: Box<dyn std::any::Any + Send>,
                _actor_ref: &$crate::ActorRef,
            ) -> anyhow::Result<Box<dyn std::any::Any + Send>> {
                $(
                    if _msg_any.is::<$msg_type>() {
                        match _msg_any.downcast::<$msg_type>() {
                            Ok(msg) => {
                                let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *msg, _actor_ref).await;
                                return Ok(Box::new(reply) as Box<dyn std::any::Any + Send>);
                            }
                            Err(_) => {
                                return Err(anyhow::anyhow!(concat!("Internal error: Downcast to ", stringify!($msg_type), " failed after type check.")));
                            }
                        }
                    }
                )*
                Err(anyhow::anyhow!(concat!(stringify!($actor_type), ": ErasedMessageHandler received unknown message type.")))
            }
        }
    };
}

/// Represents messages that can be sent to an actor's mailbox.
///
/// This enum includes both user-defined messages (wrapped in `Envelope`)
/// and control messages like `Terminate` and `StopGracefully`.
#[derive(Debug)] // Added Debug derive
enum MailboxMessage {
    /// A user-defined message to be processed by the actor.
    Envelope {
        /// The message payload.
        payload: Box<dyn Any + Send>,
        /// A channel to send the reply back to the caller. Optional for 'tell' operations.
        reply_channel: Option<oneshot::Sender<Result<Box<dyn Any + Send>>>>,
    },
    // Terminate is removed from here
    /// A signal for the actor to stop gracefully after processing existing messages in its mailbox.
    StopGracefully,
}

/// Represents control signals that can be sent to an actor.
#[derive(Debug)]
enum ControlSignal {
    /// A signal for the actor to terminate immediately.
    Terminate,
}

// Type alias for the sender part of the actor's mailbox channel.
type MailboxSender = mpsc::Sender<MailboxMessage>;

// Counter for generating unique actor IDs.
static ACTOR_COUNTER: AtomicUsize = AtomicUsize::new(1);

// Global configuration for the default mailbox capacity.
static CONFIGURED_DEFAULT_MAILBOX_CAPACITY: OnceLock<usize> = OnceLock::new();

/// The default mailbox capacity for actors.
pub const DEFAULT_MAILBOX_CAPACITY: usize = 32;

/// Sets the global default buffer size for actor mailboxes.
///
/// This function can only be called successfully once. Subsequent calls
/// will return an error. This configured value is used by the `spawn` function
/// if no specific capacity is provided to `spawn_with_mailbox_capacity`.
pub fn set_default_mailbox_capacity(size: usize) -> Result<(), String> {
    if size == 0 {
        return Err("Global default mailbox capacity must be greater than 0".to_string());
    }
    CONFIGURED_DEFAULT_MAILBOX_CAPACITY.set(size).map_err(|_| "Global default mailbox capacity has already been set".to_string())
}

/// A reference to an actor, allowing messages to be sent to it.
///
/// `ActorRef` provides a way to interact with actors without having direct access
/// to the actor instance itself. It holds a sender channel to the actor's mailbox.
///
/// ## Message Passing Methods
///
/// - **Asynchronous Methods**:
///   - [`ask`](ActorRef::ask): Send a message and await a reply.
///   - [`tell`](ActorRef::tell): Send a message without waiting for a reply.
///
/// - **Blocking Methods for Tokio Blocking Contexts**:
///   - [`ask_blocking`](ActorRef::ask_blocking): Send a message and block until a reply is received.
///   - [`tell_blocking`](ActorRef::tell_blocking): Send a message and block until it is sent.
///
///   These methods are for use within `tokio::task::spawn_blocking` contexts.
///
/// - **Control Methods**:
///   - [`stop`](ActorRef::stop): Gracefully stop the actor.
///   - [`kill`](ActorRef::kill): Immediately terminate the actor.
#[derive(Clone, Debug)]
pub struct ActorRef {
    id: usize,
    sender: MailboxSender,
    terminate_sender: mpsc::Sender<ControlSignal>, // Changed type
}

impl ActorRef {
    // Creates a new ActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    fn new_internal(id: usize, sender: MailboxSender, terminate_sender: mpsc::Sender<ControlSignal>) -> Self { // Changed type
        ActorRef {
            id,
            sender,
            terminate_sender,
        }
    }

    /// Returns the unique ID of the actor.
    pub const fn id(&self) -> usize {
        self.id
    }

    /// Returns the sender channel for the actor's mailbox.
    pub fn is_alive(&self) -> bool {
        // Check if the sender channel is open
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing.
    /// This method returns immediately.
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
    {
        // For 'tell', no reply is expected, so no need for a reply_channel.
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: None, // reply_channel is None for tell
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

    /// Sends a message to the actor and awaits a reply.
    ///
    /// The message is sent to the actor\\'s mailbox, and this method will wait for
    /// the actor to process the message and send a reply.
    pub async fn ask<M, R>(&self, msg: M) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
        };

        if self.sender.send(envelope).await.is_err() {
            return Err(anyhow::anyhow!(
                "Failed to send message to actor {}: mailbox closed",
                self.id
            ));
        }

        match reply_rx.await {
            Ok(Ok(reply_any)) => { // recv was Ok, actor reply was Ok
                match reply_any.downcast::<R>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(anyhow::anyhow!(
                        "Failed to downcast reply from actor {} to expected type",
                        self.id
                    )),
                }
            }
            Ok(Err(e)) => Err(e), // recv was Ok, actor reply was Err
            Err(_recv_err) => Err(anyhow::anyhow!( // recv itself failed
                "Failed to receive reply from actor {}: reply channel closed unexpectedly",
                self.id
            )),
        }
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The `on_stop` lifecycle hook will be called with `ActorStopReason::Killed`.
    pub fn kill(&self) -> Result<()> {
        info!("Attempting to send Terminate message to actor {} via dedicated channel using try_send", self.id);
        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) { // Changed to ControlSignal::Terminate
            Ok(_) => {
                // Successfully sent the terminate message.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The channel is full. Since it has a capacity of 1,
                // this means a Terminate message is already in the queue.
                warn!("Failed to send Terminate to actor {}: terminate mailbox is full. Actor is likely already being terminated.", self.id);
                // Considered Ok as the desired state (stopping/killed) is effectively met.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed, which implies the actor is already stopped or has finished processing.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.id);
                // Considered Ok as the desired state (stopped) is met.
                Ok(())
            }
        }
    }

    /// Sends a graceful stop signal to the actor.
    ///
    /// The actor will process all messages currently in its mailbox and then stop.
    /// New messages sent after this call might be ignored or fail.
    /// The `on_stop` lifecycle hook will be called with `ActorStopReason::Normal`
    /// if no errors occur during shutdown.
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

    // =========================================================================
    // Blocking functions for Tokio blocking tasks
    // =========================================================================

    /// # Blocking Functions for Tokio Tasks
    ///
    /// These functions are intended for scenarios where CPU-intensive or other blocking operations
    /// are performed within a `tokio::task::spawn_blocking` task, and communication
    /// with actors is necessary. They allow such tasks to interact with the actor system
    /// synchronously, without using `async/await` directly within the blocking task.
    ///
    /// ## Example
    ///
    /// The following example illustrates using `tell_blocking`. A similar approach applies to `ask_blocking`.
    ///
    /// ```rust,no_run
    /// # use rsactor::{Actor, ActorRef};
    /// # use std::time::Duration;
    /// # fn example(actor_ref: ActorRef) { // Assuming actor_ref is an ActorRef to a suitable actor
    /// let actor_clone = actor_ref.clone();
    /// tokio::task::spawn_blocking(move || {
    ///     // Perform CPU-intensive work
    ///
    ///     // Send results to actor
    ///     actor_clone.tell_blocking("Work completed", Some(Duration::from_secs(1)))
    ///         .expect("Failed to send message");
    /// });
    /// # }
    /// ```
    ///
    /// For more comprehensive examples, including `ask_blocking`, refer to
    /// `examples/actor_blocking_tasks.rs`.
    pub fn tell_blocking<M>(&self, msg: M, timeout: Option<std::time::Duration>) -> Result<()>
    where
        M: Send + 'static,
    {
        let rt = tokio::runtime::Handle::try_current().map_err(|e| {
            anyhow::anyhow!("No tokio runtime available for tell_blocking: {}", e)
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(async {
                    tokio::time::timeout(duration, self.tell(msg))
                        .await
                        .map_err(|_| anyhow::anyhow!("tell_blocking operation timed out after {:?}", duration))?
                })
            },
            None => rt.block_on(self.tell(msg)),
        }
    }

    /// Synchronous version of `ask` that blocks until the reply is received.
    ///
    /// The message is sent to the actor's mailbox, and this method will block until
    /// the actor processes the message and sends a reply or the timeout expires.
    ///
    /// # Examples
    ///
    /// For a complete example, see `examples/actor_blocking_tasks.rs`.
    ///
    /// ```rust,no_run
    /// use rsactor::ActorRef;
    /// use std::time::Duration;
    /// struct QueryMessage;
    /// fn main() -> anyhow::Result<()> {
    ///     let actor_ref: ActorRef = panic!(); // Placeholder
    ///     let result = tokio::task::spawn_blocking(move || {
    ///         let timeout = Some(Duration::from_secs(2));
    ///         let response: String = actor_ref.ask_blocking(QueryMessage, timeout).unwrap();
    ///         // Process response...
    ///         response
    ///     });
    ///     Ok(())
    /// }
    /// ```
    /// Refer to the `examples/actor_blocking_tasks.rs` file for a runnable demonstration.
    pub fn ask_blocking<M, R>(&self, msg: M, timeout: Option<std::time::Duration>) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let rt = tokio::runtime::Handle::try_current().map_err(|e| {
            anyhow::anyhow!("No tokio runtime available for ask_blocking: {}", e)
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(async {
                    tokio::time::timeout(duration, self.ask(msg))
                        .await
                        .map_err(|_| anyhow::anyhow!("ask_blocking operation timed out after {:?}", duration))?
                })
            },
            None => rt.block_on(self.ask(msg)),
        }
    }
}

/// Represents the reason an actor stopped.
#[derive(Debug)]
pub enum ActorStopReason {
    /// Actor stopped normally after processing a `StopGracefully` signal or
    /// when its `Runtime` finished processing messages.
    Normal,
    /// Actor was terminated by a `kill` signal.
    Killed,
    /// Actor stopped due to an error, such as a panic in a message handler
    /// or a failure in one of its lifecycle hooks (`on_start`, `on_stop`).
    Error(anyhow::Error),
}

/// Defines the behavior of an actor.
///
/// Actors are fundamental units of computation that communicate by exchanging messages.
/// Each actor has its own state and processes messages sequentially.
///
/// Implementors of this trait must also be `Send + 'static`.
pub trait Actor: Send + 'static {
    /// The error type that can be returned by the actor's lifecycle methods.
    type Error: Send + Debug + 'static;

    /// Called when the actor is started.
    ///
    /// This method can be used for initialization tasks.
    /// If it returns an error, the actor will fail to start, and `on_stop` will be called
    /// with an `ActorStopReason::Error`.
    fn on_start(&mut self, _actor_ref: ActorRef) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    /// Called when the actor is stopped.
    ///
    /// This method can be used for cleanup tasks.
    fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn on_run(&mut self, _actor_ref: &ActorRef) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }
}

/// A trait for messages that an actor can handle, defining the reply type.
///
/// An actor struct implements this trait for each specific message type it can process.
pub trait Message<T: Send + 'static>: Actor {
    /// The type of the reply that will be sent back to the caller.
    type Reply: Send + 'static;

    /// Handles the incoming message and produces a reply.
    ///
    /// This is an asynchronous method where the actor's business logic for
    /// processing the message `T` resides.
    fn handle(&mut self, msg: T, _actor_ref: &ActorRef) -> impl Future<Output = Self::Reply> + Send;
}

/// A trait for type-erased message handling within the actor's `Runtime`.
///
/// This trait is typically implemented automatically by the `impl_message_handler!` macro.
/// It allows the `Runtime` to handle messages of different types by downcasting
/// them to their concrete types before passing them to the actor's specific `Message::handle`
/// implementation.
pub trait MessageHandler: Send + Sync + 'static {
    /// Handles a type-erased message.
    ///
    /// The implementation should attempt to downcast `msg_any` to one of the
    /// message types the actor supports and then call the corresponding
    /// `Message::handle` method.
    fn handle(
        &mut self,
        msg_any: Box<dyn Any + Send>,
        actor_ref: &ActorRef,
    ) -> impl Future<Output = Result<Box<dyn Any + Send>>> + Send;
}

// Manages the lifecycle and message loop for a single actor instance.
struct Runtime<T: Actor + MessageHandler> {
    actor_ref: ActorRef,
    actor: T, // Actor instance is now owned by Runtime
    receiver: mpsc::Receiver<MailboxMessage>, // Receives messages for this actor
    terminate_receiver: mpsc::Receiver<ControlSignal>, // Dedicated receiver for Terminate messages - Changed type
}

impl<T: Actor + MessageHandler> Runtime<T> {
    /// Creates a new `Runtime` for the given actor.
    fn new(
        actor: T, // Actor is moved into Runtime
        actor_ref: ActorRef,
        receiver: mpsc::Receiver<MailboxMessage>,
        terminate_receiver: mpsc::Receiver<ControlSignal>, // Added parameter - Changed type
    ) -> Self {
        Runtime {
            actor_ref,
            actor,
            receiver,
            terminate_receiver, // Initialize new field
        }
    }

    // This method encapsulates the actor's entire lifecycle within its spawned task.
    // It handles on_start, message processing, and on_stop, then returns the actor
    // instance and the reason for stopping. Consumes self to return the actor.
    async fn run_actor_lifecycle(mut self) -> (T, ActorStopReason) {
        let actor_id = self.actor_ref.id();

        // Call on_start
        if let Err(e_on_start) = self.actor.on_start(self.actor_ref.clone()).await {
            let error_msg = format!("Actor {} on_start error: {:?}", actor_id, e_on_start);
            error!("{}", error_msg);
            // let base_error_msg = format!("on_start failed for actor {}: {:?}", actor_id, e_on_start);
            let mut combined_error = anyhow::Error::msg(error_msg); // Initial error from on_start

            // Attempt to call on_stop, its error (if any) should be chained.
            let on_start_failure_reason = ActorStopReason::Error(anyhow::Error::msg(format!("on_start failed for actor {}", actor_id)));

            if let Err(e_on_stop) = self.actor.on_stop(self.actor_ref.clone(), &on_start_failure_reason).await {
                let error_msg = format!("Actor {} on_stop error following on_start error: {:?}", actor_id, e_on_stop);
                error!("{}", error_msg);
                // Add context about the on_stop failure to the combined_error
                combined_error = combined_error.context(error_msg);
            }
            info!("Actor {} task finishing prematurely due to error(s) during startup.", actor_id);
            return (self.actor, ActorStopReason::Error(combined_error));
        }

        info!("Runtime for actor {} is running.", actor_id);

        let mut final_reason = ActorStopReason::Normal; // Default reason

        // Message processing loop
        loop {
            tokio::select! {
                // Handle Terminate signal with highest priority
                biased; // Ensure Terminate is checked first if multiple conditions are ready

                maybe_signal = self.terminate_receiver.recv() => {
                    if let Some(ControlSignal::Terminate) = maybe_signal {
                        info!("Actor {} received Terminate signal. Stopping immediately.", actor_id);
                        final_reason = ActorStopReason::Killed;
                    } else {
                        // Channel closed or unexpected signal, this is an error state.
                        let error_msg = format!("Actor {} terminate_receiver closed unexpectedly or received invalid signal: {:?}. Marking as error.", actor_id, maybe_signal);
                        error!("{}", error_msg);
                        final_reason = ActorStopReason::Error(anyhow::anyhow!(error_msg));
                    }
                    break; // Exit the loop to proceed to on_stop
                }

                // Process incoming messages from the main mailbox
                maybe_message = self.receiver.recv() => {
                    match maybe_message {
                        Some(MailboxMessage::Envelope { payload, reply_channel }) => {
                            trace!("Actor {} received message: {:?}", actor_id, payload);
                            match self.actor.handle(payload, &self.actor_ref).await {
                                Ok(reply) => {
                                    if let Some(tx) = reply_channel {
                                        if tx.send(Ok(reply)).is_err() {
                                            debug!("Actor {} failed to send reply: receiver dropped.", actor_id);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Actor {} error handling message: {:?}", actor_id, e);
                                    if let Some(tx) = reply_channel {
                                        // Send the error back to the asker
                                        if tx.send(Err(e)).is_err() {
                                            debug!("Actor {} failed to send error reply: receiver dropped.", actor_id);
                                        }
                                    }
                                    // If a message handler returns an error, stop the actor.
                                    final_reason = ActorStopReason::Error(anyhow::anyhow!("Error in message handler for actor {}", actor_id));
                                    break; // Exit loop, proceed to on_stop
                                }
                            }
                        }
                        Some(MailboxMessage::StopGracefully) => {
                            info!("Actor {} received StopGracefully. Will stop after processing current messages.", actor_id);
                            // Don't set final_reason yet, Normal is default.
                            break;
                        }
                        // Terminate is handled by its own dedicated channel and select branch.
                        None => {
                            // Mailbox closed, meaning all senders (ActorRefs) are dropped.
                            // This is a form of graceful shutdown.
                            info!("Actor {} mailbox closed (all ActorRefs dropped). Stopping.", actor_id);
                            // final_reason = ActorStopReason::Normal;
                            break; // Exit loop, proceed to on_stop
                        }
                    }
                }

                maybe_result = self.actor.on_run(&self.actor_ref) => {
                    match maybe_result {
                        Ok(should_continue) => {
                            if !should_continue {
                                info!("Actor {} on_run returned false. Stopping.", actor_id);
                                break; // Exit loop, proceed to on_stop
                            }
                        }
                        Err(e) => {
                            let error_msg = format!("Actor {} on_run error: {:?}", actor_id, e);
                            error!("{}", error_msg);
                            final_reason = ActorStopReason::Error(anyhow::anyhow!(error_msg));
                            break; // Exit loop, proceed to on_stop
                        }
                    }
                    // If on_run returns false, we stop the actor.
                    tokio::task::yield_now().await;
                }
            }
        }

        self.receiver.close(); // Close the main mailbox
        self.terminate_receiver.close(); // Close its own channel

        info!("Actor {} message loop ended. Reason: {:?}", actor_id, final_reason);

        // Call on_stop
        if let Err(e_on_stop) = self.actor.on_stop(self.actor_ref.clone(), &final_reason).await {
            let error_msg = format!("Actor {} on_stop error: {:?}", actor_id, e_on_stop);
            error!("{}", error_msg);
            // If final_reason was already an error, chain this new error.
            // Otherwise, this on_stop error becomes the primary reason for failure.
            match final_reason {
                ActorStopReason::Error(mut existing_err) => {
                    existing_err = existing_err.context(error_msg);
                    final_reason = ActorStopReason::Error(existing_err);
                }
                _ => {
                    final_reason = ActorStopReason::Error(anyhow::Error::msg(format!("on_stop failed: {:?}", e_on_stop)));
                }
            }
        }
        info!("Actor {} task finishing.", actor_id);
        (self.actor, final_reason)
    }
}

/// Spawns a new actor and returns an `ActorRef` to it, along with a `JoinHandle`.
///
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor instance and its `ActorStopReason`.
pub fn spawn<T: Actor + MessageHandler + 'static>(
    actor: T,
) -> (ActorRef, tokio::task::JoinHandle<(T, ActorStopReason)>) {
    let capacity = CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().copied().unwrap_or(DEFAULT_MAILBOX_CAPACITY);
    spawn_with_mailbox_capacity(actor, capacity)
}

/// Spawns a new actor with a specified mailbox capacity and returns an `ActorRef` to it, along with a `JoinHandle`.
///
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor instance and its `ActorStopReason`.
pub fn spawn_with_mailbox_capacity<T: Actor + MessageHandler + 'static>(
    actor: T, // Added actor parameter
    mailbox_capacity: usize,
) -> (ActorRef, tokio::task::JoinHandle<(T, ActorStopReason)>) {
    if mailbox_capacity == 0 {
        panic!("Mailbox capacity must be greater than 0");
    }

    let id = ACTOR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let (mailbox_tx, mailbox_rx) = mpsc::channel(mailbox_capacity);
    // Create a dedicated channel for the Terminate signal with a small capacity (e.g., 1 or 2)
    // This ensures that a kill signal can be sent even if the main mailbox is full.
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1); // Changed type

    let actor_ref = ActorRef::new_internal(id, mailbox_tx, terminate_tx); // Pass terminate_tx

    let runtime = Runtime::new(actor, actor_ref.clone(), mailbox_rx, terminate_rx); // Pass terminate_rx

    let join_handle = tokio::spawn(runtime.run_actor_lifecycle());

    (actor_ref, join_handle)
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
        assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to stopped actor should fail");
    }

    #[tokio::test]
    async fn test_actor_ref_kill() {
        let (actor_ref, handle, _counter_arc_from_setup, _lpmt_arc_from_setup, on_start_called_arc, _) =
            setup_actor().await;
        assert!(*on_start_called_arc.lock().await, "on_start should have been called");

        // Send a message that should ideally sit in the queue if kill is prioritized.
        // The initial value of counter in TestActor is 0.
        actor_ref.tell(UpdateCounterMsg(10)).await.expect("Tell UpdateCounterMsg failed");

        // Immediately send kill, without waiting for the previous message to be processed.
        // The dedicated terminate channel and biased select in Runtime should prioritize this.
        actor_ref.kill().expect("kill command failed");

        let (returned_actor, reason) = handle.await.expect("Actor task failed to complete");

        println!("Actor value: {:?}", returned_actor.counter);

        assert!(matches!(reason, ActorStopReason::Killed), "Stop reason was {:?}, expected ActorStopReason::Killed", reason);

        // Check that on_stop was called on the actor instance.
        assert!(*returned_actor.on_stop_called.lock().await, "on_stop should have been called even on kill");

        // Verify that the UpdateCounterMsg(10) was NOT processed because kill took priority.
        let final_counter = *returned_actor.counter.lock().await;
        assert_eq!(final_counter, 0, "Counter should be 0, indicating UpdateCounterMsg was not processed due to kill priority. Got: {}", final_counter);

        let final_lpmt = returned_actor.last_processed_message_type.lock().await.clone();
        assert_eq!(final_lpmt, None, "Last processed message type should be None, indicating UpdateCounterMsg was not processed. Got: {:?}", final_lpmt);

        // Interactions after kill should still fail
        assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to killed actor should fail");
        assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to killed actor should fail");
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
        fail_on_run: bool,
        on_start_attempted: Arc<Mutex<bool>>,
        on_stop_attempted: Arc<Mutex<bool>>,
        on_run_attempted: Arc<Mutex<bool>>,
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
        async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<bool, Self::Error> {
            *self.on_run_attempted.lock().await = true;
            if self.fail_on_run { Err(anyhow::anyhow!("simulated on_run failure")) } else { Ok(true) }
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
        let on_stop_attempted = Arc::new(Mutex::new(false));
        let on_run_attempted = Arc::new(Mutex::new(false));
        let actor = LifecycleErrorActor {
            id: 0,
            fail_on_start: true,
            fail_on_stop: false,
            fail_on_run: false,
            on_start_attempted: on_start_attempted.clone(),
            on_stop_attempted: on_stop_attempted.clone(),
            on_run_attempted: on_run_attempted.clone(),
        };
        let (_actor_ref, handle) = spawn(actor);

        match handle.await {
            Ok((returned_actor, reason)) => {
                assert!(matches!(reason, ActorStopReason::Error(_)), "Expected ActorStopReason::Error, got {:?}", reason);
                if let ActorStopReason::Error(e) = reason {
                    assert!(e.to_string().contains("on_start error"));
                }
                assert!(*returned_actor.on_start_attempted.lock().await);
                assert!(!*returned_actor.on_run_attempted.lock().await);
                assert!(*returned_actor.on_stop_attempted.lock().await);
            }
            Err(e) => panic!("Expected Ok with Panicked reason, got JoinError: {:?}", e),
        }
    }

        #[tokio::test]
    async fn test_actor_fail_on_run() {
        let on_start_attempted = Arc::new(Mutex::new(false));
        let on_stop_attempted = Arc::new(Mutex::new(false));
        let on_run_attempted = Arc::new(Mutex::new(false));
        let actor = LifecycleErrorActor {
            id: 0,
            fail_on_start: false,
            fail_on_run: true,
            fail_on_stop: false,
            on_start_attempted: on_start_attempted.clone(),
            on_stop_attempted: on_stop_attempted.clone(),
            on_run_attempted: on_run_attempted.clone(),
        };
        let (_actor_ref, handle) = spawn(actor);

        match handle.await {
            Ok((returned_actor, reason)) => {
                assert!(matches!(reason, ActorStopReason::Error(_)), "Expected ActorStopReason::Error, got {:?}", reason);
                if let ActorStopReason::Error(e) = reason {
                    println!("Error: ---- {:?}", e);
                    assert!(e.to_string().contains("simulated on_run failure"));
                }
                assert!(*returned_actor.on_start_attempted.lock().await);
                assert!(*returned_actor.on_run_attempted.lock().await);
                assert!(*returned_actor.on_stop_attempted.lock().await);
            }
            Err(e) => panic!("Expected Ok with Panicked reason, got JoinError: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_actor_fail_on_stop() {
        let on_start_attempted = Arc::new(Mutex::new(false));
        let on_stop_attempted = Arc::new(Mutex::new(false));
        let on_run_attempted = Arc::new(Mutex::new(false));
        let actor = LifecycleErrorActor {
            id: 0,
            fail_on_start: false,
            fail_on_run: false,
            fail_on_stop: true,
            on_start_attempted: on_start_attempted.clone(),
            on_stop_attempted: on_stop_attempted.clone(),
            on_run_attempted: on_run_attempted.clone(),
        };
        let (actor_ref, handle) = spawn(actor);

        // Added to increase the test coverage
        actor_ref.tell(NoOpMsg).await.expect("Tell should succeed");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await; // Ensure on_start runs
        assert!(*on_start_attempted.lock().await);

        actor_ref.stop().await.expect("Stop command should succeed");

        match handle.await {
            Ok((returned_actor, reason)) => {
                assert!(matches!(reason, ActorStopReason::Error(_)), "Expected ActorStopReason::Error, got {:?}", reason);
                if let ActorStopReason::Error(e) = reason {
                    assert!(e.to_string().contains("on_stop failed"));
                }
                assert!(*returned_actor.on_start_attempted.lock().await);
                assert!(*returned_actor.on_run_attempted.lock().await);
                assert!(*returned_actor.on_stop_attempted.lock().await);
            }
            Err(e) => panic!("Expected Ok with Panicked reason, got JoinError: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_set_default_mailbox_capacity_to_zero() {
        // This test is independent of whether the capacity has been set before or not.
        let result = set_default_mailbox_capacity(0);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Global default mailbox capacity must be greater than 0"
        );
    }

    #[tokio::test]
    async fn test_set_default_mailbox_capacity_ok_then_error_on_already_set() {
        // This test handles the OnceLock nature: it tries to set a value.
        // If successful, it verifies that subsequent sets fail.
        // If the first attempt to set fails (because it's already set),
        // it still verifies that another attempt to set also fails.

        // Use a unique capacity for this test if possible, to minimize interference
        // if this test doesn't run first.
        let test_capacity_value = 123;
        let initial_set_result = set_default_mailbox_capacity(test_capacity_value);

        if initial_set_result.is_ok() {
            // Successfully set it for the first time (globally for this test run, or specifically by this test)
            assert_eq!(
                *CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().unwrap(),
                test_capacity_value,
                "Capacity should be the value we just set."
            );

            // Try to set it again with a different value
            let second_set_result = set_default_mailbox_capacity(456);
            assert!(second_set_result.is_err(), "Second set attempt should fail.");
            assert_eq!(
                second_set_result.unwrap_err(),
                "Global default mailbox capacity has already been set",
                "Error message for already set should match."
            );
            // Verify the original value is still there
            assert_eq!(
                *CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().unwrap(),
                test_capacity_value,
                "Capacity should remain the initially set value."
            );

            // Try to set it again with the same value
            let third_set_result = set_default_mailbox_capacity(test_capacity_value);
            assert!(third_set_result.is_err(), "Third set attempt (same value) should fail.");
            assert_eq!(
                third_set_result.unwrap_err(),
                "Global default mailbox capacity has already been set",
                "Error message for already set (same value) should match."
            );
            assert_eq!(
                *CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().unwrap(),
                test_capacity_value,
                "Capacity should still be the initially set value."
            );
        } else {
            // The default capacity was already set before this test (or this part of the test) ran.
            // This is expected if another test that calls set_default_mailbox_capacity ran first.
            let current_set_value = CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().expect("OnceLock should be set if initial_set_result failed because it was already set.");
            println!(
                "Note: Default mailbox capacity was already set to {:?} before this test scenario.",
                current_set_value
            );
            assert_eq!(
                initial_set_result.unwrap_err(),
                "Global default mailbox capacity has already been set",
                "Error message for initial set attempt (when already set) should match."
            );


            // Even if already set, trying to set it again (e.g. to a different value) must still fail.
            let subsequent_set_result = set_default_mailbox_capacity(789);
            assert!(subsequent_set_result.is_err(), "Subsequent set attempt (when already set by other test) should fail.");
            assert_eq!(
                subsequent_set_result.unwrap_err(),
                "Global default mailbox capacity has already been set",
                "Error message for subsequent set (when already set by other test) should match."
            );
            // And the value should remain what it was.
             assert_eq!(
                *CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().unwrap(),
                *current_set_value,
                "Capacity should remain the value set by a previous test/operation."
            );
        }
    }

    // Test actor panic in message handler
    struct PanicActor {
        on_stop_called: Arc<Mutex<bool>>,
    }
    impl Actor for PanicActor {
        type Error = anyhow::Error;

        async fn on_start(&mut self, _actor_ref: ActorRef) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
            let mut called = self.on_stop_called.lock().await;
            *called = true;
            Ok(())
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

    // Test: Spawning multiple actors

    // Dummy Actor for simple spawn and stop test
    struct DummyActor;

    impl Actor for DummyActor {
        type Error = anyhow::Error;
        // Default on_start and on_stop are used
    }

    // Even if the actor handles no messages, impl_message_handler is needed.
    // We can define a dummy message or leave it empty if the macro supports it.
    // For simplicity, let's assume it needs at least one message or an empty call.
    impl_message_handler!(DummyActor, []); // Assuming this is valid for no messages

    #[tokio::test]
    async fn test_spawn_and_stop_dummy_actor() {
        let actor = DummyActor;
        let (actor_ref, handle) = spawn(actor);

        // Optionally, give a brief moment for the actor to fully start, though not strictly necessary
        // if on_start does nothing.
        // tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        actor_ref.stop().await.expect("Failed to stop dummy actor");
        let (_actor_state, reason) = handle.await.expect("Dummy actor task failed");

        assert!(matches!(reason, ActorStopReason::Normal), "Dummy actor did not stop normally. Reason: {:?}", reason);
    }

    #[tokio::test]
    async fn test_actor_ref_tell_blocking() {
        let (actor_ref, handle, counter, last_processed, _on_start, on_stop_called) =
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
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_ask_blocking() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) = setup_actor().await;

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
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_ask_blocking_timeout() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) =
            setup_actor().await;

        // SlowMsg handler sleeps for 100ms. ask_blocking timeout is 10ms.
        let actor_ref_clone = actor_ref.clone();
        let join_handle = tokio::task::spawn_blocking(move || {
            let result: Result<(), _> = actor_ref_clone
                .ask_blocking(SlowMsg, Some(std::time::Duration::from_millis(10))); // Timeout 10ms
            assert!(result.is_err(), "ask_blocking should have timed out");
            if let Err(e) = result {
                assert!(e.to_string().contains("ask_blocking operation timed out"), "Error message mismatch: {}", e);
            }
        });

        join_handle.await.expect("Blocking task panicked for ask timeout test");

        actor_ref.stop().await.expect("Failed to stop actor");
        handle.await.expect("Actor task failed");
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_tell_blocking_timeout_when_mailbox_full() {
        // Spawn an actor with a mailbox capacity of 1.
        let counter = Arc::new(Mutex::new(0));
        let last_processed_message_type = Arc::new(Mutex::new(None));
        let on_start_called = Arc::new(Mutex::new(false));
        let on_stop_called = Arc::new(Mutex::new(false));

        let actor_instance = TestActor::new(
            counter.clone(),
            last_processed_message_type.clone(),
            on_start_called.clone(),
            on_stop_called.clone(),
        );
        // Spawn with capacity 1
        let (actor_ref, handle) = spawn_with_mailbox_capacity(actor_instance, 1);
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
                assert!(e.to_string().contains("tell_blocking operation timed out"));
            }
        });

        join_handle_blocking_task.await.expect("Blocking task for tell_blocking timeout panicked");

        // Allow the actor to process messages (SlowMsg, then UpdateCounterMsg(1))
        // The UpdateCounterMsg(2) from tell_blocking should have failed and not be in the queue.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await; // Wait for SlowMsg (100ms) + UpdateCounterMsg(1)

        actor_ref.stop().await.expect("Failed to stop actor");
        let (actor_state, _reason) = handle.await.expect("Actor task failed");
        assert!(*actor_state.on_stop_called.lock().await);
        // Verify that only UpdateCounterMsg(1) was processed.
        assert_eq!(*actor_state.counter.lock().await, 1, "Counter should be 1 after SlowMsg and UpdateCounterMsg(1)");
    }

    #[tokio::test]
    async fn test_actor_ref_ask_blocking_no_timeout() {
        let (actor_ref, handle, _counter, _lpmt, _on_start, on_stop_called) =
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
        assert!(*on_stop_called.lock().await);
    }

    #[tokio::test]
    async fn test_actor_ref_kill_multiple_times() {
        let (actor_ref, handle, _counter, _lpmt, on_start_called, _) =
            setup_actor().await;
        assert!(*on_start_called.lock().await, "on_start should have been called");

        // Call kill multiple times
        actor_ref.kill().expect("First kill command failed");
        actor_ref.kill().expect("Second kill command should also succeed (idempotent)");
        actor_ref.kill().expect("Third kill command should also succeed (idempotent)");

        let (returned_actor, reason) = handle.await.expect("Actor task failed to complete");

        assert!(matches!(reason, ActorStopReason::Killed), "Stop reason was {:?}, expected ActorStopReason::Killed", reason);
        assert!(*returned_actor.on_stop_called.lock().await, "on_stop should have been called even on multiple kills");

        // Verify that messages sent before or after kill are not processed if kill is effective.
        // (This part is similar to test_actor_ref_kill, ensuring state consistency)
        let final_counter = *returned_actor.counter.lock().await;
        assert_eq!(final_counter, 0, "Counter should be 0, indicating no messages processed due to kill. Got: {}", final_counter);

        // Interactions after kill should still fail
        assert!(actor_ref.tell(UpdateCounterMsg(1)).await.is_err(), "Tell to killed actor should fail");
        assert!(actor_ref.ask::<PingMsg, String>(PingMsg("test".to_string())).await.is_err(), "Ask to killed actor should fail");
    }

    #[tokio::test]
    async fn test_actor_ref_is_alive() {
        // Test 1: Actor is alive after spawn, and dead after stop
        let (actor_ref_stop_test, handle_stop_test, _counter_stop, _lpmt_stop, on_start_called_stop, on_stop_called_stop) =
            setup_actor().await;
        assert!(*on_start_called_stop.lock().await, "on_start should be called for stop test");

        assert!(actor_ref_stop_test.is_alive(), "Actor should be alive after spawn (stop test)");

        actor_ref_stop_test.stop().await.expect("Failed to stop actor (stop test)");
        let (_actor_state_stop, reason_stop) = handle_stop_test.await.expect("Actor task failed after stop (stop test)");
        assert!(matches!(reason_stop, ActorStopReason::Normal), "Stop reason was {:?}, expected ActorStopReason::Normal", reason_stop);
        assert!(*on_stop_called_stop.lock().await, "on_stop should be called after stop (stop test)");

        assert!(!actor_ref_stop_test.is_alive(), "Actor should not be alive after stop (stop test)");

        // Test 2: Actor is alive after spawn, and dead after kill
        let (actor_ref_kill_test, handle_kill_test, _counter_kill, _lpmt_kill, on_start_called_kill, on_stop_called_kill) =
            setup_actor().await;
        assert!(*on_start_called_kill.lock().await, "on_start should be called for kill test");

        assert!(actor_ref_kill_test.is_alive(), "Actor should be alive before kill (kill test)");

        actor_ref_kill_test.kill().expect("kill command failed (kill test)");
        let (_actor_state_kill, reason_kill) = handle_kill_test.await.expect("Actor task failed after kill (kill test)");
        assert!(matches!(reason_kill, ActorStopReason::Killed), "Stop reason was {:?}, expected ActorStopReason::Killed", reason_kill);
        // on_stop is expected to be called even on kill, as per test_actor_ref_kill
        assert!(*on_stop_called_kill.lock().await, "on_stop should be called after kill (kill test)");

        assert!(!actor_ref_kill_test.is_alive(), "Actor should not be alive after kill (kill test)");
    }
}
