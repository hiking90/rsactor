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
use tracing::warn;

/// If the calling thread is currently inside a multi-thread Tokio runtime,
/// returns a `Handle` to it. Otherwise (no runtime, or current_thread runtime)
/// returns `None`.
///
/// The blocking_* APIs use this to take a near-zero-cost fast path: instead of
/// spawning a new OS thread and a fresh single-thread runtime, they call
/// `block_in_place` + `Handle::block_on` and reuse the caller's runtime.
/// `block_in_place` requires a multi-thread runtime, so current_thread is
/// excluded here and falls back to the original "new thread + new runtime"
/// path.
#[inline]
fn current_multi_thread_handle() -> Option<Handle> {
    let handle = Handle::try_current().ok()?;
    (handle.runtime_flavor() == RuntimeFlavor::MultiThread).then_some(handle)
}

#[cfg(feature = "tracing")]
use tracing::{debug, info};

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
///   - [`ask_blocking`](ActorRef::ask_blocking): *(Deprecated)* Send a message and block until a typed reply is received.
///   - [`tell_blocking`](ActorRef::tell_blocking): *(Deprecated)* Send a message and block until it is sent.
///
///   The new `blocking_*` methods use Tokio's underlying `blocking_send` and `blocking_recv` and can be used from any thread.
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
    /// Channel for registering new idle-event streams with the actor's runtime.
    /// Each subscribed stream is merged into the runtime's `SelectAll` and its
    /// events drive [`Actor::on_idle`](crate::Actor::on_idle).
    pub(crate) idle_subscribe_sender: mpsc::Sender<IdleEventStream<T>>,
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
        idle_subscribe_sender: mpsc::Sender<IdleEventStream<T>>,
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
    pub fn downgrade(this: &Self) -> ActorWeak<T> {
        ActorWeak {
            id: this.id,
            sender: this.sender.downgrade(),
            priority_sender: this.priority_sender.as_ref().map(|s| s.downgrade()),
            terminate_sender: this.terminate_sender.downgrade(),
            idle_subscribe_sender: this.idle_subscribe_sender.downgrade(),
            #[cfg(feature = "metrics")]
            metrics: this.metrics.clone(),
        }
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
        let boxed: IdleEventStream<T> = stream.boxed();
        self.idle_subscribe_sender
            .try_send(boxed)
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => Error::ChannelFull {
                    identity: self.identity(),
                    channel: "idle_subscribe",
                },
                mpsc::error::TrySendError::Closed(_) => Error::Send {
                    identity: self.identity(),
                    details: "Idle subscribe channel closed (actor is no longer alive)".to_string(),
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
        };

        #[cfg(feature = "tracing")]
        debug!("Sending tell message (fire-and-forget)");

        let result = if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "tell",
            );
            Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed".to_string(),
            })
        } else {
            Ok(())
        };

        #[cfg(feature = "tracing")]
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
        #[cfg(feature = "tracing")]
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
                    operation: "tell".to_string(),
                }
            })?;

        #[cfg(feature = "tracing")]
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
        #[cfg(feature = "deadlock-detection")]
        let _guard = {
            let caller = crate::CURRENT_ACTOR.try_with(|id| *id).ok();
            if let Some(caller) = caller {
                let callee = self.identity();
                let mut graph = crate::wait_for_graph().lock().unwrap();
                if caller.id == callee.id || crate::has_path(&graph, callee.id, caller.id) {
                    let cycle = crate::format_cycle_path(&graph, caller, callee);
                    drop(graph);
                    panic!(
                        "Deadlock detected: ask cycle {cycle}\n\
                         This is a design error. Use `tell` to break the cycle, \
                         or restructure actor dependencies."
                    );
                }
                graph.insert(caller.id, callee);
                Some(crate::WaitForGuard(caller.id))
            } else {
                None
            }
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
        };

        #[cfg(feature = "tracing")]
        debug!("Sending ask message and waiting for reply");

        if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "ask",
            );

            #[cfg(feature = "tracing")]
            warn!("Failed to send ask message: mailbox channel closed");

            return Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed".to_string(),
            });
        }

        match reply_rx.await {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        #[cfg(feature = "tracing")]
                        debug!("Ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        #[cfg(feature = "tracing")]
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>().to_string(),
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

                #[cfg(feature = "tracing")]
                warn!("Ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly".to_string(),
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
        #[cfg(feature = "tracing")]
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
                    operation: "ask".to_string(),
                }
            })?;

        #[cfg(feature = "tracing")]
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
            #[cfg(feature = "tracing")]
            warn!("tell_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
        };

        #[cfg(feature = "tracing")]
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
                    details: "Priority channel closed".to_string(),
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
                    operation: "tell_priority".to_string(),
                })
            }
        };

        #[cfg(feature = "tracing")]
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
            #[cfg(feature = "tracing")]
            warn!("ask_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let result = tokio::time::timeout(timeout, async {
            let (reply_tx, reply_rx) = oneshot::channel();
            let envelope = MailboxMessage::Envelope {
                payload: Box::new(msg),
                reply_channel: Some(reply_tx),
                actor_ref: self.clone(),
            };

            if priority_sender.send(envelope).await.is_err() {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "ask_priority",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed".to_string(),
                });
            }

            match reply_rx.await {
                Ok(reply_any) => match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        identity: self.identity(),
                        expected_type: std::any::type_name::<T::Reply>().to_string(),
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
                        details: "Reply channel closed unexpectedly".to_string(),
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
                    operation: "ask_priority".to_string(),
                })
            }
        }
    }

    /// Blocking equivalent of [`tell_priority`](Self::tell_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Multi-thread Tokio runtime (any context, including `async fn`)**: uses
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
            return tokio::task::block_in_place(|| {
                handle.block_on(self.tell_priority(msg, timeout))
            });
        }

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
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
                    details: "Priority channel closed".to_string(),
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        let identity = self.identity();
        let priority_sender = priority_sender.clone();

        let join = std::thread::spawn(move || -> Result<()> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Send {
                    identity,
                    details: format!("Failed to create runtime: {}", e),
                })?;

            runtime.block_on(async {
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
                            details: "Priority channel closed".to_string(),
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
                            operation: "tell_priority".to_string(),
                        })
                    }
                }
            })
        });

        join.join().unwrap_or_else(|_| {
            Err(Error::Send {
                identity: self.identity(),
                details: "Priority blocking thread terminated unexpectedly".to_string(),
            })
        })
    }

    /// Blocking equivalent of [`ask_priority`](Self::ask_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Multi-thread Tokio runtime (any context, including `async fn`)**: uses
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
            return tokio::task::block_in_place(|| {
                handle.block_on(self.ask_priority(msg, timeout))
            });
        }

        let self_clone = self.clone();

        let join = std::thread::spawn(move || -> Result<T::Reply> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Send {
                    identity: self_clone.identity(),
                    details: format!("Failed to create runtime: {}", e),
                })?;

            runtime.block_on(self_clone.ask_priority(msg, timeout))
        });

        join.join().unwrap_or_else(|_| {
            Err(Error::Send {
                identity: self.identity(),
                details: "Priority blocking thread terminated unexpectedly".to_string(),
            })
        })
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    ///
    /// This method is idempotent: a `Full` or `Closed` terminate channel is treated as
    /// "termination already in flight" — the desired terminal state is met either way.
    /// Both conditions are logged via `tracing::warn!` for diagnostics.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_kill", skip(self))
    )]
    pub fn kill(&self) {
        #[cfg(feature = "tracing")]
        info!(actor_id = %self.identity(), "Killing actor");

        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
            Ok(_) => {
                #[cfg(feature = "tracing")]
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
                #[cfg(feature = "tracing")]
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
    /// - **Multi-thread Tokio runtime (any context, including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **`current_thread` runtime, or no runtime context, with `timeout: None`**:
    ///   uses Tokio's `blocking_send` directly. Most efficient, but blocks
    ///   indefinitely if the mailbox is full.
    /// - **`current_thread` runtime, or no runtime context, with `timeout: Some(_)`
    ///   and mailbox empty**: a `try_send` fast path completes without spawning
    ///   a thread.
    /// - **`current_thread` runtime, or no runtime context, with `timeout: Some(_)`
    ///   and mailbox full**: spawns a short-lived dedicated thread with a
    ///   temporary single-thread runtime to await admission with the timeout.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallback (`current_thread` runtime or no runtime, with
    /// timeout, mailbox full) incurs:
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
            return tokio::task::block_in_place(|| {
                handle.block_on(async {
                    match timeout {
                        Some(t) => self.tell_with_timeout(msg, t).await,
                        None => self.tell(msg).await,
                    }
                })
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
        };

        #[cfg(feature = "tracing")]
        debug!("Sending blocking tell message (fire-and-forget)");

        let result = self.sender.blocking_send(envelope).map_err(|_| {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "blocking_tell",
            );
            Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed".to_string(),
            }
        });

        #[cfg(feature = "tracing")]
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
                    "blocking_tell",
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Mailbox channel closed".to_string(),
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        let identity = self.identity();
        let sender = self.sender.clone();

        let join = std::thread::spawn(move || -> Result<()> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Send {
                    identity,
                    details: format!("Failed to create runtime: {}", e),
                })?;

            runtime.block_on(async {
                match tokio::time::timeout(timeout, sender.send(envelope)).await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::ActorStopped,
                            "blocking_tell",
                        );
                        Err(Error::Send {
                            identity,
                            details: "Mailbox channel closed".to_string(),
                        })
                    }
                    Err(_) => {
                        crate::dead_letter::record::<M>(
                            identity,
                            crate::dead_letter::DeadLetterReason::Timeout,
                            "blocking_tell",
                        );
                        Err(Error::Timeout {
                            identity,
                            timeout,
                            operation: "blocking_tell".to_string(),
                        })
                    }
                }
            })
        });

        join.join().unwrap_or_else(|_| {
            Err(Error::Send {
                identity: self.identity(),
                details: "Timeout thread terminated unexpectedly".to_string(),
            })
        })
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method can be used from any thread, including non-async contexts,
    /// and from within `async fn` on a multi-thread Tokio runtime.
    ///
    /// # Execution paths
    ///
    /// - **Multi-thread Tokio runtime (any context, including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **`current_thread` runtime, or no runtime context, with `timeout: None`**:
    ///   uses Tokio's `blocking_send` and `blocking_recv` directly. Most
    ///   efficient, but blocks indefinitely if the actor never responds.
    /// - **`current_thread` runtime, or no runtime context, with `timeout: Some(_)`**:
    ///   spawns a short-lived dedicated thread with a temporary single-thread
    ///   runtime to await the reply with the timeout. Unlike the `tell`
    ///   variants, no `try_send` fast path is possible because a sync
    ///   `recv_timeout` for the reply channel is unavailable.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallback (`current_thread` runtime or no runtime, with
    /// timeout) incurs:
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
            return tokio::task::block_in_place(|| {
                handle.block_on(async {
                    match timeout {
                        Some(t) => self.ask_with_timeout(msg, t).await,
                        None => self.ask(msg).await,
                    }
                })
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
        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
        };

        #[cfg(feature = "tracing")]
        debug!("Sending blocking ask message and waiting for reply");

        self.sender.blocking_send(envelope).map_err(|_| {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "blocking_ask",
            );

            #[cfg(feature = "tracing")]
            warn!("Failed to send blocking ask message: mailbox channel closed");

            Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed".to_string(),
            }
        })?;

        match reply_rx.blocking_recv() {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        #[cfg(feature = "tracing")]
                        debug!("Blocking ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        #[cfg(feature = "tracing")]
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Blocking ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>().to_string(),
                        })
                    }
                }
            }
            Err(_recv_err) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ReplyDropped,
                    "blocking_ask",
                );

                #[cfg(feature = "tracing")]
                warn!("Blocking ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly".to_string(),
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

        let join = std::thread::spawn(move || -> Result<T::Reply> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Send {
                    identity: self_clone.identity(),
                    details: format!("Failed to create runtime: {}", e),
                })?;

            runtime.block_on(async {
                tokio::time::timeout(timeout, self_clone.ask(msg))
                    .await
                    .map_err(|_| {
                        crate::dead_letter::record::<M>(
                            self_clone.identity(),
                            crate::dead_letter::DeadLetterReason::Timeout,
                            "blocking_ask",
                        );
                        Error::Timeout {
                            identity: self_clone.identity(),
                            timeout,
                            operation: "blocking_ask".to_string(),
                        }
                    })?
            })
        });

        join.join().unwrap_or_else(|_| {
            Err(Error::Send {
                identity: self.identity(),
                details: "Timeout thread terminated unexpectedly".to_string(),
            })
        })
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
    ///
    /// **Deprecated**: Use [`blocking_tell`](ActorRef::blocking_tell) instead. The timeout parameter is ignored.
    #[deprecated(
        since = "0.10.0",
        note = "Use `blocking_tell` instead. Timeout parameter is ignored."
    )]
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell_blocking",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn tell_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        #[cfg(feature = "tracing")]
        debug!("Executing deprecated tell_blocking, delegating to blocking_tell");

        // Ignore timeout parameter as documented in deprecation notice
        let _ = timeout;

        self.blocking_tell(msg, None)
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
    ///
    /// **Deprecated**: Use [`blocking_ask`](ActorRef::blocking_ask) instead. The timeout parameter is ignored.
    #[deprecated(
        since = "0.10.0",
        note = "Use `blocking_ask` instead. Timeout parameter is ignored."
    )]
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_blocking",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn ask_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        #[cfg(feature = "tracing")]
        debug!("Executing deprecated ask_blocking, delegating to blocking_ask");

        // Ignore timeout parameter as documented in deprecation notice
        let _ = timeout;

        self.blocking_ask(msg, None)
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
        #[cfg(feature = "tracing")]
        tracing::debug!("Sending ask_join message and waiting for task completion");

        let join_handle = self.ask(msg).await?;

        #[cfg(feature = "tracing")]
        tracing::debug!("Received JoinHandle, awaiting task completion");

        let result = join_handle.await.map_err(|join_error| crate::Error::Join {
            identity: self.identity(),
            source: join_error,
        })?;

        #[cfg(feature = "tracing")]
        tracing::debug!("Task completed successfully");

        Ok(result)
    }

    // ==================== Metrics API ====================

    /// Returns a snapshot of all metrics for this actor.
    ///
    /// The snapshot includes message count, processing times, error count, uptime,
    /// and last activity timestamp.
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
/// let weak_ref = actor_ref.downgrade();
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
    /// Weak reference to the actor's idle-subscribe channel sender
    idle_subscribe_sender: tokio::sync::mpsc::WeakSender<IdleEventStream<T>>,
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
    /// The priority channel is treated as a secondary channel: if the original actor was
    /// spawned with priority enabled but every strong priority sender has been dropped,
    /// `upgrade()` still succeeds. The resulting [`ActorRef`] has
    /// [`has_priority_channel()`](ActorRef::has_priority_channel) returning `false`, and
    /// priority calls will fail with
    /// [`Error::PriorityChannelNotEnabled`](crate::Error::PriorityChannelNotEnabled).
    ///
    /// The mailbox, terminate, and idle-subscribe channels are all *primary* channels and
    /// must all upgrade for `upgrade()` to succeed. In practice their strong-sender counts
    /// move in lockstep — every [`ActorRef`] clone owns one of each — so they live and die
    /// together; the joint check is defensive rather than restrictive.
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade the three primary channel senders (mailbox, terminate,
        // idle-subscribe). The priority channel, when present, is best-effort.
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;
        let priority_sender = self.priority_sender.as_ref().and_then(|s| s.upgrade());
        let idle_subscribe_sender = self.idle_subscribe_sender.upgrade()?;

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
        // Every primary sender (mailbox, terminate, idle-subscribe) must still
        // have a strong reference for the actor to be alive. Mirror the same
        // set checked by `upgrade()` so the two calls cannot disagree.
        self.sender.strong_count() > 0
            && self.terminate_sender.strong_count() > 0
            && self.idle_subscribe_sender.strong_count() > 0
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
