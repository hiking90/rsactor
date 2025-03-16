use std::any::Any;

use anyhow::Result;

use crate::{actor, Errors};

/// Message type for an actor. Generally an enum
/// which muxes the various types of inner-messages the actor
/// supports
///
/// ## Example
///
/// ```rust
/// pub enum MyMessage {
///     /// Record the name to the actor state
///     RecordName(String),
///     /// Print the recorded name from the state to command line
///     PrintName,
/// }
/// ```
pub trait IMessage: Any + Send + Sized + 'static {
    fn from_message(msg: actor::Message) -> Result<Self, Errors> {
        if msg.message.is::<Self>() {
            Ok(*msg.message.downcast::<Self>().unwrap())
        } else {
            Err(Errors::Downcast)
        }
    }
}