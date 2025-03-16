// Copyright (c) Sean Lawlor
//
// This source code is licensed under both the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree.

//! Messages which are built-in for `ractor`'s processing routines
//!
//! Additionally contains definitions for [BoxedState]
//! which are used to handle strongly-typed states in a
//! generic way without having to know the strong type in the underlying framework

use std::any::Any;
use std::fmt::{self, Debug, Display};

use anyhow::Result;

// use crate::message::BoxedDowncastErr;
// use crate::ActorProcessingErr;
use crate::{IMessage, IState};

pub(crate) struct DrainMessage;
impl IMessage for DrainMessage {}

#[derive(Debug)]
pub struct Message {
    pub(crate) message: Box<dyn Any + Send>,
    #[cfg(feature = "message_span_propogation")]
    pub(crate) span: tracing::Span,
}

impl Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Message {
    pub fn new<T: IMessage>(msg: T) -> Self {
        Self {
            message: Box::new(msg),
            #[cfg(feature = "message_span_propogation")]
            span: tracing::Span::current(),
        }
    }

    pub(crate) fn new_drain() -> Self {
        Self::new(DrainMessage)
    }

    pub fn is_drain(&self) -> bool {
        self.message.is::<DrainMessage>()
    }

    #[cfg(feature = "message_span_propogation")]
    pub fn with_span<T: IMessage>(msg: T, span: tracing::Span) -> Self {
        Self {
            message: Box::new(msg),
            span,
        }
    }

    pub fn span(&self) -> Option<&tracing::Span> {
        #[cfg(feature = "message_span_propogation")]
        {
            Some(&self.span)
        }
        #[cfg(not(feature = "message_span_propogation"))]
        {
            None
        }
    }
}

impl<T: IMessage> From<T> for Message {
    fn from(msg: T) -> Self {
        Message::new(msg)
    }
}

pub struct State {
    /// The message value
    pub msg: Option<Box<dyn Any + Send>>,
}

impl Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxedState").finish()
    }
}

impl State {
    /// Create a new [BoxedState] from a strongly-typed message
    pub fn new<T>(msg: T) -> Self
    where
        T: IState,
    {
        Self {
            msg: Some(Box::new(msg)),
        }
    }

    /// Try and take the resulting message as a specific type, consumes
    /// the boxed message
    pub fn take<T>(&mut self) -> Result<T, crate::Errors>
    where
        T: IState,
    {
        match self.msg.take() {
            Some(m) => {
                if m.is::<T>() {
                    Ok(*m.downcast::<T>().unwrap())
                } else {
                    Err(crate::Errors::Downcast)
                }
            }
            None => Err(crate::Errors::Downcast),
        }
    }
}

/// Messages to stop an actor
#[derive(Debug)]
pub enum StopMessage {
    /// Normal stop
    Stop,
    /// Stop with a reason
    Reason(String),
}

impl std::fmt::Display for StopMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stop => write!(f, "Stop"),
            Self::Reason(reason) => write!(f, "Stop (reason = {reason})"),
        }
    }
}

/// A supervision event from the supervision tree
pub enum SupervisionEvent {
    /// An actor was started
    Started(super::context::Context),
    /// An actor terminated. In the event it shutdown cleanly (i.e. didn't panic or get
    /// signaled) we capture the last state of the actor which can be used to re-build an actor
    /// should the need arise. Includes an optional "exit reason" if it could be captured
    /// and was provided
    Terminated(
        super::context::Context,
        Option<State>,
        Option<String>,
    ),
    /// An actor failed (due to panic or error case)
    Failed(super::context::Context, crate::Errors),

    // /// A subscribed process group changed
    // TODO: After implementing process groups, uncomment this.
    // ProcessGroupChanged(crate::pg::GroupChangeMessage),
}

impl IMessage for SupervisionEvent {}

impl SupervisionEvent {
    /// If this supervision event refers to an [Actor] lifecycle event, return
    /// the [ActorCell] for that [actor][Actor].
    ///
    ///
    /// [ActorCell]: crate::ActorCell
    /// [Actor]: crate::Actor
    pub fn actor_cell(&self) -> Option<&super::context::Context> {
        match self {
            Self::Started(who)
            | Self::Failed(who, _)
            | Self::Terminated(who, _, _) => Some(who),
            _ => None,
        }
    }

    /// If this supervision event refers to an [Actor] lifecycle event, return
    /// the [ActorId] for that [actor][Actor].
    ///
    /// [ActorId]: crate::ActorId
    /// [Actor]: crate::Actor
    pub fn actor_id(&self) -> Option<super::id::ActorId> {
        self.actor_cell().map(|cell| cell.get_id())
    }
}

impl Debug for SupervisionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Supervision event: {self}")
    }
}

impl std::fmt::Display for SupervisionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupervisionEvent::Started(actor) => {
                write!(f, "Started actor {actor:?}")
            }
            SupervisionEvent::Terminated(actor, _, reason) => {
                if let Some(r) = reason {
                    write!(f, "Stopped actor {actor:?} (reason = {r})")
                } else {
                    write!(f, "Stopped actor {actor:?}")
                }
            }
            SupervisionEvent::Failed(actor, panic_msg) => {
                write!(f, "Actor panicked {actor:?} - {panic_msg}")
            }
            // TODO: After implementing process groups, uncomment this.
            // SupervisionEvent::ProcessGroupChanged(change) => {
            //     write!(
            //         f,
            //         "Process group {} in scope {} changed",
            //         change.get_group(),
            //         change.get_scope()
            //     )
            // }
        }
    }
}

/// A signal message which takes priority above all else
#[derive(Clone, Debug)]
pub enum Signal {
    /// Terminate the agent, cancelling all async work immediately
    Kill,
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kill => {
                write!(f, "killed")
            }
        }
    }
}
