// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor_ref::ActorRef;
use crate::{ActorResult, ControlSignal, FailurePhase, MailboxMessage, Result};
use log::{debug, error, info, trace};
use std::{any::Any, fmt::Debug, future::Future, time::Duration};
use tokio::sync::mpsc;

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
    /// use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn, ActorResult};
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
    /// // Register message handlers
    /// impl_message_handler!(SimpleActor, []);
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

    /// The primary task execution logic for the actor, designed for iterative execution.
    ///
    /// The main processing loop for the actor. This method is called repeatedly after [`on_start`](Actor::on_start) completes.
    /// If this method returns `Ok(())`, it will be called again, allowing the actor to process
    /// ongoing or periodic tasks. The actor continues running as long as `on_run` returns `Ok(())`.
    /// To stop the actor normally from within `on_run`, call [`actor_ref.stop()`](crate::actor_ref::ActorRef::stop)
    /// or [`actor_ref.kill()`](crate::actor_ref::ActorRef::kill).
    ///
    /// `on_run`\'s execution is concurrent with the actor\'s message handling capabilities,
    /// enabling the actor to perform its primary processing while continuing to
    /// respond to incoming messages in its mailbox - a key aspect of the actor model.
    ///
    /// # Key characteristics:
    ///
    /// - **Lifecycle Management**: The actor continues its lifecycle by repeatedly executing the
    ///   `on_run` method. If `on_run` returns `Ok(())`, it will be called again, enabling
    ///   continuous processing. This supports the actor model's concept of independent,
    ///   long-lived entities. For normal termination, use `actor_ref.stop()` or `actor_ref.kill()`.
    ///
    /// - **State Persistence Across Invocations**: Because `on_run` can be invoked multiple
    ///   times by the runtime (each time generating a new `Future`), any state intended
    ///   to persist across these distinct invocations *must* be stored as fields within
    ///   the actor\'s struct (`self`). Local variables declared inside `on_run` are ephemeral
    ///   and will not be preserved if `on_run` completes and is subsequently re-invoked.
    ///
    /// - **Concurrent Message Handling**: The `Future` returned by `on_run` executes
    ///   concurrently with the actor\'s message processing loop. This allows the actor to
    ///   perform its `on_run` tasks while simultaneously remaining responsive to incoming
    ///   messages.
    ///
    /// - **Full State Access**: `on_run` has full mutable access to the actor\'s state (`self`).
    ///   Modifications to `self` within `on_run` are visible to subsequent message handlers
    ///   and future `on_run` invocations.
    ///
    /// - **Essential Await Points**: The `Future` returned by `on_run` must yield control
    ///   to the Tokio runtime via `.await` points, especially within any internal loops.
    ///   Lacking these, the `on_run` task could block the actor\'s ability to process messages
    ///   or perform other concurrent activities.
    ///
    /// # Common patterns:
    ///
    /// 1. **Periodic tasks**: For executing work at regular intervals. The `on_run` future
    ///    would typically involve a single tick of an interval. The `Interval` itself
    ///    should be stored as a field in the actor\'s struct to persist across `on_run` invocations.
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
    ///    # async fn on_start(duration: Self::Args, _actor_ref: &ActorRef<MyActor>) -> std::result::Result<Self, Self::Error> {
    ///    #     let mut interval = tokio::time::interval(duration);
    ///    #     interval.set_missed_tick_behavior(MissedTickBehavior::Delay); // Or Skip, Burst
    ///    #     Ok(MyActor { interval, ticks_done: 0 })
    ///    # }
    ///    async fn on_run(&mut self, actor_ref: &ActorRef<MyActor>) -> std::result::Result<(), Self::Error> { // Note: Return type is Result<(), Self::Error>
    ///        // self.interval is stored in the MyActor struct.
    ///        self.interval.tick().await; // This await point allows message processing.
    ///
    ///        // Perform the periodic task here.
    ///        println!("Periodic task executed by actor {}", actor_ref.identity());
    ///        self.ticks_done += 1;
    ///
    ///        // If your task is computationally intensive, ensure you still have an await
    ///        // or offload it (e.g., using [`tokio::task::spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)).
    ///        if self.heavy_computation_needed() {
    ///            // Example: Offload heavy work if truly blocking
    ///            // let _ = tokio::task::spawn_blocking(|| { /* heavy work */ }).await?;
    ///            // Or, if it\'s async but long-running, ensure it yields:
    ///            tokio::task::yield_now().await;
    ///        }
    ///
    ///        // To stop the actor normally from within on_run, call actor_ref.stop() or actor_ref.kill().
    ///        // For example, to stop after 10 ticks:
    ///        if self.ticks_done >= 10 {
    ///            println!("Actor {} stopping after {} ticks.", actor_ref.identity(), self.ticks_done);
    ///            actor_ref.stop().await?; // or actor_ref.kill()?
    ///            // After calling stop/kill, on_run might not be called again as the actor shuts down.
    ///            // It\'s good practice to return Ok(()) here, or handle potential errors from stop().
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
    /// - If the `Future` returned by `on_run` completes with `Ok(())`, the actor continues running,
    ///   and the runtime will invoke `on_run` again to get the next `Future` for execution.
    /// - If the `Future` completes with `Err(_)`, the actor terminates due to an error. See the
    ///   [`Error Handling`](#error-handling) section in the `Actor` trait documentation for details
    ///   on how errors are handled.
    ///
    /// To stop the actor normally from within `on_run` (e.g., graceful shutdown),
    /// the actor should explicitly call `actor_ref.stop().await?` or `actor_ref.kill()`.
    /// After such a call, `on_run` is unlikely to be invoked again by the runtime,
    /// as the actor will be in the process of shutting down.
    ///
    /// The `actor_ref` parameter is a reference to the actor\'s own `ActorRef`.
    /// It can be used, for example, to call `actor_ref.stop()` or `actor_ref.kill()`
    /// to initiate actor termination from within `on_run`.
    ///
    /// The default implementation of `on_run` is a simple async block that sleeps for 1 second
    /// and then returns `Ok(())`, causing it to be called repeatedly until the actor is
    /// explicitly stopped or killed.
    #[allow(unused_variables)]
    fn on_run(
        &mut self,
        actor_ref: &ActorRef<Self>,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // This sleep is critical - it creates an await point that allows
        // the Tokio runtime to switch tasks and process incoming messages.
        // Without at least one await point in a loop, message processing would starve.
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(())
        }
    }

    /// Called when the actor is about to stop. This allows the actor to perform cleanup tasks.
    ///
    /// This method is called only when the actor is being explicitly stopped, either through a call
    /// to [`ActorRef::stop`](crate::actor_ref::ActorRef::stop) (graceful termination) or
    /// [`ActorRef::kill`](crate::actor_ref::ActorRef::kill) (immediate termination). It is not called
    /// if the actor stops due to other errors that occurred during message processing or in [`on_run`](Actor::on_run).
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
    /// # use rsactor::{Actor, ActorRef, Result};
    /// # use std::time::Duration;
    /// # struct MyActor { /* ... */ }
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
    /// #         Ok(MyActor { /* ... */ })
    /// #     }
    /// async fn on_stop(&mut self, actor_ref: &ActorRef<Self>, killed: bool) -> std::result::Result<(), Self::Error> {
    ///     if killed {
    ///         println!("Actor {} is being forcefully terminated, performing minimal cleanup", actor_ref.identity());
    ///         // Perform minimal, fast cleanup
    ///     } else {
    ///         println!("Actor {} is gracefully shutting down, performing full cleanup", actor_ref.identity());
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
        actor_ref: &ActorRef<Self>,
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
}

/// A trait for type-erased message handling within the actor system.
///
/// This trait is typically implemented automatically by the [`impl_message_handler!`](crate::impl_message_handler) macro.
/// It enables the actor system to handle messages of different types by downcasting
/// them to their concrete types before passing them to the actor's specific [`Message::handle`](crate::actor::Message::handle)
/// implementation. This trait powers the message processing capabilities of actors.
pub trait MessageHandler: Actor + Send + 'static {
    /// Handles a type-erased message.
    ///
    /// The implementation should attempt to downcast `msg_any` to one of the
    /// message types the actor supports and then call the corresponding
    /// [`Message::handle`](crate::actor::Message::handle) method.
    fn handle(
        &mut self,
        msg_any: Box<dyn Any + Send>,
        actor_ref: &ActorRef<Self>,
    ) -> impl Future<Output = Result<Box<dyn Any + Send>>> + Send;
}

// This method encapsulates the actor's entire lifecycle within its spawned task.
// It handles on_start, message processing, then returns the actor result.
// Consumes self to return the actor.
pub(crate) async fn run_actor_lifecycle<T: Actor + MessageHandler>(
    args: T::Args,
    actor_ref: ActorRef<T>,
    mut receiver: mpsc::Receiver<MailboxMessage>,
    mut terminate_receiver: mpsc::Receiver<ControlSignal>,
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
                killed: false,
            };
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
    ActorResult::Completed {
        actor,
        killed: was_killed,
    }
}
