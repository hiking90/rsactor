// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # rsActor
//! A Simple and Efficient In-Process Actor Model Implementation for Rust
//!
//! `rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing a simple
//! and efficient actor model for local, in-process systems. It emphasizes clean message-passing
//! semantics and straightforward actor lifecycle management while maintaining high performance for
//! Rust applications.
//!
//! ## Features
//!
//! - **Asynchronous Actors**: Actors run in their own asynchronous tasks.
//! - **Message Passing**: Actors communicate by sending and receiving messages.
//!   - [`tell`](actor_ref::ActorRef::tell): Send a message without waiting for a reply (fire-and-forget).
//!   - [`tell_with_timeout`](actor_ref::ActorRef::tell_with_timeout): Send a message without waiting for a reply, with a specified timeout.
//!   - [`ask`](actor_ref::ActorRef::ask): Send a message and await a reply.
//!   - [`ask_with_timeout`](actor_ref::ActorRef::ask_with_timeout): Send a message and await a reply, with a specified timeout.
//!   - [`tell_blocking`](actor_ref::ActorRef::tell_blocking): Blocking version of `tell` for use in [`tokio::task::spawn_blocking`] tasks.
//!   - [`ask_blocking`](actor_ref::ActorRef::ask_blocking): Blocking version of `ask` for use in [`tokio::task::spawn_blocking`] tasks.
//! - **Straightforward Actor Lifecycle**: Actors have [`on_start`](Actor::on_start), [`on_run`](Actor::on_run),
//!   and [`on_stop`](Actor::on_stop) lifecycle hooks that provide a clean and intuitive actor lifecycle management system.
//!   The framework manages the execution flow while giving developers full control over actor behavior.
//! - **Graceful Shutdown & Kill**: Actors can be stopped gracefully or killed immediately.
//! - **Typed Messages**: Messages are strongly typed, and replies are also typed.
//! - **Macro for Message Handling**: The [`impl_message_handler!`] macro simplifies
//!   handling multiple message types.
//! - **Type Safety Features**: Two actor reference types provide different levels of type safety:
//!   - [`ActorRef<T>`]: Compile-time type safety with zero runtime overhead (recommended)
//!   - [`UntypedActorRef`]: Runtime type handling for collections and dynamic scenarios
//!
//! ## Core Concepts
//!
//! - **[`Actor`]**: Trait defining actor behavior and lifecycle hooks ([`on_start`](Actor::on_start) required, [`on_run`](Actor::on_run) optional).
//! - **[`Message<M>`](actor::Message)**: Trait for handling a message type `M` and defining its reply type.
//! - **[`ActorRef`]**: Handle for sending messages to an actor.
//! - **[`spawn`]**: Function to create and start an actor, returning an [`ActorRef`] and a `JoinHandle`.
//! - **[`MessageHandler`]**: Trait for type-erased message handling. This is typically implemented automatically by the [`impl_message_handler!`] macro.
//! - **[`ActorResult`]**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
//!
//! ## Getting Started
//!
//! ### Option 1: Using the Actor Derive Macro (Recommended for Simple Cases)
//!
//! For simple actors that don't need custom initialization logic, you can use the `#[derive(Actor)]` macro:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
//!
//! // 1. Define your actor struct and derive Actor
//! #[derive(Actor)]
//! struct MyActor {
//!     name: String,
//!     count: u32,
//! }
//!
//! // 2. Define message types and implement Message<M> for each
//! struct GetName;
//! struct Increment;
//!
//! impl Message<GetName> for MyActor {
//!     type Reply = String;
//!     async fn handle(&mut self, _msg: GetName, _actor_ref: &ActorRef<Self>) -> Self::Reply {
//!         self.name.clone()
//!     }
//! }
//!
//! impl Message<Increment> for MyActor {
//!     type Reply = ();
//!     async fn handle(&mut self, _msg: Increment, _actor_ref: &ActorRef<Self>) -> Self::Reply {
//!         self.count += 1;
//!     }
//! }
//!
//! // 3. Wire up message handlers
//! impl_message_handler!(MyActor, [GetName, Increment]);
//!
//! // 4. Usage
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let actor_instance = MyActor { name: "Test".to_string(), count: 0 };
//! let (actor_ref, _join_handle) = spawn::<MyActor>(actor_instance);
//!
//! let name = actor_ref.ask(GetName).await?;
//! actor_ref.tell(Increment).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Option 2: Manual Implementation (For Complex Initialization)
//!
//! For actors that need complex initialization logic, implement the Actor trait manually:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
//! use anyhow::Result;
//!
//! // 1. Define your actor struct
//! #[derive(Debug)]
//! struct MyActor {
//!     data: String,
//!     tick_300ms: tokio::time::Interval,
//!     tick_1s: tokio::time::Interval,
//! }
//!
//! // 2. Implement the Actor trait manually
//! impl Actor for MyActor {
//!     type Args = String;
//!     type Error = anyhow::Error;
//!
//!     async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
//!         println!("MyActor (data: '{}') started!", args);
//!         Ok(MyActor {
//!             data: args,
//!             tick_300ms: tokio::time::interval(std::time::Duration::from_millis(300)),
//!             tick_1s: tokio::time::interval(std::time::Duration::from_secs(1)),
//!         })
//!     }
//!
//!     async fn on_run(&mut self, _actor_ref: &ActorRef<Self>) -> Result<(), Self::Error> {
//!         tokio::select! {
//!             _ = self.tick_300ms.tick() => {
//!                 println!("Tick: 300ms");
//!             }
//!             _ = self.tick_1s.tick() => {
//!                 println!("Tick: 1s");
//!             }
//!         }
//!         Ok(())
//!     }
//! }
//!
//! // 3. Define message types
//! struct GetData;
//! struct UpdateData(String);
//!
//! // 4. Implement Message<M> for each message type
//! impl Message<GetData> for MyActor {
//!     type Reply = String;
//!
//!     async fn handle(&mut self, _msg: GetData, _actor_ref: &ActorRef<Self>) -> Self::Reply {
//!         self.data.clone()
//!     }
//! }
//!
//! impl Message<UpdateData> for MyActor {
//!     type Reply = ();
//!
//!     async fn handle(&mut self, msg: UpdateData, _actor_ref: &ActorRef<Self>) -> Self::Reply {
//!         self.data = msg.0;
//!         println!("MyActor data updated!");
//!     }
//! }
//!
//! // 5. Use the macro to implement MessageHandler
//! impl_message_handler!(MyActor, [GetData, UpdateData]);
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let (actor_ref, join_handle) = spawn::<MyActor>("initial data".to_string());
//!
//!     // Send messages
//!     let current_data: String = actor_ref.ask(GetData).await?;
//!     println!("Received data: {}", current_data);
//!
//!     actor_ref.tell(UpdateData("new data".to_string())).await?;
//!
//!     let updated_data: String = actor_ref.ask(GetData).await?;
//!     println!("Updated data: {}", updated_data);
//!
//!     // Stop the actor
//!     actor_ref.stop().await?;
//!     let actor_result = join_handle.await?;
//!     println!("Actor stopped with result: {:?}", actor_result);
//!
//!     Ok(())
//! }
//! ```
//!
//! This crate-level documentation provides an overview of [`rsActor`](crate).
//! For more details on specific components, please refer to their individual
//! documentation.

mod error;
pub use error::{Error, Result};

mod actor_ref;
pub use actor_ref::{ActorRef, UntypedActorRef};

mod actor_result;
pub use actor_result::{ActorResult, FailurePhase};

mod actor;
pub use actor::{Actor, Message, MessageHandler};

// Re-export derive macro
pub use rsactor_derive::Actor;

use std::{
    any::Any,
    fmt::Debug,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// Unique ID of the actor
    pub id: usize,
    /// Type name of the actor
    pub type_name: &'static str,
}

impl Identity {
    /// Creates a new `ActorIdentity` with the given ID and type name.
    pub fn new(id: usize, type_name: &'static str) -> Self {
        Identity { id, type_name }
    }

    /// Returns a string representation of the actor's identity.
    pub fn name(&self) -> String {
        format!("{}#{}", self.type_name, self.id)
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.type_name, self.id)
    }
}

/// Internal helper macro that generates the common message handling logic
/// shared between generic and non-generic `impl_message_handler!` implementations.
///
/// This macro eliminates code duplication by providing the core message handling
/// logic that downcasts incoming messages and dispatches them to the appropriate
/// `Message::handle` implementation.
///
/// # Parameters
/// * `$actor_type:ty`: The actor type for which to generate the handler
/// * `[$($msg_type:ty),* $(,)?]`: List of message types to handle
///
/// # Generated Code
/// Creates an `async fn handle` method that:
/// 1. Attempts to downcast the incoming `Box<dyn Any + Send>` to each message type
/// 2. Calls the appropriate `Message::handle` implementation when a match is found
/// 3. Returns an error if no message type matches
#[macro_export]
#[doc(hidden)]
macro_rules! __impl_message_handler_body {
    ($actor_type:ty, [$($msg_type:ty),* $(,)?]) => {
        async fn handle(
            &mut self,
            _msg_any: Box<dyn std::any::Any + Send>, // This Box is consumed by the first successful downcast
            actor_ref: &$crate::ActorRef<$actor_type>,
        ) -> $crate::Result<Box<dyn std::any::Any + Send>> {
            let mut _msg_any = _msg_any; // Mutable to allow reassignment in the loop
            $(
                match _msg_any.downcast::<$msg_type>() {
                    Ok(concrete_msg_box) => {
                        // Successfully downcasted. concrete_msg_box is a Box<$msg_type>.
                        // The original _msg_any has been consumed by the downcast.
                        let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *concrete_msg_box, &actor_ref).await;
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
            let expected_msg_types: Vec<&'static str> = vec![$(stringify!($msg_type)),*];
            return Err($crate::Error::UnhandledMessageType {
                identity: actor_ref.identity(),
                expected_types: expected_msg_types,
                actual_type_id: _msg_any.type_id()
            });
        }
    };
}

/// Implements the `MessageHandler` trait for both generic and non-generic actor types.
///
/// This macro simplifies the process of handling multiple message types within an actor.
/// It generates the necessary boilerplate code to downcast a `Box<dyn Any + Send>`
/// message to its concrete type and then calls the appropriate `Message::handle`
/// implementation on the actor.
///
/// # Usage
///
/// ## For non-generic actors:
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
///     async fn handle(&mut self, msg: Msg1, _actor_ref: &ActorRef<Self>) -> Self::Reply { /* ... */ }
/// }
///
/// impl Message<Msg2> for MyActor {
///     type Reply = String;
///     async fn handle(&mut self, msg: Msg2, _actor_ref: &ActorRef<Self>) -> Self::Reply {
///         /* ... */ "response".to_string()
///     }
/// }
///
/// // This will implement `MessageHandler` for `MyActor`, allowing it to handle `Msg1` and `Msg2`.
/// impl_message_handler!(MyActor, [Msg1, Msg2]);
/// ```
///
/// ## For generic actors:
/// ```rust,ignore
/// struct GenericActor<T: Send + Debug + Clone + 'static> {
///     data: Option<T>,
/// }
///
/// impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> { /* ... */ }
///
/// struct SetValue<T: Send + Debug + 'static>(T);
/// struct GetValue;
///
/// impl<T: Send + Debug + Clone + 'static> Message<SetValue<T>> for GenericActor<T> {
///     type Reply = ();
///     async fn handle(&mut self, msg: SetValue<T>, _actor_ref: &ActorRef<Self>) -> Self::Reply { /* ... */ }
/// }
///
/// impl<T: Send + Debug + Clone + 'static> Message<GetValue> for GenericActor<T> {
///     type Reply = Option<T>;
///     async fn handle(&mut self, msg: GetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply { /* ... */ }
/// }
///
/// // This will implement `MessageHandler` for the generic `GenericActor<T>`.
/// impl_message_handler!([T: Send + Debug + Clone + 'static] for GenericActor<T>, [SetValue<T>, GetValue]);
/// ```
///
/// # Arguments
///
/// ## Non-generic form:
/// * `$actor_type:ty`: The type of the actor for which to implement `MessageHandler`.
/// * `[$($msg_type:ty),* $(,)?]`: A list of message types that the actor can handle.
/// ```ignore
/// impl_message_handler!(MyActor, [Msg1, Msg2]);
/// ```
///
/// ## Generic form:
/// * `[$($generics:tt)*]`: Generic type parameters with their trait bounds (as token tree to handle complex bounds)
/// * `for $actor_type:ty`: The generic actor type for which to implement `MessageHandler`
/// * `[$($msg_type:ty),* $(,)?]`: A list of message types that the actor can handle
/// ```ignore
/// impl_message_handler!([T: Send + Debug + Clone + 'static] for GenericActor<T>, [SetValue<T>, GetValue]);
/// ```
///
/// # Internals
/// This macro facilitates dynamic message dispatch by downcasting `Box<dyn std::any::Any + Send>`
/// message payloads to their concrete types at runtime. It implements the [`MessageHandler`](crate::actor::MessageHandler)
/// trait for your actor, enabling it to handle multiple message types through the
/// [`handle`](crate::actor::Message::handle) method. The actual message handling logic
/// is generated by the internal `__impl_message_handler_body!` helper macro to avoid code
/// duplication between different implementation patterns.
#[macro_export]
macro_rules! impl_message_handler {
    // Generic actor pattern: [generics] for ActorType<T>, [messages]
    ([$($generics:tt)*] for $actor_type:ty, [$($msg_type:ty),* $(,)?]) => {
        impl<$($generics)*> $crate::MessageHandler for $actor_type {
            $crate::__impl_message_handler_body!($actor_type, [$($msg_type),*]);
        }
    };

    // Non-generic actor pattern: ActorType, [messages]
    ($actor_type:ty, [$($msg_type:ty),* $(,)?]) => {
        impl $crate::MessageHandler for $actor_type {
            $crate::__impl_message_handler_body!($actor_type, [$($msg_type),*]);
        }
    };
}

/// Represents messages that can be sent to an actor's mailbox.
///
/// This enum includes both user-defined messages (wrapped in `Envelope`)
/// and control messages like `StopGracefully`. The `Terminate` control signal
/// is handled through a separate dedicated channel.
#[derive(Debug)]
pub(crate) enum MailboxMessage {
    // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
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
pub(crate) enum ControlSignal {
    // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
    /// A signal for the actor to terminate immediately.
    Terminate,
}

// Type alias for the sender part of the actor's mailbox channel.
pub(crate) type MailboxSender = mpsc::Sender<MailboxMessage>; // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.

// Counter for generating unique actor IDs.
static ACTOR_ID: AtomicUsize = AtomicUsize::new(1);

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

/// Spawns a new actor and returns an `ActorRef<T>` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's [`on_start`](crate::Actor::on_start) method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result as an [`ActorResult<T>`](crate::ActorResult).
pub fn spawn<T: Actor + MessageHandler + 'static>(
    args: T::Args,
) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    let capacity = CONFIGURED_DEFAULT_MAILBOX_CAPACITY
        .get()
        .copied()
        .unwrap_or(DEFAULT_MAILBOX_CAPACITY);
    spawn_with_mailbox_capacity(args, capacity)
}

/// Spawns a new actor with a specified mailbox capacity and returns an `ActorRef<T>` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's [`on_start`](crate::Actor::on_start) method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result as an [`ActorResult<T>`](crate::ActorResult). Use this version when you need
/// to control the actor's mailbox capacity.
pub fn spawn_with_mailbox_capacity<T: Actor + MessageHandler + 'static>(
    args: T::Args, // Actor initialization arguments
    mailbox_capacity: usize,
) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    if mailbox_capacity == 0 {
        panic!("Mailbox capacity must be greater than 0");
    }

    let id = ACTOR_ID.fetch_add(1, Ordering::Relaxed);
    let (mailbox_tx, mailbox_rx) = mpsc::channel(mailbox_capacity);
    // Create a dedicated channel for the Terminate signal with a small capacity (e.g., 1 or 2)
    // This ensures that a kill signal can be sent even if the main mailbox is full.
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1); // Changed type

    let untyped_actor_ref = UntypedActorRef::new(
        Identity::new(id, std::any::type_name::<T>()), // Use type name of the actor
        mailbox_tx,
        terminate_tx,
    ); // Pass terminate_tx

    let actor_ref = ActorRef::new(untyped_actor_ref);

    let join_handle = tokio::spawn(crate::actor::run_actor_lifecycle(
        args,
        actor_ref.clone(),
        mailbox_rx,
        terminate_rx,
    ));

    (actor_ref, join_handle)
}
