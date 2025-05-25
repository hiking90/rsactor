// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::Identity;
use crate::error::{Error, Result};
use crate::{MailboxMessage, MailboxSender, ControlSignal};
use std::any::Any;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use log::{debug, warn};
use tokio::runtime::Handle;

/// A reference to an actor, allowing messages to be sent to it.
///
/// `ActorRef` provides a way to interact with actors without having direct access
/// to the actor instance itself. It holds a sender channel to the actor's mailbox.
///
/// ## Message Passing Methods
///
/// - **Asynchronous Methods**:
///   - [`ask`](ActorRef::ask): Send a message and await a reply.
///   - [`ask_with_timeout`](ActorRef::ask_with_timeout): Send a message and await a reply with a timeout.
///   - [`tell`](ActorRef::tell): Send a message without waiting for a reply.
///   - [`tell_with_timeout`](ActorRef::tell_with_timeout): Send a message without waiting for a reply with a timeout.
///
/// - **Blocking Methods for Tokio Blocking Contexts**:
///   - [`ask_blocking`](ActorRef::ask_blocking): Send a message and block until a reply is received.
///   - [`tell_blocking`](ActorRef::tell_blocking): Send a message and block until it is sent.
///
///   These methods are for use within `tokio::task::spawn_blocking` contexts.
///
/// - **Control Methods**:
///   - [`stop`](ActorRef::stop): Gracefully stop the actor.
///   - [`kill`](ActorRef::kill): Immediately terminate the actor.
#[derive(Clone, Debug)]
pub struct ActorRef {
    identity: Identity,
    sender: MailboxSender,
    terminate_sender: mpsc::Sender<ControlSignal>, // Changed type
}

impl ActorRef {
    // Creates a new ActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    pub(crate) fn new( // Made pub(crate) as it's likely called from lib.rs spawn function
        identity: Identity,
        sender: MailboxSender,
        terminate_sender: mpsc::Sender<ControlSignal>,
    ) -> Self { // Changed type
        ActorRef {
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
    pub fn is_alive(&self) -> bool {
        // Check if the sender channel is open
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing.
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
    /// Similar to `tell`, but allows specifying a timeout for the send operation.
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
    /// the actor to process the message and send a reply.
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
            Ok(Ok(reply_any)) => { // recv was Ok, actor reply was Ok
                match reply_any.downcast::<R>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        identity: self.identity,
                        expected_type: std::any::type_name::<R>().to_string(),
                    }),
                }
            }
            Ok(Err(e)) => Err(e), // recv was Ok, actor reply was Err
            Err(_recv_err) => Err(Error::Receive { // recv itself failed
                identity: self.identity,
                details: "Reply channel closed unexpectedly".to_string(),
            }),
        }
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to `ask`, but allows specifying a timeout for the operation.
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
                timeout,                // Added missing fields for consistency
                operation: "ask".to_string(),
            })?
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor will stop processing messages and shut down as soon as possible.
    /// The actor's final result will indicate it was killed.
    pub fn kill(&self) -> Result<()> {
        debug!("Attempting to send Terminate message to actor {} via dedicated channel using try_send", self.identity);
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
    /// The following example illustrates using `tell_blocking`. A similar approach applies to `ask_blocking`.
    ///
    /// ```rust,no_run
    /// # use rsactor::{ActorRef, Result}; // Assuming ActorRef and Result are available
    /// # use std::time::Duration;
    /// # struct MyMessage(&'static str);
    /// # fn example(actor_ref: ActorRef) -> Result<()> { // Assuming actor_ref is an ActorRef to a suitable actor
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
    /// For more comprehensive examples, including `ask_blocking`, refer to
    /// `examples/actor_blocking_tasks.rs`.
    pub fn tell_blocking<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        M: Send + 'static,
    {
        let rt = Handle::try_current().map_err(|e| {
            Error::Runtime {
                identity: self.identity,
                details: format!("Failed to get Tokio runtime handle for tell_blocking: {}", e)
            }
        })?;

        match timeout {
            Some(duration) => {
                rt.block_on(tokio::time::timeout(duration, self.tell(msg)))
                    .map_err(|_| Error::Timeout {
                        identity: self.identity,
                        timeout: duration,
                        operation: "tell_blocking".to_string(),
                    })? // Flatten Result<Result<()>> to Result<()>
            },
            None => rt.block_on(self.tell(msg)),
        }
    }

    /// Synchronous version of `ask` that blocks until the reply is received.
    ///
    /// The message is sent to the actor's mailbox, and this method will block until
    /// the actor processes the message and sends a reply or the timeout expires.
    ///
    /// # Examples
    ///
    /// For a complete example, see `examples/actor_blocking_tasks.rs`.
    ///
    /// ```rust,no_run
    /// # use rsactor::{ActorRef, Result}; // Assuming ActorRef and Result are available
    /// # use std::time::Duration;
    /// # struct QueryMessage;
    /// # struct QueryReply(String);
    /// # fn main() -> anyhow::Result<()> {
    /// #   let actor_ref: ActorRef = panic!(); // Placeholder
    ///     let result = tokio::task::spawn_blocking(move || {
    ///         let timeout = Some(Duration::from_secs(2));
    ///         // Assuming QueryMessage results in QueryReply which can be unwrapped to String
    ///         let response: String = actor_ref.ask_blocking::<_, QueryReply>(QueryMessage, timeout).unwrap().0;
    ///         // Process response...
    ///         response
    ///     });
    /// #   Ok(())
    /// # }
    /// ```
    /// Refer to the `examples/actor_blocking_tasks.rs` file for a runnable demonstration.
    pub fn ask_blocking<M, R>(&self, msg: M, timeout: Option<Duration>) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let rt = Handle::try_current().map_err(|e| {
            Error::Runtime {
                identity: self.identity,
                details: format!("Failed to get Tokio runtime handle for ask_blocking: {}", e)
            }
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
