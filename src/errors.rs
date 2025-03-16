use thiserror::Error;

use crate::actor::Message;

#[derive(Debug, Error)]
pub enum Errors {
    #[error("Actor not found")]
    NotFound,
    #[error("Downcast failed")]
    Downcast,
    #[error("Actor processing error")]
    Processing,
    #[error("Actor channel closed")]
    ChannelClosed,
    #[error("Actor type invalid")]
    InvalidActorType,
    #[error("Actor message send failed: {0}")]
    SendFailed(Message),
    #[error("Actor spawn failed")]
    SpawnFailed,
    #[error("Actor already started")]
    ActorAlreadyStarted,
    #[error("Actor startup failed")]
    StartupFailed(Box<dyn std::any::Any + Send>),
    #[error("Actor terminattion failed")]
    ActorCancelled,
    #[error("Actor failed")]
    ActorFailed(Box<dyn std::any::Any + Send>),
    #[error("Actor unexpected error")]
    Unexpected(anyhow::Error),
    #[error("Timeout")]
    Timeout,
    #[error("Actor already registered")]
    AlreadyRegistered,
}

impl From<anyhow::Error> for Errors {
    fn from(error: anyhow::Error) -> Self {
        Errors::Unexpected(error)
    }
}