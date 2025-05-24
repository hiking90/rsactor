// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # rsActor
//! A Lightweight Rust Actor Framework with Simple Yet Powerful Task Control
//!
//! `rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing simple
//! yet powerful task control. It prioritizes simplicity and efficiency for local, in-process
//! actor systems while giving developers complete control over their actors' execution lifecycle —
//! define your own `on_run`, control execution, control the lifecycle.
//!
//! ## Features
//!
//! - **Asynchronous Actors**: Actors run in their own asynchronous tasks.
//! - **Message Passing**: Actors communicate by sending and receiving messages.
//!   - `tell`: Send a message without waiting for a reply (fire-and-forget).
//!   - `tell_with_timeout`: Send a message without waiting for a reply, with a specified timeout.
//!   - `ask`: Send a message and await a reply.
//!   - `ask_with_timeout`: Send a message and await a reply, with a specified timeout.
//!   - `tell_blocking`: Blocking version of `tell` for use in `tokio::task::spawn_blocking` tasks.
//!   - `ask_blocking`: Blocking version of `ask` for use in `tokio::task::spawn_blocking` tasks.
//! - **Actor Lifecycle with Simple Yet Powerful Task Control**: Actors have `on_start` and `on_run` lifecycle hooks.
//!   The distinctive `on_run` feature provides a dedicated task execution environment that users can control
//!   with simple yet powerful primitives, unlike other actor frameworks. This gives developers complete control over
//!   their actor's task logic while the framework manages the underlying execution.
//! - **Graceful Shutdown & Kill**: Actors can be stopped gracefully or killed immediately.
//! - **Typed Messages**: Messages are strongly typed, and replies are also typed.
//! - **Macro for Message Handling**: The `impl_message_handler!` macro simplifies
//!   handling multiple message types.
//!
//! ## Core Concepts
//!
//! - **`Actor`**: Trait defining actor behavior and lifecycle hooks (`on_start`, `on_run`).
//! - **`Message<M>`**: Trait for handling a message type `M` and defining its reply type.
//! - **`ActorRef`**: Handle for sending messages to an actor.
//! - **`spawn`**: Function to create and start an actor, returning an `ActorRef` and a `JoinHandle`.
//! - **`MessageHandler`**: Trait for type-erased message handling. This is typically implemented automatically by the `impl_message_handler!` macro.
//! - **`ActorResult`**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
//! - **`MailboxMessage(Internal)`**: Enum for messages in an actor's mailbox (user messages and control signals).
//! - **`Runtime(Internal)`**: Manages an actor's internal lifecycle and message loop.
//!
//! ## Getting Started
//!
//! Define an actor struct, implement `Actor` and `Message<M>` for each message type.
//! Use `impl_message_handler!` to wire up message handling.
//! All `Actor` lifecycle methods (`on_start`, `on_run`) are optional
//! and have default implementations.
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
//! use anyhow::Result;
//!
//! // 1. Define your actor struct
//! #[derive(Debug)]
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
//!     type Args = String; // Define an args type for actor creation
//!     type Error = anyhow::Error; // Define an error type
//!
//!     // Required: Implement on_start for actor creation and initialization
//!     async fn on_start(args: Self::Args, _actor_ref: &ActorRef) -> std::result::Result<Self, Self::Error> {
//!         println!("MyActor (data: '{}') started!", args);
//!         Ok(MyActor { data: args })
//!     }
//!
//!     // Optional: Implement on_run for the actor's main execution logic.
//!     // This method is called after on_start. If it returns Ok(false), the actor stops normally.
//!     // If it returns Err(_), the actor stops due to an error.
//!     // If it returns Ok(true), the actor continues running.
//!     async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<bool, Self::Error> {
//!         let mut tick_300ms = tokio::time::interval(std::time::Duration::from_millis(300));
//!         let mut tick_1s = tokio::time::interval(std::time::Duration::from_secs(1));
//!         tokio::select! {
//!             _ = tick_300ms.tick() => {
//!                 println!("Tick: 300ms");
//!             }
//!             _ = tick_1s.tick() => {
//!                 println!("Tick: 1s");
//!             }
//!         }
//!         Ok(true) // Continue running
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
//!     let (actor_ref, join_handle) = spawn::<MyActor>("initial data".to_string());
//!
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
//!     let actor_result = join_handle.await?;
//!     println!("Actor stopped with result: {:?}", actor_result);
//!
//!     Ok(())
//! }
//! ```
//!
//! This crate-level documentation provides an overview of `rsactor`.
//! For more details on specific components, please refer to their individual
//! documentation.

use std::{
    any::Any, fmt::Debug, future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering}, OnceLock
    },
    time::Duration
};

use tokio::sync::{mpsc, oneshot};
use log::{info, error, warn, debug, trace};

#[derive(Debug)]
pub enum Error {
    /// Error when sending a message to an actor
    Send {
        /// ID of the actor that failed to receive the message
        actor_id: usize,
        /// Additional context about the error
        details: String,
    },
    /// Error when receiving a response from an actor
    Receive {
        /// ID of the actor that failed to send a response
        actor_id: usize,
        /// Additional context about the error
        details: String,
    },
    /// Error when a request times out
    Timeout {
        /// ID of the actor that timed out
        actor_id: usize,
        /// The duration after which the request timed out
        timeout: Duration,
        /// Type of operation that timed out (e.g., "send", "ask")
        operation: String,
    },
    /// Error when a message type is not handled by an actor
    UnhandledMessageType {
        /// ID of the actor that timed out
        actor_id: usize,
        /// Actor type name
        actor_type: String,
        /// Expected message types
        expected_types: Vec<String>,
        /// Actual message type ID
        actual_type_id: std::any::TypeId,
    },
    /// Error when downcasting a reply to the expected type
    Downcast {
        /// ID of the actor that sent the incompatible reply
        actor_id: usize,
        expected_type: String,
    },
    /// Error when a runtime operation fails
    Runtime {
        /// ID of the actor where the runtime error occurred
        actor_id: usize,
        /// Additional context about the error
        details: String,
    },
    /// Error related to mailbox capacity configuration
    MailboxCapacity {
        /// The error message
        message: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Send { actor_id, details } => {
                write!(f, "Failed to send message to actor {}: {}", actor_id, details)
            }
            Error::Receive { actor_id, details } => {
                write!(f, "Failed to receive reply from actor {}: {}", actor_id, details)
            }
            Error::Timeout { actor_id, timeout, operation } => {
                write!(f, "{} operation to actor {} timed out after {:?}",
                       operation, actor_id, timeout)
            }
            Error::UnhandledMessageType { actor_id, actor_type, expected_types, actual_type_id } => {
                write!(
                    f,
                    "Actor '{}#{}' received an unhandled message type. Expected one of: [{}]. Actual message type ID: {:?}",
                    actor_type,
                    actor_id,
                    expected_types.join(", "),
                    actual_type_id
                )
            }
            Error::Downcast { actor_id , expected_type} => {
                write!(f, "Failed to downcast reply from actor {} to expected type '{}'",
                    actor_id, expected_type)
            }
            Error::Runtime { actor_id, details } => {
                write!(f, "Runtime error in actor {}: {}", actor_id, details)
            }
            Error::MailboxCapacity { message } => {
                write!(f, "Mailbox capacity error: {}", message)
            }
        }
    }
}

impl std::error::Error for Error {}

/// A Result type specialized for rsactor operations.
pub type Result<T> = std::result::Result<T, Error>;

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
                _msg_any: Box<dyn std::any::Any + Send>, // This Box is consumed by the first successful downcast
                actor_ref: &$crate::ActorRef,
            ) -> $crate::Result<Box<dyn std::any::Any + Send>> {
                let mut _msg_any = _msg_any; // Mutable to allow reassignment in the loop
                $(
                    match _msg_any.downcast::<$msg_type>() {
                        Ok(concrete_msg_box) => {
                            // Successfully downcasted. concrete_msg_box is a Box<$msg_type>.
                            // The original _msg_any has been consumed by the downcast.
                            let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *concrete_msg_box, actor_ref).await;
                            return Ok(Box::new(reply) as Box<dyn std::any::Any + Send>);
                        }
                        Err(original_box_back) => {
                            // Downcast failed. original_box_back is the original Box<dyn Any + Send>.
                            // We reassign it to _msg_any so it can be used in the next iteration of the $(...)* loop.
                            _msg_any = original_box_back;
                        }
                    }
                )*
                // If the message type was not found in the list of handled types:
                let expected_msg_types: Vec<String> = vec![$(stringify!($msg_type).to_string()),*];
                return Err($crate::Error::UnhandledMessageType {
                    actor_id: actor_ref.id(),
                    actor_type: stringify!($actor_type).to_string(),
                    expected_types: expected_msg_types,
                    actual_type_id: _msg_any.type_id()
                });
            }
        }
    };
}

/// Represents messages that can be sent to an actor's mailbox.
///
/// This enum includes both user-defined messages (wrapped in `Envelope`)
/// and control messages like `StopGracefully`. The `Terminate` control signal
/// is handled through a separate dedicated channel.
#[derive(Debug)]
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
pub fn set_default_mailbox_capacity(size: usize) -> Result<()> {
    if size == 0 {
        return Err(Error::MailboxCapacity {
            message: "Global default mailbox capacity must be greater than 0".to_string(),
        });
    }

    CONFIGURED_DEFAULT_MAILBOX_CAPACITY
        .set(size)
        .map_err(|_| Error::MailboxCapacity {
            message: "Global default mailbox capacity has already been set".to_string(),
        })
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
///   - [`ask_with_timeout`](ActorRef::ask_with_timeout): Send a message and await a reply with a timeout.
///   - [`tell`](ActorRef::tell): Send a message without waiting for a reply.
///   - [`tell_with_timeout`](ActorRef::tell_with_timeout): Send a message without waiting for a reply with a timeout.
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
    type_name: &'static str,
    sender: MailboxSender,
    terminate_sender: mpsc::Sender<ControlSignal>, // Changed type
}

impl ActorRef {
    // Creates a new ActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    fn new(
        id: usize,
        type_name: &'static str,
        sender: MailboxSender,
        terminate_sender: mpsc::Sender<ControlSignal>,
    ) -> Self { // Changed type
        ActorRef {
            id,
            type_name,
            sender,
            terminate_sender,
        }
    }

    /// Returns the unique ID of the actor.
    pub const fn id(&self) -> usize {
        self.id
    }

    /// Returns the type name of the actor.
    pub const fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// Returns a string representation of the actor's name, including its type and ID.
    pub fn name(&self) -> String {
        format!("{}#{}", self.type_name, self.id)
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
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
            Err(Error::Send {
                actor_id: self.id,
                details: "Mailbox channel closed".to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget) with a timeout.
    ///
    /// Similar to `tell`, but allows specifying a timeout for the send operation.
    /// The message is sent to the actor's mailbox, and this method will return once
    /// the message is sent or timeout if the send operation doesn't complete
    /// within the specified duration.
    pub async fn tell_with_timeout<M>(&self, msg: M, timeout: std::time::Duration) -> Result<()>
    where
        M: Send + 'static,
    {
        tokio::time::timeout(timeout, self.tell(msg))
            .await
            .map_err(|_| Error::Timeout {
                actor_id: self.id,
                timeout,
                operation: "tell".to_string(),
            })?
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
            return Err(Error::Send {
                actor_id: self.id,
                details: "Mailbox channel closed".to_string(),
            });
        }

        match reply_rx.await {
            Ok(Ok(reply_any)) => { // recv was Ok, actor reply was Ok
                match reply_any.downcast::<R>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        actor_id: self.id,
                        expected_type: std::any::type_name::<R>().to_string(),
                    }),
                }
            }
            Ok(Err(e)) => Err(e), // recv was Ok, actor reply was Err
            Err(_recv_err) => Err(Error::Receive { // recv itself failed
                actor_id: self.id,
                details: "Reply channel closed unexpectedly".to_string(),
            }),
        }
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to `ask`, but allows specifying a timeout for the operation.
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply, or timeout if the reply
    /// doesn't arrive within the specified duration.
    pub async fn ask_with_timeout<M, R>(&self, msg: M, timeout: std::time::Duration) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        tokio::time::timeout(timeout, self.ask(msg))
            .await
            .map_err(|_| Error::Timeout {
                actor_id: self.id,
                timeout,
                operation: "ask".to_string(),
            })?
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    pub fn kill(&self) -> Result<()> {
        debug!("Attempting to send Terminate message to actor {} via dedicated channel using try_send", self.id);
        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
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
    /// The actor's final result will indicate normal completion.
    pub async fn stop(&self) -> Result<()> {
        debug!("Sending StopGracefully message to actor {}", self.id);
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
            Error::Runtime {
                actor_id: self.id, // Assuming self.id is accessible here. If not, need to adjust.
                details: format!("No tokio runtime available for tell_blocking: {}", e),
            }
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(async {
                    tokio::time::timeout(duration, self.tell(msg))
                        .await
                        .map_err(|_| Error::Timeout {
                            actor_id: self.id,
                            timeout: duration,
                            operation: "tell_blocking".to_string(),
                        })?
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
            Error::Runtime {
                actor_id: self.id, // Assuming self.id is accessible here.
                details: format!("No tokio runtime available for ask_blocking: {}", e),
            }
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(async {
                    tokio::time::timeout(duration, self.ask(msg))
                        .await
                        .map_err(|_| Error::Timeout {
                            actor_id: self.id,
                            timeout: duration,
                            operation: "ask_blocking".to_string(),
                        })?
                })
            },
            None => rt.block_on(self.ask(msg)),
        }
    }
}

/// Result type returned when an actor's lifecycle completes.
#[derive(Debug)]
pub enum ActorResult<T: Actor> {
    /// Actor completed successfully and can be recovered.
    Completed {
        actor: T,
        killed: bool,
    },
    /// Actor failed to start during the `on_start` lifecycle hook.
    StartupFailed {
        cause: T::Error,
    },
    /// Actor failed during execution.
    RuntimeFailed {
        actor: Option<T>,
        cause: T::Error,
    },
}

impl<T: Actor> From<ActorResult<T>> for (Option<T>, Option<T::Error>) {
    fn from(result: ActorResult<T>) -> Self {
        match result {
            ActorResult::Completed { actor, .. } => (Some(actor), None),
            ActorResult::StartupFailed { cause } => (None, Some(cause)),
            ActorResult::RuntimeFailed { actor, cause } => (actor, Some(cause)),
        }
    }
}


impl<T: Actor> ActorResult<T> {
    /// Returns `true` if the actor completed successfully.
    pub fn is_completed(&self) -> bool {
        matches!(self, ActorResult::Completed { .. })
    }

    /// Returns `true` if the actor was killed.
    pub fn was_killed(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: true, .. })
    }

    /// Returns `true` if the actor stopped normally.
    pub fn stopped_normally(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: false, .. })
    }

    /// Returns `true` if the actor failed to start.
    pub fn is_startup_failed(&self) -> bool {
        matches!(self, ActorResult::StartupFailed { .. })
    }

    /// Returns `true` if the actor failed during runtime.
    pub fn is_runtime_failed(&self) -> bool {
        matches!(self, ActorResult::RuntimeFailed { .. })
    }
}

// pub trait ReplyError: Any + Send + Debug + 'static {}

/// Defines the behavior of an actor.
///
/// Actors are fundamental units of computation that communicate by exchanging messages.
/// Each actor has its own state and processes messages sequentially.
///
/// Implementors of this trait must also be `Send + 'static`.
pub trait Actor: Sized + Send + 'static {
    /// Type for arguments passed to `on_start` for actor initialization.
    /// This type provides the necessary data to create an instance of the actor.
    type Args: Send;
    /// The error type that can be returned by the actor's lifecycle methods.
    type Error: Send + Debug + 'static;

    /// Called when the actor is started. This is required for actor creation.
    ///
    /// The `args` parameter, of type `Self::Args`, contains the initialization data
    /// provided when the actor is spawned. This method is responsible for using
    /// these arguments to create and return the actual actor instance (`Self`).
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    /// This method should return the initialized actor instance or an error.
    fn on_start(_args: Self::Args, _actor_ref: &ActorRef) -> impl Future<Output = std::result::Result<Self, Self::Error>> + Send;

    /// The primary task execution logic for the actor, designed for iterative execution.
    ///
    /// The `rsactor` runtime calls `on_run` to obtain a `Future` representing a segment of work.
    /// If this `Future` successfully completes by returning `Ok(true)`, the runtime may invoke
    /// `on_run` again to acquire a new `Future` for the next segment of work. This pattern
    /// of repeated invocation allows the actor to manage ongoing or periodic tasks throughout
    /// its lifecycle. This method replaces the earlier `run_loop` concept.
    ///
    /// `on_run`'s execution is concurrent with the actor's message handling capabilities,
    /// enabling the actor to perform background or main-loop tasks while continuing to
    /// process incoming messages from its mailbox.
    ///
    /// # Key characteristics:
    ///
    /// - **Iterative Execution**: The `rsactor` runtime invokes `on_run` to obtain a `Future`.
    ///   If this `Future` completes with `Ok(true)`, `on_run` may be called again by the
    ///   runtime to obtain a new `Future`. This allows for continuous or step-by-step
    ///   task processing throughout the actor's active lifecycle.
    ///
    /// - **State Persistence Across Invocations**: Because `on_run` can be invoked multiple
    ///   times by the runtime (each time generating a new `Future`), any state intended
    ///   to persist across these distinct invocations *must* be stored as fields within
    ///   the actor's struct (`self`). Local variables declared inside `on_run` are ephemeral
    ///   and will not be preserved if `on_run` completes and is subsequently re-invoked.
    ///
    /// - **Concurrent Message Handling**: The `Future` returned by `on_run` executes
    ///   concurrently with the actor's message processing loop. This allows the actor to
    ///   perform its `on_run` tasks while simultaneously remaining responsive to incoming
    ///   messages.
    ///
    /// - **Full State Access**: `on_run` has full mutable access to the actor's state (`self`).
    ///   Modifications to `self` within `on_run` are visible to subsequent message handlers
    ///   and future `on_run` invocations.
    ///
    /// - **Essential Await Points**: The `Future` returned by `on_run` must yield control
    ///   to the Tokio runtime via `.await` points, especially within any internal loops.
    ///   Lacking these, the `on_run` task could block the actor's ability to process messages
    ///   or perform other concurrent activities.
    ///
    /// # Common patterns:
    ///
    /// 1. **Periodic tasks**: For executing work at regular intervals. The `on_run` future
    ///    would typically involve a single tick of an interval. The `Interval` itself
    ///    should be stored as a field in the actor's struct to persist across `on_run` invocations.
    ///    ```rust,no_run
    ///    # use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
    ///    # use anyhow::Result;
    ///    # use std::time::Duration;
    ///    # use tokio::time::{Interval, MissedTickBehavior};
    ///    # struct MyActor {
    ///    #     interval: Interval,
    ///    # }
    ///    # impl MyActor {
    ///    # fn heavy_computation_needed(&self) -> bool { false }
    ///    # }
    ///    # impl Actor for MyActor {
    ///    # type Args = Duration; // Example: pass interval duration via Args
    ///    # type Error = anyhow::Error;
    ///    # async fn on_start(duration: Self::Args, _actor_ref: &ActorRef) -> std::result::Result<Self, Self::Error> {
    ///    #     let mut interval = tokio::time::interval(duration);
    ///    #     interval.set_missed_tick_behavior(MissedTickBehavior::Delay); // Or Skip, Burst
    ///    #     Ok(MyActor { interval })
    ///    # }
    ///    async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<bool, Self::Error> {
    ///        // self.interval is stored in the MyActor struct.
    ///        self.interval.tick().await; // This await point allows message processing.
    ///
    ///        // Perform the periodic task here.
    ///        println!("Periodic task executed by actor {}", _actor_ref.name());
    ///
    ///        // If your task is computationally intensive, ensure you still have an await
    ///        // or offload it (e.g., using tokio::task::spawn_blocking).
    ///        if self.heavy_computation_needed() {
    ///            // Example: Offload heavy work if truly blocking
    ///            // let _ = tokio::task::spawn_blocking(|| { /* heavy work */ }).await?;
    ///            // Or, if it's async but long-running, ensure it yields:
    ///            tokio::task::yield_now().await;
    ///        }
    ///
    ///        // Return Ok(true) to have on_run called again by the runtime for the next tick.
    ///        // To stop the actor based on some condition, return Ok(false) or an Err.
    ///        Ok(true)
    ///    }
    ///    # }
    ///    ```
    ///
    /// # Termination:
    ///
    /// The `Future` returned by `on_run` can signal actor termination in two ways when it completes:
    /// - Resolve to `Ok(false)` for normal, graceful termination.
    /// - Resolve to `Err(_)` to indicate termination due to an error condition.
    ///
    /// If the `Future` resolves to `Ok(true)`, the actor continues running, and the runtime
    /// will invoke `on_run` again to get the next `Future` for execution.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    ///
    /// If this method returns `Ok(false)` or `Err(_)`, the actor will be stopped.
    /// Returning `Ok(false)` signifies normal termination.
    /// Returning `Err(_)` signifies termination due to an error.
    /// The specific outcome is captured in the `ActorResult`.
    fn on_run(&mut self, _actor_ref: &ActorRef) -> impl Future<Output = std::result::Result<bool, Self::Error>> + Send {
        // This sleep is critical - it creates an await point that allows
        // the Tokio runtime to switch tasks and process incoming messages.
        // Without at least one await point in a loop, message processing would starve.
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(true)
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
pub trait MessageHandler: Send + 'static {
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

// This method encapsulates the actor's entire lifecycle within its spawned task.
// It handles on_start, message processing, then returns the actor result.
// Consumes self to return the actor.
async fn run_actor_lifecycle<T: Actor + MessageHandler>(
    args: T::Args,
    actor_ref: ActorRef,
    mut receiver: mpsc::Receiver<MailboxMessage>,
    mut terminate_receiver: mpsc::Receiver<ControlSignal>, // Added parameter - Changed type
) -> ActorResult<T> {
    let actor_id = actor_ref.id();

    let mut actor = match T::on_start(args, &actor_ref).await {
        Ok(actor) => {
            debug!("Actor {} on_start completed successfully.", actor_id);
            actor
        }
        Err(e) => {
            error!("Actor {} on_start failed: {:?}", actor_id, e);
            return ActorResult::StartupFailed { cause: e }
        }
    };

    debug!("Runtime for actor {} is running.", actor_id);

    let mut was_killed = false;

    // Message processing loop
    loop {
        tokio::select! {
            // Handle Terminate signal with highest priority
            biased; // Ensure Terminate is checked first if multiple conditions are ready

            maybe_signal = terminate_receiver.recv() => {
                if let Some(ControlSignal::Terminate) = maybe_signal {
                    info!("Actor {} received Terminate signal. Stopping immediately.", actor_id);
                    was_killed = true;
                    break; // Exit the loop
                } else {
                    // Channel closed or unexpected signal, this is an error state.
                    unreachable!("Actor {} terminate_receiver closed unexpectedly or received invalid signal: {:?}", actor_id, maybe_signal);
                }
            }

            // Process incoming messages from the main mailbox
            maybe_message = receiver.recv() => {
                match maybe_message {
                    Some(MailboxMessage::Envelope { payload, reply_channel }) => {
                        trace!("Actor {} received message: {:?}", actor_id, payload);
                        match actor.handle(payload, &actor_ref).await {
                            Ok(reply) => {
                                if let Some(tx) = reply_channel {
                                    if tx.send(Ok(reply)).is_err() {
                                        debug!("Actor {} failed to send reply: receiver dropped.", actor_id);
                                    }
                                }
                            }
                            Err(e) => { // e is crate::Error
                                error!("Actor {} error handling message: {:?}", actor_id, &e); // Log with a reference

                                if let Some(tx) = reply_channel {
                                    // Send the error back to the asker
                                    if tx.send(Err(e)).is_err() {
                                        debug!("Actor {} failed to send error reply: receiver dropped.", actor_id);
                                    }
                                }
                                // User send a wrong message type. So, we don't stop the actor in release mode.
                                // In debug mode, we can panic to help the developer.
                                #[cfg(debug_assertions)]
                                {
                                    panic!("Actor {} error handling message", actor_id);
                                }
                            }
                        }
                    }
                    Some(MailboxMessage::StopGracefully) => {
                        info!("Actor {} received StopGracefully. Will stop after processing current messages.", actor_id);
                        break;
                    }
                    // Terminate is handled by its own dedicated channel and select branch.
                    None => {
                        // Mailbox closed, meaning all senders (ActorRefs) are dropped.
                        // This is a form of graceful shutdown.
                        debug!("Actor {} mailbox closed (all ActorRefs dropped). Stopping.", actor_id);
                        break; // Exit loop
                    }
                }
            }

            maybe_result = actor.on_run(&actor_ref) => {
                match maybe_result {
                    Ok(keep_running) => {
                        if !keep_running {
                            // If on_run returns false, we stop the actor.
                            info!("Actor {} on_run signaled to stop.", actor_id);
                            break; // Exit loop
                        }
                    }
                    Err(e) => { // e is A::Error
                        let error_msg = format!("Actor {} on_run error: {:?}", actor_id, e);
                        error!("{}", error_msg);
                        return ActorResult::RuntimeFailed {
                            actor: Some(actor),
                            cause: e
                        };
                    }
                }
            }
        }
    }

    receiver.close(); // Close the main mailbox
    terminate_receiver.close(); // Close its own channel

    debug!("Actor {} message loop ended.", actor_id);

    // Return completed actor result
    ActorResult::Completed { actor, killed: was_killed }
}

/// Spawns a new actor and returns an `ActorRef` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's `on_start` method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result.
pub fn spawn<T: Actor + MessageHandler + 'static>(
    args: T::Args,
) -> (ActorRef, tokio::task::JoinHandle<ActorResult<T>>) {
    let capacity = CONFIGURED_DEFAULT_MAILBOX_CAPACITY.get().copied().unwrap_or(DEFAULT_MAILBOX_CAPACITY);
    spawn_with_mailbox_capacity(args, capacity)
}

/// Spawns a new actor with a specified mailbox capacity and returns an `ActorRef` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's `on_start` method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result.
pub fn spawn_with_mailbox_capacity<T: Actor + MessageHandler + 'static>(
    args: T::Args, // Actor initialization arguments
    mailbox_capacity: usize,
) -> (ActorRef, tokio::task::JoinHandle<ActorResult<T>>) {
    if mailbox_capacity == 0 {
        panic!("Mailbox capacity must be greater than 0");
    }

    let id = ACTOR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let (mailbox_tx, mailbox_rx) = mpsc::channel(mailbox_capacity);
    // Create a dedicated channel for the Terminate signal with a small capacity (e.g., 1 or 2)
    // This ensures that a kill signal can be sent even if the main mailbox is full.
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1); // Changed type

    let actor_ref = ActorRef::new(
        id,
        std::any::type_name::<T>(), // Use type name of the actor
        mailbox_tx,
        terminate_tx); // Pass terminate_tx

    let join_handle = tokio::spawn(run_actor_lifecycle(
        args,
        actor_ref.clone(),
        mailbox_rx,
        terminate_rx
    ));

    (actor_ref, join_handle)
}
