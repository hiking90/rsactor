// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor_ref::ActorRef;
use crate::{ActorResult, ActorWeak, ControlSignal, FailurePhase, MailboxMessage};
use futures::stream::{BoxStream, SelectAll, StreamExt};
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc;
use tracing::{debug, error, Instrument};

/// Boxed, dynamically-typed stream of idle events delivered to an actor's [`Actor::on_idle`]
/// handler. Subscriptions are added via [`ActorRef::subscribe_idle`].
pub(crate) type IdleEventStream<T> = BoxStream<'static, <T as Actor>::IdleEvent>;

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

/// Process a single Envelope from any of the message-receiving paths
/// (regular mailbox, priority mailbox, or stop-time priority drain).
///
/// Centralizes the tracing span, optional metrics guard, and dispatch boilerplate
/// so the three call sites stay in sync.
macro_rules! process_envelope {
    (
        actor_id = $actor_id:expr,
        actor = $actor:expr,
        payload = $payload:expr,
        reply_channel = $reply_channel:expr,
        actor_ref = $actor_ref:expr,
        span_name = $span_name:literal,
        guard = $guard_type:ident $(,)?
    ) => {{
        #[cfg(feature = "tracing")]
        let msg_span = tracing::debug_span!($span_name);
        #[cfg(not(feature = "tracing"))]
        let msg_span = tracing::Span::none();

        #[cfg(feature = "tracing")]
        let start_time = std::time::Instant::now();

        // Hold the metrics collector by Arc clone (1 atomic increment) rather
        // than cloning the entire ActorRef (which would clone every channel
        // sender — 4 atomic increments). The collector must outlive the move
        // of $actor_ref into handle_message below.
        #[cfg(feature = "metrics")]
        let metrics_collector = $actor_ref.metrics.clone();
        #[cfg(feature = "metrics")]
        let _metrics_guard = crate::metrics::collector::$guard_type::new(&metrics_collector);

        run_with_actor_scope!(
            $actor_id,
            $payload
                .handle_message(&mut $actor, $actor_ref, $reply_channel)
                .instrument(msg_span)
        );

        #[cfg(feature = "tracing")]
        debug!(
            "Actor {} processed {} in {:?}",
            $actor_id,
            $span_name,
            start_time.elapsed()
        );
    }};
}

/// Defines the behavior of an actor.
///
/// Actors are fundamental units of computation that communicate by exchanging messages.
/// Each actor has its own state and processes messages sequentially.
///
/// # Error Handling
///
/// Actor lifecycle methods ([`on_start`](Actor::on_start), [`on_idle`](Actor::on_idle), [`on_stop`](Actor::on_stop)) can return errors. How these errors
/// are handled depends on when they occur:
///
/// 1. Errors in [`on_start`](Actor::on_start): The actor fails to initialize. The error is captured in
///    [`ActorResult::Failed`](crate::ActorResult::Failed) with `phase` set to
///    [`FailurePhase::OnStart`](crate::FailurePhase::OnStart) and `actor` set to `None`.
///
/// 2. Errors in [`on_idle`](Actor::on_idle): The actor terminates during runtime. The error is captured in
///    [`ActorResult::Failed`](crate::ActorResult::Failed) with `phase` set to
///    [`FailurePhase::OnIdle`](crate::FailurePhase::OnIdle) and `actor` contains the actor instance.
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
    /// The error type that can be returned by the actor's lifecycle methods.
    /// Used in [`on_start`](Actor::on_start), [`on_idle`](Actor::on_idle), and [`on_stop`](Actor::on_stop).
    type Error: Send + Debug;
    /// Event type carried by streams subscribed via
    /// [`ActorRef::subscribe_idle`](crate::actor_ref::ActorRef::subscribe_idle) and dispatched
    /// to [`on_idle`](Actor::on_idle).
    ///
    /// Actors that never subscribe an idle stream should set this to `()`. The
    /// `#[derive(Actor)]` macro fills this in automatically. When implementing
    /// [`Actor`] manually, declare it explicitly:
    ///
    /// ```rust,ignore
    /// impl Actor for MyActor {
    ///     type Args = ();
    ///     type Error = anyhow::Error;
    ///     type IdleEvent = ();
    ///     // ...
    /// }
    /// ```
    type IdleEvent: Send + 'static;

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
    ///     type IdleEvent = ();
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

    /// Handler invoked for each event yielded by streams subscribed via
    /// [`ActorRef::subscribe_idle`](crate::actor_ref::ActorRef::subscribe_idle).
    ///
    /// The runtime owns a [`SelectAll`](futures::stream::SelectAll) of every subscribed
    /// stream and polls it as one branch of its `select!` loop. When any stream yields an
    /// event, `on_idle` is called with `&mut self` and full mutable access to actor state —
    /// no concurrent borrow with message handlers, no surprise cancellation of work in
    /// progress. A returned `Err` terminates the actor with
    /// [`FailurePhase::OnIdle`](crate::FailurePhase::OnIdle).
    ///
    /// # Why a stream — not a free-form async block
    ///
    /// In rsActor 0.15 and earlier, idle work was expressed via `on_run`, an async method the
    /// runtime called repeatedly inside `select!`. Each iteration produced a *new* future, so
    /// any timing or async state created inside `on_run` (e.g. a raw `tokio::time::sleep`)
    /// was dropped whenever a message arrived — a 1-second sleep that races with frequent
    /// messages may never fire.
    ///
    /// Subscription-based idle events move the timing/event state out of the cancellable
    /// future and into a [`Stream`](futures::Stream) owned by the runtime. Stream
    /// implementations like [`IntervalStream`](https://docs.rs/tokio-stream) are
    /// cancel-safe by construction: even when the surrounding `select!` arm is cancelled,
    /// the stream's internal schedule survives across iterations.
    ///
    /// # Priority relative to messages
    ///
    /// The runtime's `select!` is `biased` with this order: terminate signal, priority
    /// mailbox, new subscriptions, regular mailbox, idle events. Idle events therefore
    /// have *lower* priority than messages — high message throughput can starve idle
    /// processing. This mirrors the previous `on_run` semantics. If you need strict
    /// fairness, send self-tells from a separate task instead.
    ///
    /// # Common patterns
    ///
    /// **Periodic tick:** subscribe an [`IntervalStream`](https://docs.rs/tokio-stream)
    /// in `on_start` and react in `on_idle`.
    ///
    /// ```rust,no_run
    /// # use rsactor::{Actor, ActorRef, ActorWeak};
    /// # use anyhow::Result;
    /// # use std::time::Duration;
    /// # use futures::stream::StreamExt;
    /// # struct MyActor { ticks: u32 }
    /// # impl Actor for MyActor {
    /// # type Args = ();
    /// # type Error = anyhow::Error;
    /// # type IdleEvent = ();
    /// # async fn on_start(_: (), actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
    /// #     // pseudo-code: wrap a tokio::time::Interval as a stream and subscribe
    /// #     // actor_ref.subscribe_idle(IntervalStream::new(...).map(|_| ()))?;
    /// #     Ok(MyActor { ticks: 0 })
    /// # }
    /// async fn on_idle(&mut self, _: (), actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
    ///     self.ticks += 1;
    ///     println!("tick {} on actor {}", self.ticks, actor_weak.identity());
    ///     Ok(())
    /// }
    /// # }
    /// ```
    ///
    /// **Dynamic subscription:** add a new idle source from a message handler.
    ///
    /// ```rust,ignore
    /// async fn handle_start_sensor(&mut self, _: StartSensor, actor_ref: &ActorRef<Self>) {
    ///     actor_ref
    ///         .subscribe_idle(self.sensor.events().map(MyEvent::SensorTick))
    ///         .ok();
    /// }
    /// ```
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(())` immediately. Actors that never subscribe an idle stream
    /// can leave this unimplemented and set `type IdleEvent = ();`.
    #[allow(unused_variables)]
    fn on_idle(
        &mut self,
        event: Self::IdleEvent,
        actor_weak: &ActorWeak<Self>,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    /// Called when the actor is about to stop. This allows the actor to perform cleanup tasks.
    ///
    /// This method is called when the actor is stopping, including:
    /// - Explicit stop via [`ActorRef::stop`](crate::actor_ref::ActorRef::stop) (graceful termination)
    /// - Explicit kill via [`ActorRef::kill`](crate::actor_ref::ActorRef::kill) (immediate termination)
    /// - Cleanup after an [`on_idle`](Actor::on_idle) error
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
    /// #     type IdleEvent = ();
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
///                    │ + on_idle   │
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
    mut priority_receiver: Option<mpsc::Receiver<MailboxMessage<T>>>,
    mut terminate_receiver: mpsc::Receiver<ControlSignal>,
    idle_subscribe_receiver: mpsc::Receiver<IdleEventStream<T>>,
) -> ActorResult<T> {
    // Track the subscribe channel as Option to mirror the priority-channel
    // close-detection pattern: when all strong senders drop and `recv()`
    // returns None, replace with `None` so the select arm becomes pending and
    // does not busy-loop. We still allow normal shutdown via the mailbox arm.
    let mut idle_subscribe_receiver = Some(idle_subscribe_receiver);
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

    // Aggregates every stream registered via `ActorRef::subscribe_idle`. New
    // streams arrive on `idle_subscribe_receiver`; completed streams are removed
    // automatically by `SelectAll`. The branch below is guarded so the runtime
    // does not poll the empty set forever — `futures::stream::SelectAll::next()`
    // on an empty set returns `Poll::Ready(None)` immediately, which would
    // busy-loop without the guard.
    let mut idle_streams: SelectAll<IdleEventStream<T>> = SelectAll::new();

    // Drain any subscriptions queued during `on_start`. The subscribe arm in
    // the main loop would install them one per select! iteration; doing this
    // synchronously up front means the very first iteration can already
    // service idle events and avoids N round-trips through the select! when
    // `on_start` calls `subscribe_idle` N times.
    if let Some(rx) = idle_subscribe_receiver.as_mut() {
        while let Ok(stream) = rx.try_recv() {
            idle_streams.push(stream);
        }
    }

    // Main actor processing loop. The `tokio::select!` is `biased` with this
    // priority order: termination signals → priority mailbox → new idle
    // subscriptions → regular mailbox → idle events.
    loop {
        #[cfg(feature = "tracing")]
        let on_idle_span = tracing::debug_span!("actor_on_idle");
        #[cfg(not(feature = "tracing"))]
        let on_idle_span = tracing::Span::none();

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

            // Process priority messages with higher priority than the regular mailbox.
            // When the priority channel is disabled (`None`), this branch becomes a
            // never-ready future and is effectively skipped on every iteration.
            maybe_priority = async {
                match priority_receiver.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending::<Option<MailboxMessage<T>>>().await,
                }
            } => {
                match maybe_priority {
                    Some(MailboxMessage::Envelope { payload, reply_channel, actor_ref }) => {
                        process_envelope!(
                            actor_id = actor_id,
                            actor = actor,
                            payload = payload,
                            reply_channel = reply_channel,
                            actor_ref = actor_ref,
                            span_name = "actor_process_priority_message",
                            guard = PriorityMessageProcessingGuard,
                        );
                    }
                    Some(MailboxMessage::StopGracefully(_)) => {
                        // Invariant: stop() only writes to the regular mailbox, never the
                        // priority channel. Hitting this arm would mean a future change
                        // started routing StopGracefully through priority too — flag it
                        // loudly in debug builds.
                        debug_assert!(
                            false,
                            "MailboxMessage::StopGracefully observed on the priority channel; \
                             priority channel must only carry Envelope variants"
                        );
                    }
                    None => {
                        // All strong priority senders were dropped. A closed receiver
                        // returns Ready(None) on every poll, which would busy-loop the
                        // priority branch forever. Disable the branch by clearing the
                        // receiver — subsequent iterations hit the `None` arm of the
                        // async block above and resolve to `pending()`. The actor stays
                        // alive on the regular mailbox until normal termination.
                        priority_receiver = None;
                    }
                }
            }

            // Install newly subscribed idle streams as soon as they arrive.
            // Subscriptions are typically pushed from `on_start` (before this
            // loop runs the first time) or from a message handler that just
            // completed. Polling this branch ahead of the regular mailbox
            // ensures freshly-added streams are active before the next message
            // is processed. When the channel closes (all `ActorRef`/`ActorWeak`
            // strong senders dropped) we set the option to None so the branch
            // becomes pending — graceful termination then propagates via the
            // mailbox arm below.
            maybe_subscription = async {
                match idle_subscribe_receiver.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending::<Option<IdleEventStream<T>>>().await,
                }
            } => {
                match maybe_subscription {
                    Some(stream) => {
                        idle_streams.push(stream);
                    }
                    None => {
                        idle_subscribe_receiver = None;
                    }
                }
            }

            // Process incoming messages from the actor's mailbox
            // Messages can be: regular message envelopes or graceful stop signals
            maybe_message = receiver.recv() => {
                match maybe_message {
                    Some(MailboxMessage::Envelope { payload, reply_channel, actor_ref }) => {
                        process_envelope!(
                            actor_id = actor_id,
                            actor = actor,
                            payload = payload,
                            reply_channel = reply_channel,
                            actor_ref = actor_ref,
                            span_name = "actor_process_message",
                            guard = MessageProcessingGuard,
                        );
                    }
                    Some(MailboxMessage::StopGracefully(_)) | None => {
                        #[cfg(feature = "tracing")]
                        debug!("Actor termination due to graceful stop");

                        // Drain any priority messages already enqueued before stopping.
                        // close() refuses new priority sends (they get Closed errors and
                        // are recorded as dead letters), then try_recv() pulls everything
                        // currently in the slot. This closes the race where a sender
                        // pushed a priority message just before stop() arrived.
                        if let Some(rx) = priority_receiver.as_mut() {
                            rx.close();
                            while let Ok(msg) = rx.try_recv() {
                                match msg {
                                    MailboxMessage::Envelope { payload, reply_channel, actor_ref } => {
                                        process_envelope!(
                                            actor_id = actor_id,
                                            actor = actor,
                                            payload = payload,
                                            reply_channel = reply_channel,
                                            actor_ref = actor_ref,
                                            span_name = "actor_drain_priority_message",
                                            guard = PriorityMessageProcessingGuard,
                                        );
                                    }
                                    MailboxMessage::StopGracefully(_) => {
                                        // Same invariant as the priority select! branch:
                                        // stop() never writes to the priority channel.
                                        debug_assert!(
                                            false,
                                            "MailboxMessage::StopGracefully observed during \
                                             priority drain; priority channel must only \
                                             carry Envelope variants"
                                        );
                                    }
                                }
                            }
                        }

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

            // Dispatch one event from the merged set of subscribed idle streams
            // to the actor's `on_idle` handler. The `if !idle_streams.is_empty()`
            // guard is essential: `SelectAll::next()` on an empty set resolves
            // to `Poll::Ready(None)` immediately and would busy-loop.
            //
            // Each underlying stream is responsible for its own cancel-safety.
            // Streams that complete (return None) are removed from the set
            // automatically by `SelectAll`.
            Some(event) = idle_streams.next(), if !idle_streams.is_empty() => {
                let result = run_with_actor_scope!(
                    actor_id,
                    actor.on_idle(event, &actor_weak).instrument(on_idle_span)
                );

                if let Err(e) = result {
                    let error_msg = format!("Actor {actor_id} on_idle error: {e:?}");
                    error!("{error_msg}");

                    // Call on_stop for cleanup even after on_idle failure
                    #[cfg(feature = "tracing")]
                    let on_stop_span = tracing::debug_span!("actor_on_stop", killed = false);
                    #[cfg(not(feature = "tracing"))]
                    let on_stop_span = tracing::Span::none();

                    let phase = if let Err(stop_err) = run_with_actor_scope!(
                        actor_id,
                        actor.on_stop(&actor_weak, false).instrument(on_stop_span)
                    ) {
                        error!(
                            "Actor {actor_id} on_stop failed during on_idle error cleanup: {stop_err:?}"
                        );
                        FailurePhase::OnIdleThenOnStop
                    } else {
                        FailurePhase::OnIdle
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

    // Cleanup: Close channels to signal shutdown completion
    receiver.close(); // Close the message mailbox
    terminate_receiver.close(); // Close the termination signal channel
    if let Some(rx) = priority_receiver.as_mut() {
        rx.close(); // Close the optional priority channel
    }
    if let Some(rx) = idle_subscribe_receiver.as_mut() {
        rx.close(); // Close the idle subscribe channel
    }

    debug!("Actor {actor_id} lifecycle completed - exiting runtime loop.");

    // Return the final actor state - successful completion
    ActorResult::Completed { actor, killed }
}
