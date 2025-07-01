// Copyright 2022 Jeff Kim <hiking90@gmail./// ### Option 1: Using the Actor Derive Macro (Recommended for Simple Cases)om>
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
//! - **Macro for Message Handling**: Multiple approaches available:
//!   - [`message_handlers`] attribute macro with `#[handler]` method attributes for automatic message handling (recommended)
//!   - [`impl_message_handler!`] macro for manual message routing (deprecated, use `#[message_handlers]` instead)
//! - **Type Safety Features**: Two actor reference types provide different levels of type safety:
//!   - [`ActorRef<T>`]: Compile-time type safety with zero runtime overhead (recommended)
//!   - [`UntypedActorRef`]: Runtime type handling for collections and dynamic scenarios
//! - **Optional Tracing Support**: Built-in observability using the [`tracing`](https://crates.io/crates/tracing) crate (enable with `tracing` feature):
//!   - Actor lifecycle event tracing (start, stop, different termination scenarios)
//!   - Message handling with timing and performance metrics
//!   - Reply processing and error handling tracing
//!   - Structured, non-redundant logs for easier debugging and monitoring
//!
//! ## Core Concepts
//!
//! - **[`Actor`]**: Trait defining actor behavior and lifecycle hooks ([`on_start`](Actor::on_start) required, [`on_run`](Actor::on_run) optional).
//! - **[`Message<M>`](actor::Message)**: Trait for handling a message type `M` and defining its reply type.
//! - **[`ActorRef`]**: Handle for sending messages to an actor.
//! - **[`spawn`]**: Function to create and start an actor, returning an [`ActorRef`] and a `JoinHandle`.
//! - **[`MessageHandler`]**: Trait for type-erased message handling. This is typically implemented automatically by the [`message_handlers`] macro (recommended) or the deprecated [`impl_message_handler!`] macro.
//! - **[`ActorResult`]**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
//!
//! ## Getting Started
//!
//! ### Option A: Using Message Handlers Macro (Recommended)
//!
//! For the most concise approach, use the `#[message_handlers]` attribute macro with `#[handler]` method attributes:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, message_handlers, spawn};
//!
//! // 1. Define your actor struct and derive Actor
//! #[derive(Actor)]
//! struct MyActor {
//!     name: String,
//!     count: u32,
//! }
//!
//! // 2. Define message types
//! struct GetName;
//! struct Increment;
//!
//! // 3. Use message_handlers macro with handler attributes
//! #[message_handlers]
//! impl MyActor {
//!     #[handler]
//!     async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> String {
//!         self.name.clone()
//!     }
//!
//!     #[handler]
//!     async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> () {
//!         self.count += 1;
//!     }
//!
//!     // Regular methods can coexist without the #[handler] attribute
//!     fn get_count(&self) -> u32 {
//!         self.count
//!     }
//! }
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
//! ### Option B: Manual Message Implementation (Deprecated - Use Option A Instead)
//!
//! **⚠️ DEPRECATED**: This approach using manual `Message` trait implementation and `impl_message_handler!`
//! is deprecated and will be removed in version 1.0. Please use the `#[message_handlers]` macro approach shown in Option A instead.
//!
//! For cases where you need more control over message handling, you can manually implement the Message trait:
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
//! Both approaches also work with enums, making it easy to create state machine actors:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, message_handlers, spawn};
//!
//! // Using message_handlers macro approach
//! #[derive(Actor, Clone)]
//! enum StateActor {
//!     Idle,
//!     Processing(String),
//!     Completed(i32),
//! }
//!
//! struct GetState;
//! struct StartProcessing(String);
//! struct Complete(i32);
//!
//! #[message_handlers]
//! impl StateActor {
//!     #[handler]
//!     async fn handle_get_state(&mut self, _msg: GetState, _: &ActorRef<Self>) -> StateActor {
//!         self.clone()
//!     }
//!
//!     #[handler]
//!     async fn handle_start_processing(&mut self, msg: StartProcessing, _: &ActorRef<Self>) -> () {
//!         *self = StateActor::Processing(msg.0);
//!     }
//!
//!     #[handler]
//!     async fn handle_complete(&mut self, msg: Complete, _: &ActorRef<Self>) -> () {
//!         *self = StateActor::Completed(msg.0);
//!     }
//! }
//! ```
//!
//! ### Option C: Manual Actor Implementation (Advanced Usage - Consider Using Option A)
//!
//! **Note**: While this approach is still supported, consider using the `#[message_handlers]` macro
//! for message handling even with custom Actor implementations.
//!
//! For actors that need complex initialization logic, implement the Actor trait manually:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, ActorWeak, message_handlers, spawn};
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
//!     async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
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
//! // 4. Use message_handlers macro for message handling (recommended approach)
//! #[message_handlers]
//! impl MyActor {
//!     #[handler]
//!     async fn handle_get_data(&mut self, _msg: GetData, _actor_ref: &ActorRef<Self>) -> String {
//!         self.data.clone()
//!     }
//!
//!     #[handler]
//!     async fn handle_update_data(&mut self, msg: UpdateData, _actor_ref: &ActorRef<Self>) -> () {
//!         self.data = msg.0;
//!         println!("MyActor data updated!");
//!     }
//! }
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
//! ## Tracing Support
//!
//! rsActor provides optional tracing support for comprehensive observability. Enable it with the `tracing` feature:
//!
//! ```toml
//! [dependencies]
//! rsactor = { version = "0.9", features = ["tracing"] }
//! tracing = "0.1"
//! tracing-subscriber = "0.3"
//! ```
//!
//! When enabled, rsActor emits structured trace events for:
//! - Actor lifecycle events (start, stop, termination scenarios)
//! - Message sending and handling with timing information
//! - Reply processing and error handling
//! - Performance metrics (message processing duration)
//!
//! All examples support tracing with conditional compilation. Here's the integration pattern:
//!
//! ```rust,no_run
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize tracing if the feature is enabled
//!     #[cfg(feature = "tracing")]
//!     {
//!         tracing_subscriber::fmt()
//!             .with_max_level(tracing::Level::DEBUG)
//!             .with_target(false)
//!             .init();
//!         println!("🚀 Demo: Tracing is ENABLED");
//!     }
//!
//!     #[cfg(not(feature = "tracing"))]
//!     {
//!         env_logger::init();
//!         println!("📝 Demo: Tracing is DISABLED");
//!     }
//!
//!     // Your existing actor code here...
//!     // When tracing is enabled, you'll see detailed logs automatically
//!     Ok(())
//! }
//! ```
//!
//! Run any example with tracing enabled:
//! ```bash
//! RUST_LOG=debug cargo run --example basic --features tracing
//! ```
//!
//! This crate-level documentation provides an overview of [`rsActor`](crate).
//! For more details on specific components, please refer to their individual
//! documentation.

mod error;
pub use error::{Error, Result};

mod actor_ref;
pub use actor_ref::{ActorRef, ActorWeak};

mod actor_result;
pub use actor_result::{ActorResult, FailurePhase};

mod actor;
pub use actor::{Actor, Message};

use futures::FutureExt;
// Re-export derive macro
pub use rsactor_derive::{message_handlers, Actor};

use std::{any::TypeId, fmt::Debug, future::Future, sync::OnceLock};

use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// Unique ID of the actor
    pub id: TypeId,
    /// Type name of the actor
    pub type_name: &'static str,
}

impl Identity {
    /// Creates a new `Identity` for the specified type T.
    /// This is a generic function that automatically infers the type ID and type name.
    pub fn of<T: 'static>() -> Self {
        Identity {
            id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
        }
    }

    /// Returns a string representation of the actor's identity.
    pub fn name(&self) -> &'static str {
        self.type_name
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_name)
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
#[deprecated(
    since = "0.9.0",
    note = "This is an internal helper for the deprecated `impl_message_handler!` macro. Will be removed in version 1.0."
)]
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

                        #[cfg(feature = "tracing")]
                        {
                            // Update the current span with the actual message type
                            let current_span = tracing::Span::current();
                            current_span.record("message_type", std::any::type_name::<$msg_type>());

                            tracing::debug!(
                                message_type = %std::any::type_name::<$msg_type>(),
                                "Actor handler processing message"
                            );
                        }

                        let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *concrete_msg_box, &actor_ref).await;

                        #[cfg(feature = "tracing")]
                        tracing::debug!(
                            message_type = %std::any::type_name::<$msg_type>(),
                            "Actor handler completed message processing"
                        );

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

            #[cfg(feature = "tracing")]
            tracing::warn!(
                expected_types = ?expected_msg_types,
                actual_type_id = ?_msg_any.type_id(),
                "Unhandled message type"
            );

            return Err($crate::Error::UnhandledMessageType {
                identity: actor_ref.identity(),
                expected_types: expected_msg_types,
                actual_type_id: _msg_any.type_id()
            });
        }
    };
}

/// Type-erased payload handler trait that can be used with dynamic dispatch
trait PayloadHandler<A>: Send
where
    A: Actor,
{
    /// Process the message payload and return a result
    fn handle_message(
        self: Box<Self>,
        actor: &mut A,
        actor_ref: ActorRef<A>,
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
    ) -> BoxFuture<'_, ()>;
}

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl<A, T> PayloadHandler<A> for T
where
    A: Actor + Message<T> + 'static,
    T: Send + 'static,
{
    fn handle_message(
        self: Box<Self>,
        actor: &mut A,
        actor_ref: ActorRef<A>,
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
    ) -> BoxFuture<'_, ()> {
        async move {
            let result = Message::handle(actor, *self, &actor_ref).await;
            if let Some(channel) = reply_channel {
                match channel.send(Box::new(result)) {
                    Ok(_) => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(
                            actor = %actor_ref.identity(),
                            "Reply sent successfully"
                        );
                    }
                    Err(_) => {
                        #[cfg(feature = "tracing")]
                        tracing::error!(
                            actor = %actor_ref.identity(),
                            error = ?e,
                            "Failed to send reply"
                        );
                        log::error!(
                            "Failed to send reply for actor {}: {}",
                            std::any::type_name::<A>(),
                            std::any::type_name::<T>()
                        );
                    }
                }
            }
        }
        .boxed()
    }
}

/// Represents messages that can be sent to an actor's mailbox.
///
/// This enum includes both user-defined messages (wrapped in `Envelope`)
/// and control messages like `StopGracefully`. The `Terminate` control signal
/// is handled through a separate dedicated channel.
pub(crate) enum MailboxMessage<T>
where
    T: Actor + 'static,
{
    // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
    /// A user-defined message to be processed by the actor.
    Envelope {
        /// The message payload.
        payload: Box<dyn PayloadHandler<T>>,
        /// A channel to send the reply back to the caller. Optional for 'tell' operations.
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
        actor_ref: ActorRef<T>, // The actor reference that sent the message
    },
    // Terminate is removed from here
    /// A signal for the actor to stop gracefully after processing existing messages in its mailbox.
    #[allow(dead_code)] // To avoid dropping ActorRef until StopGracefully is delivered.
    StopGracefully(ActorRef<T>),
}

/// Represents control signals that can be sent to an actor.
#[derive(Debug)]
pub(crate) enum ControlSignal {
    // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
    /// A signal for the actor to terminate immediately.
    Terminate,
}

// Type alias for the sender part of the actor's mailbox channel.
pub(crate) type MailboxSender<T> = mpsc::Sender<MailboxMessage<T>>; // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.

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
pub fn spawn<T: Actor + 'static>(
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
pub fn spawn_with_mailbox_capacity<T: Actor + 'static>(
    args: T::Args, // Actor initialization arguments
    mailbox_capacity: usize,
) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    if mailbox_capacity == 0 {
        panic!("Mailbox capacity must be greater than 0");
    }

    let (mailbox_tx, mailbox_rx) = mpsc::channel(mailbox_capacity);
    // Create a dedicated channel for the Terminate signal with a small capacity (e.g., 1 or 2)
    // This ensures that a kill signal can be sent even if the main mailbox is full.
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1); // Changed type

    let actor_ref = ActorRef::new(mailbox_tx, terminate_tx); // Pass terminate_tx

    let join_handle = tokio::spawn(crate::actor::run_actor_lifecycle(
        args,
        actor_ref.clone(),
        mailbox_rx,
        terminate_rx,
    ));

    (actor_ref, join_handle)
}
