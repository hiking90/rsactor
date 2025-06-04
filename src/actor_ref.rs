// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::error::{Error, Result};
use crate::Identity;
use crate::{Actor, ControlSignal, MailboxMessage, MailboxSender, Message};
use log::{debug, warn};
use std::any::Any;
use std::marker::PhantomData;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

/// A type-erased reference to an actor, allowing messages to be sent to it without type safety.
///
/// `UntypedActorRef` provides a way to interact with actors without having direct access
/// to the actor instance itself. It holds a sender channel to the actor's mailbox.
/// This is the internal implementation that handles the actual message passing.
///
/// ## Creating UntypedActorRef
///
/// `UntypedActorRef` instances are typically not created directly, but obtained from a typed [`ActorRef<T>`]:
///
/// ```ignore
/// // Get a reference to the untyped actor ref
/// let untyped_ref = actor_ref.untyped_actor_ref();
///
/// // Make a clone if needed
/// let cloned_untyped_ref = untyped_ref.clone();
/// ```
///
/// ## Type Safety Warning
///
/// **Developer Responsibility**: When using `UntypedActorRef`, you are responsible for ensuring
/// that message types match the target actor at runtime. Incorrect message types will result
/// in runtime errors instead of compile-time errors.
///
/// **Recommended Usage**: Use [`ActorRef<T>`] by default for compile-time type safety.
/// Only use `UntypedActorRef` when you specifically need type erasure for:
/// - Collections of heterogeneous actors (`Vec<UntypedActorRef>`, `HashMap<String, UntypedActorRef>`)
/// - Plugin systems with dynamically loaded actors
/// - Generic actor management interfaces
///
/// ## Message Passing Methods
///
/// - **Asynchronous Methods**:
///   - [`ask`](UntypedActorRef::ask): Send a message and await a reply.
///   - [`ask_with_timeout`](UntypedActorRef::ask_with_timeout): Send a message and await a reply with a timeout.
///   - [`tell`](UntypedActorRef::tell): Send a message without waiting for a reply.
///   - [`tell_with_timeout`](UntypedActorRef::tell_with_timeout): Send a message without waiting for a reply with a timeout.
///
/// - **Blocking Methods for Tokio Blocking Contexts**:
///   - [`ask_blocking`](UntypedActorRef::ask_blocking): Send a message and block until a reply is received.
///   - [`tell_blocking`](UntypedActorRef::tell_blocking): Send a message and block until it is sent.
///
///   These methods are for use within `tokio::task::spawn_blocking` contexts.
///
/// - **Control Methods**:
///   - [`stop`](UntypedActorRef::stop): Gracefully stop the actor.
///   - [`kill`](UntypedActorRef::kill): Immediately terminate the actor.
#[derive(Clone, Debug)]
pub struct UntypedActorRef {
    identity: Identity,
    sender: MailboxSender,
    terminate_sender: mpsc::Sender<ControlSignal>, // Changed type
}

impl UntypedActorRef {
    // Creates a new UntypedActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    pub(crate) fn new(
        // Made pub(crate) as it's likely called from lib.rs spawn function
        identity: Identity,
        sender: MailboxSender,
        terminate_sender: mpsc::Sender<ControlSignal>,
    ) -> Self {
        // Changed type
        UntypedActorRef {
            identity,
            sender,
            terminate_sender,
        }
    }

    /// Returns the unique ID of the actor.
    pub const fn identity(&self) -> Identity {
        self.identity
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
    /// Returns true only if both mailbox and terminate channels are open.
    pub fn is_alive(&self) -> bool {
        // Both channels must be open for the actor to be considered alive
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing via the actor's
    /// [`handle`](crate::actor::Message::handle) method implementation.
    /// This method returns immediately.
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
    {
        // For 'tell', no reply is expected, so no need for a reply_channel.
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: None, // reply_channel is None for tell
        };

        if self.sender.send(envelope).await.is_err() {
            Err(Error::Send {
                identity: self.identity,
                details: "Mailbox channel closed".to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget) with a timeout.
    ///
    /// Similar to [`UntypedActorRef::tell`], but allows specifying a timeout for the send operation.
    /// The message is sent to the actor's mailbox, and this method will return once
    /// the message is sent or timeout if the send operation doesn't complete
    /// within the specified duration.
    pub async fn tell_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
    {
        tokio::time::timeout(timeout, self.tell(msg))
            .await
            .map_err(|_| Error::Timeout {
                identity: self.identity,
                timeout,
                operation: "tell".to_string(),
            })?
    }

    /// Sends a message to the actor and awaits a reply.
    ///
    /// The message is sent to the actor\'s mailbox, and this method will wait for
    /// the actor to process the message via its [`handle`](crate::actor::Message::handle) method
    /// and send a reply back.
    pub async fn ask<M, R>(&self, msg: M) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
        };

        if self.sender.send(envelope).await.is_err() {
            return Err(Error::Send {
                identity: self.identity,
                details: "Mailbox channel closed".to_string(),
            });
        }

        match reply_rx.await {
            Ok(Ok(reply_any)) => {
                // recv was Ok, actor reply was Ok
                match reply_any.downcast::<R>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        identity: self.identity,
                        expected_type: std::any::type_name::<R>().to_string(),
                    }),
                }
            }
            Ok(Err(e)) => Err(e), // recv was Ok, actor reply was Err
            Err(_recv_err) => Err(Error::Receive {
                // recv itself failed
                identity: self.identity,
                details: "Reply channel closed unexpectedly".to_string(),
            }),
        }
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to [`UntypedActorRef::ask`], but allows specifying a timeout for the operation.
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply, or timeout if the reply
    /// doesn't arrive within the specified duration.
    pub async fn ask_with_timeout<M, R>(&self, msg: M, timeout: Duration) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        tokio::time::timeout(timeout, self.ask(msg))
            .await
            .map_err(|_| Error::Timeout {
                identity: self.identity, // Added missing fields for consistency
                timeout,                 // Added missing fields for consistency
                operation: "ask".to_string(),
            })?
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    /// This will trigger the actor's [`on_stop`](crate::Actor::on_stop) method with `killed = true`.
    pub fn kill(&self) -> Result<()> {
        debug!(
            "Attempting to send Terminate message to actor {} via dedicated channel using try_send",
            self.identity
        );
        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
            Ok(_) => {
                // Successfully sent the terminate message.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The channel is full. Since it has a capacity of 1,
                // this means a Terminate message is already in the queue.
                warn!("Failed to send Terminate to actor {}: terminate mailbox is full. Actor is likely already being terminated.", self.identity);
                // Considered Ok as the desired state (stopping/killed) is effectively met.
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed, which implies the actor is already stopped or has finished processing.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.identity);
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
    /// This will trigger the actor's [`on_stop`](crate::Actor::on_stop) method with `killed = false`.
    pub async fn stop(&self) -> Result<()> {
        debug!("Sending StopGracefully message to actor {}", self.identity);
        match self.sender.send(MailboxMessage::StopGracefully).await {
            Ok(_) => Ok(()),
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.identity);
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }

    // =========================================================================
    // Blocking functions for Tokio blocking tasks
    // =========================================================================

    /// # Blocking Functions for Tokio Tasks
    ///
    /// These functions are intended for scenarios where CPU-intensive or other blocking operations
    /// are performed within a `tokio::task::spawn_blocking` task, and communication
    /// with actors is necessary. They allow such tasks to interact with the actor system
    /// synchronously, without using `async/await` directly within the blocking task.
    ///
    /// ## Example
    ///
    /// The following example illustrates using [`UntypedActorRef::tell_blocking`]. A similar approach applies to [`UntypedActorRef::ask_blocking`].
    ///
    /// ```rust,no_run
    /// # use rsactor::{ActorRef, Result, Actor, Message};
    /// # use std::time::Duration;
    /// # struct MyActor;
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
    /// #         Ok(MyActor)
    /// #     }
    /// # }
    /// # struct MyMessage(&'static str);
    /// # impl Message<MyMessage> for MyActor {
    /// #     type Reply = ();
    /// #     async fn handle(&mut self, _msg: MyMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {
    /// #         ()
    /// #     }
    /// # }
    /// # fn example(actor_ref: ActorRef<MyActor>) -> Result<()> {
    /// let actor_clone = actor_ref.clone();
    /// tokio::task::spawn_blocking(move || {
    ///     // Perform CPU-intensive work
    ///
    ///     // Send results to actor
    ///     actor_clone.tell_blocking(MyMessage("Work completed"), Some(Duration::from_secs(1)))
    ///         .expect("Failed to send message");
    /// });
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// For more comprehensive examples, including [`UntypedActorRef::ask_blocking`], refer to
    /// `examples/actor_blocking_tasks.rs`.
    pub fn tell_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        M: Send + 'static,
    {
        let rt = Handle::try_current().map_err(|e| Error::Runtime {
            identity: self.identity,
            details: format!(
                "Failed to get Tokio runtime handle for tell_blocking: {}",
                e
            ),
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(tokio::time::timeout(duration, self.tell(msg)))
                    .map_err(|_| Error::Timeout {
                        identity: self.identity,
                        timeout: duration,
                        operation: "tell_blocking".to_string(),
                    })? // Flatten Result<Result<()>> to Result<()>
            }
            None => rt.block_on(self.tell(msg)),
        }
    }

    /// Synchronous version of [`UntypedActorRef::ask`] that blocks until the reply is received.
    ///
    /// The message is sent to the actor's mailbox, and this method will block until
    /// the actor processes the message and sends a reply or the timeout expires.
    ///
    /// # Examples
    ///
    /// For a complete example, see `examples/actor_blocking_tasks.rs`.
    ///
    /// ```rust,no_run
    /// # use rsactor::{ActorRef, Result, Actor, Message};
    /// # use std::time::Duration;
    /// # struct QueryActor;
    /// # impl Actor for QueryActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
    /// #         Ok(QueryActor)
    /// #     }
    /// # }
    /// # struct QueryMessage;
    /// # struct QueryReply(String);
    /// # impl Message<QueryMessage> for QueryActor {
    /// #     type Reply = QueryReply;
    /// #     async fn handle(&mut self, _msg: QueryMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {
    /// #         QueryReply("response".to_string())
    /// #     }
    /// # }
    /// # fn example(actor_ref: ActorRef<QueryActor>) -> anyhow::Result<()> {
    /// let actor_ref_clone = actor_ref.clone();
    /// let result = tokio::task::spawn_blocking(move || {
    ///     let timeout = Some(Duration::from_secs(2));
    ///     // Send query and wait for reply
    ///     let response: QueryReply = actor_ref_clone
    ///         .ask_blocking(QueryMessage, timeout)
    ///         .unwrap();
    ///     // Process response...
    ///     response.0
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Refer to the `examples/actor_blocking_tasks.rs` file for a runnable demonstration.
    pub fn ask_blocking<M, R>(&self, msg: M, timeout: Option<Duration>) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let rt = Handle::try_current().map_err(|e| Error::Runtime {
            identity: self.identity,
            details: format!("Failed to get Tokio runtime handle for ask_blocking: {}", e),
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(tokio::time::timeout(duration, self.ask(msg)))
                    .map_err(|_| Error::Timeout {
                        identity: self.identity,
                        timeout: duration,
                        operation: "ask_blocking".to_string(),
                    })? // Flatten Result<Result<R>> to Result<R>
            }
            None => rt.block_on(self.ask(msg)),
        }
    }
}

/// A type-safe reference to an actor of type `T`.
///
/// `ActorRef<T>` provides type-safe message passing to actors, ensuring that only
/// messages that the actor can handle are sent, and that reply types are correctly typed.
/// It wraps an [`UntypedActorRef`] and provides compile-time type safety through Rust's
/// type system and trait bounds.
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
///   - [`untyped_actor_ref`](ActorRef::untyped_actor_ref): Access the underlying [`UntypedActorRef`].
///
/// ## Recommended Usage
///
/// Use [`ActorRef<T>`] by default for all actor communication. It provides the same functionality
/// as [`UntypedActorRef`] but with compile-time guarantees that prevent type-related runtime errors.
///
/// **When to use `ActorRef<T>`**:
/// - Default choice for actor communication
/// - When you know the actor type at compile time
/// - When you want compile-time message validation
/// - When working with strongly-typed actor systems
///
/// **When to use `UntypedActorRef`**:
/// - Collections of heterogeneous actors (`Vec<UntypedActorRef>`, `HashMap<String, UntypedActorRef>`)
/// - Plugin systems with dynamically loaded actors
/// - Generic actor management interfaces
/// - When you need type erasure for dynamic scenarios
#[derive(Debug)]
pub struct ActorRef<T: Actor> {
    untyped_ref: UntypedActorRef,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Actor> ActorRef<T> {
    /// Creates a new type-safe ActorRef from an UntypedActorRef.
    #[inline]
    pub(crate) fn new(untyped_ref: UntypedActorRef) -> Self {
        ActorRef {
            untyped_ref,
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the underlying UntypedActorRef for cloning or other operations.
    #[inline]
    pub fn untyped_actor_ref(&self) -> &UntypedActorRef {
        &self.untyped_ref
    }

    /// Returns the unique ID of the actor.
    #[inline]
    pub fn identity(&self) -> Identity {
        self.untyped_ref.identity()
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
    #[inline]
    pub fn is_alive(&self) -> bool {
        self.untyped_ref.is_alive()
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing.
    /// This method returns immediately.
    ///
    /// Type safety: Only messages that the actor `T` can handle via [`Message<M>`] trait are accepted.
    #[inline]
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        self.untyped_ref.tell(msg).await
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget) with a timeout.
    ///
    /// Similar to [`ActorRef::tell`], but allows specifying a timeout for the send operation.
    /// The message is sent to the actor's mailbox, and this method will return once
    /// the message is sent or timeout if the send operation doesn't complete
    /// within the specified duration.
    #[inline]
    pub async fn tell_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        self.untyped_ref.tell_with_timeout(msg, timeout).await
    }

    /// Sends a message to the actor and awaits a reply.
    ///
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply.
    ///
    /// Type safety: The return type `R` is automatically inferred from the [`Message<M>`] trait
    /// implementation, ensuring compile-time type safety for replies.
    #[inline]
    pub async fn ask<M>(&self, msg: M) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.untyped_ref.ask(msg).await
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to [`ActorRef::ask`], but allows specifying a timeout for the operation.
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply, or timeout if the reply
    /// doesn't arrive within the specified duration.
    #[inline]
    pub async fn ask_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.untyped_ref.ask_with_timeout(msg, timeout).await
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    #[inline]
    pub fn kill(&self) -> Result<()> {
        self.untyped_ref.kill()
    }

    /// Sends a graceful stop signal to the actor.
    ///
    /// The actor will process all messages currently in its mailbox and then stop.
    /// New messages sent after this call might be ignored or fail.
    /// The actor's final result will indicate normal completion.
    #[inline]
    pub async fn stop(&self) -> Result<()> {
        self.untyped_ref.stop().await
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
    #[inline]
    pub fn tell_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        self.untyped_ref.tell_blocking(msg, timeout)
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method is intended for use within `tokio::task::spawn_blocking` contexts.
    #[inline]
    pub fn ask_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.untyped_ref.ask_blocking(msg, timeout)
    }
}

impl<T: Actor> Clone for ActorRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorRef {
            untyped_ref: self.untyped_ref.clone(),
            _phantom: PhantomData,
        }
    }
}
