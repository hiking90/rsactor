// Copyright (c) Sean Lawlor
//
// This source code is licensed under both the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedReceiver as InputPortReceiver;
use tokio::sync::mpsc::UnboundedSender as InputPort;
use tokio::sync::oneshot::Receiver as OneshotReceiver;
use tokio::sync::oneshot::Sender as OneshotInputPort;
use tokio::sync::{self, mpsc};

use crate::actor::messages::StopMessage;
// use crate::actor::supervision::SupervisionTree;
use crate::{IMessage, Errors};
use crate::actor::{
    IActor, Message, ActorId, ActorStatus, Signal, 
    SupervisionEvent, SupervisionTree,
};

// The inner-properties of an Actor
#[derive(Debug)]
pub(crate) struct Properties {
    pub(crate) id: ActorId,
    pub(crate) name: Option<String>,
    status: Arc<AtomicU8>,
    wait_handler: Arc<sync::Notify>,
    pub(crate) signal: Mutex<Option<OneshotInputPort<Signal>>>,
    pub(crate) stop: Mutex<Option<OneshotInputPort<StopMessage>>>,
    pub(crate) supervision: InputPort<SupervisionEvent>,
    pub(crate) message: InputPort<Message>,
    pub(crate) tree: SupervisionTree,
    pub(crate) type_id: std::any::TypeId,
}

impl Properties {
    pub(crate) fn new<TActor>(
        name: Option<String>,
    ) -> (
        Self,
        OneshotReceiver<Signal>,
        OneshotReceiver<StopMessage>,
        InputPortReceiver<SupervisionEvent>,
        InputPortReceiver<Message>,
    )
    where
        TActor: IActor,
    {
        let id = crate::actor::get_new_local_id();
        let (tx_signal, rx_signal) = sync::oneshot::channel();
        let (tx_stop, rx_stop) = sync::oneshot::channel();
        let (tx_supervision, rx_supervision) = mpsc::unbounded_channel();
        let (tx_message, rx_message) = mpsc::unbounded_channel();
        (
            Self {
                id,
                name,
                status: Arc::new(AtomicU8::new(ActorStatus::Unstarted as u8)),
                signal: Mutex::new(Some(tx_signal)),
                wait_handler: Arc::new(sync::Notify::new()),
                stop: Mutex::new(Some(tx_stop)),
                supervision: tx_supervision,
                message: tx_message,
                tree: SupervisionTree::default(),
                type_id: std::any::TypeId::of::<TActor::Msg>(),
            },
            rx_signal,
            rx_stop,
            rx_supervision,
            rx_message,
        )
    }

    pub(crate) fn get_status(&self) -> ActorStatus {
        match self.status.load(Ordering::SeqCst) {
            0u8 => ActorStatus::Unstarted,
            1u8 => ActorStatus::Starting,
            2u8 => ActorStatus::Running,
            3u8 => ActorStatus::Upgrading,
            4u8 => ActorStatus::Draining,
            5u8 => ActorStatus::Stopping,
            _ => ActorStatus::Stopped,
        }
    }

    pub(crate) fn set_status(&self, status: ActorStatus) {
        self.status.store(status as u8, Ordering::SeqCst);
    }

    pub(crate) fn send_signal(&self, signal: Signal) -> Result<(), Errors> {
        self.signal
            .lock()
            .unwrap()
            .take()
            .map_or(Err(Errors::ChannelClosed), |prt| {
                prt.send(signal).map_err(|_| Errors::ChannelClosed)
            })
    }

    // TODO: After implementing SupervisionTree, uncomment this.
    /*
    pub(crate) fn send_supervisor_evt(
        &self,
        message: SupervisionEvent,
    ) -> Result<(), Errors> {
        self.supervision.send(message).map_err(|e| e.into())
    }
    */

    pub(crate) fn send_message<TMessage>(
        &self,
        message: TMessage,
    ) -> Result<(), Errors>
    where
        TMessage: IMessage,
    {
        if self.type_id != std::any::TypeId::of::<TMessage>() {
            return Err(Errors::InvalidActorType);
        }

        let status = self.get_status();
        if status >= ActorStatus::Draining {
            // if currently draining, stopping or stopped: reject messages directly.
            return Err(Errors::SendFailed(message.into()));
        }

        self.message
            .send(message.into())
            .map_err(|e| Errors::SendFailed(e.0))
    }

    pub(crate) fn drain(&self) -> Result<(), Errors> {
        let _ = self
            .status
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |f| {
                if f < (ActorStatus::Stopping as u8) {
                    Some(ActorStatus::Draining as u8)
                } else {
                    None
                }
            });
        self.message
            .send(Message::new_drain())
            .map_err(|e| Errors::SendFailed(e.0))
    }

    /// Start draining, and wait for the actor to exit
    pub(crate) async fn drain_and_wait(&self) -> Result<(), Errors> {
        let rx = self.wait_handler.notified();
        self.drain()?;
        rx.await;
        Ok(())
    }

    pub(crate) fn send_stop(
        &self,
        reason: Option<String>,
    ) -> Result<(), Errors> {
        let msg = reason.map(StopMessage::Reason).unwrap_or(StopMessage::Stop);
        self.stop
            .lock()
            .unwrap()
            .take()
            .map_or(Err(Errors::ChannelClosed), |prt| {
                prt.send(msg).map_err(|_| Errors::ChannelClosed)
            })
    }

    /// Send the stop signal, threading in a OneShot sender which notifies when the shutdown is completed
    pub(crate) async fn send_stop_and_wait(
        &self,
        reason: Option<String>,
    ) -> Result<(), Errors> {
        let rx = self.wait_handler.notified();
        self.send_stop(reason)?;
        rx.await;
        Ok(())
    }

    /*
    /// Wait for the actor to exit
    pub(crate) async fn wait(&self) {
        let rx = self.wait_handler.notified();
        rx.await;
    }

    /// Send the kill signal, threading in a OneShot sender which notifies when the shutdown is completed
    pub(crate) async fn send_signal_and_wait(
        &self,
        signal: Signal,
    ) -> Result<(), Errors> {
        // first bind the wait handler
        let rx = self.wait_handler.notified();
        let _ = self.send_signal(signal);
        rx.await;
        Ok(())
    }
    */

    pub(crate) fn notify_stop_listener(&self) {
        self.wait_handler.notify_waiters();
        // make sure that any future caller immediately returns by pre-storing
        // a notify permit (i.e. the actor stops, but you are only start waiting
        // after the actor has already notified it's dead.)
        self.wait_handler.notify_one();
    }

    pub(crate) fn send_supervisor_evt(
        &self,
        message: SupervisionEvent,
    ) -> Result<(), Errors> {
        self.supervision.send(message)
            .map_err(|e| Errors::SendFailed(e.0.into()))
    }
}
