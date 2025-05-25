// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor_ref::ActorRef;
use crate::{Result, ActorResult, MailboxMessage, ControlSignal, FailurePhase};
use std::{any::Any, fmt::Debug, future::Future, time::Duration};
use tokio::sync::mpsc;
use log::{info, error, debug, trace};

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
    /// The error type that can be returned by the actor\'s lifecycle methods.
    type Error: Send + Debug + 'static;

    /// Called when the actor is started. This is required for actor creation.
    ///
    /// The `args` parameter, of type `Self::Args`, contains the initialization data
    /// provided when the actor is spawned. This method is responsible for using
    /// these arguments to create and return the actual actor instance (`Self`).
    /// The `actor_ref` parameter is a reference to the actor\'s own `AnyActorRef`.
    /// This method should return the initialized actor instance or an error.
    fn on_start(_args: Self::Args, _actor_ref: ActorRef<Self>) -> impl Future<Output = std::result::Result<Self, Self::Error>> + Send;

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
    /// `on_run`\'s execution is concurrent with the actor\'s message handling capabilities,
    /// enabling the actor to perform background or main-loop tasks while continuing to
    /// process incoming messages from its mailbox.
    ///
    /// # Key characteristics:
    ///
    /// - **Iterative Execution**: The `rsactor` runtime invokes `on_run` to obtain a `Future`.
    ///   If this `Future` completes with `Ok(())`, `on_run` will be called again by the
    ///   runtime to obtain a new `Future`. This allows for continuous or step-by-step
    ///   task processing throughout the actor\'s active lifecycle. The actor continues as long
    ///   as `Err(_)` is not returned. For normal termination from within `on_run`,
    ///   use `actor_ref.stop()` or `actor_ref.kill()`.
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
    ///    # async fn on_start(duration: Self::Args, _actor_ref: ActorRef<MyActor>) -> std::result::Result<Self, Self::Error> {
    ///    #     let mut interval = tokio::time::interval(duration);
    ///    #     interval.set_missed_tick_behavior(MissedTickBehavior::Delay); // Or Skip, Burst
    ///    #     Ok(MyActor { interval, ticks_done: 0 })
    ///    # }
    ///    async fn on_run(&mut self, actor_ref: ActorRef<MyActor>) -> std::result::Result<(), Self::Error> { // Note: Return type is Result<(), Self::Error>
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
    /// - If the `Future` returned by `on_run` completes with `Err(_)`, the actor terminates due to an error.
    /// - If the `Future` completes with `Ok(())`, the actor continues running, and the runtime
    ///   will invoke `on_run` again to get the next `Future` for execution.
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
    /// - Returning `Ok(())` signifies that the current segment of work completed successfully,
    ///   and the actor should continue running (i.e., `on_run` will be called again).
    /// - Returning `Err(_)` signifies termination due to an error.
    /// The specific outcome is captured in the `ActorResult`.
    ///
    /// The default implementation of `on_run` is a simple async block that sleeps for 1 second
    /// and then returns `Ok(())`, causing it to be called repeatedly until the actor is
    /// explicitly stopped or killed.
    fn on_run(&mut self, _actor_ref: ActorRef<Self>) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // This sleep is critical - it creates an await point that allows
        // the Tokio runtime to switch tasks and process incoming messages.
        // Without at least one await point in a loop, message processing would starve.
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(())
        }
    }

    fn on_stop(&mut self, _actor_ref: ActorRef<Self>, _killed: bool) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send {
        // Default implementation does nothing on stop.
        // Override this method in your actor if you need to perform cleanup.
        async { Ok(()) }
    }
}

/// A trait for messages that an actor can handle, defining the reply type.
///
/// An actor struct implements this trait for each specific message type it can process.
pub trait Message<T: Send + 'static> {
    /// The type of the reply that will be sent back to the caller.
    type Reply: Send + 'static;

    /// Handles the incoming message and produces a reply.
    ///
    /// The `actor_ref` parameter is a reference to the actor\'s own `ActorRef`.
    /// This is an asynchronous method where the actor\'s business logic for
    /// processing the message `T` resides.
    fn handle(&mut self, msg: T, actor_ref: ActorRef<Self>) -> impl Future<Output = Self::Reply> + Send
    where
        Self: Actor;
}

/// A trait for type-erased message handling within the actor\'s `Runtime`.
///
/// This trait is typically implemented automatically by the `impl_message_handler!` macro.
/// It allows the `Runtime` to handle messages of different types by downcasting
/// them to their concrete types before passing them to the actor\'s specific `Message::handle`
/// implementation.
pub trait MessageHandler: Actor + Send + 'static {
    /// Handles a type-erased message.
    ///
    /// The implementation should attempt to downcast `msg_any` to one of the
    /// message types the actor supports and then call the corresponding
    /// `Message::handle` method.
    fn handle(
        &mut self,
        msg_any: Box<dyn Any + Send>,
        actor_ref: ActorRef<Self>,
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

    let mut actor = match T::on_start(args, actor_ref.clone()).await {
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
                    if let Err(e) = actor.on_stop(actor_ref.clone(), true).await {
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
                        match actor.handle(payload, actor_ref.clone()).await {
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
                        if let Err(e) = actor.on_stop(actor_ref.clone(), false).await {
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

            maybe_result = actor.on_run(actor_ref.clone()) => {
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
