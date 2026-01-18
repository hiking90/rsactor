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

impl Error {
    /// Returns whether this error might succeed if retried.
    ///
    /// # ⚠️ Important Caveat
    ///
    /// This method checks the error type only and does **not** account for elapsed time.
    /// If you store an error instance and check `is_retryable()` later, it will still
    /// return `true` for `Timeout` errors even if significant time has passed.
    ///
    /// **Best Practice:** Always use fresh error instances for retry decisions.
    /// Do not cache error instances for later retry logic.
    ///
    /// # Retryable Errors
    ///
    /// | Error Type | Retryable | Reason |
    /// |------------|-----------|--------|
    /// | `Timeout` | ✓ Yes | Transient; may succeed with longer timeout |
    /// | `Send` | ✗ No | Actor stopped; channel permanently closed |
    /// | `Receive` | ✗ No | Reply channel dropped; cannot recover |
    /// | `Downcast` | ✗ No | Type mismatch; programming error |
    /// | `Runtime` | ✗ No | Actor lifecycle failure |
    /// | `MailboxCapacity` | ✗ No | Configuration error |
    /// | `Join` | ✗ No | Task panic or cancellation |
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use rsactor::{ActorRef, Actor, Error, Message};
    /// use std::time::Duration;
    ///
    /// async fn send_with_retry<T, M>(
    ///     actor: &ActorRef<T>,
    ///     msg: M,
    ///     max_attempts: usize,
    /// ) -> Result<(), Error>
    /// where
    ///     T: Actor + Message<M>,
    ///     M: Clone + Send + 'static,
    /// {
    ///     let mut attempts = 0;
    ///     loop {
    ///         // Always get a fresh error from the current attempt
    ///         match actor.tell(msg.clone()).await {
    ///             Ok(()) => return Ok(()),
    ///             Err(e) if e.is_retryable() && attempts < max_attempts => {
    ///                 attempts += 1;
    ///                 tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
    ///             }
    ///             Err(e) => return Err(e),
    ///         }
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Error::Timeout { .. })
    }

    /// Returns actionable debugging tips for this error.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use rsactor::Error;
    ///
    /// fn log_error(err: &Error) {
    ///     eprintln!("Error: {}", err);
    ///     for tip in err.debugging_tips() {
    ///         eprintln!("  - {}", tip);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn debugging_tips(&self) -> &'static [&'static str] {
        match self {
            Error::Send { .. } => &[
                "Verify the actor is still running with `actor_ref.is_alive()`",
                "The actor's mailbox is closed - the actor has terminated",
                "Consider using `ActorWeak` for long-lived references",
            ],
            Error::Receive { .. } => &[
                "The actor dropped the reply channel before responding",
                "Check if the message handler panicked or returned early",
                "Verify the handler correctly awaits async operations",
            ],
            Error::Timeout { .. } => &[
                "Consider increasing the timeout duration",
                "Check if the actor is processing a slow operation",
                "Verify there's no deadlock in the message handler",
                "Use `tell` instead if you don't need a response",
            ],
            Error::Downcast { .. } => &[
                "The handler returned a different type than expected",
                "Verify the Message trait impl returns correct Reply type",
                "This usually indicates a bug in handler implementation",
            ],
            Error::Runtime { .. } => &[
                "Check if on_start() or on_run() returned an error",
                "Look for panic messages in the error details field",
                "Use `ActorResult::is_start_failed()` or `is_run_failed()` to identify failure phase",
                "Call `ActorResult::error()` to get the underlying error details",
                "Initialize tracing-subscriber and set RUST_LOG=debug for lifecycle diagnostics",
            ],
            Error::MailboxCapacity { .. } => &[
                "Mailbox capacity must be greater than 0",
                "set_default_mailbox_capacity() can only be called once",
                "Call it early in main() before spawning actors",
            ],
            Error::Join { .. } => &[
                "The spawned task panicked or was cancelled by the runtime",
                "Run with RUST_BACKTRACE=1 or RUST_BACKTRACE=full for panic details",
                "Use `ActorResult::is_join_failed()` to confirm this failure type",
                "Check for unwrap(), expect(), or panic!() calls in actor code",
                "Verify tokio runtime wasn't shut down while actor was running",
            ],
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
