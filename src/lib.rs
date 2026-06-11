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
//!   - [`blocking_tell`](actor_ref::ActorRef::blocking_tell): Blocking version of `tell` for use in [`tokio::task::spawn_blocking`] tasks or non-async threads.
//!   - [`blocking_ask`](actor_ref::ActorRef::blocking_ask): Blocking version of `ask` for use in [`tokio::task::spawn_blocking`] tasks or non-async threads.
//! - **Priority Channel** (opt-in via [`SpawnOptions::with_priority`]):
//!   A dedicated mpsc channel of fixed capacity 1 that the runtime polls with
//!   higher priority than the regular mailbox but lower priority than the
//!   `kill()` (terminate signal). Use it for short, infrequent control messages
//!   such as health checks and pause/resume. Send via
//!   [`tell_priority`](actor_ref::ActorRef::tell_priority) /
//!   [`ask_priority`](actor_ref::ActorRef::ask_priority). The priority channel
//!   is **off by default**; calls on a non-priority actor return
//!   [`Error::PriorityChannelNotEnabled`].
//! - **Straightforward Actor Lifecycle**: Actors have [`on_start`](Actor::on_start), [`on_idle`](Actor::on_idle),
//!   and [`on_stop`](Actor::on_stop) lifecycle hooks. Idle work is driven by streams subscribed via
//!   [`ActorRef::subscribe_idle`](actor_ref::ActorRef::subscribe_idle) — each yielded event is dispatched to
//!   `on_idle` with `&mut self`, so timer / channel state never gets cancelled by a competing `select!` arm.
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
//! - **Dead Letter Tracking**: Automatic logging of undelivered messages via [`DeadLetterReason`]:
//!   - All failed message deliveries are logged with actor and message type information
//!   - Helps identify stopped actors, timeouts, and dropped replies
//!   - Zero overhead on successful message delivery (hot path optimization)
//! - **Enhanced Error Debugging**: Rich error information via [`Error::debugging_tips()`](Error::debugging_tips) and [`Error::is_retryable()`](Error::is_retryable):
//!   - Actionable debugging tips for each error type
//!   - Retry classification for timeout errors
//!
//! ## Core Concepts
//!
//! - **[`Actor`]**: Trait defining actor behavior and lifecycle hooks ([`on_start`](Actor::on_start) required, [`on_idle`](Actor::on_idle) optional).
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
//!     type IdleEvent = ();
//!
//!     // on_start is required and must be implemented.
//!     // on_idle and on_stop are optional and have default implementations.
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
//! actor_ref.stop().await;
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
//! rsactor = { version = "0.16", features = ["tracing"] }
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
//! All examples support tracing. Here's the integration pattern:
//!
//! ```rust,no_run
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize tracing subscriber to see logs
//!     // The `tracing` crate is always available for logging
//!     tracing_subscriber::fmt()
//!         .with_max_level(tracing::Level::DEBUG)
//!         .with_target(false)
//!         .init();
//!
//!     // Your existing actor code here...
//!     // Logs are automatically emitted via tracing::warn!, tracing::error!, etc.
//!     Ok(())
//! }
//! ```
//!
//! Run any example with debug logging:
//! ```bash
//! RUST_LOG=debug cargo run --example basic
//! ```
//!
//! Enable instrumentation spans with the `tracing` feature:
//! ```bash
//! RUST_LOG=debug cargo run --example basic --features tracing
//! ```
//!
//! This crate-level documentation provides an overview of [`rsActor`](crate).
//! For more details on specific components, please refer to their individual
//! documentation.

mod error;
pub use error::{Error, Result};

mod dead_letter;
pub use dead_letter::DeadLetterReason;

// Re-export test utilities when test-utils feature is enabled
#[cfg(any(test, feature = "test-utils"))]
pub use dead_letter::{dead_letter_count, reset_dead_letter_count};

#[cfg(feature = "metrics")]
mod metrics;
#[cfg(feature = "metrics")]
pub use metrics::MetricsSnapshot;

mod actor_ref;
pub use actor_ref::{ActorRef, ActorWeak};

mod actor_result;
pub use actor_result::{ActorFailure, ActorResult, FailurePhase};

mod actor;
pub use actor::{Actor, Message};

mod handler;
pub use handler::{AskHandler, TellHandler, WeakAskHandler, WeakTellHandler};

mod actor_control;
pub use actor_control::{ActorControl, WeakActorControl};

// Runtime deadlock detection lives in its own module, gated entirely by the
// feature. `CURRENT_ACTOR` and `register_ask_edge` are re-exported at the crate
// root so existing `crate::`-qualified references keep working unchanged.
#[cfg(feature = "deadlock-detection")]
mod deadlock;
#[cfg(feature = "deadlock-detection")]
pub(crate) use deadlock::{register_ask_edge, CURRENT_ACTOR};

/// Slot type for the deadlock-detection token carried by ask envelopes (see
/// [`deadlock::AskEdgeToken`]). Defined as `()` when the feature is off so
/// envelope construction and destructuring read identically in both builds —
/// `Default::default()` produces the empty slot either way.
#[cfg(feature = "deadlock-detection")]
pub(crate) type AskEdgeSlot = Option<deadlock::AskEdgeToken>;
/// Feature-off stand-in for the deadlock-detection envelope slot.
#[cfg(not(feature = "deadlock-detection"))]
pub(crate) type AskEdgeSlot = ();

use futures::FutureExt;
// Re-export derive macros for convenient access
pub use rsactor_derive::{message_handlers, Actor};

/// Internal function used by derive macros to log handler errors.
///
/// Surfaces a user handler's `Result::Err` value through `tracing::error!`. The
/// `tracing` crate is always a dependency — the `tracing` *feature* only gates
/// `#[tracing::instrument]` spans, not logging — so this routes through the user's
/// configured subscriber unconditionally, exactly like dead-letter logging. Keying
/// the channel on the `tracing` feature instead would be wrong: the feature is
/// unrelated to whether a subscriber exists, so it could send handler errors to
/// `eprintln!` even when the user has a subscriber wired up, or silently drop them
/// when they don't.
///
/// A handler returning `Err` is a genuine actor-side fault (unlike a dropped reply,
/// which is the caller's decision), so `error!` is the appropriate level.
#[doc(hidden)]
pub fn __log_handler_error(
    actor: &dyn std::fmt::Display,
    message_type: &str,
    error: &dyn std::fmt::Display,
) {
    tracing::error!(
        actor = %actor,
        message_type = %message_type,
        "handler returned error: {}", error
    );
}

use std::{future::Future, sync::atomic::AtomicU64, sync::OnceLock};

use tokio::sync::{mpsc, oneshot};

/// Unique identifier of a spawned actor: a process-wide ID plus the actor's
/// type name.
///
/// # Uniqueness
///
/// The `id` of every Identity handed out by the framework comes from a single
/// process-global counter, so framework-issued identities never collide and
/// internal subsystems (e.g. the `deadlock-detection` wait-for graph) key on
/// `id` alone. [`Identity::new`] exists for constructing identities in tests
/// and in user implementations of [`ActorControl`] — values built with it
/// carry **no uniqueness guarantee** and can collide (`Eq`/`Hash`) with real
/// actors; do not mix hand-built identities into maps keyed by framework
/// identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    /// Unique ID of the actor
    pub id: u64,
    /// Type name of the actor
    pub type_name: &'static str,
}

impl Identity {
    /// Creates a new `Identity` with the given ID and type name.
    ///
    /// Intended for tests and custom [`ActorControl`] implementations. See the
    /// type-level docs: identities built here carry no uniqueness guarantee.
    pub fn new(id: u64, type_name: &'static str) -> Self {
        Identity { id, type_name }
    }

    /// Returns the type name of the actor.
    pub fn name(&self) -> &'static str {
        self.type_name
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}(#{})", self.type_name, self.id)
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
/// Identical to `futures::future::BoxFuture` but defined locally to avoid exposing
/// the `futures` crate in the public API surface.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl<A, T> PayloadHandler<A> for T
where
    A: Actor + Message<T>,
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
                // `oneshot::Sender::send` fails only when the receiver has been
                // dropped, i.e. the caller stopped waiting for the reply before
                // the handler finished (`ask_with_timeout` elapsed, the caller's
                // future was cancelled, a hedged request lost the race, ...). That
                // is a normal, caller-driven outcome — not an actor fault — and the
                // caller side already records a dead letter. So this logs at
                // `debug`, never `error`, to keep routine timeouts/cancellations
                // out of error-level monitoring. The success arm is deliberately
                // silent: every delivered reply paying a tracing event would
                // contradict the hot-path zero-overhead goal and carries no
                // diagnostic value.
                if channel.send(Box::new(result)).is_err() {
                    tracing::debug!(
                        actor = %actor_ref.identity(),
                        message_type = %std::any::type_name::<T>(),
                        "Reply not delivered - receiver no longer waiting"
                    );
                }
            } else {
                <A as Message<T>>::on_tell_result(&result, &actor_ref);
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
    T: Actor,
{
    /// A user-defined message to be processed by the actor.
    Envelope {
        /// The message payload containing the actual message data.
        payload: Box<dyn PayloadHandler<T>>,
        /// Optional channel to send the reply back to the caller (used for `ask` operations).
        reply_channel: Option<oneshot::Sender<Box<dyn std::any::Any + Send>>>,
        /// The actor reference for potential self-messaging or context.
        actor_ref: ActorRef<T>,
        /// Deadlock-detection slot: the callee-side edge-removal token for an
        /// `ask`, dropped by the runtime right after the reply is sent. Empty
        /// for `tell` envelopes; a plain `()` when the feature is off.
        ask_edge: AskEdgeSlot,
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

/// The fixed capacity of the priority channel when it is enabled.
///
/// The priority channel is intentionally limited to a single in-flight slot. The slot is
/// released as soon as the actor's runtime loop calls `recv()`, so admission resumes
/// immediately after the actor reaches the next select! iteration. See
/// [`SpawnOptions::with_priority`] for the rationale.
pub(crate) const PRIORITY_CHANNEL_CAPACITY: usize = 1;

/// Capacity of the idle-subscribe channel used by
/// [`ActorRef::subscribe_idle`](crate::ActorRef::subscribe_idle).
///
/// Subscriptions are rare events (typically a handful per actor, established
/// during `on_start` or in response to occasional control messages), and the
/// runtime drains the channel on every loop iteration. The buffer absorbs
/// bursts that occur **before the runtime enters its select! loop** —
/// specifically, every subscription made inside `on_start` is queued here
/// because the receiver is not polled until `on_start` returns. A capacity of
/// 32 leaves comfortable headroom for fan-out patterns (e.g. one subscription
/// per item in a small config list) without resorting to an unbounded channel.
///
/// `subscribe_idle` uses `try_send` and returns [`Error::ChannelFull`] when the
/// buffer is full, so the failure mode is a loud, actionable error rather
/// than a silent hang — callers can batch or raise this constant if it is
/// ever hit in practice.
pub const IDLE_SUBSCRIBE_CHANNEL_CAPACITY: usize = 32;

/// Sets the global default buffer size for actor mailboxes.
///
/// This function can only be called successfully once. Subsequent calls
/// will return an error. This configured value is used by the `spawn` function
/// if no specific capacity is provided to `spawn_with_mailbox_capacity`.
pub fn set_default_mailbox_capacity(size: usize) -> Result<()> {
    if size == 0 {
        return Err(Error::MailboxCapacity {
            message: "Global default mailbox capacity must be greater than 0",
        });
    }

    CONFIGURED_DEFAULT_MAILBOX_CAPACITY
        .set(size)
        .map_err(|_| Error::MailboxCapacity {
            message: "Global default mailbox capacity has already been set",
        })
}

/// Configuration options for spawning an actor.
///
/// `SpawnOptions` is a builder used by [`spawn_with_options`] to control aspects of the
/// actor's runtime that are not part of the actor's own definition: mailbox capacity and
/// optional activation of the priority channel.
///
/// # Examples
///
/// ```rust,no_run
/// use rsactor::{spawn_with_options, SpawnOptions, Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct MyActor;
///
/// struct Ping;
///
/// #[message_handlers]
/// impl MyActor {
///     #[handler]
///     async fn handle_ping(&mut self, _: Ping, _: &ActorRef<Self>) -> () {}
/// }
///
/// # fn main() {
/// let opts = SpawnOptions::new().mailbox_capacity(64).with_priority();
/// let (actor_ref, _join) = spawn_with_options::<MyActor>(MyActor, opts);
/// assert!(actor_ref.has_priority_channel());
/// # }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Capacity of the regular mailbox channel. Must be greater than 0.
    /// Set via [`SpawnOptions::mailbox_capacity`].
    pub(crate) mailbox_capacity: usize,
    /// Whether to enable the priority channel. When `false` (default) no priority channel
    /// is created and any call to [`tell_priority`](crate::ActorRef::tell_priority) etc.
    /// returns [`Error::PriorityChannelNotEnabled`]. Toggled via
    /// [`SpawnOptions::with_priority`].
    pub(crate) priority_enabled: bool,
    /// Whether to enable the idle-event channel. When `false` (default) no idle-subscribe
    /// channel is created: [`Actor::on_idle`](crate::Actor::on_idle) is never driven and any
    /// call to [`subscribe_idle`](crate::ActorRef::subscribe_idle) returns
    /// [`Error::IdleChannelNotEnabled`]. Toggled via [`SpawnOptions::with_idle`].
    ///
    /// Disabling it by default removes one always-active branch from the actor's runtime
    /// `select!` loop, which the majority of actors (those that never use `on_idle`) would
    /// otherwise pay on every message.
    pub(crate) idle_enabled: bool,
}

impl SpawnOptions {
    /// Creates a new `SpawnOptions` with default mailbox capacity and the priority channel
    /// disabled.
    pub fn new() -> Self {
        let capacity = CONFIGURED_DEFAULT_MAILBOX_CAPACITY
            .get()
            .copied()
            .unwrap_or(DEFAULT_MAILBOX_CAPACITY);
        Self {
            mailbox_capacity: capacity,
            priority_enabled: false,
            idle_enabled: false,
        }
    }

    /// Sets the mailbox capacity. Must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0`.
    pub fn mailbox_capacity(mut self, n: usize) -> Self {
        assert!(n > 0, "Mailbox capacity must be greater than 0");
        self.mailbox_capacity = n;
        self
    }

    /// Enables the priority channel for the spawned actor.
    ///
    /// When enabled, the actor gets a second mpsc channel of fixed capacity 1 that the
    /// runtime polls with higher priority than the regular mailbox but lower than
    /// `kill()`. Use it for short, infrequent control-plane messages such
    /// as health checks or pause/resume signals.
    ///
    /// The capacity is fixed at 1 so the API enforces the intended semantics ("short and
    /// rare"). The single slot is released the moment the actor calls `recv()` on the
    /// channel, so admission resumes immediately at the next select! iteration. Callers
    /// must always pass a [`Duration`](std::time::Duration) so a wedged actor can never
    /// block a sender indefinitely.
    pub fn with_priority(mut self) -> Self {
        self.priority_enabled = true;
        self
    }

    /// Enables the idle-event channel for the spawned actor.
    ///
    /// When enabled, the actor gains the [`subscribe_idle`](crate::ActorRef::subscribe_idle)
    /// channel and its runtime drives [`Actor::on_idle`] for every
    /// event yielded by a subscribed stream. When **disabled** (the default), no such channel
    /// is created: `subscribe_idle` returns [`Error::IdleChannelNotEnabled`] and `on_idle` is
    /// never called.
    ///
    /// Idle support is off by default because it costs an always-active branch in the
    /// runtime's `select!` loop — a per-message overhead that only actors using `on_idle`
    /// should pay. Enable it when (and only when) the actor subscribes idle streams.
    pub fn with_idle(mut self) -> Self {
        self.idle_enabled = true;
        self
    }
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawns a new actor and returns an `ActorRef<T>` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's [`on_start`](crate::Actor::on_start) method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result as an [`ActorResult<T>`](crate::ActorResult).
///
/// # Panics
///
/// Panics if called outside a Tokio runtime context (this uses [`tokio::spawn`]).
pub fn spawn<T: Actor>(args: T::Args) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    spawn_with_options(args, SpawnOptions::new())
}

/// Spawns a new actor with a specified mailbox capacity and returns an `ActorRef<T>` to it, along with a `JoinHandle`.
///
/// Takes initialization arguments that will be passed to the actor's [`on_start`](crate::Actor::on_start) method.
/// The `JoinHandle` can be used to await the actor's termination and retrieve
/// the actor result as an [`ActorResult<T>`](crate::ActorResult). Use this version when you need
/// to control the actor's mailbox capacity.
///
/// # Panics
///
/// Panics if `mailbox_capacity == 0` (via
/// [`SpawnOptions::mailbox_capacity`]), or if called outside a Tokio runtime
/// context. Note the asymmetry with [`set_default_mailbox_capacity`], which
/// returns an `Err` for `0`: a literal zero capacity at a spawn site is a
/// programming error, while the global default is runtime configuration.
pub fn spawn_with_mailbox_capacity<T: Actor>(
    args: T::Args,
    mailbox_capacity: usize,
) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    spawn_with_options(args, SpawnOptions::new().mailbox_capacity(mailbox_capacity))
}

/// Spawns a new actor with the given [`SpawnOptions`] and returns an `ActorRef<T>` along
/// with a `JoinHandle`.
///
/// This is the most general spawn entry point. Use it when you need to enable the
/// priority channel via [`SpawnOptions::with_priority`] or configure both mailbox
/// capacity and priority in a single call.
///
/// # Panics
///
/// Panics if called outside a Tokio runtime context (this uses [`tokio::spawn`]).
pub fn spawn_with_options<T: Actor>(
    args: T::Args,
    opts: SpawnOptions,
) -> (ActorRef<T>, tokio::task::JoinHandle<ActorResult<T>>) {
    // Defensive only: every public path into `SpawnOptions` already rejects 0
    // (the builder asserts, the global-default setter returns Err), so this
    // assert is unreachable from the public API and merely guards future
    // internal construction paths.
    assert!(
        opts.mailbox_capacity > 0,
        "Mailbox capacity must be greater than 0"
    );

    static ACTOR_IDS: AtomicU64 = AtomicU64::new(1);

    let actor_id = Identity::new(
        ACTOR_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::any::type_name::<T>(),
    );

    let (mailbox_tx, mailbox_rx) = mpsc::channel(opts.mailbox_capacity);
    let (terminate_tx, terminate_rx) = mpsc::channel::<ControlSignal>(1);

    let (priority_tx, priority_rx) = if opts.priority_enabled {
        let (tx, rx) = mpsc::channel(PRIORITY_CHANNEL_CAPACITY);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let (idle_subscribe_tx, idle_subscribe_rx) = if opts.idle_enabled {
        let (tx, rx) = mpsc::channel(IDLE_SUBSCRIBE_CHANNEL_CAPACITY);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    #[cfg(feature = "metrics")]
    let metrics = std::sync::Arc::new(metrics::MetricsCollector::new());

    let actor_ref = ActorRef::new(
        actor_id,
        mailbox_tx,
        priority_tx,
        terminate_tx,
        idle_subscribe_tx,
        #[cfg(feature = "metrics")]
        metrics,
    );

    let join_handle = tokio::spawn(crate::actor::run_actor_lifecycle(
        args,
        actor_ref.clone(),
        mailbox_rx,
        priority_rx,
        terminate_rx,
        idle_subscribe_rx,
    ));

    (actor_ref, join_handle)
}
