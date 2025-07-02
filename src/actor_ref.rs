// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::error::{Error, Result};
use crate::Identity;
use crate::{Actor, ControlSignal, MailboxMessage, MailboxSender, Message};
use log::{debug, warn};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

#[cfg(feature = "tracing")]
use tracing::{debug as trace_debug, info, warn as trace_warn};

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
/// - **Blocking Methods for Tokio Blocking Contexts**:
///   - [`ask_blocking`](ActorRef::ask_blocking): Send a message and block until a typed reply is received.
///   - [`tell_blocking`](ActorRef::tell_blocking): Send a message and block until it is sent.
///
///   These methods are for use within `tokio::task::spawn_blocking` contexts.
///
/// - **Control Methods**:
///   - [`stop`](ActorRef::stop): Gracefully stop the actor.
///   - [`kill`](ActorRef::kill): Immediately terminate the actor.
///
/// - **Utility Methods**:
///   - [`identity`](ActorRef::identity): Get the unique ID of the actor.
///   - [`is_alive`](ActorRef::is_alive): Check if the actor is still running.
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
    id: Identity, // Added identity field for actor identification
    sender: MailboxSender<T>,
    pub(crate) terminate_sender: mpsc::Sender<ControlSignal>, // Changed type
}

impl<T: Actor> ActorRef<T> {
    /// Creates a new type-safe ActorRef.
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
    pub fn downgrade(this: &Self) -> ActorWeak<T> {
        ActorWeak {
            id: this.id, // Clone the Identity for type safety
            sender: this.sender.downgrade(),
            terminate_sender: this.terminate_sender.downgrade(),
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
        trace_debug!("Sending tell message (fire-and-forget)");

        let result = if self.sender.send(envelope).await.is_err() {
            Err(Error::Send {
                identity: self.identity(), // Use Identity::of for type safety
                details: "Mailbox channel closed".to_string(),
            })
        } else {
            Ok(())
        };

        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => trace_debug!("Tell message sent successfully"),
            Err(e) => trace_warn!(error = %e, "Failed to send tell message"),
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
        trace_debug!(
            timeout_ms = timeout.as_millis(),
            "Sending tell message with timeout"
        );

        let result = tokio::time::timeout(timeout, self.tell(msg))
            .await
            .map_err(|_| Error::Timeout {
                identity: self.identity(),
                timeout,
                operation: "tell".to_string(),
            })?;

        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => trace_debug!("Tell with timeout completed successfully"),
            Err(e) => trace_warn!(error = %e, "Tell with timeout failed"),
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
        trace_debug!("Sending ask message and waiting for reply");

        if self.sender.send(envelope).await.is_err() {
            #[cfg(feature = "tracing")]
            trace_warn!("Failed to send ask message: mailbox channel closed");

            return Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed".to_string(),
            });
        }

        match reply_rx.await {
            Ok(reply_any) => {
                // recv was Ok, actor reply was Ok
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        #[cfg(feature = "tracing")]
                        trace_debug!("Ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        #[cfg(feature = "tracing")]
                        trace_warn!(
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
                #[cfg(feature = "tracing")]
                trace_warn!("Ask reply channel closed unexpectedly");
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
        trace_debug!(
            timeout_ms = timeout.as_millis(),
            "Sending ask message with timeout"
        );

        let result = tokio::time::timeout(timeout, self.ask(msg))
            .await
            .map_err(|_| Error::Timeout {
                identity: self.identity(),
                timeout, // Added missing fields for consistency
                operation: "ask".to_string(),
            })?;

        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => trace_debug!("Ask with timeout completed successfully"),
            Err(e) => trace_warn!(error = %e, "Ask with timeout failed"),
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

        debug!(
            "Attempting to send Terminate message to actor {} via dedicated channel using try_send",
            self.identity()
        );
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
                #[cfg(feature = "tracing")]
                trace_warn!(
                    "Kill signal not sent: terminate mailbox full, actor already terminating"
                );
                // Considered Ok as the desired state (stopping/killed) is effectively met.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed, which implies the actor is already stopped or has finished processing.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.identity());
                #[cfg(feature = "tracing")]
                trace_warn!(
                    "Kill signal not sent: terminate mailbox closed, actor already stopped"
                );
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
                #[cfg(feature = "tracing")]
                trace_warn!(
                    "Stop signal not sent: mailbox closed, actor already stopped or stopping"
                );
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
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
        trace_debug!("Executing tell_blocking");

        let rt = Handle::try_current().map_err(|e| Error::Runtime {
            identity: self.identity(),
            details: format!("Failed to get Tokio runtime handle for tell_blocking: {e}"),
        })?;

        let result = match timeout {
            Some(duration) => {
                rt.block_on(tokio::time::timeout(duration, self.tell(msg)))
                    .map_err(|_| Error::Timeout {
                        identity: self.identity(),
                        timeout: duration,
                        operation: "tell_blocking".to_string(),
                    })? // Flatten Result<Result<()>> to Result<()>
            }
            None => rt.block_on(self.tell(msg)),
        };

        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => trace_debug!("Tell blocking completed successfully"),
            Err(e) => trace_warn!(error = %e, "Tell blocking failed"),
        }

        result
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
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
        trace_debug!("Executing ask_blocking");

        let rt = Handle::try_current().map_err(|e| Error::Runtime {
            identity: self.identity(),
            details: format!("Failed to get Tokio runtime handle for ask_blocking: {e}"),
        })?;

        let result = match timeout {
            Some(duration) => {
                rt.block_on(tokio::time::timeout(duration, self.ask(msg)))
                    .map_err(|_| Error::Timeout {
                        identity: self.identity(),
                        timeout: duration,
                        operation: "ask_blocking".to_string(),
                    })? // Flatten Result<Result<R>> to Result<R>
            }
            None => rt.block_on(self.ask(msg)),
        };

        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => trace_debug!("Ask blocking completed successfully"),
            Err(e) => trace_warn!(error = %e, "Ask blocking failed"),
        }

        result
    }
}

impl<T: Actor> Clone for ActorRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorRef {
            id: self.id, // Clone the Identity
            sender: self.sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
        }
    }
}

/// A weak, type-safe reference to an actor of type `T`.
///
/// `ActorWeak<T>` is a weak reference that does not prevent the actor from being dropped
/// and can be upgraded to a strong [`ActorRef<T>`] if the actor is still alive.
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
    id: Identity, // Added identity field for actor identification
    sender: tokio::sync::mpsc::WeakSender<MailboxMessage<T>>,
    terminate_sender: tokio::sync::mpsc::WeakSender<ControlSignal>,
}

impl<T: Actor> ActorWeak<T> {
    /// Attempts to upgrade the weak reference to a strong, type-safe reference.
    ///
    /// Returns `Some(ActorRef<T>)` if the actor is still alive, or `None` if the actor
    /// has been dropped.
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade both the mailbox sender and terminate sender
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;

        Some(ActorRef {
            id: self.id, // Use the stored Identity
            sender,
            terminate_sender,
        })
    }

    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Checks if the actor might still be alive.
    ///
    /// This method returns `true` if weak references can potentially be upgraded,
    /// but does not guarantee that a subsequent [`upgrade`](ActorWeak::upgrade) call will succeed.
    ///
    /// Returns `false` if the actor is definitely dead (all strong references dropped).
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
            id: self.id, // Clone the Identity
            sender: self.sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
        }
    }
}
