use std::any::Any;
use std::fmt::Debug;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use log::{info, error, warn};

#[macro_export]
macro_rules! impl_message_handler {
    ($actor_type:ty, [$($msg_type:ty),+ $(,)?]) => {
        impl $crate::MessageHandler for $actor_type {
            async fn handle(
                &mut self,
                msg_any: Box<dyn std::any::Any + Send>,
            ) -> anyhow::Result<Box<dyn std::any::Any + Send>> {
                $(
                    if msg_any.is::<$msg_type>() {
                        match msg_any.downcast::<$msg_type>() {
                            Ok(msg) => {
                                let reply = <$actor_type as $crate::Message<$msg_type>>::handle(self, *msg).await;
                                return Ok(Box::new(reply) as Box<dyn std::any::Any + Send>);
                            }
                            Err(_) => {
                                return Err(anyhow::anyhow!(concat!("Internal error: Downcast to ", stringify!($msg_type), " failed after type check.")));
                            }
                        }
                    }
                )+
                Err(anyhow::anyhow!(concat!(stringify!($actor_type), ": ErasedMessageHandler received unknown message type.")))
            }
        }
    };
}

// Enum to represent messages within the actor system, including control messages.
pub enum MailboxMessage {
    Envelope {
        payload: Box<dyn Any + Send>,
        reply_channel: oneshot::Sender<Result<Box<dyn Any + Send>>>,
    },
    Terminate, // Signal to terminate the actor
    StopGracefully, // Signal to stop after processing existing messages
}

// Type alias for the sender part of the actor's mailbox channel.
type MailboxSender = mpsc::Sender<MailboxMessage>;

// Counter for generating unique actor IDs.
static ACTOR_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
pub struct ActorRef {
    id: usize,
    sender: MailboxSender,
}

impl ActorRef {
    // Creates a new ActorRef with a unique ID and the mailbox sender.
    // This is typically called by the System when an actor is spawned.
    fn new_internal(id: usize, sender: MailboxSender) -> Self {
        ActorRef {
            id,
            sender,
        }
    }

    pub const fn id(&self) -> usize {
        self.id
    }

    // Sends a message to the actor without awaiting a reply (fire and forget).
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
    {
        // We still need to create a oneshot channel because MailboxMessage expects it.
        // However, the sender part (reply_tx) will be dropped immediately by this method,
        // and the actor's run loop should handle the case where sending a reply fails.
        let (reply_tx, _reply_rx) = oneshot::channel::<Result<Box<dyn Any + Send>>>();
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: reply_tx,
        };

        if self.sender.send(envelope).await.is_err() {
            Err(anyhow::anyhow!(
                "Failed to send message to actor {}: mailbox channel closed",
                self.id
            ))
        } else {
            Ok(())
        }
    }

    // Sends a message to the actor and awaits a reply.
    // M is the message type, R is the expected reply type.
    pub async fn ask<M, R>(&self, msg: M) -> Result<R>
    where
        M: Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_any = Box::new(msg) as Box<dyn Any + Send>;

        let envelope = MailboxMessage::Envelope {
            payload: msg_any,
            reply_channel: reply_tx,
        };

        // Use the ActorRef's own sender
        if self.sender.send(envelope).await.is_err() {
            return Err(anyhow::anyhow!(
                "Failed to send message to actor {}: mailbox channel closed",
                self.id
            ));
        }

        match reply_rx.await {
            Ok(Ok(reply_any)) => reply_any
                .downcast::<R>()
                .map(|r| *r)
                .map_err(|_| anyhow::anyhow!("Failed to downcast reply to the expected type")),
            Ok(Err(e)) => Err(e), // Error from the actor's handler
            Err(_) => Err(anyhow::anyhow!(
                "Failed to receive reply from actor {}: reply channel closed",
                self.id
            )),
        }
    }

    // New method to send a termination signal to the actor.
    pub async fn kill(&self) -> Result<()> {
        info!("Sending Terminate message to actor {}", self.id);
        match self.sender.send(MailboxMessage::Terminate).await {
            Ok(_) => Ok(()),
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send Terminate to actor {}: mailbox closed. Actor might already be stopped.", self.id);
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }

    // New method to send a graceful stop signal to the actor.
    // The actor will process all messages currently in its mailbox and then stop.
    // New messages sent after this call might be ignored or fail.
    pub async fn stop(&self) -> Result<()> {
        info!("Sending StopGracefully message to actor {}", self.id);
        match self.sender.send(MailboxMessage::StopGracefully).await {
            Ok(_) => Ok(()),
            Err(_) => {
                // This error means the actor's mailbox channel is closed,
                // which implies the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.id);
                // Considered Ok as the desired state (stopped/stopping) is met.
                Ok(())
            }
        }
    }
}

pub trait Actor: Send + 'static {
    type Error: Send + Debug + 'static; // Error type for the actor

    fn on_start(&mut self, _actor_ref: ActorRef) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn on_stop(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }
}

// Trait for messages that an actor can handle.
// The actor struct (e.g., MyActor) implements this for each message type it handles.
pub trait Message<T: Send + 'static>: Actor {
    type Reply: Send + 'static; // Reply type must be Send and 'static for oneshot channel

    // Handles the message. This is an async method.
    fn handle(&mut self, msg: T) -> impl Future<Output = Self::Reply> + Send;
}

// New trait for type-erased message handling within the Runtime.
// Actors that can be managed by the system must implement this.
pub trait MessageHandler: Send + Sync + 'static {
    fn handle(
        &mut self,
        msg_any: Box<dyn Any + Send>,
    ) -> impl Future<Output = Result<Box<dyn Any + Send>>> + Send;
}

// Manages the lifecycle and message loop for a single actor instance.
pub struct Runtime<T: Actor + MessageHandler> {
    actor_ref: ActorRef,
    actor: T,
    receiver: mpsc::Receiver<MailboxMessage>, // Receives messages for this actor
}

impl<T: Actor + MessageHandler> Runtime<T> {
    pub fn new(
        actor: T,
        actor_ref: ActorRef,
        receiver: mpsc::Receiver<MailboxMessage>,
    ) -> Self {
        Runtime {
            actor_ref,
            actor,
            receiver,
        }
    }

    // The main loop for the actor. Receives messages and dispatches them.
    pub async fn run(&mut self) -> Result<(), T::Error> {
        self.actor.on_start(self.actor_ref.clone()).await?;
        info!("Runtime for actor {} is running.", self.actor_ref.id());

        let mut gracefully_stopping = false;

        while let Some(actor_message) = self.receiver.recv().await {
            match actor_message {
                MailboxMessage::Envelope { payload, reply_channel } => {
                    if gracefully_stopping {
                        warn!("Actor {} is stopping gracefully, ignoring Envelope message.", self.actor_ref.id());
                        // The reply_channel will be dropped, causing the ask() call to receive an error.
                        continue;
                    }
                    let handling_actor_id = self.actor_ref.id(); // For logging
                    let result = self.actor.handle(payload).await;
                    if reply_channel.send(result).is_err() {
                        // This can happen if the sender (ActorRef::ask) drops the reply_rx
                        // before a reply is sent, e.g., due to a timeout or cancellation.
                        warn!(
                            "Failed to send reply: reply channel closed for actor {}",
                            handling_actor_id
                        );
                    }
                }
                MailboxMessage::Terminate => {
                    info!("Actor {} received Terminate message. Initiating immediate shutdown.", self.actor_ref.id());
                    break; // Exit the loop to proceed to on_stop
                }
                MailboxMessage::StopGracefully => {
                    info!("Actor {} received StopGracefully message. Will process remaining messages then shut down. New messages will be ignored.", self.actor_ref.id());
                    gracefully_stopping = true;
                    self.receiver.close(); // Close the receiver. recv() will drain existing messages then return None.
                }
            }
        }

        // This part is reached when the mailbox channel is closed (all Senders dropped)
        // or when a Terminate message is received, or StopGracefully is processed and queue emptied.
        info!(
            "Runtime for actor {} is shutting down.",
            self.actor_ref.id()
        );
        self.actor.on_stop().await?;
        Ok(())
    }
}

// Spawns a new actor and returns an ActorRef to it.
pub async fn spawn<T: Actor + MessageHandler + 'static>(
    actor: T,
) -> Result<ActorRef> {
    let actor_id = ACTOR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel::<MailboxMessage>(32); // Mailbox with buffer size 32

    let actor_ref = ActorRef::new_internal(actor_id, tx);

    let mut runtime = Runtime::new(actor, actor_ref.clone(), rx);
    let runtime_actor_id = runtime.actor_ref.id(); // For logging

    tokio::spawn(async move {
        info!("Spawning task for actor {}.", runtime_actor_id);
        if let Err(e) = runtime.run().await {
            error!(
                "Actor {} runtime error: {:?}",
                runtime_actor_id, e
            );
        }
        info!("Actor {} task finished.", runtime_actor_id);
    });

    Ok(actor_ref)
}

