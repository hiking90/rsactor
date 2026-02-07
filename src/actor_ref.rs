// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::error::{Error, Result};
use crate::Identity;
use crate::{Actor, ControlSignal, MailboxMessage, MailboxSender, Message};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

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
    /// Channel for sending control signals (e.g., terminate) to the actor
    pub(crate) terminate_sender: mpsc::Sender<ControlSignal>,
    /// Per-actor metrics collector (when metrics feature is enabled)
    #[cfg(feature = "metrics")]
    pub(crate) metrics: Arc<MetricsCollector>,
}

impl<T: Actor> ActorRef<T> {
    /// Creates a new type-safe ActorRef.
    #[cfg(not(feature = "metrics"))]
    pub(crate) fn new(
        id: Identity,
        sender: MailboxSender<T>,
        terminate_sender: mpsc::Sender<ControlSignal>,
    ) -> Self {
        ActorRef {
            id,
            sender,
            terminate_sender,
        }
    }

    /// Creates a new type-safe ActorRef with metrics collector.
    #[cfg(feature = "metrics")]
    pub(crate) fn new(
        id: Identity,
        sender: MailboxSender<T>,
        terminate_sender: mpsc::Sender<ControlSignal>,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        ActorRef {
            id,
            sender,
            terminate_sender,
            metrics,
        }
    }

    /// Returns the unique ID of the actor.
    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
    #[inline]
    pub fn is_alive(&self) -> bool {
        // Both channels must be open for the actor to be considered alive
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Creates a weak, type-safe reference to this actor.
    ///
    /// The returned [`ActorWeak<T>`] can be used to check if the actor is still alive
    /// and optionally upgrade back to a strong [`ActorRef<T>`] without keeping the actor alive.
    #[cfg(not(feature = "metrics"))]
    pub fn downgrade(this: &Self) -> ActorWeak<T> {
        ActorWeak {
            id: this.id,
            sender: this.sender.downgrade(),
            terminate_sender: this.terminate_sender.downgrade(),
        }
    }

    /// Creates a weak, type-safe reference to this actor.
    ///
    /// The returned [`ActorWeak<T>`] can be used to check if the actor is still alive
    /// and optionally upgrade back to a strong [`ActorRef<T>`] without keeping the actor alive.
    /// Metrics are preserved via strong reference for post-mortem analysis.
    #[cfg(feature = "metrics")]
    pub fn downgrade(this: &Self) -> ActorWeak<T> {
        ActorWeak {
            id: this.id,
            sender: this.sender.downgrade(),
            terminate_sender: this.terminate_sender.downgrade(),
            metrics: this.metrics.clone(), // Strong ref - survives actor drop
        }
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

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_kill", skip(self))
    )]
    pub fn kill(&self) -> Result<()> {
        #[cfg(feature = "tracing")]
        info!(actor_id = %self.identity(), "Killing actor");

        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
            Ok(_) => {
                // Successfully sent the terminate message.
                #[cfg(feature = "tracing")]
                info!("Kill signal sent successfully");
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The channel is full. Since it has a capacity of 1,
                // this means a Terminate message is already in the queue.
                warn!("Failed to send Terminate to actor {}: terminate mailbox is full. Actor is likely already being terminated.", self.identity());
                // Considered Ok as the desired state (stopping/killed) is effectively met.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed, which implies the actor is already stopped or has finished processing.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.identity());
                // Considered Ok as the desired state (stopped) is met.
                Ok(())
            }
        }
    }

    /// Sends a graceful stop signal to the actor.
    ///
    /// The actor will process all messages currently in its mailbox and then stop.
    /// New messages sent after this call might be ignored or fail.
    /// The actor's final result will indicate normal completion.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_stop", skip(self))
    )]
    pub async fn stop(&self) -> Result<()> {
        match self
            .sender
            .send(MailboxMessage::StopGracefully(self.clone()))
            .await
        {
            Ok(_) => {
                #[cfg(feature = "tracing")]
                info!(actor_id = %self.identity(), "Actor stop signal sent successfully");
                Ok(())
            }
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.identity());
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method can be used from any thread, including non-async contexts.
    /// It does not require a Tokio runtime context.
    ///
    /// # Timeout Behavior
    ///
    /// - **`timeout: None`**: Uses Tokio's `blocking_send` directly. This is the most efficient
    ///   option but will block indefinitely if the mailbox is full.
    ///
    /// - **`timeout: Some(duration)`**: Spawns a separate thread with a temporary Tokio runtime
    ///   to handle the timeout. This approach has additional overhead but guarantees the call
    ///   will return within the specified duration.
    ///
    /// # Performance Considerations
    ///
    /// When using a timeout, this method incurs the following overhead:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    /// - **Channel synchronization**: Minimal overhead for result passing
    ///
    /// For performance-critical code paths where timeout is not required, pass `None` to avoid
    /// this overhead. The timeout variant is designed for scenarios where bounded waiting is
    /// more important than raw performance.
    ///
    /// # Thread Safety
    ///
    /// This method is safe to call from within an existing Tokio runtime context because
    /// the timeout implementation spawns a separate thread with its own runtime, avoiding
    /// the "cannot start a runtime from within a runtime" panic.
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
        let self_clone = self.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build();

            let result = match rt {
                Ok(runtime) => runtime.block_on(async {
                    tokio::time::timeout(timeout, self_clone.tell(msg))
                        .await
                        .map_err(|_| {
                            crate::dead_letter::record::<M>(
                                self_clone.identity(),
                                crate::dead_letter::DeadLetterReason::Timeout,
                                "blocking_tell",
                            );
                            Error::Timeout {
                                identity: self_clone.identity(),
                                timeout,
                                operation: "blocking_tell".to_string(),
                            }
                        })?
                }),
                Err(e) => Err(Error::Send {
                    identity: self_clone.identity(),
                    details: format!("Failed to create runtime: {}", e),
                }),
            };

            let _ = tx.send(result);
        });

        rx.recv().map_err(|_| Error::Send {
            identity: self.identity(),
            details: "Timeout thread terminated unexpectedly".to_string(),
        })?
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method can be used from any thread, including non-async contexts.
    /// It does not require a Tokio runtime context.
    ///
    /// # Timeout Behavior
    ///
    /// - **`timeout: None`**: Uses Tokio's `blocking_send` and `blocking_recv` directly.
    ///   This is the most efficient option but will block indefinitely if the actor
    ///   never responds.
    ///
    /// - **`timeout: Some(duration)`**: Spawns a separate thread with a temporary Tokio runtime
    ///   to handle the timeout. This approach has additional overhead but guarantees the call
    ///   will return within the specified duration.
    ///
    /// # Performance Considerations
    ///
    /// When using a timeout, this method incurs the following overhead:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    /// - **Channel synchronization**: Minimal overhead for result passing
    ///
    /// For performance-critical code paths where timeout is not required, pass `None` to avoid
    /// this overhead. The timeout variant is designed for scenarios where bounded waiting is
    /// more important than raw performance.
    ///
    /// # Thread Safety
    ///
    /// This method is safe to call from within an existing Tokio runtime context because
    /// the timeout implementation spawns a separate thread with its own runtime, avoiding
    /// the "cannot start a runtime from within a runtime" panic.
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
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build();

            let result = match rt {
                Ok(runtime) => runtime.block_on(async {
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
                }),
                Err(e) => Err(Error::Send {
                    identity: self_clone.identity(),
                    details: format!("Failed to create runtime: {}", e),
                }),
            };

            let _ = tx.send(result);
        });

        rx.recv().map_err(|_| Error::Send {
            identity: self.identity(),
            details: "Timeout thread terminated unexpectedly".to_string(),
        })?
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

    /// Returns the total number of errors during message handling.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn error_count(&self) -> u64 {
        self.metrics.error_count()
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

    /// Returns a reference to the internal metrics collector.
    ///
    /// This is primarily for internal use by the message processing loop.
    #[cfg(feature = "metrics")]
    #[inline]
    pub(crate) fn metrics_collector(&self) -> &MetricsCollector {
        &self.metrics
    }
}

impl<T: Actor> Clone for ActorRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorRef {
            id: self.id,
            sender: self.sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
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
    /// Weak reference to the actor's terminate signal sender
    terminate_sender: tokio::sync::mpsc::WeakSender<ControlSignal>,
    /// Strong reference to metrics (survives actor drop for post-mortem analysis)
    #[cfg(feature = "metrics")]
    metrics: Arc<MetricsCollector>,
}

impl<T: Actor> ActorWeak<T> {
    /// Attempts to upgrade the weak reference to a strong, type-safe reference.
    ///
    /// Returns `Some(ActorRef<T>)` if the actor is still alive, or `None` if the actor
    /// has been dropped.
    #[cfg(not(feature = "metrics"))]
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade both the mailbox sender and terminate sender
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;

        Some(ActorRef {
            id: self.id,
            sender,
            terminate_sender,
        })
    }

    /// Attempts to upgrade the weak reference to a strong, type-safe reference.
    ///
    /// Returns `Some(ActorRef<T>)` if the actor is still alive, or `None` if the actor
    /// has been dropped.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade both the mailbox sender and terminate sender
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;

        Some(ActorRef {
            id: self.id,
            sender,
            terminate_sender,
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
        // Both channels must have strong references for the actor to be alive
        self.sender.strong_count() > 0 && self.terminate_sender.strong_count() > 0
    }
}

impl<T: Actor> Clone for ActorWeak<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorWeak {
            id: self.id,
            sender: self.sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }
}
