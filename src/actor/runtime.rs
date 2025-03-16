use std::fmt::Debug;
use std::panic::AssertUnwindSafe;

use tokio::task::JoinHandle;

use crate::{
    Errors, IMessage,
    actor::{
        IActor, Context, ActorId, ActorStatus, State, 
        Message, Signal,
        Receivers, SupervisionEvent,
        ActorPortMessage, StopMessage
    },
};

/// Helper struct for tracking the results from actor processing loops
#[doc(hidden)]
struct ActorLoopResult {
    should_exit: bool,
    exit_reason: Option<String>,
    was_killed: bool,
}

impl ActorLoopResult {
    pub(crate) fn ok() -> Self {
        Self {
            should_exit: false,
            exit_reason: None,
            was_killed: false,
        }
    }

    pub(crate) fn stop(reason: Option<String>) -> Self {
        Self {
            should_exit: true,
            exit_reason: reason,
            was_killed: false,
        }
    }

    pub(crate) fn signal(signal_str: Option<String>) -> Self {
        Self {
            should_exit: true,
            exit_reason: signal_str,
            was_killed: true,
        }
    }
}

/// [ActorRuntime] is a struct which represents the processing actor.
///
///  This struct is consumed by the `start` operation, but results in an
/// [ActorRef] to communicate and operate with along with the [JoinHandle]
/// representing the actor's async processing loop.
pub struct Runtime<TActor>
where
    TActor: IActor,
{
    actor_ref: Context,
    handler: TActor,
    id: ActorId,
    name: Option<String>,
}

impl<TActor: IActor> Debug for Runtime<TActor> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRuntime")
            .field("name", &self.name)
            .field("id", &self.id)
            .finish()
    }
}

impl<TActor> Runtime<TActor>
where
    TActor: IActor,
{
    /// Spawn an actor, which is unsupervised, automatically starting the actor
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The [Actor] defining the logic for this actor
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    ///   initial state creation
    ///
    /// Returns a [Ok((ActorRef, JoinHandle<()>))] upon successful start, denoting the actor reference
    /// along with the join handle which will complete when the actor terminates. Returns [Err(SpawnErr)] if
    /// the actor failed to start
    pub async fn spawn(
        system: &crate::System,
        name: Option<String>,
        handler: TActor,
    ) -> Result<(Context, JoinHandle<()>), Errors> {
        let (actor, ports) = Self::new(system, name, handler)?;
        let context = actor.actor_ref.clone();
        let result = actor.start(ports, None).await;
        if result.is_err() {
            context.set_status(ActorStatus::Stopped);
        }
        result
    }

    /// Spawn an actor with a supervisor, automatically starting the actor
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The [Actor] defining the logic for this actor
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    ///   initial state creation
    /// * `supervisor`: The [Context] which is to become the supervisor (parent) of this actor
    ///
    /// Returns a [Ok((ActorRef, JoinHandle<()>))] upon successful start, denoting the actor reference
    /// along with the join handle which will complete when the actor terminates. Returns [Err(SpawnErr)] if
    /// the actor failed to start
    pub async fn spawn_linked(
        system: &crate::System,
        name: Option<String>,
        handler: TActor,
        supervisor: Context,
    ) -> Result<(Context, JoinHandle<()>), Errors> {
        let (actor, ports) = Self::new(system, name, handler)?;
        let context = actor.actor_ref.clone();
        let result = actor.start(ports, Some(supervisor)).await;
        if result.is_err() {
            context.set_status(ActorStatus::Stopped);
        }
        result
    }

    /// Spawn an actor instantly, not waiting on the actor's `pre_start` routine. This is helpful
    /// for actors where you want access to the send messages into the actor's message queue
    /// without waiting on an asynchronous context.
    ///
    /// **WARNING** Failures in the pre_start routine need to be waited on in the join handle
    /// since they will NOT fail the spawn operation in this context
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The [Actor] defining the logic for this actor
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    ///   initial state creation
    ///
    /// Returns a [Ok((ActorRef, JoinHandle<Result<JoinHandle<()>, Errors>>))] upon successful creation of the
    /// message queues, so you can begin sending messages. However the associated [JoinHandle] contains the inner
    /// information around if the actor successfully started or not in it's `pre_start` routine. Returns [Err(SpawnErr)] if
    /// the actor name is already allocated
    #[allow(clippy::type_complexity)]
    pub fn spawn_instant(
        system: &crate::System,
        name: Option<String>,
        handler: TActor,
    ) -> Result<
        (
            Context,
            JoinHandle<Result<JoinHandle<()>, Errors>>,
        ),
        Errors,
    > {
        let (actor, ports) = Self::new(system, name.clone(), handler)?;
        let actor_ref = actor.actor_ref.clone();
        let actor_ref2 = actor_ref.clone();
        let join_op = tokio::spawn(async move {
            let result = actor.start(ports, None).await;
            if result.is_err() {
                actor_ref2.set_status(ActorStatus::Stopped);
            }
            let (_, handle) = result?;
            Ok(handle)
        });
        Ok((actor_ref, join_op))
    }

    /// Spawn an actor instantly with supervision, not waiting on the actor's `pre_start` routine.
    /// This is helpful for actors where you want access to the send messages into the actor's
    /// message queue without waiting on an asynchronous context.
    ///
    /// **WARNING** Failures in the pre_start routine need to be waited on in the join handle
    /// since they will NOT fail the spawn operation in this context. Additionally the supervision
    /// tree will **NOT** be linked until the `pre_start` completes so there is a chance an actor
    /// is lost during `pre_start` and not successfully started unless it's specifically handled
    /// by the caller by awaiting later.
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The [Actor] defining the logic for this actor
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    ///   initial state creation
    /// * `supervisor`: The [Context] which is to become the supervisor (parent) of this actor
    ///
    /// Returns a [Ok((ActorRef, JoinHandle<Result<JoinHandle<()>, Errors>>))] upon successful creation of the
    /// message queues, so you can begin sending messages. However the associated [JoinHandle] contains the inner
    /// information around if the actor successfully started or not in it's `pre_start` routine. Returns [Err(SpawnErr)] if
    /// the actor name is already allocated
    #[allow(clippy::type_complexity)]
    pub fn spawn_linked_instant(
        system: &crate::System,
        name: Option<String>,
        handler: TActor,
        supervisor: Context,
    ) -> Result<
        (
            Context,
            JoinHandle<Result<JoinHandle<()>, Errors>>,
        ),
        Errors,
    > {
        let (actor, ports) = Self::new(system, name.clone(), handler)?;
        let actor_ref = actor.actor_ref.clone();
        let actor_ref2 = actor_ref.clone();
        let join_op = tokio::spawn(async move {
            let result = actor.start(ports, Some(supervisor)).await;
            if result.is_err() {
                actor_ref2.set_status(ActorStatus::Stopped);
            }
            let (_, handle) = result?;
            Ok(handle)
        });
        Ok((actor_ref, join_op))
    }

    /// Create a new actor with some handler implementation and initial state
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The [Actor] defining the logic for this actor
    ///
    /// Returns A tuple [(Actor, Receivers)] to be passed to the `start` function of [Actor]
    fn new(system: &crate::System, name: Option<String>, handler: TActor) -> Result<(Self, Receivers), Errors> {
        let (actor_cell, ports) = Context::new::<TActor>(system, name)?;
        let id = actor_cell.get_id();
        let name = actor_cell.get_name().map(|s| s.to_string());
        Ok((
            Self {
                actor_ref: actor_cell.into(),
                handler,
                id,
                name,
            },
            ports,
        ))
    }

    /// Start the actor immediately, optionally linking to a parent actor (supervision tree)
    ///
    /// NOTE: This returned [crate::concurrency::JoinHandle] is guaranteed to not panic (unless the runtime is shutting down perhaps).
    /// An inner join handle is capturing panic results from any part of the inner tasks, so therefore
    /// we can safely ignore it, or wait on it to block on the actor's progress
    ///
    /// * `ports` - The [Receivers] for this actor
    /// * `supervisor` - The optional [Context] representing the supervisor of this actor
    ///
    /// Returns a [Ok((ActorRef, JoinHandle<()>))] upon successful start, denoting the actor reference
    /// along with the join handle which will complete when the actor terminates. Returns [Err(SpawnErr)] if
    /// the actor failed to start
    #[tracing::instrument(name = "Actor", skip(self, ports, supervisor), fields(id = self.id.to_string(), name = self.name))]
    async fn start(
        self,
        ports: Receivers,
        supervisor: Option<Context>,
    ) -> Result<(Context, JoinHandle<()>), Errors> {
        // cannot start an actor more than once
        if self.actor_ref.get_status() != ActorStatus::Unstarted {
            return Err(Errors::ActorAlreadyStarted);
        }

        let Self {
            handler,
            actor_ref,
            id,
            name,
        } = self;

        actor_ref.set_status(ActorStatus::Starting);

        // Perform the pre-start routine, crashing immediately if we fail to start
        let mut state = Self::do_pre_start(actor_ref.clone(), &handler)
            .await?
            .map_err(|err| err)?;

            // setup supervision
        if let Some(sup) = &supervisor {
            actor_ref.link(sup.clone());
        }

        // Generate the ActorRef which will be returned
        let myself_ret = actor_ref.clone();

        // run the processing loop, backgrounding the work
        let handle = tokio::spawn(async move {
            let myself = actor_ref.clone();
            let evt = match Self::processing_loop(ports, &mut state, &handler, actor_ref, id, name)
                .await
            {
                Ok(exit_reason) => SupervisionEvent::Terminated(
                    myself.clone(),
                    Some(State::new(state)),
                    exit_reason,
                ),
                Err(actor_err) => match actor_err {
                    Errors::ActorCancelled => SupervisionEvent::Terminated(
                        myself.clone(),
                        None,
                        Some("killed".to_string()),
                    ),
                    _ => SupervisionEvent::Failed(myself.clone(), actor_err),
                },
            };

            // terminate children
            myself.terminate();

            // notify supervisors of the actor's death
            myself.notify_supervisor(evt);

            // unlink superisors
            if let Some(sup) = supervisor {
                myself.unlink(sup);
            }

            // set status to stopped
            myself.set_status(ActorStatus::Stopped);
        });

        Ok((myself_ret, handle))
    }

    #[tracing::instrument(name = "Actor", skip(ports, state, handler, myself, _id, _name), fields(id = _id.to_string(), name = _name))]
    async fn processing_loop(
        mut ports: Receivers,
        state: &mut TActor::State,
        handler: &TActor,
        myself: Context,
        _id: ActorId,
        _name: Option<String>,
    ) -> Result<Option<String>, Errors> {
        // perform the post-start, with supervision enabled
        Self::do_post_start(myself.clone(), handler, state)
            .await?;

        myself.set_status(ActorStatus::Running);
        myself.notify_supervisor(SupervisionEvent::Started(myself.clone()));

        let myself_clone = myself.clone();

        let future = async move {
            // the message processing loop. If we get an exit flag, try and capture the exit reason if there
            // is one
            loop {
                let ActorLoopResult {
                    should_exit,
                    exit_reason,
                    was_killed,
                } = Self::process_message(myself.clone(), state, handler, &mut ports)
                    .await?;
                // processing loop exit
                if should_exit {
                    return Ok::<_, Errors>((state, exit_reason, was_killed));
                }
            }
        };

        // capture any panics in this future and convert to an ActorErr
        let loop_done = futures::FutureExt::catch_unwind(AssertUnwindSafe(future))
            .await
            .map_err(|err| Errors::ActorFailed(err));

        // set status to stopping
        myself_clone.set_status(ActorStatus::Stopping);

        let (exit_state, exit_reason, was_killed) = loop_done??;

        // if we didn't exit in error mode, call `post_stop`
        if !was_killed {
            Self::do_post_stop(myself_clone, handler, exit_state)
                .await?;
        }

        Ok(exit_reason)
    }

    /// Process a message, returning the "new" state (if changed)
    /// along with optionally whether we were signaled mid-processing or not
    ///
    /// * `myself` - The current [ActorRef]
    /// * `state` - The current [Actor::State] object
    /// * `handler` - Pointer to the [Actor] definition
    /// * `ports` - The mutable [Receivers] which are the message ports for this actor
    ///
    /// Returns a tuple of the next [Actor::State] and a flag to denote if the processing
    /// loop is done
    async fn process_message(
        myself: Context,
        state: &mut TActor::State,
        handler: &TActor,
        ports: &mut Receivers,
    ) -> Result<ActorLoopResult, Errors> {
        match ports.listen_in_priority().await {
            Ok(actor_port_message) => match actor_port_message {
                ActorPortMessage::Signal(signal) => {
                    Ok(ActorLoopResult::signal(Self::handle_signal(myself, signal)))
                }
                ActorPortMessage::Stop(stop_message) => {
                    let exit_reason = match stop_message {
                        StopMessage::Stop => {
                            tracing::trace!("Actor {:?} stopped with no reason", myself.get_id());
                            None
                        }
                        StopMessage::Reason(reason) => {
                            tracing::trace!(
                                "Actor {:?} stopped with reason '{reason}'",
                                myself.get_id(),
                            );
                            Some(reason)
                        }
                    };
                    Ok(ActorLoopResult::stop(exit_reason))
                }
                ActorPortMessage::Supervision(supervision) => {
                    let future = Self::handle_supervision_message(
                        myself.clone(),
                        state,
                        handler,
                        supervision,
                    );
                    match ports.run_with_signal(future).await {
                        Ok(Ok(())) => Ok(ActorLoopResult::ok()),
                        Ok(Err(internal_err)) => Err(internal_err),
                        Err(signal) => {
                            Ok(ActorLoopResult::signal(Self::handle_signal(myself, signal)))
                        }
                    }
                }
                ActorPortMessage::Message(msg) => {
                    if msg.is_drain() {
                        // Drain is a stub marker that the actor should now stop, we've processed
                        // all the messages and we want the actor to die now
                        Ok(ActorLoopResult::stop(Some("Drained".to_string())))
                    } else {
                        let future = Self::handle_message(myself.clone(), state, handler, msg);
                        match ports.run_with_signal(future).await {
                            Ok(Ok(())) => Ok(ActorLoopResult::ok()),
                            Ok(Err(internal_err)) => Err(internal_err),
                            Err(signal) => {
                                Ok(ActorLoopResult::signal(Self::handle_signal(myself, signal)))
                            }
                        }
                    }
                }
            },
            Err(Errors::ChannelClosed) => {
                // one of the channels is closed, this means
                // the receiver was dropped and in this case
                // we should always die. Therefore we flag
                // to terminate
                Ok(ActorLoopResult::signal(Self::handle_signal(
                    myself,
                    Signal::Kill,
                )))
            }
            Err(Errors::InvalidActorType) => {
                // not possible. Treat like a channel closed
                Ok(ActorLoopResult::signal(Self::handle_signal(
                    myself,
                    Signal::Kill,
                )))
            }
            Err(Errors::SendFailed(_)) => {
                // not possible. Treat like a channel closed
                Ok(ActorLoopResult::signal(Self::handle_signal(
                    myself,
                    Signal::Kill,
                )))
            }
            _ => {
                unimplemented!();
            }
        }
    }

    async fn handle_message(
        myself: Context,
        state: &mut TActor::State,
        handler: &TActor,
        mut _msg: Message,
    ) -> Result<(), Errors> {
        // An error here will bubble up to terminate the actor
        let typed_msg = TActor::Msg::from_message(_msg)?;
        #[cfg(not(feature = "message_span_propogation"))]
        {
            handler.handle(myself, typed_msg, state).await
        }
        #[cfg(feature = "message_span_propogation")]
        {
            // The current [tracing::Span] is retrieved, boxed, and included in every
            // `BoxedMessage` during the conversion of this `TActor::Msg`. It is used
            // to automatically continue tracing span nesting when sending messages to Actors.
            handler
                .handle(myself, typed_msg, state)
                .instrument(_msg.span)
                .await
        }
    }

    fn handle_signal(myself: Context, signal: Signal) -> Option<String> {
        match &signal {
            Signal::Kill => {
                myself.terminate();
            }
        }
        Some(signal.to_string())
    }

    async fn handle_supervision_message(
        myself: Context,
        state: &mut TActor::State,
        handler: &TActor,
        message: SupervisionEvent,
    ) -> Result<(), Errors> {
        handler.handle_supervisor_evt(myself, message, state).await
    }

    async fn do_pre_start(
        myself: Context,
        handler: &TActor,
    ) -> Result<Result<TActor::State, Errors>, Errors> {
        let future = handler.pre_start(myself);
        futures::FutureExt::catch_unwind(AssertUnwindSafe(future))
            .await
            .map_err(|err| Errors::StartupFailed(err))
    }

    async fn do_post_start(
        myself: Context,
        handler: &TActor,
        state: &mut TActor::State,
    ) -> Result<Result<(), Errors>, Errors> {
        let future = handler.post_start(myself, state);
        futures::FutureExt::catch_unwind(AssertUnwindSafe(future))
            .await
            .map_err(|err| Errors::ActorFailed(err))
    }

    async fn do_post_stop(
        myself: Context,
        handler: &TActor,
        state: &mut TActor::State,
    ) -> Result<Result<(), Errors>, Errors> {
        let future = handler.post_stop(myself, state);
        futures::FutureExt::catch_unwind(AssertUnwindSafe(future))
            .await
            .map_err(|err| Errors::ActorFailed(err))
    }
}
