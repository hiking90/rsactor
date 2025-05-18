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
//! - **Actor Lifecycle**: Actors have `on_start`, `on_stop`, and `run_loop` lifecycle hooks.
//!   The `run_loop` method is called after `on_start` and contains the main execution
//!   logic of the actor, running for its lifetime.
//! - **Graceful Shutdown & Kill**: Actors can be stopped gracefully or killed immediately.
//! - **Typed Messages**: Messages are strongly typed, and replies are also typed.
//! - **Macro for Message Handling**: The `impl_message_handler!` macro simplifies
//!   handling multiple message types.
//!
//! ## Core Concepts
//!
//! - **`Actor`**: Trait defining actor behavior and lifecycle hooks (`on_start`, `on_stop`, `run_loop`).
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
//! All `Actor` lifecycle methods (`on_start`, `on_stop`, `run_loop`) are optional
//! and have default implementations.
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
//! // 2. Implement the Actor trait (on_start, on_stop, on_run are optional)
//! impl Actor for MyActor {
//!     type Error = anyhow::Error; // Define an error type
//!
//!     // Optional: Implement on_start for initialization
//!     // async fn on_start(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
//!     //     println!("MyActor (data: '{}') started!", self.data);
//!     //     Ok(())
//!     // }
//!
//!     // Optional: Implement on_stop for cleanup
//!     // async fn on_stop(&mut self, _actor_ref: &ActorRef, _reason: &rsactor::ActorStopReason) -> Result<(), Self::Error> {
//!     //     println!("MyActor (data: '{}') stopped!", self.data);
//!     //     Ok(())
//!     // }
//!
//!     // Optional: Implement run_loop for the actor's main execution logic.
//!     // This method is called after on_start. If it returns Ok(()), the actor stops normally.
//!     // If it returns Err(_), the actor stops due to an error.
//!     // async fn run_loop(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
//!     //     // Example: Perform some work in a loop or a long-running task.
//!     //     // loop {
//!     //     //     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!     //     //     // if some_condition { break; } // Or return Ok(()) to stop.
//!     //     // }
//!     //     Ok(()) // Actor stops when run_loop completes.
//!     // }
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

use std::{
    any::Any,
    fmt::Debug,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
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
                Err(anyhow::anyhow!(concat!(stringify!($actor_type), ": MessageHandler received unknown message type.")))
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

    /// Called when the actor is started. This is optional and has a default implementation.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    /// This method can be used for initialization tasks.
    /// If it returns an error, the actor will fail to start, and `on_stop` will be called
    /// with an `ActorStopReason::Error`.
    fn on_start(&mut self, _actor_ref: &ActorRef) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    /// Called when the actor is stopped. This is optional and has a default implementation.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    /// The `stop_reason` parameter indicates why the actor is stopping.
    /// This method can be used for cleanup tasks.
    fn on_stop(&mut self, _actor_ref: &ActorRef, _stop_reason: &ActorStopReason) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    /// The main execution loop for the actor.
    ///
    /// This method is called after `on_start` and is expected to contain the primary logic
    /// of the actor. It typically runs for the entire lifetime of the actor.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    ///
    /// If this method returns `Ok(())` or `Err(_)`, the actor will be stopped.
    /// If it returns `Ok(())`, `on_stop` will be called with `ActorStopReason::Normal`.
    /// If it returns `Err(_)`, `on_stop` will be called with `ActorStopReason::Error`.
    fn run_loop(&mut self, _actor_ref: &ActorRef) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async {
            loop {
                // Placeholder for the actor's main loop logic.
                // This could be a long-running task or periodic work.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
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
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    /// This is an asynchronous method where the actor's business logic for
    /// processing the message `T` resides.
    fn handle(&mut self, msg: T, actor_ref: &ActorRef) -> impl Future<Output = Self::Reply> + Send;
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
        if let Err(e_on_start) = self.actor.on_start(&self.actor_ref).await {
            let error_msg = format!("Actor {} on_start error: {:?}", actor_id, e_on_start);
            error!("{}", error_msg);

            info!("Actor {} task finishing prematurely due to error(s) during startup.", actor_id);
            return (self.actor, ActorStopReason::Error(anyhow::Error::msg(error_msg)));
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

                maybe_result = self.actor.run_loop(&self.actor_ref) => {
                    match maybe_result {
                        Ok(_) => {
                            // If None is returned, we stop the actor.
                            info!("Actor {} is stopping.", actor_id);
                        }
                        Err(e) => {
                            let error_msg = format!("Actor {} on_run error: {:?}", actor_id, e);
                            error!("{}", error_msg);
                            final_reason = ActorStopReason::Error(anyhow::anyhow!(error_msg));
                        }
                    }
                    break; // Exit loop, proceed to on_stop
                }
            }
        }

        self.receiver.close(); // Close the main mailbox
        self.terminate_receiver.close(); // Close its own channel

        info!("Actor {} message loop ended. Reason: {:?}", actor_id, final_reason);

        // Call on_stop
        if let Err(e_on_stop) = self.actor.on_stop(&self.actor_ref, &final_reason).await {
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
                    final_reason = ActorStopReason::Error(anyhow::Error::msg(error_msg));
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


#[cfg(test)]
mod tests {
    use super::*;

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
}
