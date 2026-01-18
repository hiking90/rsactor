// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Dead Letter Tracking Module
//!
//! This module provides infrastructure for tracking and recording dead letters—
//! messages that could not be delivered to their intended recipients.
//!
//! # Background
//!
//! Dead letters occur when a message cannot be delivered to an actor, such as:
//! - The actor's mailbox channel has closed (actor stopped)
//! - A send or ask operation times out
//! - The reply channel was dropped before responding
//!
//! # Observability
//!
//! Dead letters are always logged with structured fields via `tracing`:
//!
//! ```text
//! WARN dead_letter: Dead letter: message could not be delivered
//!   actor.id=42
//!   actor.type_name="MyActor"
//!   message.type_name="PingMessage"
//!   dead_letter.reason="actor stopped"
//!   dead_letter.operation="tell"
//! ```
//!
//! # Performance Characteristics
//!
//! Dead letter recording is designed for minimal overhead:
//!
//! | Scenario | Overhead |
//! |----------|----------|
//! | Successful message delivery (hot path) | **Zero** - no code executes |
//! | Dead letter, no tracing subscriber | ~5-50 ns (fast check + early return) |
//! | Dead letter, subscriber active | ~1-10 μs (logging + serialization) |
//!
//! Key optimizations:
//! - `#[cold]` attribute hints compiler to optimize hot path
//! - `Ordering::Relaxed` for atomic counter (no memory barriers)
//! - Static string references for operation names (no allocation)
//! - `std::any::type_name::<M>()` is compile-time computed (zero runtime cost)
//!
//! # Testing Support
//!
//! When the `test-utils` feature is enabled (or in unit tests), a counter tracks
//! the number of dead letters for verification purposes. Use `dead_letter_count()`
//! and `reset_dead_letter_count()` to inspect and reset this counter.
//!
//! ```toml
//! [dev-dependencies]
//! rsactor = { version = "...", features = ["test-utils"] }
//! ```
//!
//! # Example: Observing Dead Letters
//!
//! ```rust,ignore
//! use rsactor::{spawn, Actor, ActorRef};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Initialize tracing to see dead letter logs
//!     tracing_subscriber::fmt()
//!         .with_env_filter("rsactor=debug")
//!         .init();
//!
//!     // Dead letters are automatically logged when message delivery fails
//!     let (actor_ref, handle) = spawn::<MyActor>(MyActor);
//!     actor_ref.stop().await.unwrap();
//!     handle.await.unwrap();
//!
//!     // This will log a dead letter warning
//!     let _ = actor_ref.tell(MyMessage).await;
//! }
//! ```
//!
//! # Security Warning
//!
//! **Never enable `test-utils` in production builds!** This feature exposes
//! internal metrics that could be used to:
//! - Monitor message delivery failure rates
//! - Reset monitoring metrics to evade detection
//!
//! For production observability, rely on the structured `tracing::warn!` logs instead.

use crate::Identity;

#[cfg(any(test, feature = "test-utils"))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(any(test, feature = "test-utils"))]
static DEAD_LETTER_COUNT: AtomicU64 = AtomicU64::new(0);

/// Reason why a message became a dead letter.
///
/// This enum is marked `#[non_exhaustive]` to allow adding new variants
/// in future versions without breaking existing code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeadLetterReason {
    /// Actor's mailbox channel was closed.
    ///
    /// This occurs when attempting to send a message to an actor that is no longer
    /// running. The actor may have stopped normally via [`ActorRef::stop`](crate::ActorRef::stop) or
    /// terminated abnormally.
    ActorStopped,

    /// A send or ask operation exceeded its timeout.
    ///
    /// When using [`ActorRef::tell_with_timeout`](crate::ActorRef::tell_with_timeout) or
    /// [`ActorRef::ask_with_timeout`](crate::ActorRef::ask_with_timeout),
    /// if the message cannot be delivered within the specified duration,
    /// it becomes a dead letter.
    Timeout,

    /// The reply channel was dropped before a response could be sent.
    ///
    /// When using [`ActorRef::ask`](crate::ActorRef::ask), the handler may fail or the message processing
    /// may be interrupted before sending a reply.
    ReplyDropped,
}

impl std::fmt::Display for DeadLetterReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeadLetterReason::ActorStopped => write!(f, "actor stopped"),
            DeadLetterReason::Timeout => write!(f, "timeout"),
            DeadLetterReason::ReplyDropped => write!(f, "reply dropped"),
        }
    }
}

/// Records a dead letter event with structured logging.
///
/// This function is called automatically by `ActorRef` methods when a message
/// cannot be delivered. It serves two purposes:
///
/// 1. **Observability**: Logs a warning-level event with structured fields
///    for debugging and monitoring (always available via `tracing`).
///
/// 2. **Testing**: When `test-utils` feature is enabled, increments an atomic counter
///    that can be queried via [`dead_letter_count()`] to verify dead letter behavior.
///
/// # Arguments
///
/// * `identity` - The identity of the actor that failed to receive the message
/// * `reason` - Why the message became a dead letter
/// * `operation` - The operation that failed ("tell", "ask", "blocking_tell", etc.)
///
/// # Type Parameters
///
/// * `M` - The message type (used for logging the type name)
///
/// # Why `#[cold]`?
///
/// Dead letters are exceptional paths - they occur when something goes wrong.
/// The `#[cold]` attribute hints to the compiler that this function is rarely
/// called, allowing better optimization of the hot path (successful message delivery).
#[cold]
pub(crate) fn record<M: 'static>(
    identity: Identity,
    reason: DeadLetterReason,
    operation: &'static str,
) {
    #[cfg(any(test, feature = "test-utils"))]
    DEAD_LETTER_COUNT.fetch_add(1, Ordering::Relaxed);

    // Always available - tracing is now a required dependency
    tracing::warn!(
        actor.id = identity.id,
        actor.type_name = identity.name(),
        message.type_name = std::any::type_name::<M>(),
        dead_letter.reason = %reason,
        dead_letter.operation = operation,
        "Dead letter: message could not be delivered"
    );
}

/// Returns the total number of dead letters recorded.
///
/// This function is only available when the `test-utils` feature is enabled.
#[cfg(any(test, feature = "test-utils"))]
pub fn dead_letter_count() -> u64 {
    DEAD_LETTER_COUNT.load(Ordering::Relaxed)
}

/// Resets the dead letter counter.
///
/// This function is only available when the `test-utils` feature is enabled.
#[cfg(any(test, feature = "test-utils"))]
pub fn reset_dead_letter_count() {
    DEAD_LETTER_COUNT.store(0, Ordering::Relaxed);
}
