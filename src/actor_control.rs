// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor lifecycle control traits for type-erased actor management.
//!
//! This module provides traits for controlling actor lifecycle without requiring
//! knowledge of the actor's message types. This enables storing different actor types
//! in a single collection for unified lifecycle management.
//!
//! # Overview
//!
//! - [`ActorControl`]: Strong reference lifecycle control (keeps actors alive)
//! - [`WeakActorControl`]: Weak reference lifecycle control (does not keep actors alive)
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use rsactor::ActorControl;
//!
//! // Store different actor types in a single collection
//! let controls: Vec<Box<dyn ActorControl>> = vec![
//!     (&worker_actor).into(),   // ActorRef<WorkerActor>
//!     (&logger_actor).into(),   // ActorRef<LoggerActor>
//!     (&cache_actor).into(),    // ActorRef<CacheActor>
//! ];
//!
//! // Check status of all actors
//! for control in &controls {
//!     println!("Actor {} alive: {}", control.identity(), control.is_alive());
//! }
//!
//! // Stop all actors gracefully
//! for control in &controls {
//!     control.stop().await?;
//! }
//! ```

use crate::{Actor, ActorRef, ActorWeak, BoxFuture, Identity, Result};
use futures::FutureExt;
use std::fmt;

// ============================================================================
// Strong ActorControl Trait
// ============================================================================

/// Type-erased trait for actor lifecycle control with strong references.
///
/// This trait allows managing different actor types through a unified interface
/// without knowing their message types. The handlers maintain strong references
/// to actors, keeping them alive.
///
/// # Example
///
/// ```rust,ignore
/// let controls: Vec<Box<dyn ActorControl>> = vec![
///     (&actor_a).into(),
///     (&actor_b).into(),
/// ];
///
/// // Stop all actors
/// for control in &controls {
///     control.stop().await?;
/// }
/// ```
pub trait ActorControl: Send + Sync {
    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor is still alive.
    fn is_alive(&self) -> bool;

    /// Gracefully stops the actor.
    ///
    /// The actor will process all remaining messages in its mailbox before stopping.
    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    /// Immediately terminates the actor.
    ///
    /// The actor will stop without processing remaining messages.
    fn kill(&self) -> Result<()>;

    /// Downgrades to a weak control reference.
    fn downgrade(&self) -> Box<dyn WeakActorControl>;

    /// Clone this control into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn ActorControl>;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

// ============================================================================
// Weak ActorControl Trait
// ============================================================================

/// Type-erased trait for actor lifecycle control with weak references.
///
/// Unlike [`ActorControl`], this does not keep the actor alive.
/// Must call [`upgrade()`](WeakActorControl::upgrade) to obtain a strong control before
/// performing lifecycle operations.
///
/// # Example
///
/// ```rust,ignore
/// let weak_controls: Vec<Box<dyn WeakActorControl>> = vec![
///     ActorRef::downgrade(&actor_a).into(),
///     ActorRef::downgrade(&actor_b).into(),
/// ];
///
/// for control in &weak_controls {
///     if let Some(strong) = control.upgrade() {
///         strong.stop().await?;
///     }
/// }
/// ```
pub trait WeakActorControl: Send + Sync {
    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor might still be alive (heuristic, not guaranteed).
    fn is_alive(&self) -> bool;

    /// Attempts to upgrade to a strong control reference.
    /// Returns `None` if the actor has been dropped.
    fn upgrade(&self) -> Option<Box<dyn ActorControl>>;

    /// Clone this control into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn WeakActorControl>;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

// ============================================================================
// Clone and Debug implementations for Box<dyn ActorControl>
// ============================================================================

impl Clone for Box<dyn ActorControl> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl Clone for Box<dyn WeakActorControl> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl fmt::Debug for Box<dyn ActorControl> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl fmt::Debug for Box<dyn WeakActorControl> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

// ============================================================================
// Blanket implementations for ActorRef
// ============================================================================

impl<T: Actor + 'static> ActorControl for ActorRef<T> {
    fn identity(&self) -> Identity {
        ActorRef::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorRef::is_alive(self)
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        ActorRef::stop(self).boxed()
    }

    fn kill(&self) -> Result<()> {
        ActorRef::kill(self)
    }

    fn downgrade(&self) -> Box<dyn WeakActorControl> {
        Box::new(ActorRef::downgrade(self))
    }

    fn clone_boxed(&self) -> Box<dyn ActorControl> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActorControl")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// ============================================================================
// Blanket implementations for ActorWeak
// ============================================================================

impl<T: Actor + 'static> WeakActorControl for ActorWeak<T> {
    fn identity(&self) -> Identity {
        ActorWeak::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorWeak::is_alive(self)
    }

    fn upgrade(&self) -> Option<Box<dyn ActorControl>> {
        ActorWeak::upgrade(self).map(|r| Box::new(r) as Box<dyn ActorControl>)
    }

    fn clone_boxed(&self) -> Box<dyn WeakActorControl> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakActorControl")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// ============================================================================
// From trait implementations
// ============================================================================

// From ActorRef (ownership transfer) to Box<dyn ActorControl>
impl<T: Actor + 'static> From<ActorRef<T>> for Box<dyn ActorControl> {
    fn from(actor_ref: ActorRef<T>) -> Self {
        Box::new(actor_ref)
    }
}

// From &ActorRef (clone) to Box<dyn ActorControl>
impl<T: Actor + 'static> From<&ActorRef<T>> for Box<dyn ActorControl> {
    fn from(actor_ref: &ActorRef<T>) -> Self {
        Box::new(actor_ref.clone())
    }
}

// From ActorWeak (ownership transfer) to Box<dyn WeakActorControl>
impl<T: Actor + 'static> From<ActorWeak<T>> for Box<dyn WeakActorControl> {
    fn from(actor_weak: ActorWeak<T>) -> Self {
        Box::new(actor_weak)
    }
}

// From &ActorWeak (clone) to Box<dyn WeakActorControl>
impl<T: Actor + 'static> From<&ActorWeak<T>> for Box<dyn WeakActorControl> {
    fn from(actor_weak: &ActorWeak<T>) -> Self {
        Box::new(actor_weak.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time tests to verify trait bounds
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_actor_control_traits_are_send_sync() {
        // These will fail to compile if traits are not Send + Sync
        assert_send_sync::<Box<dyn ActorControl>>();
        assert_send_sync::<Box<dyn WeakActorControl>>();
    }
}
