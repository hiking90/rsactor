// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor::IdleEventStream;
use crate::error::{Error, Result};
use crate::Identity;
use crate::{Actor, ControlSignal, MailboxMessage, MailboxSender, Message};
use futures::stream::{Stream, StreamExt};
use std::time::Duration;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// If the calling thread is currently inside a multi-thread Tokio runtime,
/// returns a `Handle` to it. Otherwise (no runtime, or current_thread runtime)
/// returns `None`.
///
/// The blocking_* APIs use this to take a near-zero-cost fast path: instead of
/// spawning a new OS thread and a fresh single-thread runtime, they call
/// `block_in_place` + `Handle::block_on` and reuse the caller's runtime.
/// `block_in_place` requires a multi-thread runtime, so current_thread is
/// excluded here and falls back to the original "new thread + new runtime"
/// path. The comparison deliberately allows *only* `MultiThread`: should tokio
/// ever add another flavor to the `#[non_exhaustive]` enum, the safe failure
/// mode is the slow path, not a `block_in_place` panic.
#[inline]
fn current_multi_thread_handle() -> Option<Handle> {
    let handle = Handle::try_current().ok()?;
    (handle.runtime_flavor() == RuntimeFlavor::MultiThread).then_some(handle)
}

/// Fast-path helper shared by every `blocking_*` API: runs `fut` to completion
/// on the caller's own multi-thread runtime via `block_in_place` + `block_on`.
/// No new thread or runtime is created.
#[inline]
fn block_on_current_runtime<R>(handle: Handle, fut: impl std::future::Future<Output = R>) -> R {
    tokio::task::block_in_place(|| handle.block_on(fut))
}

/// Slow-path helper shared by `blocking_*` APIs: spawns a dedicated thread,
/// builds a single-thread Tokio runtime on it, and runs `fut` to completion.
///
/// Used only when the caller is *not* inside a multi-thread runtime (i.e. when
/// the fast `block_in_place` path is unavailable). Centralizes the
/// thread/runtime/join boilerplate so all slow paths report errors identically:
/// a failure to spawn the thread or to build the temporary runtime becomes
/// `Error::Runtime` carrying the underlying [`std::io::Error`] as its
/// [`source`](std::error::Error::source).
///
/// With `deadlock-detection` enabled, the calling actor's task-local identity
/// is captured here and re-established inside the new runtime, so `ask` cycles
/// taken through this slow path are still detected. A deadlock panic raised on
/// the dedicated thread is resumed on the caller's thread (preserving the
/// documented panic-on-detection contract); any other worker-thread panic is
/// reported as `Error::Runtime` with `panic_msg` and the panic payload in its
/// details.
fn run_blocking_with_runtime<F, R>(identity: Identity, panic_msg: &'static str, fut: F) -> Result<R>
where
    F: std::future::Future<Output = Result<R>> + Send + 'static,
    R: Send + 'static,
{
    // Capture the current actor (if any) *before* hopping threads — tokio
    // task-locals do not propagate to new threads or runtimes.
    #[cfg(feature = "deadlock-detection")]
    let caller = crate::CURRENT_ACTOR.try_with(|id| *id).ok();

    let join = std::thread::Builder::new()
        .name("rsactor-blocking".to_string())
        .spawn(move || -> Result<R> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Runtime {
                    identity,
                    details: "Failed to build blocking runtime".to_string(),
                    source: Some(std::sync::Arc::new(e)),
                })?;
            #[cfg(feature = "deadlock-detection")]
            {
                match caller {
                    Some(id) => runtime.block_on(crate::CURRENT_ACTOR.scope(id, fut)),
                    None => runtime.block_on(fut),
                }
            }
            #[cfg(not(feature = "deadlock-detection"))]
            {
                runtime.block_on(fut)
            }
        })
        .map_err(|e| Error::Runtime {
            identity,
            details: "Failed to spawn blocking thread".to_string(),
            source: Some(std::sync::Arc::new(e)),
        })?;

    join.join().unwrap_or_else(|payload| {
        let panic_text = payload
            .downcast_ref::<&str>()
            .copied()
            .map(str::to_owned)
            .or_else(|| payload.downcast_ref::<String>().cloned());
        // Deadlock detection promises an *immediate panic* on an ask cycle;
        // swallowing it into Error::Runtime would silently downgrade that
        // contract on the slow path.
        #[cfg(feature = "deadlock-detection")]
        if panic_text
            .as_deref()
            .is_some_and(|m| m.starts_with("Deadlock detected"))
        {
            std::panic::resume_unwind(payload);
        }
        Err(Error::Runtime {
            identity,
            details: match panic_text {
                Some(text) => format!("{panic_msg}: {text}"),
                None => panic_msg.to_string(),
            },
            source: None,
        })
    })
}

#[cfg(feature = "metrics")]
use crate::metrics::MetricsCollector;
#[cfg(feature = "metrics")]
use std::sync::Arc;

/// A type-safe reference to an actor of type `T`.
///
/// `ActorRef<T>` provides type-safe message passing to actors, ensuring that only
/// messages that the actor can handle are sent, and that reply types are correctly typed.
/// It provides compile-time type safety through Rust's type system and trait bounds.
///
/// ## Type Safety Benefits
///
/// - **Compile-Time Message Validation**: Only messages implementing [`Message<M>`] for actor `T` are accepted
/// - **Automatic Reply Type Inference**: Return types are inferred from trait implementations
/// - **Zero Runtime Overhead**: Type safety is enforced at compile time with no performance cost
/// - **IDE Support**: Full autocomplete and type checking support
/// - **Prevention of Runtime Type Errors**: Eliminates downcasting failures and type mismatches
///
/// ## Message Passing Methods
///
/// - **Asynchronous Methods**:
///   - [`ask`](ActorRef::ask): Send a message and await a typed reply.
///   - [`ask_with_timeout`](ActorRef::ask_with_timeout): Send a message and await a typed reply with a timeout.
///   - [`tell`](ActorRef::tell): Send a message without waiting for a reply.
///   - [`tell_with_timeout`](ActorRef::tell_with_timeout): Send a message without waiting for a reply with a timeout.
///
/// - **Blocking Methods**:
///   - [`blocking_ask`](ActorRef::blocking_ask): Send a message and block until a typed reply is received (no runtime context required).
///   - [`blocking_tell`](ActorRef::blocking_tell): Send a message and block until it is sent (no runtime context required).
///
///   The `blocking_*` methods can be called from any thread and from within a
///   multi-thread Tokio runtime. The exact mechanism (`block_in_place` +
///   `block_on`, `blocking_send` / `blocking_recv`, or a temporary runtime on a
///   dedicated thread) depends on the calling context — see each method's
///   "Execution paths" section for details.
///
/// - **Control Methods**:
///   - [`stop`](ActorRef::stop): Gracefully stop the actor.
///   - [`kill`](ActorRef::kill): Immediately terminate the actor.
///
/// - **Utility Methods**:
///   - [`identity`](ActorRef::identity): Get the unique ID of the actor.
///   - [`is_alive`](ActorRef::is_alive): Check if the actor is still running.
///   - [`ask_join`](ActorRef::ask_join): Send a message expecting a JoinHandle and await its completion.
///
/// ## Recommended Usage
///
/// Use [`ActorRef<T>`] by default for all actor communication. It provides compile-time
/// guarantees that prevent type-related runtime errors.
///
/// **When to use `ActorRef<T>`**:
/// - Default choice for actor communication
/// - When you know the actor type at compile time
/// - When you want compile-time message validation
/// - When working with strongly-typed actor systems
#[derive(Debug)]
pub struct ActorRef<T: Actor> {
    /// The unique identifier for this actor instance
    id: Identity,
    /// Channel for sending messages to the actor's mailbox
    sender: MailboxSender<T>,
    /// Optional channel for sending priority messages to the actor.
    /// `None` when the actor was spawned without `SpawnOptions::with_priority()`.
    pub(crate) priority_sender: Option<MailboxSender<T>>,
    /// Channel for sending control signals (e.g., terminate) to the actor
    pub(crate) terminate_sender: mpsc::Sender<ControlSignal>,
    /// Optional channel for registering new idle-event streams with the actor's runtime.
    /// Each subscribed stream is merged into the runtime's `SelectAll` and its
    /// events drive [`Actor::on_idle`](crate::Actor::on_idle).
    /// `None` when the actor was spawned without `SpawnOptions::with_idle()`.
    pub(crate) idle_subscribe_sender: Option<mpsc::Sender<IdleEventStream<T>>>,
    /// Per-actor metrics collector (when metrics feature is enabled)
    #[cfg(feature = "metrics")]
    pub(crate) metrics: Arc<MetricsCollector>,
}

impl<T: Actor> ActorRef<T> {
    /// Creates a new type-safe ActorRef.
    pub(crate) fn new(
        id: Identity,
        sender: MailboxSender<T>,
        priority_sender: Option<MailboxSender<T>>,
        terminate_sender: mpsc::Sender<ControlSignal>,
        idle_subscribe_sender: Option<mpsc::Sender<IdleEventStream<T>>>,
        #[cfg(feature = "metrics")] metrics: Arc<MetricsCollector>,
    ) -> Self {
        ActorRef {
            id,
            sender,
            priority_sender,
            terminate_sender,
            idle_subscribe_sender,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    /// Returns the unique ID of the actor.
    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Returns `true` if this actor was spawned with the priority channel enabled
    /// via [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    ///
    /// When this returns `false`, calls to [`tell_priority`](Self::tell_priority),
    /// [`ask_priority`](Self::ask_priority) and their blocking counterparts return
    /// [`Error::PriorityChannelNotEnabled`](crate::Error::PriorityChannelNotEnabled).
    #[inline]
    pub fn has_priority_channel(&self) -> bool {
        self.priority_sender.is_some()
    }

    /// Returns `true` if this actor was spawned with the idle-event channel enabled
    /// via [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle).
    ///
    /// When this returns `false`, [`Actor::on_idle`](crate::Actor::on_idle) is never
    /// driven and calls to [`subscribe_idle`](Self::subscribe_idle) return
    /// [`Error::IdleChannelNotEnabled`](crate::Error::IdleChannelNotEnabled).
    #[inline]
    pub fn has_idle_channel(&self) -> bool {
        self.idle_subscribe_sender.is_some()
    }

    /// Test-only: drops this `ActorRef`'s strong priority sender without affecting
    /// the regular mailbox or the terminate channel.
    ///
    /// Used to write deterministic tests for the
    /// [`ActorWeak::upgrade`](crate::ActorWeak::upgrade) policy where every strong
    /// priority sender is dropped while the actor is still alive on its other
    /// channels. Production code has no reason to drop one channel without the
    /// others — clone or drop the entire `ActorRef` instead.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn drop_priority_sender_for_test(&mut self) {
        self.priority_sender = None;
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
    ///
    /// The optional priority channel is intentionally **not** consulted: it is a
    /// secondary channel, and the actor is considered alive as long as the regular
    /// mailbox and the terminate channel remain open.
    ///
    /// **Note**: This is a heuristic, point-in-time check. The channels are
    /// closed only after the actor's loop exits, so this can briefly report
    /// `true` after `on_stop` has already run, and a `true` result does not
    /// guarantee that a subsequent send will succeed. A successful send is the
    /// only definitive liveness signal.
    #[inline]
    pub fn is_alive(&self) -> bool {
        // Both channels must be open for the actor to be considered alive
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Creates a weak, type-safe reference to this actor.
    ///
    /// The returned [`ActorWeak<T>`] can be used to check if the actor is still alive
    /// and optionally upgrade back to a strong [`ActorRef<T>`] without keeping the actor alive.
    /// When the `metrics` feature is enabled, metrics are preserved via strong reference
    /// for post-mortem analysis.
    ///
    /// This is an inherent `&self` method (callable as `actor_ref.downgrade()`
    /// or `ActorRef::downgrade(&actor_ref)`). Inherent methods win method
    /// resolution, so the call stays unambiguous even with the
    /// [`TellHandler`](crate::TellHandler) / [`AskHandler`](crate::AskHandler) /
    /// [`ActorControl`](crate::ActorControl) traits in scope, all of which also
    /// declare a `downgrade`; reach those via trait objects or fully-qualified
    /// syntax.
    pub fn downgrade(&self) -> ActorWeak<T> {
        ActorWeak {
            id: self.id,
            sender: self.sender.downgrade(),
            priority_sender: self.priority_sender.as_ref().map(|s| s.downgrade()),
            terminate_sender: self.terminate_sender.downgrade(),
            idle_subscribe_sender: self.idle_subscribe_sender.as_ref().map(|s| s.downgrade()),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }

    /// Resolves once the actor has fully stopped — its runtime loop has exited
    /// (after `on_stop` ran) and the mailbox channel has closed.
    ///
    /// Unlike awaiting the `JoinHandle` returned by [`spawn`](crate::spawn),
    /// this works from any clone of the `ActorRef` and is type-erasable (see
    /// [`ActorControl::wait_stopped`](crate::ActorControl::wait_stopped)), but
    /// it only signals completion — it cannot return the
    /// [`ActorResult`](crate::ActorResult).
    ///
    /// Returns immediately if the actor has already stopped. Note that holding
    /// this `ActorRef` does not keep the actor alive forever: an actor also
    /// stops when every *other* strong ref is dropped only if this one is
    /// dropped too, so pair this with [`stop`](Self::stop) / [`kill`](Self::kill)
    /// rather than waiting for ref-drop termination from a live ref.
    pub async fn wait_stopped(&self) {
        self.sender.closed().await
    }

    /// Registers a [`Stream`] of idle events to be merged into the actor's
    /// runtime. Each event yielded by the stream is dispatched to
    /// [`Actor::on_idle`](crate::Actor::on_idle).
    ///
    /// Streams are owned by the runtime — their internal state (timer schedules,
    /// channel buffers, etc.) survives across runtime-loop iterations, so a
    /// stream like [`IntervalStream`](https://docs.rs/tokio-stream) fires reliably even
    /// when the surrounding `select!` arm is cancelled by a higher-priority
    /// branch. This is the chief reason to prefer subscriptions over open-coded
    /// idle loops.
    ///
    /// Subscription can be called any number of times, from any context with
    /// access to an [`ActorRef`] — typically from [`Actor::on_start`] for the
    /// initial sources, and from message handlers to attach additional sources
    /// dynamically.
    ///
    /// This method is synchronous and never awaits. It uses [`try_send`] under
    /// the hood so that subscriptions inside `on_start` cannot deadlock the
    /// runtime: at that moment the lifecycle is still awaiting `on_start`, so
    /// the subscribe channel's receiver is not yet being polled and any
    /// `.await` would block forever. The bounded buffer (capacity
    /// [`IDLE_SUBSCRIBE_CHANNEL_CAPACITY`](crate::IDLE_SUBSCRIBE_CHANNEL_CAPACITY))
    /// absorbs bursts; an explicit `Err` is returned if it is exceeded.
    ///
    /// [`try_send`]: tokio::sync::mpsc::Sender::try_send
    ///
    /// # Errors
    ///
    /// - [`Error::IdleChannelNotEnabled`] when the actor was spawned without
    ///   [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle). The idle channel is
    ///   **off by default**; enable it explicitly to use `on_idle`. This is a configuration
    ///   error and is terminal.
    /// - [`Error::ChannelFull`] when the bounded subscribe buffer is at capacity. This is
    ///   transient ([`Error::is_retryable`] returns `true`) — batch your subscriptions
    ///   across separate handler invocations or raise
    ///   [`IDLE_SUBSCRIBE_CHANNEL_CAPACITY`](crate::IDLE_SUBSCRIBE_CHANNEL_CAPACITY).
    /// - [`Error::Send`] when the actor is no longer alive (channel closed) — terminal.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Inside `on_start`:
    /// actor_ref.subscribe_idle(
    ///     tokio_stream::wrappers::IntervalStream::new(
    ///         tokio::time::interval(std::time::Duration::from_secs(1))
    ///     ).map(|_| Tick)
    /// )?;
    /// ```
    pub fn subscribe_idle<S>(&self, stream: S) -> Result<()>
    where
        S: Stream<Item = T::IdleEvent> + Send + 'static,
    {
        let Some(idle_subscribe_sender) = self.idle_subscribe_sender.as_ref() else {
            warn!("subscribe_idle called on actor without an idle channel");
            return Err(Error::IdleChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let boxed: IdleEventStream<T> = stream.boxed();
        idle_subscribe_sender.try_send(boxed).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => Error::ChannelFull {
                identity: self.identity(),
                channel: "idle_subscribe",
            },
            mpsc::error::TrySendError::Closed(_) => Error::Send {
                identity: self.identity(),
                details: "Idle subscribe channel closed (actor is no longer alive)",
            },
        })
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing.
    /// This method returns immediately.
    ///
    /// Type safety: Only messages that the actor `T` can handle via [`Message<M>`] trait are accepted.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>()
        ),
        skip(self, msg)
    ))]
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,     // reply_channel is None for tell
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        debug!("Sending tell message (fire-and-forget)");

        let result = if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "tell",
            );
            Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            })
        } else {
            Ok(())
        };

        match &result {
            Ok(_) => debug!("Tell message sent successfully"),
            Err(e) => warn!(error = %e, "Failed to send tell message"),
        }

        result
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget) with a timeout.
    ///
    /// Similar to [`ActorRef::tell`], but allows specifying a timeout for the send operation.
    /// The message is sent to the actor's mailbox, and this method will return once
    /// the message is sent or timeout if the send operation doesn't complete
    /// within the specified duration.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell_with_timeout",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn tell_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending tell message with timeout"
        );

        let result = tokio::time::timeout(timeout, self.tell(msg))
            .await
            .map_err(|_| {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "tell",
                );
                Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: "tell",
                }
            })?;

        match &result {
            Ok(_) => debug!("Tell with timeout completed successfully"),
            Err(e) => warn!(error = %e, "Tell with timeout failed"),
        }

        result
    }

    /// Sends a message to the actor and awaits a reply.
    ///
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply.
    ///
    /// Type safety: The return type `R` is automatically inferred from the [`Message<M>`] trait
    /// implementation, ensuring compile-time type safety for replies.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>()
        ),
        skip(self, msg)
    ))]
    pub async fn ask<M>(&self, msg: M) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        // Deadlock detection: register this `ask` as a wait-for edge and hold
        // the guard until the reply is received (or this future is dropped).
        // Panics if it would close an ask cycle. The envelope carries a token
        // so the callee removes the edge the moment it sends the reply —
        // waiting for this future to resume would leave a stale-edge window
        // that produces false-positive cycle panics.
        #[cfg(feature = "deadlock-detection")]
        let _guard = crate::register_ask_edge(self.identity(), "ask");
        #[cfg(feature = "deadlock-detection")]
        let ask_edge = _guard.as_ref().map(|g| g.token());
        #[cfg(not(feature = "deadlock-detection"))]
        let ask_edge = ();

        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge,
        };

        debug!("Sending ask message and waiting for reply");

        if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "ask",
            );

            warn!("Failed to send ask message: mailbox channel closed");

            return Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            });
        }

        match reply_rx.await {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        debug!("Ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>(),
                        })
                    }
                }
            }
            Err(_recv_err) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ReplyDropped,
                    "ask",
                );

                warn!("Ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly",
                })
            }
        }
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to [`ActorRef::ask`], but allows specifying a timeout for the operation.
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply, or timeout if the reply
    /// doesn't arrive within the specified duration.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_with_timeout",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending ask message with timeout"
        );

        let result = tokio::time::timeout(timeout, self.ask(msg))
            .await
            .map_err(|_| {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "ask",
                );
                Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: "ask",
                }
            })?;

        match &result {
            Ok(_) => debug!("Ask with timeout completed successfully"),
            Err(e) => warn!(error = %e, "Ask with timeout failed"),
        }

        result
    }

    /// Sends a message through the priority channel without awaiting a reply (fire-and-forget).
    ///
    /// The priority channel bypasses the regular mailbox queue: at every iteration of the
    /// actor's runtime loop it is polled with higher priority than the regular mailbox,
    /// but lower priority than the `kill()` (terminate) signal. This is intended for short,
    /// infrequent control-plane messages such as health checks or pause/resume signals.
    ///
    /// `timeout` is **mandatory** and applies to the admission of the message into the
    /// priority channel. The priority channel has a fixed capacity of 1, so a wedged
    /// actor would otherwise block the sender indefinitely; the timeout makes that
    /// failure mode visible.
    ///
    /// # Errors
    ///
    /// - [`Error::PriorityChannelNotEnabled`] if the actor was spawned without
    ///   [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    ///   This is a configuration error and is **not** recorded as a dead letter.
    /// - [`Error::Send`] if the actor has already stopped (recorded as a dead letter).
    /// - [`Error::Timeout`] if admission did not complete within `timeout` (recorded as
    ///   a dead letter).
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell_priority",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn tell_priority<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            warn!("tell_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
            ask_edge: Default::default(),
        };

        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending priority tell message"
        );

        let send_fut = priority_sender.send(envelope);
        let result = match tokio::time::timeout(timeout, send_fut).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "tell_priority",
                );
                Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                })
            }
            Err(_) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "tell_priority",
                );
                Err(Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: "tell_priority",
                })
            }
        };

        match &result {
            Ok(_) => debug!("Priority tell message sent successfully"),
            Err(e) => warn!(error = %e, "Priority tell failed"),
        }

        result
    }

    /// Sends a message through the priority channel and awaits a typed reply.
    ///
    /// `timeout` is **mandatory** and applies to the entire round-trip (admission into
    /// the priority channel plus reply wait). The reply travels through a dedicated
    /// `oneshot` channel separate from the priority slot, so a single in-flight priority
    /// message does not block another `ask_priority` from issuing a fresh request as
    /// soon as the actor pulls the previous one off the channel.
    ///
    /// # Errors
    ///
    /// - [`Error::PriorityChannelNotEnabled`] if the actor was spawned without the
    ///   priority channel enabled (not recorded as a dead letter).
    /// - [`Error::Send`] if the actor has already stopped (recorded as a dead letter).
    /// - [`Error::Timeout`] if the round-trip did not complete within `timeout`
    ///   (recorded as a dead letter).
    /// - [`Error::Receive`] if the reply channel was dropped before a response arrived
    ///   (recorded as a dead letter).
    /// - [`Error::Downcast`] if the handler returned a value of an unexpected type.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_priority",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_priority<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            warn!("ask_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        // Deadlock detection: a priority ask still parks the calling actor's
        // message loop while it awaits the reply, so a cycle through the priority
        // channel stalls just like a regular `ask` (here bounded by `timeout`).
        // Register the edge before sending and hold the guard across the wait so
        // the cycle is detected immediately instead of only timing out.
        #[cfg(feature = "deadlock-detection")]
        let _guard = crate::register_ask_edge(self.identity(), "ask_priority");
        #[cfg(feature = "deadlock-detection")]
        let ask_edge = _guard.as_ref().map(|g| g.token());
        #[cfg(not(feature = "deadlock-detection"))]
        let ask_edge = ();

        let result = tokio::time::timeout(timeout, async {
            let (reply_tx, reply_rx) = oneshot::channel();
            let envelope = MailboxMessage::Envelope {
                payload: Box::new(msg),
                reply_channel: Some(reply_tx),
                actor_ref: self.clone(),
                ask_edge,
            };

            if priority_sender.send(envelope).await.is_err() {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "ask_priority",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                });
            }

            match reply_rx.await {
                Ok(reply_any) => match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        identity: self.identity(),
                        expected_type: std::any::type_name::<T::Reply>(),
                    }),
                },
                Err(_) => {
                    crate::dead_letter::record::<M>(
                        self.identity(),
                        crate::dead_letter::DeadLetterReason::ReplyDropped,
                        "ask_priority",
                    );
                    Err(Error::Receive {
                        identity: self.identity(),
                        details: "Reply channel closed unexpectedly",
                    })
                }
            }
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "ask_priority",
                );
                Err(Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: "ask_priority",
                })
            }
        }
    }

    /// Blocking equivalent of [`tell_priority`](Self::tell_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **Priority slot empty, no runtime context**: a `try_send` fast path
    ///   admits the message immediately without spawning a thread.
    /// - **`current_thread` runtime, or priority slot full with no runtime**:
    ///   spawns a short-lived dedicated thread with a temporary single-thread
    ///   runtime to await admission with the timeout.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`tell_priority`](Self::tell_priority)
    /// directly from async code where possible.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// routes back into this actor. If the priority slot is full, the call parks
    /// the actor's message loop synchronously while waiting for admission that
    /// only that same loop could make room for — an unrecoverable hang that
    /// `kill()` cannot interrupt (note that `deadlock-detection` only tracks
    /// `ask` cycles, not `tell`). Prefer the async
    /// [`tell_priority`](Self::tell_priority) from handler code.
    pub fn blocking_tell_priority<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        // Fast path: caller is inside a multi-thread runtime. Reuse it.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(handle, self.tell_priority(msg, timeout));
        }

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
            ask_edge: Default::default(),
        };

        // Fast path: priority channel has capacity 1, but if it is empty we
        // can finish without spawning a thread or building a runtime.
        let envelope = match priority_sender.try_send(envelope) {
            Ok(()) => return Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "tell_priority",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        let identity = self.identity();
        let priority_sender = priority_sender.clone();

        run_blocking_with_runtime(
            identity,
            "Priority blocking thread terminated unexpectedly",
            async move {
                match tokio::time::timeout(timeout, priority_sender.send(envelope)).await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::ActorStopped,
                            "tell_priority",
                        );
                        Err(Error::Send {
                            identity,
                            details: "Priority channel closed",
                        })
                    }
                    Err(_) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::Timeout,
                            "tell_priority",
                        );
                        Err(Error::Timeout {
                            identity,
                            timeout,
                            operation: "tell_priority",
                        })
                    }
                }
            },
        )
    }

    /// Blocking equivalent of [`ask_priority`](Self::ask_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **`current_thread` runtime, or no runtime context**: spawns a short-lived
    ///   dedicated thread with a temporary single-thread runtime to await the
    ///   reply with the timeout.
    ///
    /// Unlike the `tell` variants, `ask` cannot take a `try_send` fast path
    /// because a sync `recv_timeout` for the reply channel is unavailable.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// asks back into this actor. The call parks the actor's message loop
    /// synchronously while waiting for a reply that only that same loop could
    /// produce — an unrecoverable hang that `kill()` cannot interrupt. Enable
    /// the `deadlock-detection` feature to turn such a cycle into an immediate
    /// panic instead, or use the async [`ask_priority`](Self::ask_priority) from
    /// handler code.
    pub fn blocking_ask_priority<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        if self.priority_sender.is_none() {
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        }

        // Fast path: caller is inside a multi-thread runtime. Reuse it.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(handle, self.ask_priority(msg, timeout));
        }

        let self_clone = self.clone();
        run_blocking_with_runtime(
            self.identity(),
            "Priority blocking thread terminated unexpectedly",
            async move { self_clone.ask_priority(msg, timeout).await },
        )
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor stops as soon as it returns to its message loop: any messages still
    /// queued in the mailbox are dropped, then `on_stop(killed = true)` runs and the
    /// actor's final result indicates it was killed.
    ///
    /// **Cooperative, not preemptive.** `kill()` cannot interrupt a message handler
    /// (or [`on_idle`](crate::Actor::on_idle)) that is *already executing* — the
    /// in-flight future runs until its next return before the terminate signal is
    /// observed. A handler blocked forever on an `.await`, or spinning in a
    /// synchronous/CPU-bound loop, therefore blocks `kill()` as well; the terminate
    /// signal is delivered but cannot be acted on until control returns to the loop.
    /// Guard against this with timeouts on external operations, the
    /// `deadlock-detection` feature for `ask` cycles, and by isolating blocking work
    /// (e.g. `tokio::task::spawn_blocking` or a separate process).
    ///
    /// This method is idempotent: a `Full` or `Closed` terminate channel is treated as
    /// "termination already in flight" — the desired terminal state is met either way.
    /// Both conditions are logged via `tracing::warn!` for diagnostics.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_kill", skip(self))
    )]
    pub fn kill(&self) {
        info!(actor_id = %self.identity(), "Killing actor");

        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
            Ok(_) => {
                info!("Kill signal sent successfully");
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The channel has capacity 1, so Full means a Terminate is already queued.
                warn!("Failed to send Terminate to actor {}: terminate mailbox is full. Actor is likely already being terminated.", self.identity());
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed; the actor is already stopped or has finished.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.identity());
            }
        }
    }

    /// Sends a graceful stop signal to the actor.
    ///
    /// The actor will process all messages currently in its mailbox and then stop.
    /// New messages sent after this call might be ignored or fail.
    /// The actor's final result will indicate normal completion.
    ///
    /// The returned future resolves once the stop signal is **enqueued**, not
    /// when the actor has finished stopping. Await the `JoinHandle` from
    /// [`spawn`](crate::spawn) or [`wait_stopped`](Self::wait_stopped) to
    /// observe completion.
    ///
    /// This method is idempotent: a closed mailbox channel is treated as "stop already
    /// in flight" — the desired terminal state is met either way. The condition is
    /// logged via `tracing::warn!` for diagnostics.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_stop", skip(self))
    )]
    pub async fn stop(&self) {
        match self
            .sender
            .send(MailboxMessage::StopGracefully(self.clone()))
            .await
        {
            Ok(_) => {
                info!(actor_id = %self.identity(), "Actor stop signal sent successfully");
            }
            Err(_) => {
                // Mailbox channel is closed; the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.identity());
            }
        }
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method can be used from any thread, including non-async contexts,
    /// and from within `async fn` on a multi-thread Tokio runtime.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **Any other context, mailbox has room**: a `try_send` fast path
    ///   completes without blocking or spawning a thread (both `timeout`
    ///   variants).
    /// - **Mailbox full, `timeout: Some(_)`**: spawns a short-lived dedicated
    ///   thread with a temporary single-thread runtime to await admission with
    ///   the timeout.
    /// - **Mailbox full, `timeout: None`, no runtime context**: uses Tokio's
    ///   `blocking_send` directly; blocks indefinitely until admitted.
    /// - **Mailbox full, `timeout: None`, inside a `current_thread` runtime or
    ///   a thread driving `Handle::block_on`**: spawns a dedicated thread (like
    ///   the timeout path, but unbounded) — calling `blocking_send` there would
    ///   panic. The calling runtime thread is parked for the duration.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallbacks (mailbox full outside a multi-thread runtime)
    /// incur:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    ///
    /// All other execution paths above are sub-microsecond.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`tell`](Self::tell) directly
    /// from async code where possible.
    ///
    /// # Panics
    ///
    /// Panics when called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    /// running on a multi-thread runtime: the runtime handle reports
    /// multi-thread flavor, but `block_in_place` is not permitted there.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// routes back into this actor. If the target mailbox is full, the call
    /// parks the actor's message loop synchronously while waiting for admission
    /// that only that same loop could make room for — an unrecoverable hang that
    /// `kill()` cannot interrupt (note that `deadlock-detection` only tracks
    /// `ask` cycles, not `tell`). Prefer the async [`tell`](Self::tell) from
    /// handler code.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_blocking_tell",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn blocking_tell<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        // Fast path: caller is inside a multi-thread runtime. Reuse it via
        // `block_in_place` + `Handle::block_on` instead of spawning a new
        // thread and runtime. Safe to call from async contexts on a
        // multi-thread runtime.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(handle, async {
                match timeout {
                    Some(t) => self.tell_with_timeout(msg, t).await,
                    None => self.tell(msg).await,
                }
            });
        }

        match timeout {
            Some(timeout_duration) => self.blocking_tell_with_timeout_impl(msg, timeout_duration),
            None => self.blocking_tell_no_timeout(msg),
        }
    }

    /// Internal implementation of blocking_tell without timeout.
    fn blocking_tell_no_timeout<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,     // reply_channel is None for tell
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        debug!("Sending blocking tell message (fire-and-forget)");

        // Fast path: room in the mailbox — finish without blocking at all.
        let envelope = match self.sender.try_send(envelope) {
            Ok(()) => return Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "tell",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Mailbox channel closed",
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        // Inside *any* runtime context (a current_thread runtime task, or a
        // thread driving `Handle::block_on`), tokio's `blocking_send` panics
        // with "Cannot block the current thread from within a runtime". Route
        // through the dedicated-thread slow path instead: same
        // blocks-until-admitted semantics, no panic. The calling runtime
        // thread is still parked for the duration — see the deadlock warning
        // on [`blocking_tell`](Self::blocking_tell).
        if Handle::try_current().is_ok() {
            let identity = self.identity();
            let sender = self.sender.clone();
            return run_blocking_with_runtime(
                identity,
                "Blocking thread terminated unexpectedly",
                async move {
                    sender.send(envelope).await.map_err(|_| {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::ActorStopped,
                            "tell",
                        );
                        Error::Send {
                            identity,
                            details: "Mailbox channel closed",
                        }
                    })
                },
            );
        }

        let result = self.sender.blocking_send(envelope).map_err(|_| {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "tell",
            );
            Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            }
        });

        match &result {
            Ok(_) => debug!("Blocking tell message sent successfully"),
            Err(e) => warn!(error = %e, "Failed to send blocking tell message"),
        }

        result
    }

    /// Internal implementation of blocking_tell with timeout using a separate thread and runtime.
    fn blocking_tell_with_timeout_impl<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
            ask_edge: Default::default(),
        };

        // Fast path: if the mailbox has room, finish without spawning a
        // thread or building a runtime. Recovers the message back via
        // `TrySendError::Full(envelope)` and falls through to the slow path
        // only when the mailbox is actually full.
        let envelope = match self.sender.try_send(envelope) {
            Ok(()) => return Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "tell",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Mailbox channel closed",
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        let identity = self.identity();
        let sender = self.sender.clone();

        run_blocking_with_runtime(
            identity,
            "Timeout thread terminated unexpectedly",
            async move {
                match tokio::time::timeout(timeout, sender.send(envelope)).await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::ActorStopped,
                            "tell",
                        );
                        Err(Error::Send {
                            identity,
                            details: "Mailbox channel closed",
                        })
                    }
                    Err(_) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::Timeout,
                            "tell",
                        );
                        Err(Error::Timeout {
                            identity,
                            timeout,
                            operation: "blocking_tell",
                        })
                    }
                }
            },
        )
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method can be used from any thread, including non-async contexts,
    /// and from within `async fn` on a multi-thread Tokio runtime.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **`timeout: Some(_)`, any other context**: spawns a short-lived
    ///   dedicated thread with a temporary single-thread runtime to await the
    ///   reply with the timeout. Unlike the `tell` variants, no `try_send`
    ///   fast path is possible because a sync `recv_timeout` for the reply
    ///   channel is unavailable.
    /// - **`timeout: None`, no runtime context**: uses Tokio's `blocking_send`
    ///   and `blocking_recv` directly; blocks indefinitely until the reply
    ///   arrives.
    /// - **`timeout: None`, inside a `current_thread` runtime or a thread
    ///   driving `Handle::block_on`**: spawns a dedicated thread (like the
    ///   timeout path, but unbounded) — calling `blocking_send`/`blocking_recv`
    ///   there would panic. The calling runtime thread is parked for the
    ///   duration.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallbacks (dedicated thread + temporary runtime) incur:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    ///
    /// All other execution paths above are sub-microsecond plus the actor's
    /// own reply latency.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`ask`](Self::ask) directly
    /// from async code where possible.
    ///
    /// # Panics
    ///
    /// Panics when called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    /// running on a multi-thread runtime: the runtime handle reports
    /// multi-thread flavor, but `block_in_place` is not permitted there.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// asks back into this actor. The call parks the actor's message loop
    /// synchronously while waiting for a reply that only that same loop could
    /// produce — an unrecoverable hang that `kill()` cannot interrupt. Enable
    /// the `deadlock-detection` feature to turn such a cycle into an immediate
    /// panic instead, or use the async [`ask`](Self::ask) from handler code.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_blocking_ask",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn blocking_ask<M>(&self, msg: M, timeout: Option<Duration>) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        // Fast path: caller is inside a multi-thread runtime. Reuse it via
        // `block_in_place` + `Handle::block_on` instead of spawning a new
        // thread and runtime. Safe to call from async contexts on a
        // multi-thread runtime.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(handle, async {
                match timeout {
                    Some(t) => self.ask_with_timeout(msg, t).await,
                    None => self.ask(msg).await,
                }
            });
        }

        match timeout {
            Some(timeout_duration) => self.blocking_ask_with_timeout_impl(msg, timeout_duration),
            None => self.blocking_ask_no_timeout(msg),
        }
    }

    /// Internal implementation of blocking_ask without timeout.
    fn blocking_ask_no_timeout<M>(&self, msg: M) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        debug!("Sending blocking ask message and waiting for reply");

        // Inside *any* runtime context (a current_thread runtime task, or a
        // thread driving `Handle::block_on`), tokio's `blocking_send` /
        // `blocking_recv` panic with "Cannot block the current thread from
        // within a runtime". Route the whole ask through the dedicated-thread
        // slow path instead: same blocks-until-replied semantics, no panic.
        // Deadlock detection still works on this path —
        // `run_blocking_with_runtime` re-establishes the calling actor's
        // identity inside the temporary runtime.
        if Handle::try_current().is_ok() {
            let self_clone = self.clone();
            return run_blocking_with_runtime(
                self.identity(),
                "Blocking thread terminated unexpectedly",
                async move { self_clone.ask(msg).await },
            );
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        self.sender.blocking_send(envelope).map_err(|_| {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "ask",
            );

            warn!("Failed to send blocking ask message: mailbox channel closed");

            Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            }
        })?;

        match reply_rx.blocking_recv() {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        debug!("Blocking ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Blocking ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>(),
                        })
                    }
                }
            }
            Err(_recv_err) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ReplyDropped,
                    "ask",
                );

                warn!("Blocking ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly",
                })
            }
        }
    }

    /// Internal implementation of blocking_ask with timeout using a separate thread and runtime.
    fn blocking_ask_with_timeout_impl<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        let self_clone = self.clone();
        let identity = self.identity();

        run_blocking_with_runtime(
            identity,
            "Timeout thread terminated unexpectedly",
            async move {
                tokio::time::timeout(timeout, self_clone.ask(msg))
                    .await
                    .map_err(|_| {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::Timeout,
                            "ask",
                        );
                        Error::Timeout {
                            identity,
                            timeout,
                            operation: "blocking_ask",
                        }
                    })?
            },
        )
    }

    /// Sends a message to an actor expecting a JoinHandle reply and awaits its completion.
    ///
    /// This method is specifically designed for the pattern where message handlers spawn
    /// long-running asynchronous tasks using `tokio::spawn` and return the `JoinHandle`.
    /// Instead of manually handling the `JoinHandle`, this method automatically awaits
    /// the spawned task and returns the final result.
    ///
    /// # Common Pattern
    ///
    /// This method supports the following actor pattern:
    ///
    /// ```rust,no_run
    /// use rsactor::{Actor, ActorRef, message_handlers};
    /// use tokio::task::JoinHandle;
    /// use std::time::Duration;
    ///
    /// #[derive(Actor)]
    /// struct WorkerActor;
    ///
    /// struct HeavyTask {
    ///     data: String,
    /// }
    ///
    /// #[message_handlers]
    /// impl WorkerActor {
    ///     #[handler]
    ///     async fn handle_heavy_task(
    ///         &mut self,
    ///         msg: HeavyTask,
    ///         _: &ActorRef<Self>
    ///     ) -> JoinHandle<String> {
    ///         let data = msg.data.clone();
    ///         // Spawn a long-running task to avoid blocking the actor
    ///         tokio::spawn(async move {
    ///             tokio::time::sleep(Duration::from_secs(5)).await;
    ///             format!("Processed: {}", data)
    ///         })
    ///     }
    /// }
    /// ```
    ///
    /// # Usage Examples
    ///
    /// See the `examples/ask_join_demo.rs` file for a complete working example that demonstrates:
    /// - Handlers that return `JoinHandle<T>` from spawned tasks
    /// - Using `ask_join` vs regular `ask` with manual await
    /// - Error handling for panicked and cancelled tasks
    /// - Concurrent task execution patterns
    ///
    /// # Error Handling
    ///
    /// This method can return errors in several scenarios:
    /// - Communication errors (actor not available, channels closed): [`Error::Send`], [`Error::Receive`]
    /// - Task execution errors (task panicked, cancelled): [`Error::Join`]
    /// - Type casting errors: [`Error::Downcast`]
    ///
    /// The [`Error::Join`] variant includes the original [`tokio::task::JoinError`]
    /// which provides detailed information about task failures, such as whether
    /// the task was cancelled or panicked.
    ///
    /// # Type Safety
    ///
    /// This method enforces compile-time type safety:
    /// - The message type `M` must be handled by actor `T`
    /// - The handler must return `tokio::task::JoinHandle<R>`
    /// - The final return type `R` is automatically inferred and type-checked
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_join",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            result_type = %std::any::type_name::<R>()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_join<M, R>(&self, msg: M) -> crate::Result<R>
    where
        T: Message<M, Reply = tokio::task::JoinHandle<R>>,
        M: Send + 'static,
        R: Send + 'static,
    {
        tracing::debug!("Sending ask_join message and waiting for task completion");

        let join_handle = self.ask(msg).await?;

        tracing::debug!("Received JoinHandle, awaiting task completion");

        let result = join_handle.await.map_err(|join_error| crate::Error::Join {
            identity: self.identity(),
            source: std::sync::Arc::new(join_error),
        })?;

        tracing::debug!("Task completed successfully");

        Ok(result)
    }

    // ==================== Metrics API ====================

    /// Returns a snapshot of all metrics for this actor.
    ///
    /// The snapshot includes message count, processing times, priority message
    /// counts, uptime, and last activity timestamp.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let metrics = actor_ref.metrics();
    /// println!("Processed {} messages", metrics.message_count);
    /// println!("Avg time: {:?}", metrics.avg_processing_time);
    /// ```
    #[cfg(feature = "metrics")]
    pub fn metrics(&self) -> crate::MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Returns the total number of messages processed by this actor.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn message_count(&self) -> u64 {
        self.metrics.message_count()
    }

    /// Returns the average message processing time.
    ///
    /// Returns `Duration::ZERO` if no messages have been processed yet.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn avg_processing_time(&self) -> std::time::Duration {
        self.metrics.avg_processing_time()
    }

    /// Returns the maximum message processing time observed.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn max_processing_time(&self) -> std::time::Duration {
        self.metrics.max_processing_time()
    }

    /// Returns the total number of priority messages processed by this actor.
    ///
    /// Always `0` for actors spawned without
    /// [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn priority_message_count(&self) -> u64 {
        self.metrics.priority_message_count()
    }

    /// Returns the average priority message processing time.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn avg_priority_processing_time(&self) -> std::time::Duration {
        self.metrics.avg_priority_processing_time()
    }

    /// Returns the maximum priority message processing time observed.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn max_priority_processing_time(&self) -> std::time::Duration {
        self.metrics.max_priority_processing_time()
    }

    /// Returns the time elapsed since the actor started.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn uptime(&self) -> std::time::Duration {
        self.metrics.uptime()
    }

    /// Returns the timestamp of the last message processing.
    ///
    /// Returns `None` if no messages have been processed yet.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn last_activity(&self) -> Option<std::time::SystemTime> {
        self.metrics.last_activity()
    }
}

impl<T: Actor> Clone for ActorRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorRef {
            id: self.id,
            sender: self.sender.clone(),
            priority_sender: self.priority_sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
            idle_subscribe_sender: self.idle_subscribe_sender.clone(),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }
}

/// A weak, type-safe reference to an actor of type `T`.
///
/// `ActorWeak<T>` is a weak reference that does not prevent the actor from being dropped
/// and can be upgraded to a strong [`ActorRef<T>`] if the actor is still alive.
/// Like [`ActorRef<T>`], it maintains compile-time type safety for the actor type `T`.
///
/// ## Creating `ActorWeak<T>`
///
/// `ActorWeak<T>` instances are created by calling [`downgrade`](ActorRef::downgrade) on an
/// existing [`ActorRef<T>`]:
///
/// ```ignore
/// let weak_ref = ActorRef::downgrade(&actor_ref);
/// ```
///
/// ## Upgrading to `ActorRef<T>`
///
/// An `ActorWeak<T>` can be upgraded to an `ActorRef<T>` using the [`upgrade`](ActorWeak::upgrade) method:
///
/// ```ignore
/// if let Some(strong_ref) = weak_ref.upgrade() {
///     // Successfully upgraded, actor is still alive
///     strong_ref.tell(MyMessage).await?;
/// } else {
///     // Actor is no longer alive
/// }
/// ```
#[derive(Debug)]
pub struct ActorWeak<T: Actor> {
    /// The unique identifier for this actor instance
    id: Identity,
    /// Weak reference to the actor's mailbox sender
    sender: tokio::sync::mpsc::WeakSender<MailboxMessage<T>>,
    /// Weak reference to the actor's optional priority sender (None if priority disabled)
    priority_sender: Option<tokio::sync::mpsc::WeakSender<MailboxMessage<T>>>,
    /// Weak reference to the actor's terminate signal sender
    terminate_sender: tokio::sync::mpsc::WeakSender<ControlSignal>,
    /// Weak reference to the actor's optional idle-subscribe channel sender
    /// (None if the idle channel was not enabled via `SpawnOptions::with_idle()`).
    idle_subscribe_sender: Option<tokio::sync::mpsc::WeakSender<IdleEventStream<T>>>,
    /// Strong reference to metrics (survives actor drop for post-mortem analysis)
    #[cfg(feature = "metrics")]
    metrics: Arc<MetricsCollector>,
}

impl<T: Actor> ActorWeak<T> {
    /// Attempts to upgrade the weak reference to a strong, type-safe reference.
    ///
    /// Returns `Some(ActorRef<T>)` if the actor is still alive, or `None` if the actor
    /// has been dropped.
    ///
    /// The priority and idle-subscribe channels are treated as *secondary* channels: if the
    /// original actor was spawned with one of them enabled but every strong sender for it has
    /// been dropped, `upgrade()` still succeeds. The resulting [`ActorRef`] reports the channel
    /// as absent ([`has_priority_channel()`](ActorRef::has_priority_channel) /
    /// [`has_idle_channel()`](ActorRef::has_idle_channel) returning `false`), and calls on it
    /// fail with [`Error::PriorityChannelNotEnabled`](crate::Error::PriorityChannelNotEnabled)
    /// / [`Error::IdleChannelNotEnabled`](crate::Error::IdleChannelNotEnabled).
    ///
    /// The mailbox and terminate channels are the *primary* channels and must both upgrade for
    /// `upgrade()` to succeed. In practice their strong-sender counts move in lockstep — every
    /// [`ActorRef`] clone owns one of each — so they live and die together; the joint check is
    /// defensive rather than restrictive.
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade the two primary channel senders (mailbox, terminate).
        // The priority and idle-subscribe channels, when present, are best-effort.
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;
        let priority_sender = self.priority_sender.as_ref().and_then(|s| s.upgrade());
        let idle_subscribe_sender = self
            .idle_subscribe_sender
            .as_ref()
            .and_then(|s| s.upgrade());

        Some(ActorRef {
            id: self.id,
            sender,
            priority_sender,
            terminate_sender,
            idle_subscribe_sender,
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        })
    }

    /// Returns the unique ID of the actor this weak reference points to.
    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Checks if the actor might still be alive.
    ///
    /// This method returns `true` if weak references can potentially be upgraded,
    /// but does not guarantee that a subsequent [`upgrade`](ActorWeak::upgrade) call will succeed
    /// due to potential race conditions.
    ///
    /// Returns `false` if the actor is definitely dead (all strong references dropped).
    ///
    /// **Note**: This is a heuristic check. For definitive actor state, always use
    /// [`upgrade`](ActorWeak::upgrade) and check the returned `Option`.
    #[inline]
    pub fn is_alive(&self) -> bool {
        // Every primary sender (mailbox, terminate) must still have a strong
        // reference for the actor to be alive. Mirror the same set checked by
        // `upgrade()` so the two calls cannot disagree. The priority and
        // idle-subscribe channels are secondary and intentionally not consulted.
        self.sender.strong_count() > 0 && self.terminate_sender.strong_count() > 0
    }
}

impl<T: Actor> Clone for ActorWeak<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorWeak {
            id: self.id,
            sender: self.sender.clone(),
            priority_sender: self.priority_sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
            idle_subscribe_sender: self.idle_subscribe_sender.clone(),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }
}
