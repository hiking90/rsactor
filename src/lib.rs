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
//! - **Macro for Message Handling**:
//!   - [`message_handlers`] attribute macro with `#[handler]` method attributes for automatic message handling (recommended)
//! - **Type Safety Features**: [`ActorRef<T>`] provides compile-time type safety with zero runtime overhead
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
//! - **[`ActorResult`]**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
//!
//! ## Getting Started
//!
//! ### Message Handling with `#[message_handlers]`
//!
//! rsActor uses the `#[message_handlers]` attribute macro combined with `#[handler]` method attributes
//! for message handling. This is **required** for all actors and offers several advantages:
//!
//! - **Selective Processing**: Only methods marked with `#[handler]` are treated as message handlers.
//! - **Clean Separation**: Regular methods can coexist with message handlers within the same `impl` block.
//! - **Automatic Generation**: The macro automatically generates the necessary `Message` trait implementations and handler registrations.
//! - **Type Safety**: Message handler signatures are verified at compile time.
//! - **Reduced Boilerplate**: Eliminates the need to manually implement `Message` traits.
//!
//! ### Option A: Simple Actor with `#[derive(Actor)]`
//!
//! For simple actors that don't need complex initialization logic, use the `#[derive(Actor)]` macro:
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
//! ### Option B: Custom Actor Implementation with Manual Initialization
//!
//! For actors that need custom initialization logic, implement the `Actor` trait manually:
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, message_handlers, spawn};
//! use anyhow::Result;
//!
//! // 1. Define your actor struct
//! #[derive(Debug)] // Added Debug for printing the actor in ActorResult
//! struct MyActor {
//!     data: String,
//!     count: u32,
//! }
//!
//! // 2. Implement the Actor trait manually
//! impl Actor for MyActor {
//!     type Args = String;
//!     type Error = anyhow::Error;
//!
//!     // on_start is required and must be implemented.
//!     // on_run and on_stop are optional and have default implementations.
//!     async fn on_start(initial_data: Self::Args, actor_ref: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
//!         println!("MyActor (id: {}) started with data: '{}'", actor_ref.identity(), initial_data);
//!         Ok(MyActor {
//!             data: initial_data,
//!             count: 0,
//!         })
//!     }
//! }
//!
//! // 3. Define message types
//! struct GetData;
//! struct IncrementMsg(u32);
//!
//! // 4. Use message_handlers macro for message handling
//! #[message_handlers]
//! impl MyActor {
//!     #[handler]
//!     async fn handle_get_data(&mut self, _msg: GetData, _actor_ref: &ActorRef<Self>) -> String {
//!         self.data.clone()
//!     }
//!
//!     #[handler]
//!     async fn handle_increment(&mut self, msg: IncrementMsg, _actor_ref: &ActorRef<Self>) -> u32 {
//!         self.count += msg.0;
//!         self.count
//!     }
//! }
//!
//! // 5. Usage
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (actor_ref, join_handle) = spawn::<MyActor>("initial data".to_string());
//!
//! let current_data: String = actor_ref.ask(GetData).await?;
//! let new_count: u32 = actor_ref.ask(IncrementMsg(5)).await?;
//!
//! actor_ref.stop().await?;
//! let actor_result = join_handle.await?;
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

mod handler;
pub use handler::{AskHandler, TellHandler, WeakAskHandler, WeakTellHandler};

mod actor_control;
pub use actor_control::{ActorControl, WeakActorControl};

use futures::FutureExt;
// Re-export derive macros for convenient access
pub use rsactor_derive::{message_handlers, Actor};

use std::{fmt::Debug, future::Future, sync::atomic::AtomicU32, sync::OnceLock};

use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// Unique ID of the actor
    pub id: u32,
    /// Type name of the actor
    pub type_name: &'static str,
}

impl Identity {
    /// Creates a new `Identity` with the given ID and type name.
    pub fn new(id: u32, type_name: &'static str) -> Self {
        Identity { id, type_name }
    }

    /// Returns the type name of the actor.
    pub fn name(&self) -> &'static str {
        self.type_name
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_name)
    }
}

/// Type-erased payload handler trait for dynamic message dispatch.
///
/// This trait allows different message types to be handled uniformly within the actor system,
/// enabling storage of various message types in the same mailbox while preserving type safety
/// through the `Message` trait implementation.
trait PayloadHandler<A>: Send
where
    A: Actor,
{
    /// Handles the message by calling the appropriate handler and optionally sending a reply.
    ///
    /// # Parameters
    /// - `actor`: Mutable reference to the actor instance
    /// - `actor_ref`: Reference to the actor for potential self-messaging
    /// - `reply_channel`: Optional channel to send the result back for `ask` operations
    fn handle_message(
        self: Box<Self>,
        actor: &mut A,
        actor_ref: ActorRef<A>,
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
    ) -> BoxFuture<'_, ()>;
}

/// A boxed future that is Send and can be stored in collections.
///
/// This type alias is used throughout the handler traits for object-safe async methods.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
                            "Failed to send reply - receiver dropped"
                        );
                        log::error!(
                            "Failed to send reply for actor {}: {} - receiver dropped",
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
    /// A user-defined message to be processed by the actor.
    Envelope {
        /// The message payload containing the actual message data.
        payload: Box<dyn PayloadHandler<T>>,
        /// Optional channel to send the reply back to the caller (used for `ask` operations).
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
        /// The actor reference for potential self-messaging or context.
        actor_ref: ActorRef<T>,
    },
    /// A signal for the actor to stop gracefully after processing existing messages in its mailbox.
    ///
    /// The contained `ActorRef<T>` prevents the actor from being dropped until this message is processed.
    #[allow(dead_code)]
    StopGracefully(ActorRef<T>),
}

/// Control signals sent through a dedicated high-priority channel.
///
/// These signals are processed with higher priority than regular mailbox messages
/// to ensure timely actor termination even when the mailbox is full.
#[derive(Debug)]
pub(crate) enum ControlSignal {
    /// A signal for the actor to terminate immediately without processing remaining mailbox messages.
    Terminate,
}

/// Type alias for the sender side of an actor's mailbox channel.
///
/// This is used by `ActorRef` to send messages to the actor's mailbox.
pub(crate) type MailboxSender<T> = mpsc::Sender<MailboxMessage<T>>;

/// Global configuration for the default mailbox capacity.
///
/// This value can be set once using `set_default_mailbox_capacity()` and will be used
/// by the `spawn()` function when no specific capacity is provided.
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

    static ACTOR_IDS: AtomicU32 = AtomicU32::new(1);

    let actor_id = Identity::new(
        ACTOR_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        std::any::type_name::<T>(),
    );

    let (mailbox_tx, mailbox_rx) = mpsc::channel(mailbox_capacity);

    // Create a dedicated high-priority channel for terminate signals.
    // This ensures that kill signals can be sent even when the main mailbox is full,
    // allowing for immediate actor termination regardless of mailbox state.
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1);

    let actor_ref = ActorRef::new(actor_id, mailbox_tx, terminate_tx);

    let join_handle = tokio::spawn(crate::actor::run_actor_lifecycle(
        args,
        actor_ref.clone(),
        mailbox_rx,
        terminate_rx,
    ));

    (actor_ref, join_handle)
}
