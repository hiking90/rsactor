// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::Identity;
use std::time::Duration;

#[derive(Debug)]
/// Represents errors that can occur in the rsactor framework.
///
/// These errors may be encountered during various actor operations, such as sending messages
/// with [`tell`](crate::actor_ref::ActorRef::tell) or [`ask`](crate::actor_ref::ActorRef::ask),
/// or during actor lifecycle operations like [`spawn`](crate::spawn).
pub enum Error {
    /// Error when sending a message to an actor
    Send {
        /// ID of the actor that failed to receive the message
        identity: Identity,
        /// Additional context about the error
        details: String,
    },
    /// Error when receiving a response from an actor
    Receive {
        /// ID of the actor that failed to send a response
        identity: Identity,
        /// Additional context about the error
        details: String,
    },
    /// Error when a request times out
    Timeout {
        /// ID of the actor that timed out
        identity: Identity,
        /// The duration after which the request timed out
        timeout: Duration,
        /// Type of operation that timed out (e.g., "send", "ask")
        operation: String,
    },
    /// Error when downcasting a reply to the expected type
    Downcast {
        /// ID of the actor that sent the incompatible reply
        identity: Identity,
        /// The expected type name that the downcast failed to match
        expected_type: String,
    },
    /// Error when a runtime operation fails
    Runtime {
        /// ID of the actor where the runtime error occurred
        identity: Identity,
        /// Additional context about the error
        details: String,
    },
    /// Error related to mailbox capacity configuration
    MailboxCapacity {
        /// Detailed error message describing the mailbox capacity issue
        message: String,
    },
    /// Error when awaiting a JoinHandle fails
    Join {
        /// ID of the actor that spawned the task
        identity: Identity,
        /// The original JoinError from tokio
        source: tokio::task::JoinError,
    },
}

/// Implementation of the Display trait for Error enum.
///
/// Provides human-readable error messages for each error variant.
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Send {
                identity: actor_id,
                details,
            } => {
                write!(
                    f,
                    "Failed to send message to actor {}: {}",
                    actor_id.name(),
                    details
                )
            }
            Error::Receive {
                identity: actor_id,
                details,
            } => {
                write!(
                    f,
                    "Failed to receive reply from actor {}: {}",
                    actor_id.name(),
                    details
                )
            }
            Error::Timeout {
                identity: actor_id,
                timeout,
                operation,
            } => {
                write!(
                    f,
                    "{} operation to actor {} timed out after {:?}",
                    operation,
                    actor_id.name(),
                    timeout
                )
            }
            Error::Downcast {
                identity: actor_id,
                expected_type,
            } => {
                write!(
                    f,
                    "Failed to downcast reply from actor {} to expected type '{}'",
                    actor_id.name(),
                    expected_type
                )
            }
            Error::Runtime {
                identity: actor_id,
                details,
            } => {
                write!(f, "Runtime error in actor {}: {}", actor_id.name(), details)
            }
            Error::MailboxCapacity { message } => {
                write!(f, "Mailbox capacity error: {message}")
            }
            Error::Join { identity, source } => {
                write!(
                    f,
                    "Failed to join spawned task from actor {}: {}",
                    identity.name(),
                    source
                )
            }
        }
    }
}

/// Implementation of the standard Error trait for rsactor Error enum.
///
/// This allows Error to be used with standard error handling mechanisms.
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Join { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A Result type specialized for rsactor operations.
///
/// This type is returned by most actor operations like [`tell`](crate::actor_ref::ActorRef::tell),
/// [`ask`](crate::actor_ref::ActorRef::ask), [`stop`](crate::actor_ref::ActorRef::stop), etc.
///
/// # Examples
///
/// ```rust
/// use rsactor::Result;
///
/// fn actor_operation() -> Result<String> {
///     // ... actor operation logic
///     Ok("success".to_string())
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;
