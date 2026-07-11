// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::Identity;
use std::time::Duration;

/// Identifies the operation family that produced an [`Error::Timeout`].
///
/// Each async and blocking send/ask method reports its own name here, so a
/// caller inspecting a timeout can tell which API it invoked regardless of
/// which internal execution path served it. Marked `#[non_exhaustive]` so new
/// operations can be added without breaking exhaustive matches — include a `_`
/// arm when matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operation {
    /// [`tell`](crate::ActorRef::tell) / [`tell_with_timeout`](crate::ActorRef::tell_with_timeout).
    Tell,
    /// [`ask`](crate::ActorRef::ask) / [`ask_with_timeout`](crate::ActorRef::ask_with_timeout).
    Ask,
    /// [`tell_priority`](crate::ActorRef::tell_priority).
    TellPriority,
    /// [`ask_priority`](crate::ActorRef::ask_priority).
    AskPriority,
    /// [`blocking_tell`](crate::ActorRef::blocking_tell).
    BlockingTell,
    /// [`blocking_tell_priority`](crate::ActorRef::blocking_tell_priority).
    BlockingTellPriority,
    /// [`blocking_ask`](crate::ActorRef::blocking_ask).
    BlockingAsk,
    /// [`blocking_ask_priority`](crate::ActorRef::blocking_ask_priority).
    BlockingAskPriority,
}

impl Operation {
    /// Returns the stable string label for this operation (e.g. `"tell"`).
    ///
    /// The value matches the public method name and is what the dead-letter
    /// and deadlock-detection subsystems log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Operation::Tell => "tell",
            Operation::Ask => "ask",
            Operation::TellPriority => "tell_priority",
            Operation::AskPriority => "ask_priority",
            Operation::BlockingTell => "blocking_tell",
            Operation::BlockingTellPriority => "blocking_tell_priority",
            Operation::BlockingAsk => "blocking_ask",
            Operation::BlockingAskPriority => "blocking_ask_priority",
        }
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Identifies which bounded channel produced an [`Error::ChannelFull`].
///
/// Marked `#[non_exhaustive]` so new bounded channels can report a full-buffer
/// error without breaking exhaustive matches — include a `_` arm when matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Channel {
    /// The per-actor idle-subscribe buffer used by
    /// [`ActorRef::subscribe_idle`](crate::ActorRef::subscribe_idle).
    IdleSubscribe,
}

impl Channel {
    /// Returns the stable string label for this channel (e.g. `"idle_subscribe"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Channel::IdleSubscribe => "idle_subscribe",
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
/// Represents errors that can occur in the rsactor framework.
///
/// These errors may be encountered during various actor operations, such as sending messages
/// with [`tell`](crate::actor_ref::ActorRef::tell) or [`ask`](crate::actor_ref::ActorRef::ask),
/// or during actor lifecycle operations like [`spawn`](crate::spawn).
///
/// This enum is marked `#[non_exhaustive]` so new variants can be added in future versions
/// without breaking existing exhaustive matches — callers should include a `_` arm when
/// matching on `Error`.
pub enum Error {
    /// Error when sending a message to an actor's channel that has been closed
    /// (the actor is no longer alive).
    ///
    /// For "channel full" failures see [`Error::ChannelFull`].
    #[non_exhaustive]
    Send {
        /// ID of the actor that failed to receive the message
        identity: Identity,
        /// Additional context about the error.
        ///
        /// Every emitter uses a fixed message, so this is `&'static str`
        /// rather than `String` (no allocation on this failure path).
        details: &'static str,
    },
    /// Error when a bounded channel is currently at capacity.
    ///
    /// Unlike [`Error::Send`] (which means the actor is dead), this is a *transient*
    /// failure — the actor is alive and the channel will drain as the runtime makes
    /// progress. [`Error::is_retryable`] returns `true` for this variant.
    ///
    /// **Retry only from outside the target actor.** The saturated buffer is
    /// drained by the actor's own runtime loop, which runs only *between* that
    /// actor's handler invocations (and not at all until `on_start` returns).
    /// A retry loop inside the actor's own `on_start` or message handler
    /// therefore spins forever on a buffer that can never drain — and because
    /// the loop never returns, the actor cannot even be `kill()`ed. From
    /// inside the actor, treat this error as terminal for the current attempt:
    /// spawn with a larger
    /// [`SpawnOptions::with_idle_capacity`](crate::SpawnOptions::with_idle_capacity),
    /// merge sources into one stream, or defer the work to a later handler
    /// invocation.
    ///
    /// Currently emitted only by
    /// [`ActorRef::subscribe_idle`](crate::ActorRef::subscribe_idle) when the bounded
    /// subscribe buffer (default capacity
    /// [`IDLE_SUBSCRIBE_CHANNEL_CAPACITY`](crate::IDLE_SUBSCRIBE_CHANNEL_CAPACITY),
    /// configurable per actor via
    /// [`SpawnOptions::with_idle_capacity`](crate::SpawnOptions::with_idle_capacity))
    /// is saturated; the rejected stream is handed back via
    /// [`IdleSubscribeError`](crate::IdleSubscribeError) so a retry can reuse it.
    #[non_exhaustive]
    ChannelFull {
        /// ID of the actor whose channel was full
        identity: Identity,
        /// Which bounded channel was full.
        channel: Channel,
    },
    /// Error when receiving a response from an actor
    #[non_exhaustive]
    Receive {
        /// ID of the actor that failed to send a response
        identity: Identity,
        /// Additional context about the error.
        ///
        /// Every emitter uses a fixed message, so this is `&'static str`
        /// rather than `String` (no allocation on this failure path).
        details: &'static str,
    },
    /// Error when a request times out
    #[non_exhaustive]
    Timeout {
        /// ID of the actor that timed out
        identity: Identity,
        /// The duration after which the request timed out
        timeout: Duration,
        /// The operation family whose deadline was missed. See [`Operation`]:
        /// [`tell_with_timeout`](crate::ActorRef::tell_with_timeout) reports
        /// [`Operation::Tell`] and [`ask_with_timeout`](crate::ActorRef::ask_with_timeout)
        /// reports [`Operation::Ask`] (the async `tell`/`ask` themselves carry
        /// no deadline); the `*_priority` and `blocking_*` methods report their
        /// own operation, regardless of which execution path served them.
        operation: Operation,
    },
    /// Error when downcasting a reply to the expected type
    #[non_exhaustive]
    Downcast {
        /// ID of the actor that sent the incompatible reply
        identity: Identity,
        /// The expected type name that the downcast failed to match
        expected_type: &'static str,
    },
    /// Error from a runtime-level operation.
    ///
    /// Emitted when a `blocking_*` call has to run on a dedicated thread with a
    /// temporary Tokio runtime (because the caller is not already inside a
    /// multi-thread runtime) and that runtime fails to build, or its worker
    /// thread panics. Actor *lifecycle* failures (`on_start` / `on_idle` /
    /// `on_stop`) are **not** reported here — they surface through
    /// [`ActorResult::Failed`](crate::ActorResult) carrying the actor's own
    /// error type.
    #[non_exhaustive]
    Runtime {
        /// ID of the actor where the runtime error occurred
        identity: Identity,
        /// Additional context about the error
        details: String,
        /// Underlying cause, when one exists (e.g. the [`std::io::Error`] from
        /// failing to build the runtime). Returned from
        /// [`Error::source`](std::error::Error::source). `None` when the failure
        /// has no chainable source (e.g. a worker-thread panic).
        source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
    },
    /// Error related to mailbox capacity configuration
    #[non_exhaustive]
    MailboxCapacity {
        /// Static label describing the mailbox capacity issue.
        ///
        /// Every emitter uses a fixed message, so this is `&'static str` rather
        /// than `String` (no allocation on this cold configuration path).
        message: &'static str,
    },
    /// Error when awaiting a JoinHandle fails
    #[non_exhaustive]
    Join {
        /// ID of the actor that spawned the task
        identity: Identity,
        /// The original JoinError from tokio.
        ///
        /// Wrapped in an [`Arc`](std::sync::Arc) because `JoinError` is not
        /// `Clone`; this lets [`Error`] as a whole derive `Clone`. Deref still
        /// gives access to `is_panic()` / `is_cancelled()`.
        source: std::sync::Arc<tokio::task::JoinError>,
    },
    /// Error when a priority channel operation is attempted on an actor that
    /// did not enable the priority channel via [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    ///
    /// This is a configuration error (not a delivery failure) and is therefore
    /// not recorded as a dead letter. Callers can guard against this with
    /// [`ActorRef::has_priority_channel`](crate::ActorRef::has_priority_channel).
    #[non_exhaustive]
    PriorityChannelNotEnabled {
        /// ID of the actor that does not have a priority channel.
        identity: Identity,
    },
    /// Error when an idle-channel operation (e.g. [`subscribe_idle`](crate::ActorRef::subscribe_idle))
    /// is attempted on an actor that did not enable the idle channel via
    /// [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle).
    ///
    /// This is a configuration error (not a delivery failure) and is therefore
    /// not recorded as a dead letter. Callers can guard against this with
    /// [`ActorRef::has_idle_channel`](crate::ActorRef::has_idle_channel).
    #[non_exhaustive]
    IdleChannelNotEnabled {
        /// ID of the actor that does not have an idle channel.
        identity: Identity,
    },
}

/// Implementation of the Display trait for Error enum.
///
/// Provides human-readable error messages for each error variant.
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Send { identity, details } => {
                write!(f, "Failed to send message to actor {identity}: {details}")
            }
            Error::ChannelFull { identity, channel } => {
                write!(
                    f,
                    "Bounded channel '{channel}' for actor {identity} is at capacity"
                )
            }
            Error::Receive { identity, details } => {
                write!(
                    f,
                    "Failed to receive reply from actor {identity}: {details}"
                )
            }
            Error::Timeout {
                identity,
                timeout,
                operation,
            } => {
                write!(
                    f,
                    "{operation} operation to actor {identity} timed out after {timeout:?}"
                )
            }
            Error::Downcast {
                identity,
                expected_type,
            } => {
                write!(
                    f,
                    "Failed to downcast reply from actor {identity} to expected type '{expected_type}'"
                )
            }
            Error::Runtime {
                identity, details, ..
            } => {
                write!(f, "Runtime error in actor {identity}: {details}")
            }
            Error::MailboxCapacity { message } => {
                write!(f, "Mailbox capacity error: {message}")
            }
            Error::Join { identity, source } => {
                write!(
                    f,
                    "Failed to join spawned task from actor {identity}: {source}"
                )
            }
            Error::PriorityChannelNotEnabled { identity } => {
                write!(
                    f,
                    "Priority channel is not enabled for actor {identity}: enable it via SpawnOptions::with_priority()"
                )
            }
            Error::IdleChannelNotEnabled { identity } => {
                write!(
                    f,
                    "Idle channel is not enabled for actor {identity}: enable it via SpawnOptions::with_idle()"
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
            Error::Join { source, .. } => Some(source.as_ref()),
            Error::Runtime {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
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
    /// | `ChannelFull` | ✓ Yes | Transient; bounded buffer drains as actor makes progress |
    /// | `Send` | ✗ No | Actor stopped; channel permanently closed |
    /// | `Receive` | ✗ No | Reply channel dropped; cannot recover |
    /// | `Downcast` | ✗ No | Type mismatch; programming error |
    /// | `Runtime` | ✗ No | Failed to spawn/build the temporary blocking runtime |
    /// | `MailboxCapacity` | ✗ No | Configuration error |
    /// | `Join` | ✗ No | Task panic or cancellation |
    /// | `PriorityChannelNotEnabled` | ✗ No | Configuration error |
    /// | `IdleChannelNotEnabled` | ✗ No | Configuration error |
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
        matches!(self, Error::Timeout { .. } | Error::ChannelFull { .. })
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
            Error::ChannelFull { .. } => &[
                "Transient failure - the actor is alive and its runtime loop drains the buffer",
                "Retry only from OUTSIDE the target actor: the buffer drains between handler \
                 invocations, so a retry loop inside the actor's own on_start/handler spins \
                 forever and cannot be kill()ed",
                "For subscribe_idle specifically, spawn with SpawnOptions::with_idle_capacity(n) \
                 if you regularly fan out more than 32 streams, merge sources into one stream \
                 (futures::stream::select_all), or batch subscriptions across handler invocations",
                "The rejected stream is returned in IdleSubscribeError - reuse it for the retry",
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
                "Emitted when a blocking_* call fails to build or run its dedicated temporary runtime",
                "Match on `Error::Runtime { source, .. }` and inspect `source` for the underlying cause (e.g. std::io::Error)",
                "This usually indicates OS resource exhaustion (unable to spawn a thread or build a runtime)",
                "Prefer calling blocking_* from within an existing multi-thread runtime, or use the async ask()/tell() APIs",
            ],
            Error::MailboxCapacity { .. } => &[
                "Mailbox capacity must be greater than 0",
                "set_default_mailbox_capacity() can only be called once",
                "Call it early in main() before spawning actors",
            ],
            Error::Join { .. } => &[
                "The spawned task panicked or was cancelled by the runtime",
                "Run with RUST_BACKTRACE=1 or RUST_BACKTRACE=full for panic details",
                "Match on `Error::Join { source, .. }` and use `source.is_panic()` / `source.is_cancelled()` to distinguish the cause",
                "Check for unwrap(), expect(), or panic!() calls in actor code",
                "Verify tokio runtime wasn't shut down while actor was running",
            ],
            Error::PriorityChannelNotEnabled { .. } => &[
                "Spawn the actor with SpawnOptions::new().with_priority() via spawn_with_options()",
                "Use ActorRef::has_priority_channel() to check before sending priority messages",
                "If priority is not required, use the regular tell()/ask() methods instead",
            ],
            Error::IdleChannelNotEnabled { .. } => &[
                "Spawn the actor with SpawnOptions::new().with_idle() via spawn_with_options()",
                "Use ActorRef::has_idle_channel() to check before calling subscribe_idle()",
                "on_idle is only driven when the idle channel is enabled",
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

#[cfg(test)]
mod tests {
    // These exercise `Error`'s own `Display` / `debugging_tips` / `source` /
    // `is_retryable` by constructing variants directly. They live here as unit
    // tests (not in `tests/`) because the variants are `#[non_exhaustive]` and
    // therefore cannot be constructed from an external integration-test crate.
    use super::*;
    use crate::Identity;
    use std::error::Error as StdError;
    use std::time::Duration;

    #[test]
    fn operation_and_channel_labels_round_trip() {
        // `as_str` and `Display` must agree, and the labels are the stable
        // strings the dead-letter and deadlock subsystems log.
        for (op, label) in [
            (Operation::Tell, "tell"),
            (Operation::Ask, "ask"),
            (Operation::TellPriority, "tell_priority"),
            (Operation::AskPriority, "ask_priority"),
            (Operation::BlockingTell, "blocking_tell"),
            (Operation::BlockingTellPriority, "blocking_tell_priority"),
            (Operation::BlockingAsk, "blocking_ask"),
            (Operation::BlockingAskPriority, "blocking_ask_priority"),
        ] {
            assert_eq!(op.as_str(), label);
            assert_eq!(op.to_string(), label);
        }
        assert_eq!(Channel::IdleSubscribe.as_str(), "idle_subscribe");
        assert_eq!(Channel::IdleSubscribe.to_string(), "idle_subscribe");
    }

    #[test]
    fn is_retryable_for_all_variants() {
        let identity = Identity::new(1, "TestActor");

        // Timeout and ChannelFull are the transient (retryable) variants.
        let timeout_err = Error::Timeout {
            identity,
            timeout: Duration::from_secs(1),
            operation: Operation::Ask,
        };
        assert!(timeout_err.is_retryable());

        let channel_full = Error::ChannelFull {
            identity,
            channel: Channel::IdleSubscribe,
        };
        assert!(channel_full.is_retryable());

        // All others are NOT retryable.
        let send_err = Error::Send {
            identity,
            details: "channel closed",
        };
        assert!(!send_err.is_retryable());

        let receive_err = Error::Receive {
            identity,
            details: "channel closed",
        };
        assert!(!receive_err.is_retryable());

        let downcast_err = Error::Downcast {
            identity,
            expected_type: "String",
        };
        assert!(!downcast_err.is_retryable());

        let runtime_err = Error::Runtime {
            identity,
            details: "test error".into(),
            source: None,
        };
        assert!(!runtime_err.is_retryable());

        let mailbox_err = Error::MailboxCapacity {
            message: "invalid capacity",
        };
        assert!(!mailbox_err.is_retryable());
    }

    #[test]
    fn all_errors_have_debugging_tips() {
        let identity = Identity::new(1, "TestActor");

        let errors: Vec<Error> = vec![
            Error::Send {
                identity,
                details: "test",
            },
            Error::Receive {
                identity,
                details: "test",
            },
            Error::Timeout {
                identity,
                timeout: Duration::from_secs(1),
                operation: Operation::Ask,
            },
            Error::Downcast {
                identity,
                expected_type: "String",
            },
            Error::Runtime {
                identity,
                details: "test".into(),
                source: None,
            },
            Error::MailboxCapacity { message: "test" },
        ];

        for err in &errors {
            let tips = err.debugging_tips();
            assert!(!tips.is_empty(), "Missing tips for: {:?}", err);
            for tip in tips {
                assert!(tip.len() > 10, "Tip too short to be useful: {}", tip);
            }
        }
    }

    #[test]
    fn runtime_error_tips_are_specific() {
        let identity = Identity::new(1, "TestActor");
        let err = Error::Runtime {
            identity,
            details: "test".into(),
            source: None,
        };
        let tips = err.debugging_tips();

        let tips_text = tips.join(" ");
        assert!(
            tips_text.contains("blocking"),
            "Runtime tips should mention the blocking_* runtime source"
        );
        assert!(
            tips_text.contains("source"),
            "Runtime tips should point at the chainable source"
        );
    }

    #[test]
    fn downcast_error_debugging_tips() {
        let identity = Identity::new(1, "TestActor");
        let err = Error::Downcast {
            identity,
            expected_type: "String",
        };

        let tips = err.debugging_tips();
        assert!(!tips.is_empty(), "Downcast error should have tips");

        let tips_text = tips.join(" ");
        assert!(
            tips_text.contains("Message") || tips_text.contains("handler"),
            "Downcast tips should mention Message trait or handler"
        );
    }

    #[test]
    fn mailbox_capacity_error_tips() {
        let err = Error::MailboxCapacity {
            message: "capacity must be greater than 0",
        };

        let tips = err.debugging_tips();
        assert!(
            !tips.is_empty(),
            "MailboxCapacity should have debugging tips"
        );

        let tips_text = tips.join(" ");
        assert!(
            tips_text.contains("greater than 0") || tips_text.contains("capacity"),
            "Tips should mention capacity requirements"
        );
        assert!(
            tips_text.contains("set_default_mailbox_capacity") || tips_text.contains("once"),
            "Tips should mention set_default_mailbox_capacity behavior"
        );
    }

    #[test]
    fn error_display_all_variants() {
        let identity = Identity::new(1, "TestActor");

        let errors = vec![
            Error::Send {
                identity,
                details: "channel closed",
            },
            Error::Receive {
                identity,
                details: "reply dropped",
            },
            Error::Timeout {
                identity,
                timeout: Duration::from_secs(5),
                operation: Operation::Ask,
            },
            Error::Downcast {
                identity,
                expected_type: "String",
            },
            Error::Runtime {
                identity,
                details: "panic in handler".into(),
                source: None,
            },
            Error::MailboxCapacity {
                message: "capacity must be > 0",
            },
            Error::ChannelFull {
                identity,
                channel: Channel::IdleSubscribe,
            },
        ];

        for err in &errors {
            let display = format!("{}", err);
            assert!(!display.is_empty(), "Display should not be empty");
            assert!(display.len() > 5, "Display should be descriptive");
        }
    }

    #[test]
    fn runtime_error_display_format() {
        let identity = Identity::new(1, "TestActor");
        let error = Error::Runtime {
            identity,
            details: "Test runtime error details".to_string(),
            source: None,
        };

        let display_str = format!("{error}");
        assert!(
            display_str.contains("Runtime error in actor"),
            "Display should mention runtime error"
        );
        assert!(
            display_str.contains("TestActor"),
            "Display should contain actor name"
        );
        assert!(
            display_str.contains("Test runtime error details"),
            "Display should contain error details"
        );
    }

    #[test]
    fn error_source_returns_none_for_non_join() {
        let identity = Identity::new(1, "TestActor");

        let errors: Vec<Error> = vec![
            Error::Send {
                identity,
                details: "test",
            },
            Error::Receive {
                identity,
                details: "test",
            },
            Error::Timeout {
                identity,
                timeout: Duration::from_secs(1),
                operation: Operation::Ask,
            },
            Error::Downcast {
                identity,
                expected_type: "String",
            },
            Error::Runtime {
                identity,
                details: "test".into(),
                source: None,
            },
            Error::MailboxCapacity { message: "test" },
        ];

        for err in &errors {
            assert!(
                err.source().is_none(),
                "Non-Join error {:?} should have no source",
                err
            );
        }
    }

    #[test]
    fn runtime_error_with_source_exposes_source() {
        let identity = Identity::new(1, "TestActor");
        let io_err = std::io::Error::other("boom");
        let err = Error::Runtime {
            identity,
            details: "Failed to build blocking runtime".into(),
            source: Some(std::sync::Arc::new(io_err)),
        };

        let source = err
            .source()
            .expect("Runtime error with a cause should expose source()");
        assert!(
            source.to_string().contains("boom"),
            "source() should surface the underlying io::Error message"
        );
    }

    #[tokio::test]
    async fn error_join_display() {
        let handle = tokio::spawn(async {
            panic!("test panic for JoinError");
        });
        let join_error = handle.await.unwrap_err();
        let identity = Identity::new(1, "TestActor");

        let error = Error::Join {
            identity,
            source: std::sync::Arc::new(join_error),
        };

        let display = format!("{}", error);
        assert!(display.contains("Failed to join"));
        assert!(display.contains("TestActor"));
        assert!(error.source().is_some(), "Join error should have a source");
    }

    #[tokio::test]
    async fn error_join_debugging_tips() {
        let handle = tokio::spawn(async {
            panic!("test panic for debugging tips");
        });
        let join_error = handle.await.unwrap_err();
        let identity = Identity::new(1, "TestActor");

        let error = Error::Join {
            identity,
            source: std::sync::Arc::new(join_error),
        };

        let tips = error.debugging_tips();
        assert!(!tips.is_empty(), "Join error should have debugging tips");

        let tips_text = tips.join(" ");
        assert!(
            tips_text.contains("panic") || tips_text.contains("cancelled"),
            "Join tips should mention panic or cancellation"
        );
        assert!(
            tips_text.contains("RUST_BACKTRACE"),
            "Join tips should mention RUST_BACKTRACE"
        );
    }

    #[tokio::test]
    async fn error_join_is_not_retryable() {
        let handle = tokio::spawn(async {
            panic!("test panic for retryable check");
        });
        let join_error = handle.await.unwrap_err();
        let identity = Identity::new(1, "TestActor");

        let error = Error::Join {
            identity,
            source: std::sync::Arc::new(join_error),
        };

        assert!(!error.is_retryable(), "Join error should not be retryable");
    }
}
