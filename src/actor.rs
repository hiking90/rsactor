// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor_ref::ActorRef;
use crate::{ActorResult, ActorWeak, ControlSignal, FailurePhase, MailboxMessage};
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc;
use tracing::{debug, error, Instrument};

/// Wrap a future with CURRENT_ACTOR scope and await it.
/// Used for on_start, message handler, on_stop.
macro_rules! run_with_actor_scope {
    ($actor_id:expr, $fut:expr) => {{
        #[cfg(feature = "deadlock-detection")]
        {
            crate::CURRENT_ACTOR.scope($actor_id, $fut).await
        }
        #[cfg(not(feature = "deadlock-detection"))]
        {
            $fut.await
        }
    }};
}

/// Wrap a future with CURRENT_ACTOR scope without awaiting.
/// Used for tokio::select! branch expressions (on_run).
macro_rules! with_actor_scope {
    ($actor_id:expr, $fut:expr) => {{
        #[cfg(feature = "deadlock-detection")]
        {
            crate::CURRENT_ACTOR.scope($actor_id, $fut)
        }
        #[cfg(not(feature = "deadlock-detection"))]
        {
            $fut
        }
    }};
}

/// Defines the behavior of an actor.
///
/// Actors are fundamental units of computation that communicate by exchanging messages.
/// Each actor has its own state and processes messages sequentially.
///
/// # Error Handling
///
/// Actor lifecycle methods ([`on_start`](Actor::on_start), [`on_run`](Actor::on_run), [`on_stop`](Actor::on_stop)) can return errors. How these errors
/// are handled depends on when they occur:
///
/// 1. Errors in [`on_start`](Actor::on_start): The actor fails to initialize. The error is captured in
///    [`ActorResult::Failed`](crate::ActorResult::Failed) with `phase` set to
///    [`FailurePhase::OnStart`](crate::FailurePhase::OnStart) and `actor` set to `None`.
///
/// 2. Errors in [`on_run`](Actor::on_run): The actor terminates during runtime. The error is captured in
///    [`ActorResult::Failed`](crate::ActorResult::Failed) with `phase` set to
///    [`FailurePhase::OnRun`](crate::FailurePhase::OnRun) and `actor` contains the actor instance.
///
/// 3. Errors in [`on_stop`](Actor::on_stop): The actor fails during cleanup. The error is captured in
///    [`ActorResult::Failed`](crate::ActorResult::Failed) with `phase` set to
///    [`FailurePhase::OnStop`](crate::FailurePhase::OnStop) and `actor` contains the actor instance.
///
/// When awaiting the completion of an actor, check the [`ActorResult`] to determine
/// the outcome and access any errors:
///
/// - Use methods like [`is_failed`](crate::ActorResult::is_failed),
///   [`is_runtime_failed`](crate::ActorResult::is_runtime_failed), etc. to identify the error type
/// - Access the error via [`error`](crate::ActorResult::error) or
///   [`into_error`](crate::ActorResult::into_error) to retrieve error details
/// - If the actor instance is available ([`has_actor`](crate::ActorResult::has_actor) returns true),
///   you can recover it using [`actor`](crate::ActorResult::actor) or
///   [`into_actor`](crate::ActorResult::into_actor) for further processing
///
/// Implementors of this trait must also be `Send + 'static`.
pub trait Actor: Sized + Send + 'static {
    /// Type for arguments passed to [`on_start`](Actor::on_start) for actor initialization.
    /// This type provides the necessary data to create an instance of the actor.
    type Args: Send;
    /// The error type that can be returned by the actor\'s lifecycle methods.
    /// Used in [`on_start`](Actor::on_start), [`on_run`](Actor::on_run), and [`on_stop`](Actor::on_stop).
    type Error: Send + Debug;

    /// Called when the actor is started. This is required for actor creation.
    ///
    /// This method is the initialization point for an actor and a fundamental part of the actor model design.
    /// Unlike traditional object construction, the actor's instance is created within this asynchronous method,
    /// allowing for complex initialization that may require awaiting resources. This method is called by
    /// [`spawn`](crate::spawn) or [`spawn_with_mailbox_capacity`](crate::spawn_with_mailbox_capacity).
    ///
    /// # Actor State Initialization
    ///
    /// In the actor model, each actor encapsulates its own state. Creating the actor inside `on_start`
    /// ensures that:
    ///
    /// - The actor's state is always valid before it begins processing messages
    /// - The need for `Option<T>` fields is minimized, as state can be fully initialized
    /// - Asynchronous resources (like database connections) can be acquired during initialization
    /// - Initialization failures can be cleanly handled before the actor enters the message processing phase
    ///
    /// # Parameters
    ///
    /// - `args`: Initialization data (of type `Self::Args`) provided when the actor is spawned
    /// - `actor_ref`: A reference to the actor's own [`ActorRef`](crate::actor_ref::ActorRef), which can
    ///   be stored in the actor for self-reference or for initializing child actors
    ///
    /// # Returns
    ///
    /// - `Ok(Self)`: A fully initialized actor instance
    /// - `Err(Self::Error)`: If initialization fails
    ///
    /// If this method returns an error, the actor will not be created, and the error
    /// will be captured in the [`ActorResult`](crate::ActorResult) with
    /// [`FailurePhase::OnStart`](crate::FailurePhase::OnStart).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use rsactor::{Actor, ActorRef, Message, spawn, ActorResult};
    /// use std::time::Duration;
    /// use anyhow::Result;
    ///
    /// // Simple actor that holds a name
    /// #[derive(Debug)]
    /// struct SimpleActor {
    ///     name: String,
    /// }
    ///
    /// // Implement Actor trait with focus on on_start
    /// impl Actor for SimpleActor {
    ///     type Args = String; // Name parameter
    ///     type Error = anyhow::Error;
    ///
    ///     async fn on_start(name: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
    ///         // Create and return the actor instance
    ///         Ok(Self { name })
    ///     }
    /// }
    ///
    /// // Main function showing the basic lifecycle
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     // Spawn the actor with a name argument using the [`spawn`](crate::spawn) function
    ///     let (actor_ref, join_handle) = spawn::<SimpleActor>("MyActor".to_string());
    ///
    ///     // Gracefully stop the actor using [`stop`](crate::actor_ref::ActorRef::stop)
    ///     actor_ref.stop().await?;
    ///
    ///     // Wait for the actor to complete and get its final state
    ///     // The JoinHandle returns an [`ActorResult`](crate::ActorResult) enum
    ///     match join_handle.await? {
    ///         ActorResult::Completed { actor, killed } => {
    ///             println!("Actor '{}' completed. Killed: {}", actor.name, killed);
    ///             // The `actor` field contains the final actor instance
    ///             // The `killed` flag indicates whether the actor was stopped or killed
    ///         }
    ///         ActorResult::Failed { error, phase, .. } => {
    ///             println!("Actor failed in phase {:?}: {}", phase, error);
    ///             // The `phase` field indicates which lifecycle method caused the failure
    ///             // See [`FailurePhase`](crate::FailurePhase) enum for possible values
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    fn on_start(
        args: Self::Args,
        actor_ref: &ActorRef<Self>,
    ) -> impl Future<Output = std::result::Result<Self, Self::Error>> + Send;

    /// The idle handler for the actor, similar to `on_idle` in traditional event loops.
    ///
    /// This method is called when the actor's message queue is empty, allowing the actor to
    /// perform background processing or periodic tasks. The return value controls whether
    /// `on_run` continues to be called:
    ///
    /// - `Ok(true)`: Continue calling `on_run` (equivalent to `G_SOURCE_CONTINUE` in GTK+)
    /// - `Ok(false)`: Stop calling `on_run`, only process messages (equivalent to `G_SOURCE_REMOVE`)
    /// - `Err(e)`: Terminate the actor with an error
    ///
    /// # Key characteristics:
    ///
    /// - **Idle Processing**: `on_run` is only called when the message queue is empty,
    ///   thanks to the `biased` selection in the runtime loop. Messages always have
    ///   higher priority than idle processing.
    ///
    /// - **Dynamic Control**: The return value allows dynamic control over idle processing.
    ///   Return `Ok(true)` to continue, or `Ok(false)` when idle processing is no longer needed.
    ///
    /// - **State Persistence Across Invocations**: Because `on_run` can be invoked multiple
    ///   times by the runtime (each time generating a new `Future`), any state intended
    ///   to persist across these distinct invocations *must* be stored as fields within
    ///   the actor's struct (`self`). Local variables declared inside `on_run` are ephemeral
    ///   and will not be preserved if `on_run` completes and is subsequently re-invoked.
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
    /// 1. **Periodic tasks**: For executing work at regular intervals.
    ///    ```rust,no_run
    ///    # use rsactor::{Actor, ActorRef, ActorWeak, Message, spawn};
    ///    # use anyhow::Result;
    ///    # use std::time::Duration;
    ///    # use tokio::time::{Interval, MissedTickBehavior};
    ///    # struct MyActor {
    ///    #     interval: Interval,
    ///    #     ticks_done: u32,
    ///    # }
    ///    # impl Actor for MyActor {
    ///    # type Args = Duration;
    ///    # type Error = anyhow::Error;
    ///    # async fn on_start(duration: Self::Args, _actor_ref: &ActorRef<MyActor>) -> std::result::Result<Self, Self::Error> {
    ///    #     let mut interval = tokio::time::interval(duration);
    ///    #     interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ///    #     Ok(MyActor { interval, ticks_done: 0 })
    ///    # }
    ///    async fn on_run(&mut self, actor_weak: &ActorWeak<MyActor>) -> std::result::Result<bool, Self::Error> {
    ///        self.interval.tick().await;
    ///        println!("Periodic task executed by actor {}", actor_weak.identity());
    ///        self.ticks_done += 1;
    ///
    ///        // Stop idle processing after 10 ticks, but actor continues processing messages
    ///        if self.ticks_done >= 10 {
    ///            return Ok(false);
    ///        }
    ///
    ///        Ok(true)  // Continue calling on_run
    ///    }
    ///    # }
    ///    ```
    ///
    /// 2. **One-time initialization then message-only**: Perform setup work, then only handle messages.
    ///    ```rust,no_run
    ///    # use rsactor::{Actor, ActorRef, ActorWeak};
    ///    # struct MyActor { initialized: bool }
    ///    # impl Actor for MyActor {
    ///    # type Args = ();
    ///    # type Error = anyhow::Error;
    ///    # async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> { Ok(MyActor { initialized: false }) }
    ///    async fn on_run(&mut self, _: &ActorWeak<Self>) -> std::result::Result<bool, Self::Error> {
    ///        if !self.initialized {
    ///            // Perform one-time initialization
    ///            self.initialized = true;
    ///        }
    ///        Ok(false)  // No more idle processing needed
    ///    }
    ///    # }
    ///    ```
    ///
    /// # Default Implementation
    ///
    /// The default implementation returns `Ok(false)`, meaning `on_run` is called once
    /// and then disabled. Actors that don't override `on_run` will only process messages.
    #[allow(unused_variables)]
    fn on_run(
        &mut self,
        actor_weak: &ActorWeak<Self>,
    ) -> impl Future<Output = std::result::Result<bool, Self::Error>> + Send {
        // Default implementation returns Ok(false) to indicate no idle processing needed.
        // The actor will only process messages without calling on_run again.
        // Override this method and return Ok(true) to have on_run called repeatedly.
        async { Ok(false) }
    }

    /// Called when the actor is about to stop. This allows the actor to perform cleanup tasks.
    ///
    /// This method is called when the actor is stopping, including:
    /// - Explicit stop via [`ActorRef::stop`](crate::actor_ref::ActorRef::stop) (graceful termination)
    /// - Explicit kill via [`ActorRef::kill`](crate::actor_ref::ActorRef::kill) (immediate termination)
    /// - Cleanup after an [`on_run`](Actor::on_run) error
    ///
    /// It is **not** called if the actor fails during message processing (handler panic/error).
    /// The result of this method affects the final [`ActorResult`](crate::ActorResult) returned when awaiting the join handle.
    ///
    /// The `killed` parameter indicates how the actor is being stopped:
    /// - `killed = false`: The actor is stopping gracefully (via `stop()` call)
    /// - `killed = true`: The actor is being forcefully terminated (via `kill()` call)
    ///
    /// Cleanup operations that should be performed regardless of how the actor terminates
    /// should be implemented as a `Drop` implementation on the actor struct instead.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use rsactor::{Actor, ActorRef, ActorWeak, Result};
    /// # use std::time::Duration;
    /// # struct MyActor { /* ... */ }
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
    /// #         Ok(MyActor { /* ... */ })
    /// #     }
    /// async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> std::result::Result<(), Self::Error> {
    ///     if killed {
    ///         println!("Actor {} is being forcefully terminated, performing minimal cleanup", actor_weak.identity());
    ///         // Perform minimal, fast cleanup
    ///     } else {
    ///         println!("Actor {} is gracefully shutting down, performing full cleanup", actor_weak.identity());
    ///         // Perform thorough cleanup
    ///     }
    ///     Ok(())
    /// }
    /// # }
    /// ```
    ///
    /// The [`identity`](crate::actor_ref::ActorRef::identity) method provides a unique identifier for the actor.
    ///
    /// For a complete lifecycle example, see the example in [`on_start`](Actor::on_start) that demonstrates
    /// actor creation with [`spawn`](crate::spawn), graceful termination with [`stop`](crate::actor_ref::ActorRef::stop),
    /// and handling the [`ActorResult`](crate::ActorResult) from the join handle.
    #[allow(unused_variables)]
    fn on_stop(
        &mut self,
        actor_weak: &ActorWeak<Self>,
        killed: bool,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // Default implementation does nothing on stop.
        // Override this method in your actor if you need to perform cleanup.
        async { Ok(()) }
    }
}

/// A trait for messages that an actor can handle, defining the reply type.
///
/// An actor struct implements this trait for each specific message type it can process.
/// Messages are sent to actors using methods like [`tell`](crate::actor_ref::ActorRef::tell),
/// [`ask`](crate::actor_ref::ActorRef::ask), or their variants.
pub trait Message<T: Send + 'static>: Actor {
    /// The type of the reply that will be sent back to the caller.
    type Reply: Send + 'static;

    /// Handles the incoming message and produces a reply.
    ///
    /// The `actor_ref` parameter is a reference to the actor's own [`ActorRef`].
    /// This is an asynchronous method where the actor's business logic for
    /// processing the message `T` resides.
    ///
    /// This method is called by the actor system when a message is received via
    /// [`tell`](crate::actor_ref::ActorRef::tell), [`ask`](crate::actor_ref::ActorRef::ask),
    /// or their variants.
    fn handle(
        &mut self,
        msg: T,
        actor_ref: &ActorRef<Self>,
    ) -> impl Future<Output = Self::Reply> + Send;

    /// Called after a `tell()` message handler completes.
    ///
    /// This hook allows inspecting the handler's return value for fire-and-forget messages.
    /// For `ask()` calls, this method is NOT invoked — the caller receives the result directly.
    ///
    /// # Design decisions
    ///
    /// This hook is designed as a **synchronous, stateless logging hook**:
    /// - **No `&self`**: The actor's mutable state is not accessible. This prevents
    ///   side effects and keeps the hook purely observational.
    /// - **Not `async`**: Asynchronous operations (e.g., sending messages to other actors)
    ///   are intentionally excluded to avoid hidden control flow in fire-and-forget paths.
    /// - These constraints may be relaxed in future versions if use cases arise.
    ///
    /// The default implementation does nothing. When using the `#[handler]` macro with a
    /// `Result<T, E>` return type (where `E: Display`), an override is automatically
    /// generated that logs errors via `tracing::error!`.
    ///
    /// # `Display` bound enforcement
    ///
    /// The generated `tracing::error!("...: {}", e)` implicitly requires `E: Display`.
    /// Since `E` is embedded inside `Self::Reply = Result<T, E>`, a `where E: Display`
    /// clause cannot be added directly on a trait method override. In the
    /// `#[handler(result)]` + type alias case, the proc macro cannot know the concrete
    /// `E` type at all, making an explicit bound fundamentally impossible.
    ///
    /// Therefore, implicit enforcement via the `{}` format specifier is adopted.
    /// If `E` does not implement `Display`, the compiler emits
    /// `` `MyError` doesn't implement `std::fmt::Display` ``
    /// which is clear enough for the user to diagnose immediately.
    ///
    /// # Automatic generation
    ///
    /// The `#[handler]` macro generates an override when:
    /// - The return type is syntactically `Result<T, E>` (auto-detected)
    /// - The attribute `#[handler(result)]` is explicitly specified
    ///
    /// Use `#[handler(no_log)]` to suppress automatic generation.
    ///
    /// # Manual override
    ///
    /// For manual `Message` trait implementations, override this method directly:
    ///
    /// ```rust,ignore
    /// fn on_tell_result(result: &Self::Reply, actor_ref: &ActorRef<Self>) {
    ///     if let Err(ref e) = result {
    ///         tracing::error!(actor = %actor_ref.identity(), "error: {}", e);
    ///     }
    /// }
    /// ```
    fn on_tell_result(_result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        // default: do nothing
    }
}

/// Executes the complete lifecycle of an actor within its spawned task.
///
/// This function is the core runtime for an actor, handling the entire lifecycle from
/// initialization through message processing to termination. It orchestrates:
///
/// 1. **Initialization**: Calls `on_start` to create the actor instance
/// 2. **Runtime Loop**: Concurrently handles messages and executes `on_run`
/// 3. **Termination**: Manages graceful/forceful shutdown and calls `on_stop`
///
/// # Arguments
///
/// * `args` - Initialization arguments passed to the actor's `on_start` method
/// * `actor_ref` - Strong reference to the actor (dropped after initialization)
/// * `receiver` - Channel receiver for incoming messages
/// * `terminate_receiver` - Channel receiver for termination signals
///
/// # Returns
///
/// Returns an `ActorResult<T>` indicating the final state of the actor:
/// - `ActorResult::Completed` if the actor terminated normally
/// - `ActorResult::Failed` if an error occurred during any lifecycle phase
///
/// # Lifecycle Flow
///
/// ```text
/// ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
/// │  on_start   │───▶│   Runtime   │───▶│   on_stop   │
/// │ (create)    │    │   Loop      │    │ (cleanup)   │
/// └─────────────┘    └─────────────┘    └─────────────┘
///                           │
///                           ▼
///                    ┌─────────────┐
///                    │ Message     │
///                    │ Processing  │
///                    │ + on_run    │
///                    └─────────────┘
/// ```
#[cfg_attr(feature = "tracing", tracing::instrument(
    level = "debug",
    name = "actor_lifecycle",
    fields(
        actor_id = %actor_ref.identity(),
        actor_type = %std::any::type_name::<T>()
    ),
    skip_all
))]
pub(crate) async fn run_actor_lifecycle<T: Actor>(
    args: T::Args,
    actor_ref: ActorRef<T>,
    mut receiver: mpsc::Receiver<MailboxMessage<T>>,
    mut terminate_receiver: mpsc::Receiver<ControlSignal>,
) -> ActorResult<T> {
    let actor_id = actor_ref.identity();

    #[cfg(feature = "tracing")]
    let on_start_span = tracing::debug_span!("actor_on_start");
    #[cfg(not(feature = "tracing"))]
    let on_start_span = tracing::Span::none();

    let mut actor = match run_with_actor_scope!(
        actor_id,
        T::on_start(args, &actor_ref).instrument(on_start_span)
    ) {
        Ok(actor) => {
            debug!("Actor {actor_id} on_start completed successfully.");
            actor
        }
        Err(e) => {
            error!("Actor {actor_id} on_start failed: {e:?}");
            return ActorResult::Failed {
                actor: None,
                error: e,
                phase: FailurePhase::OnStart,
                killed: false,
            };
        }
    };

    debug!("Actor {actor_id} runtime starting - entering main processing loop.");

    let actor_weak = ActorRef::downgrade(&actor_ref);
    drop(actor_ref); // Drop the strong reference to allow graceful shutdown detection

    let mut killed = false;
    let mut idle_enabled = true;

    // Main actor processing loop - handles three concurrent operations:
    // 1. Termination signals (highest priority via biased select)
    // 2. Incoming messages from the mailbox
    // 3. Actor's on_run lifecycle method execution
    loop {
        #[cfg(feature = "tracing")]
        let on_run_span = tracing::debug_span!("actor_on_run");
        #[cfg(not(feature = "tracing"))]
        let on_run_span = tracing::Span::none();

        tokio::select! {
            // Handle termination signals with highest priority using biased selection
            // This ensures that termination requests are processed immediately when available
            biased;

            maybe_terminate = terminate_receiver.recv() => {
                match maybe_terminate {
                    Some(_) => {
                        // Explicit kill() was called
                        #[cfg(feature = "tracing")]
                        debug!("Actor termination via kill() method");
                        killed = true;
                    }
                    None => {
                        // All actor_ref instances were dropped
                        #[cfg(feature = "tracing")]
                        debug!("Actor termination due to all actor_ref instances being dropped");
                        killed = false;
                    }
                }

                // Call on_stop for termination scenario
                #[cfg(feature = "tracing")]
                let on_stop_span = tracing::debug_span!("actor_on_stop", killed);
                #[cfg(not(feature = "tracing"))]
                let on_stop_span = tracing::Span::none();

                if let Err(e) = run_with_actor_scope!(
                    actor_id,
                    actor.on_stop(&actor_weak, killed).instrument(on_stop_span)
                ) {
                    error!("Actor {actor_id} on_stop failed during termination: {e:?}");
                    return ActorResult::Failed {
                        actor: Some(actor),
                        error: e,
                        phase: FailurePhase::OnStop,
                        killed,
                    };
                }
                break; // Exit the loop
            }

            // Process incoming messages from the actor's mailbox
            // Messages can be: regular message envelopes or graceful stop signals
            maybe_message = receiver.recv() => {
                match maybe_message {
                    Some(MailboxMessage::Envelope { payload, reply_channel, actor_ref }) => {
                        #[cfg(feature = "tracing")]
                        let msg_span = tracing::debug_span!("actor_process_message");
                        #[cfg(not(feature = "tracing"))]
                        let msg_span = tracing::Span::none();

                        #[cfg(feature = "tracing")]
                        let start_time = std::time::Instant::now();

                        // Create metrics guard to automatically record processing time
                        // Clone actor_ref first to preserve ownership for handle_message
                        #[cfg(feature = "metrics")]
                        let metrics_ref = actor_ref.clone();
                        #[cfg(feature = "metrics")]
                        let _metrics_guard = crate::metrics::collector::MessageProcessingGuard::new(
                            metrics_ref.metrics_collector()
                        );

                        run_with_actor_scope!(
                            actor_id,
                            payload.handle_message(&mut actor, actor_ref, reply_channel)
                                .instrument(msg_span)
                        );

                        #[cfg(feature = "tracing")]
                        debug!("Actor {} processed message in {:?}", actor_id, start_time.elapsed());
                    }
                    Some(MailboxMessage::StopGracefully(_)) | None => {
                        #[cfg(feature = "tracing")]
                        debug!("Actor termination due to graceful stop");

                        // Call on_stop for graceful stop scenario
                        #[cfg(feature = "tracing")]
                        let on_stop_span = tracing::debug_span!("actor_on_stop", killed = false);
                        #[cfg(not(feature = "tracing"))]
                        let on_stop_span = tracing::Span::none();

                        if let Err(e) = run_with_actor_scope!(
                            actor_id,
                            actor.on_stop(&actor_weak, false).instrument(on_stop_span)
                        ) {
                            error!("Actor {actor_id} on_stop failed during graceful stop: {e:?}");
                            return ActorResult::Failed {
                                actor: Some(actor),
                                error: e,
                                phase: FailurePhase::OnStop,
                                killed: false,
                            };
                        }

                        break;
                    }
                }
            }

            // Execute the actor's on_run method (idle handler) when enabled.
            // This branch is disabled when on_run returns Ok(false).
            maybe_result = with_actor_scope!(
                actor_id,
                actor.on_run(&actor_weak).instrument(on_run_span)
            ), if idle_enabled => {
                match maybe_result {
                    Ok(true) => {
                        // on_run completed successfully, continue calling on_run
                    }
                    Ok(false) => {
                        // on_run indicated no more idle processing needed
                        idle_enabled = false;
                    }
                    Err(e) => {
                        // on_run returned an error - terminate the actor with failure status
                        let error_msg = format!("Actor {actor_id} on_run error: {e:?}");
                        error!("{error_msg}");

                        // Call on_stop for cleanup even after on_run failure
                        #[cfg(feature = "tracing")]
                        let on_stop_span = tracing::debug_span!("actor_on_stop", killed = false);
                        #[cfg(not(feature = "tracing"))]
                        let on_stop_span = tracing::Span::none();

                        let phase = if let Err(stop_err) = run_with_actor_scope!(
                            actor_id,
                            actor.on_stop(&actor_weak, false).instrument(on_stop_span)
                        ) {
                            error!(
                                "Actor {actor_id} on_stop failed during on_run error cleanup: {stop_err:?}"
                            );
                            FailurePhase::OnRunThenOnStop
                        } else {
                            FailurePhase::OnRun
                        };

                        return ActorResult::Failed {
                            actor: Some(actor),
                            error: e,
                            phase,
                            killed,
                        };
                    }
                }
            }
        }
    }

    // Cleanup: Close channels to signal shutdown completion
    receiver.close(); // Close the message mailbox
    terminate_receiver.close(); // Close the termination signal channel

    debug!("Actor {actor_id} lifecycle completed - exiting runtime loop.");

    // Return the final actor state - successful completion
    ActorResult::Completed { actor, killed }
}
