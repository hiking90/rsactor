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
//!     tick_300ms: tokio::time::Interval,
//!     tick_1s: tokio::time::Interval,
//! }
//!
//! impl MyActor {
//!     fn new(data: &str) -> Self {
//!         MyActor {
//!             data: data.to_string(),
//!             tick_300ms: tokio::time::interval(std::time::Duration::from_millis(300)),
//!             tick_1s: tokio::time::interval(std::time::Duration::from_secs(1)),
//!         }
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
//!         Ok(MyActor {
//!             data: args,
//!             tick_300ms: tokio::time::interval(std::time::Duration::from_millis(300)),
//!             tick_1s: tokio::time::interval(std::time::Duration::from_secs(1)),
//!         })
//!     }
//!
//!     // Optional: Implement on_run for the actor's main execution logic.
//!     // This method is called after on_start. If it returns Ok(false), the actor stops normally.
//!     // If it returns Err(_), the actor stops due to an error.
//!     // If it returns Ok(true), the actor continues running.
//!     async fn on_run(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
//!         tokio::select! {
//!             _ = self.tick_300ms.tick() => {
//!                 println!("Tick: 300ms");
//!             }
//!             _ = self.tick_1s.tick() => {
//!                 println!("Tick: 1s");
//!             }
//!         }
//!         Ok(()) // Continue running
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

mod error;
pub use error::{Error, Result};

mod actor_ref;
pub use actor_ref::ActorRef;

use std::{
    any::Any, fmt::Debug, future::Future, sync::{
        atomic::{AtomicUsize, Ordering}, OnceLock
    }, time::Duration
};

use tokio::sync::{mpsc, oneshot};
use log::{info, error, debug, trace};

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
                    identity: actor_ref.identity(),
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
enum MailboxMessage { // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
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
enum ControlSignal { // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.
    /// A signal for the actor to terminate immediately.
    Terminate,
}

// Type alias for the sender part of the actor's mailbox channel.
type MailboxSender = mpsc::Sender<MailboxMessage>; // This needs to be pub(crate) or pub for actor_ref.rs to use it, or moved.

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

// ActorRef struct and impl removed

/// Represents the phase during which an actor failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePhase {
    /// Actor failed during the `on_start` lifecycle hook.
    OnStart,
    /// Actor failed during execution.
    OnRun,
    /// Actor failed during the `on_stop` lifecycle hook.
    OnStop,
}

impl std::fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailurePhase::OnStart => write!(f, "OnStart"),
            FailurePhase::OnRun => write!(f, "OnRun"),
            FailurePhase::OnStop => write!(f, "OnStop"),
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
    /// Actor failed during one of its lifecycle phases.
    Failed {
        actor: Option<T>,
        error: T::Error,
        phase: FailurePhase,
        killed: bool,
    },
}

impl<T: Actor> From<ActorResult<T>> for (Option<T>, Option<T::Error>) {
    fn from(result: ActorResult<T>) -> Self {
        match result {
            ActorResult::Completed { actor, .. } => (Some(actor), None),
            ActorResult::Failed { actor, error: cause, .. } => (actor, Some(cause)),
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
        matches!(self, ActorResult::Completed { killed: true, .. } | ActorResult::Failed { killed: true, .. })
    }

    /// Returns `true` if the actor stopped normally.
    pub fn stopped_normally(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: false, .. })
    }

    /// Returns `true` if the actor failed to start.
    pub fn is_startup_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnStart, .. })
    }

    /// Returns `true` if the actor failed during runtime.
    pub fn is_runtime_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnRun, .. })
    }

    /// Returns `true` if the actor failed during the stop phase.
    pub fn is_stop_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnStop, .. })
    }

    /// Returns the actor instance if available, regardless of the result type.
    pub fn actor(&self) -> Option<&T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { actor, .. } => actor.as_ref(),
        }
    }

    /// Consumes the result and returns the actor instance if available.
    pub fn into_actor(self) -> Option<T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { actor, .. } => actor,
        }
    }

    /// Returns the error if the result represents a failure.
    pub fn error(&self) -> Option<&T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
        }
    }

    /// Consumes the result and returns the error if it represents a failure.
    pub fn into_error(self) -> Option<T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
        }
    }

    /// Returns true if the result represents any kind of failure.
    pub fn is_failed(&self) -> bool {
        !self.is_completed()
    }

    /// Returns true if the result contains an actor instance.
    pub fn has_actor(&self) -> bool {
        self.actor().is_some()
    }

    /// Maps the actor instance if present, leaving other variants unchanged.
    pub fn map_actor<U, F>(self, f: F) -> ActorResult<U>
    where
        F: FnOnce(T) -> U,
        U: Actor<Error = T::Error>
    {
        match self {
            ActorResult::Completed { actor, killed } => {
                ActorResult::Completed { actor: f(actor), killed }
            }
            ActorResult::Failed { actor, error: cause, phase, killed } => {
                ActorResult::Failed {
                    actor: actor.map(f),
                    error: cause,
                    phase,
                    killed
                }
            }
        }
    }

    /// Applies a function to the actor if present and successful completion.
    pub fn and_then<F, R, E>(self, f: F) -> std::result::Result<R, E>
    where
        F: FnOnce(T) -> std::result::Result<R, E>,
        E: From<T::Error> + From<&'static str>
    {
        match self {
            ActorResult::Completed { actor, killed: false } => f(actor),
            ActorResult::Completed { killed: true, .. } => {
                Err(E::from("Actor was killed"))
            }
            ActorResult::Failed { error: cause, .. } => Err(E::from(cause)),
        }
    }

    /// Converts to a standard Result, preserving the actor on success
    pub fn to_result(self) -> std::result::Result<T, T::Error> {
        match self {
            ActorResult::Completed { actor, .. } => Ok(actor),
            ActorResult::Failed { error: cause, .. } => Err(cause),
        }
    }

    /// Converts to a standard Result, only succeeding if actor stopped normally (not killed)
    pub fn to_result_if_normal(self) -> std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>
    where
        T::Error: std::error::Error + Send + Sync + 'static
    {
        match self {
            ActorResult::Completed { actor, killed: false } => Ok(actor),
            ActorResult::Completed { killed: true, .. } => {
                Err("Actor was killed".into())
            }
            ActorResult::Failed { error: cause, .. } => Err(Box::new(cause)),
        }
    }
}


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
    /// If this `Future` successfully completes by returning `Ok(())`, the runtime will invoke
    /// `on_run` again to acquire a new `Future` for the next segment of work. This pattern
    /// of repeated invocation allows the actor to manage ongoing or periodic tasks throughout
    /// its lifecycle. The actor continues running as long as `on_run` returns `Ok(())`.
    /// If `on_run` returns an `Err(_)`, the actor stops due to an error.
    /// To stop the actor normally from within `on_run`, it should call `actor_ref.stop()` or `actor_ref.kill()`.
    ///
    /// `on_run`'s execution is concurrent with the actor's message handling capabilities,
    /// enabling the actor to perform background or main-loop tasks while continuing to
    /// process incoming messages from its mailbox.
    ///
    /// # Key characteristics:
    ///
    /// - **Iterative Execution**: The `rsactor` runtime invokes `on_run` to obtain a `Future`.
    ///   If this `Future` completes with `Ok(())`, `on_run` will be called again by the
    ///   runtime to obtain a new `Future`. This allows for continuous or step-by-step
    ///   task processing throughout the actor's active lifecycle. The actor continues as long
    ///   as `Err(_)` is not returned. For normal termination from within `on_run`,
    ///   use `actor_ref.stop()` or `actor_ref.kill()`.
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
    ///    #     ticks_done: u32, // Example state for controlling shutdown
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
    ///    #     Ok(MyActor { interval, ticks_done: 0 })
    ///    # }
    ///    async fn on_run(&mut self, actor_ref: &ActorRef) -> std::result::Result<(), Self::Error> { // Note: Return type is Result<(), Self::Error>
    ///        // self.interval is stored in the MyActor struct.
    ///        self.interval.tick().await; // This await point allows message processing.
    ///
    ///        // Perform the periodic task here.
    ///        println!("Periodic task executed by actor {}", actor_ref.identity());
    ///        self.ticks_done += 1;
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
    ///        // To stop the actor normally from within on_run, call actor_ref.stop() or actor_ref.kill().
    ///        // For example, to stop after 10 ticks:
    ///        if self.ticks_done >= 10 {
    ///            println!("Actor {} stopping after {} ticks.", actor_ref.identity(), self.ticks_done);
    ///            actor_ref.stop().await?; // or actor_ref.kill()?
    ///            // After calling stop/kill, on_run might not be called again as the actor shuts down.
    ///            // It's good practice to return Ok(()) here, or handle potential errors from stop().
    ///            return Ok(());
    ///        }
    ///
    ///        // Return Ok(()) to have on_run called again by the runtime for the next tick.
    ///        // If an Err is returned, the actor stops due to an error.
    ///        Ok(())
    ///    }
    ///    # }
    ///    ```
    ///
    /// # Termination:
    ///
    /// - If the `Future` returned by `on_run` completes with `Err(_)`, the actor terminates due to an error.
    /// - If the `Future` completes with `Ok(())`, the actor continues running, and the runtime
    ///   will invoke `on_run` again to get the next `Future` for execution.
    ///
    /// To stop the actor normally from within `on_run` (e.g., graceful shutdown),
    /// the actor should explicitly call `actor_ref.stop().await?` or `actor_ref.kill()`.
    /// After such a call, `on_run` is unlikely to be invoked again by the runtime,
    /// as the actor will be in the process of shutting down.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own `ActorRef`.
    /// It can be used, for example, to call `actor_ref.stop()` or `actor_ref.kill()`
    /// to initiate actor termination from within `on_run`.
    ///
    /// - Returning `Ok(())` signifies that the current segment of work completed successfully,
    ///   and the actor should continue running (i.e., `on_run` will be called again).
    /// - Returning `Err(_)` signifies termination due to an error.
    /// The specific outcome is captured in the `ActorResult`.
    ///
    /// The default implementation of `on_run` is a simple async block that sleeps for 1 second
    /// and then returns `Ok(())`, causing it to be called repeatedly until the actor is
    /// explicitly stopped or killed.
    fn on_run(&mut self, _actor_ref: &ActorRef) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // This sleep is critical - it creates an await point that allows
        // the Tokio runtime to switch tasks and process incoming messages.
        // Without at least one await point in a loop, message processing would starve.
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(())
        }
    }

    fn on_stop(&mut self, _actor_ref: &ActorRef, _killed: bool) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // Default implementation does nothing on stop.
        // Override this method in your actor if you need to perform cleanup.
        async { Ok(()) }
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
    let actor_id = actor_ref.identity();

    let mut actor = match T::on_start(args, &actor_ref).await {
        Ok(actor) => {
            debug!("Actor {} on_start completed successfully.", actor_id);
            actor
        }
        Err(e) => {
            error!("Actor {} on_start failed: {:?}", actor_id, e);
            return ActorResult::Failed {
                actor: None,
                error: e,
                phase: FailurePhase::OnStart,
                killed: false
            }
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

                    // Call on_stop for kill scenario
                    if let Err(e) = actor.on_stop(&actor_ref, true).await {
                        error!("Actor {} on_stop failed during kill: {:?}", actor_id, e);
                        return ActorResult::Failed {
                            actor: Some(actor),
                            error: e,
                            phase: FailurePhase::OnStop,
                            killed: true,
                        };
                    }

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
                                        error!("Actor {} failed to send error reply: receiver dropped.", actor_id);
                                    }
                                } else {
                                    // If no reply channel, we can't send an error back.
                                    // This is a design choice: if the message was sent with 'tell',
                                    // we don't have a reply channel to send errors back.
                                    error!("Actor {} received message without reply channel, cannot send error reply.", actor_id);
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

                        // Call on_stop for graceful stop scenario
                        if let Err(e) = actor.on_stop(&actor_ref, false).await {
                            error!("Actor {} on_stop failed during graceful stop: {:?}", actor_id, e);
                            return ActorResult::Failed {
                                actor: Some(actor),
                                error: e,
                                phase: FailurePhase::OnStop,
                                killed: false,
                            };
                        }

                        break;
                    }
                    // Terminate is handled by its own dedicated channel and select branch.
                    None => {
                        // Mailbox closed, meaning all senders (ActorRefs) are dropped.
                        unreachable!("Actor {} mailbox closed unexpectedly. This should not happen unless all ActorRefs are dropped.", actor_id);
                    }
                }
            }

            maybe_result = actor.on_run(&actor_ref) => {
                match maybe_result {
                    Ok(_) => {
                        // on_run completed successfully, continue processing messages.
                        debug!("Actor {} on_run completed successfully, continuing.", actor_id);
                    }
                    Err(e) => { // e is A::Error
                        let error_msg = format!("Actor {} on_run error: {:?}", actor_id, e);
                        error!("{}", error_msg);
                        return ActorResult::Failed {
                            actor: Some(actor),
                            error: e,
                            phase: FailurePhase::OnRun,
                            killed: false
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
        Identity::new(id, std::any::type_name::<T>()), // Use type name of the actor
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
