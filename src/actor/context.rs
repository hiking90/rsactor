use std::sync::Arc;

use futures::FutureExt;

use tokio::time;
use tokio::sync::mpsc::UnboundedReceiver as InputPortReceiver;
use tokio::sync::oneshot::Receiver as OneshotReceiver;

use crate::{
    Errors, IMessage, 
    actor::{
        Properties, IActor, Signal, StopMessage, Message, ActorId,
        SupervisionEvent
    },
};


/// [ActorStatus] represents the status of an actor's lifecycle
#[derive(Debug, Clone, Eq, PartialEq, Copy, PartialOrd, Ord)]
#[repr(u8)]
pub enum ActorStatus {
    /// Created, but not yet started
    Unstarted = 0u8,
    /// Starting
    Starting = 1u8,
    /// Executing (or waiting on messages)
    Running = 2u8,
    /// Upgrading
    Upgrading = 3u8,
    /// Draining
    Draining = 4u8,
    /// Stopping
    Stopping = 5u8,
    /// Dead
    Stopped = 6u8,
}


/// The collection of ports an actor needs to listen to
pub(crate) struct Receivers {
    /// The inner signal port
    pub(crate) signal_rx: OneshotReceiver<Signal>,
    /// The inner stop port
    pub(crate) stop_rx: OneshotReceiver<StopMessage>,
    /// The inner supervisor port
    pub(crate) supervisor_rx: InputPortReceiver<SupervisionEvent>,
    /// The inner message port
    pub(crate) message_rx: InputPortReceiver<Message>,
}

impl Drop for Receivers {
    fn drop(&mut self) {
        // Close all the message ports and flush all the message queue backlogs.
        // See: https://docs.rs/tokio/0.1.22/tokio/sync/mpsc/index.html#clean-shutdown
        self.signal_rx.close();
        self.stop_rx.close();
        // TODO: After implementing SupervisionTree, uncomment this.
        // self.supervisor_rx.close();
        self.message_rx.close();

        while self.signal_rx.try_recv().is_ok() {}
        while self.stop_rx.try_recv().is_ok() {}
        while self.supervisor_rx.try_recv().is_ok() {}
        while self.message_rx.try_recv().is_ok() {}
    }
}

/// Messages that come in off an actor's port, with associated priority
pub(crate) enum ActorPortMessage {
    /// A signal message
    Signal(Signal),
    /// A stop message
    Stop(StopMessage),
    /// A supervision message
    Supervision(SupervisionEvent),
    /// A regular message
    Message(Message),
}

impl Receivers {
    /// Run a future beside the signal port, so that
    /// the signal port can terminate the async work
    ///
    /// * `future` - The future to execute
    ///
    /// Returns [Ok(`TState`)] when the future completes without
    /// signal interruption, [Err(Signal)] in the event the
    /// signal interrupts the async work.
    pub(crate) async fn run_with_signal<TState>(
        &mut self,
        future: impl std::future::Future<Output = TState>,
    ) -> Result<TState, Signal>
    where
        TState: crate::IState,
    {
        tokio::select! {
            // supervision or message processing work
            // can be interrupted by the signal port receiving
            // a kill signal
            signal = (&mut self.signal_rx).fuse() => {
                Err(signal.unwrap_or(Signal::Kill))
            }
            new_state = future.fuse() => {
                Ok(new_state)
            }
        }
    }

    /// List to the input ports in priority. The priority of listening for messages is
    /// 1. Signal port
    /// 2. Stop port
    /// 3. Supervision message port
    /// 4. General message port
    ///
    /// Returns [Ok(ActorPortMessage)] on a successful message reception, [MessagingErr]
    /// in the event any of the channels is closed.
    pub(crate) async fn listen_in_priority(
        &mut self,
    ) -> Result<ActorPortMessage, Errors> {
        tokio::select! {
            signal = (&mut self.signal_rx).fuse() => {
                signal.map(ActorPortMessage::Signal).map_err(|_| Errors::ChannelClosed)
            }
            stop = (&mut self.stop_rx).fuse() => {
                stop.map(ActorPortMessage::Stop).map_err(|_| Errors::ChannelClosed)
            }
            // TODO: After implementing SupervisionTree, uncomment this.
            // supervision = self.supervisor_rx.recv().fuse() => {
            //     supervision.map(ActorPortMessage::Supervision).ok_or(Errors::ChannelClosed)
            // }
            message = self.message_rx.recv().fuse() => {
                message.map(ActorPortMessage::Message).ok_or(Errors::ChannelClosed)
            }
        }
    }
}


/// An [ActorHandle] is a strongly-typed wrapper over an [ActorCell]
/// to provide some syntactic wrapping on the requirement to pass
/// the actor's message type everywhere.
///
/// An [ActorHandle] is the primary means of communication typically used
/// when interfacing with [super::Actor]s
pub struct Context {
    pub(crate) inner: Arc<Properties>,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Context {
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("name", &self.get_name())
            .field("id", &self.get_id())
            .finish()
    }
}

impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        other.get_id() == self.get_id()
    }
}

impl Eq for Context {}

impl std::hash::Hash for Context {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_id().hash(state)
    }
}

impl Context {
    /// Construct a new [ActorCell] pointing to an [super::Actor] and return the message reception channels as a [ActorPortSet]
    ///
    /// * `name` - Optional name for the actor
    ///
    /// Returns a tuple [(ActorCell, ActorPortSet)] to bootstrap the [crate::Actor]
    pub(crate) fn new<TActor>(system: &crate::System, name: Option<String>) -> Result<(Self, Receivers), Errors>
    where
        TActor: IActor,
    {
        let (props, rx1, rx2, rx3, rx4) = Properties::new::<TActor>(name.clone());
        let cell = Self {
            inner: Arc::new(props),
        };

        if let Some(r_name) = name {
            system.register(r_name, cell.clone())?;
        }

        Ok((
            cell,
            Receivers {
                signal_rx: rx1,
                stop_rx: rx2,
                supervisor_rx: rx3,
                message_rx: rx4,
            },
        ))
    }

    /// Retrieve the [super::Actor]'s unique identifier [ActorId]
    pub fn get_id(&self) -> ActorId {
        self.inner.id
    }

    /// Retrieve the [super::Actor]'s name
    pub fn get_name(&self) -> Option<&str> {
        self.inner.name.as_deref()
    }
    
    // TODO: After implementing SupervisionTree, uncomment this.
    /*
    /// Notify the supervisor and all monitors that a supervision event occurred.
    /// Monitors receive a reduced copy of the supervision event which won't contain
    /// the [crate::actor::BoxedState] and collapses the [crate::ActorProcessingErr]
    /// exception to a [String]
    ///
    /// * `evt` - The event to send to this [crate::Actor]'s supervisors
    pub fn notify_supervisor_and_monitors(&self, evt: SupervisionEvent) {
        self.inner.notify_supervisor(evt)
    }
    */

    /// Stop this [super::Actor] gracefully (stopping message processing)
    ///
    /// * `reason` - An optional string reason why the stop is occurring
    pub fn stop(&self, reason: Option<String>) {
        // ignore failures, since that means the actor is dead already
        let _ = self.inner.send_stop(reason);
    }


    /// Set the status of the [super::Actor]. If the status is set to
    /// [ActorStatus::Stopping] or [ActorStatus::Stopped] the actor
    /// will also be unenrolled from both the named registry ([crate::registry])
    /// and the PG groups ([crate::pg]) if it's enrolled in any
    ///
    /// * `status` - The [ActorStatus] to set
    pub(crate) fn set_status(&self, status: ActorStatus) {
        // The actor is shut down
        // TODO: After implementing Registry, uncomment this.
        // if status == ActorStatus::Stopped || status == ActorStatus::Stopping {
        //     // If it's enrolled in the registry, remove it
        //     if let Some(name) = self.get_name() {
        //         crate::registry::unregister(name);
        //     }
        //     // Leave all + stop monitoring pg groups (if any)
        //     crate::pg::demonitor_all(self.get_id());
        //     crate::pg::leave_all(self.get_id());
        // }

        // Fix for #254. We should only notify the stop listener AFTER post_stop
        // has executed, which is when the state gets set to `Stopped`.
        if status == ActorStatus::Stopped {
            // notify whoever might be waiting on the stop signal
            self.inner.notify_stop_listener();
        }

        self.inner.set_status(status)
    }

    /// Retrieve the current status of an [super::Actor]
    ///
    /// Returns the [super::Actor]'s current [ActorStatus]
    pub fn get_status(&self) -> ActorStatus {
        self.inner.get_status()
    }

        /// Link this [super::Actor] to the provided supervisor
    ///
    /// * `supervisor` - The supervisor [super::Actor] of this actor
    pub fn link(&self, supervisor: Context) {
        supervisor.inner.tree.insert_child(self.clone());
        self.inner.tree.set_supervisor(supervisor);
    }

    /// Unlink this [super::Actor] from the supervisor if it's
    /// currently linked (if self's supervisor is `supervisor`)
    ///
    /// * `supervisor` - The supervisor to unlink this [super::Actor] from
    pub fn unlink(&self, supervisor: Context) {
        if self.inner.tree.is_child_of(supervisor.get_id()) {
            supervisor.inner.tree.remove_child(self.get_id());
            self.inner.tree.clear_supervisor();
        }
    }

    /// Terminate this [super::Actor] and all it's children
    pub(crate) fn terminate(&self) {
        // we don't need to notify of exit if we're already stopping or stopped
        if self.get_status() as u8 <= ActorStatus::Upgrading as u8 {
            // kill myself immediately. Ignores failures, as a failure means either
            // 1. we're already dead or
            // 2. the channel is full of "signals"
            self.kill();
        }

        // notify children they should die. They will unlink themselves from the supervisor
        self.inner.tree.terminate_all_children();
    }

    /// Kill this [super::Actor] forcefully (terminates async work)
    pub fn kill(&self) {
        let _ = self.inner.send_signal(Signal::Kill);
    }

    /// Clear the supervisor field
    pub(crate) fn clear_supervisor(&self) {
        self.inner.tree.clear_supervisor();
    }

    /// Drain the actor's message queue and when finished processing, terminate the actor.
    ///
    /// Any messages received after the drain marker but prior to shutdown will be rejected
    pub fn drain(&self) -> Result<(), Errors> {
        self.inner.drain()
    }

    /// Notify the supervisor and all monitors that a supervision event occurred.
    /// Monitors receive a reduced copy of the supervision event which won't contain
    /// the [crate::actor::BoxedState] and collapses the [crate::ActorProcessingErr]
    /// exception to a [String]
    ///
    /// * `evt` - The event to send to this [super::Actor]'s supervisors
    pub fn notify_supervisor(&self, evt: SupervisionEvent) {
        self.inner.tree.notify_supervisor(evt)
    }

    /// Stop the [super::Actor] gracefully (stopping messaging processing)
    /// and wait for the actor shutdown to complete
    ///
    /// * `reason` - An optional string reason why the stop is occurring
    /// * `timeout` - An optional timeout duration to wait for shutdown to occur
    ///
    /// Returns [Ok(())] upon the actor being stopped/shutdown. [Err(RactorErr::Messaging(_))] if the channel is closed
    /// or dropped (which may indicate some other process is trying to shutdown this actor) or [Err(RactorErr::Timeout)]
    /// if timeout was hit before the actor was successfully shut down (when set)
    pub async fn stop_and_wait(
        &self,
        reason: Option<String>,
        timeout: Option<time::Duration>,
    ) -> Result<(), Errors> {
        if let Some(to) = timeout {
            match crate::timeout(to, self.inner.send_stop_and_wait(reason)).await {
                Err(_) => Err(Errors::Timeout),
                Ok(Err(e)) => Err(e.into()),
                Ok(_) => Ok(()),
            }
        } else {
            Ok(self.inner.send_stop_and_wait(reason).await?)
        }
    }

    /// Drain the actor's message queue and when finished processing, terminate the actor,
    /// notifying on this handler that the actor has drained and exited (stopped).
    ///
    /// * `timeout`: The optional amount of time to wait for the drain to complete.
    ///
    /// Any messages received after the drain marker but prior to shutdown will be rejected
    pub async fn drain_and_wait(
        &self,
        timeout: Option<time::Duration>,
    ) -> Result<(), Errors> {
        if let Some(to) = timeout {
            match crate::timeout(to, self.inner.drain_and_wait()).await {
                Err(_) => Err(Errors::Timeout),
                Ok(Err(e)) => Err(e.into()),
                Ok(_) => Ok(()),
            }
        } else {
            Ok(self.inner.drain_and_wait().await?)
        }
    }

    /// Send a supervisor event to the supervisory port
    ///
    /// * `message` - The [SupervisionEvent] to send to the supervisory port
    ///
    /// Returns [Ok(())] on successful message send, [Err(MessagingErr)] otherwise
    pub(crate) fn send_supervisor_evt(
        &self,
        message: SupervisionEvent,
    ) -> Result<(), Errors> {
        self.inner.send_supervisor_evt(message)
    }
}

impl Context
{
    /// Send a strongly-typed message, constructing the boxed message on the fly
    ///
    /// * `message` - The message to send
    ///
    /// Returns [Ok(())] on successful message send, [Err(MessagingErr)] otherwise
    pub fn send_message<TMessage: IMessage>(&self, message: TMessage) -> Result<(), Errors> {
        self.inner.send_message::<TMessage>(message)
    }
}
