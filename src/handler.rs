// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Handler traits for unified actor message handling.
//!
//! This module provides type-erased handler traits that allow different Actor types
//! handling the same message to be stored in a unified collection.
//!
//! # Overview
//!
//! The handler traits come in two categories:
//!
//! - **Strong handlers** ([`TellHandler`], [`AskHandler`]): Keep actors alive through strong references
//! - **Weak handlers** ([`WeakTellHandler`], [`WeakAskHandler`]): Do not keep actors alive, require explicit upgrade
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use rsactor::{TellHandler, AskHandler, WeakTellHandler};
//!
//! // Strong reference handlers (keeps actors alive)
//! let handlers: Vec<Box<dyn TellHandler<PingMsg>>> = vec![
//!     (&actor_a).into(),  // From<&ActorRef<T>> - clones the reference
//!     actor_b.into(),     // From<ActorRef<T>> - moves ownership
//! ];
//!
//! for handler in &handlers {
//!     handler.tell(PingMsg { timestamp: 12345 }).await?;
//! }
//!
//! // Weak reference handlers (does NOT keep actors alive)
//! let weak_handlers: Vec<Box<dyn WeakTellHandler<PingMsg>>> = vec![
//!     ActorRef::downgrade(&actor_a).into(),
//!     ActorRef::downgrade(&actor_b).into(),
//! ];
//!
//! // Must upgrade before use
//! for handler in &weak_handlers {
//!     if let Some(strong) = handler.upgrade() {
//!         strong.tell(PingMsg { timestamp: 12345 }).await?;
//!     }
//! }
//! ```

use crate::{Actor, ActorRef, ActorWeak, BoxFuture, Identity, Message, Result};
use futures::FutureExt;
use std::fmt;
use std::time::Duration;

// ============================================================================
// Strong Handler Traits
// ============================================================================

/// Fire-and-forget message handler for strong references (object-safe).
///
/// This trait allows storing different actor types that handle the same message type
/// in a unified collection. The handlers maintain strong references to actors,
/// keeping them alive.
///
/// # Example
///
/// ```rust,ignore
/// let handlers: Vec<Box<dyn TellHandler<MyMessage>>> = vec![
///     (&actor_a).into(),
///     (&actor_b).into(),
/// ];
///
/// for handler in &handlers {
///     handler.tell(MyMessage { data: 42 }).await?;
/// }
/// ```
pub trait TellHandler<M: Send + 'static>: Send + Sync {
    /// Sends a message without waiting for a reply.
    fn tell(&self, msg: M) -> BoxFuture<'_, Result<()>>;

    /// Sends a message with timeout.
    fn tell_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<()>>;

    /// Blocking version of tell.
    fn blocking_tell(&self, msg: M) -> Result<()>;

    /// Clone this handler into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn TellHandler<M>>;

    /// Downgrade to a weak handler.
    fn downgrade(&self) -> Box<dyn WeakTellHandler<M>>;

    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor is still alive.
    fn is_alive(&self) -> bool;

    /// Gracefully stops the actor.
    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    /// Immediately terminates the actor.
    fn kill(&self) -> Result<()>;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Request-response message handler for strong references (object-safe).
///
/// This trait allows storing different actor types that handle the same message type
/// and return the same reply type in a unified collection. The handlers maintain
/// strong references to actors, keeping them alive.
///
/// # Example
///
/// ```rust,ignore
/// let handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
///     (&actor_a).into(),
///     (&actor_b).into(),
/// ];
///
/// for handler in &handlers {
///     let status = handler.ask(GetStatus).await?;
///     println!("Status: {:?}", status);
/// }
/// ```
pub trait AskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    /// Sends a message and awaits a reply.
    fn ask(&self, msg: M) -> BoxFuture<'_, Result<R>>;

    /// Sends a message and awaits a reply with timeout.
    fn ask_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<R>>;

    /// Blocking version of ask.
    fn blocking_ask(&self, msg: M) -> Result<R>;

    /// Clone this handler into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn AskHandler<M, R>>;

    /// Downgrade to a weak handler.
    fn downgrade(&self) -> Box<dyn WeakAskHandler<M, R>>;

    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor is still alive.
    fn is_alive(&self) -> bool;

    /// Gracefully stops the actor.
    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    /// Immediately terminates the actor.
    fn kill(&self) -> Result<()>;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

// ============================================================================
// Weak Handler Traits
// ============================================================================

/// Weak handler for fire-and-forget messages (object-safe).
///
/// Unlike [`TellHandler`], this does not keep the actor alive.
/// Must call [`upgrade()`](WeakTellHandler::upgrade) to obtain a strong handler before sending messages.
///
/// # Example
///
/// ```rust,ignore
/// let weak_handlers: Vec<Box<dyn WeakTellHandler<MyMessage>>> = vec![
///     ActorRef::downgrade(&actor_a).into(),
///     ActorRef::downgrade(&actor_b).into(),
/// ];
///
/// for handler in &weak_handlers {
///     if let Some(strong) = handler.upgrade() {
///         strong.tell(MyMessage { data: 42 }).await?;
///     }
/// }
/// ```
pub trait WeakTellHandler<M: Send + 'static>: Send + Sync {
    /// Attempts to upgrade to a strong handler.
    /// Returns `None` if the actor has been dropped.
    fn upgrade(&self) -> Option<Box<dyn TellHandler<M>>>;

    /// Clone this handler into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn WeakTellHandler<M>>;

    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor might still be alive (heuristic, not guaranteed).
    fn is_alive(&self) -> bool;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Weak handler for request-response messages (object-safe).
///
/// Unlike [`AskHandler`], this does not keep the actor alive.
/// Must call [`upgrade()`](WeakAskHandler::upgrade) to obtain a strong handler before sending messages.
///
/// # Example
///
/// ```rust,ignore
/// let weak_handlers: Vec<Box<dyn WeakAskHandler<GetStatus, Status>>> = vec![
///     ActorRef::downgrade(&actor_a).into(),
///     ActorRef::downgrade(&actor_b).into(),
/// ];
///
/// for handler in &weak_handlers {
///     if let Some(strong) = handler.upgrade() {
///         let status = strong.ask(GetStatus).await?;
///         println!("Status: {:?}", status);
///     }
/// }
/// ```
pub trait WeakAskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    /// Attempts to upgrade to a strong handler.
    /// Returns `None` if the actor has been dropped.
    fn upgrade(&self) -> Option<Box<dyn AskHandler<M, R>>>;

    /// Clone this handler into a new boxed instance.
    fn clone_boxed(&self) -> Box<dyn WeakAskHandler<M, R>>;

    /// Returns the unique identity of the actor.
    fn identity(&self) -> Identity;

    /// Checks if the actor might still be alive (heuristic, not guaranteed).
    fn is_alive(&self) -> bool;

    /// Debug formatting support for trait objects.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

// ============================================================================
// Clone and Debug implementations for Box<dyn Handler>
// ============================================================================

// Strong handlers
impl<M: Send + 'static> Clone for Box<dyn TellHandler<M>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static, R: Send + 'static> Clone for Box<dyn AskHandler<M, R>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static> fmt::Debug for Box<dyn TellHandler<M>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl<M: Send + 'static, R: Send + 'static> fmt::Debug for Box<dyn AskHandler<M, R>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

// Weak handlers
impl<M: Send + 'static> Clone for Box<dyn WeakTellHandler<M>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static, R: Send + 'static> Clone for Box<dyn WeakAskHandler<M, R>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static> fmt::Debug for Box<dyn WeakTellHandler<M>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl<M: Send + 'static, R: Send + 'static> fmt::Debug for Box<dyn WeakAskHandler<M, R>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

// ============================================================================
// Blanket implementations for ActorRef
// ============================================================================

impl<T, M> TellHandler<M> for ActorRef<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn tell(&self, msg: M) -> BoxFuture<'_, Result<()>> {
        ActorRef::tell(self, msg).boxed()
    }

    fn tell_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<()>> {
        ActorRef::tell_with_timeout(self, msg, timeout).boxed()
    }

    fn blocking_tell(&self, msg: M) -> Result<()> {
        ActorRef::blocking_tell(self, msg)
    }

    fn clone_boxed(&self) -> Box<dyn TellHandler<M>> {
        Box::new(self.clone())
    }

    fn downgrade(&self) -> Box<dyn WeakTellHandler<M>> {
        Box::new(ActorRef::downgrade(self))
    }

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

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TellHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl<T, M> AskHandler<M, <T as Message<M>>::Reply> for ActorRef<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn ask(&self, msg: M) -> BoxFuture<'_, Result<<T as Message<M>>::Reply>> {
        ActorRef::ask(self, msg).boxed()
    }

    fn ask_with_timeout(
        &self,
        msg: M,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<<T as Message<M>>::Reply>> {
        ActorRef::ask_with_timeout(self, msg, timeout).boxed()
    }

    fn blocking_ask(&self, msg: M) -> Result<<T as Message<M>>::Reply> {
        ActorRef::blocking_ask(self, msg)
    }

    fn clone_boxed(&self) -> Box<dyn AskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(self.clone())
    }

    fn downgrade(&self) -> Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(ActorRef::downgrade(self))
    }

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

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// ============================================================================
// Blanket implementations for ActorWeak
// ============================================================================

impl<T, M> WeakTellHandler<M> for ActorWeak<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn upgrade(&self) -> Option<Box<dyn TellHandler<M>>> {
        ActorWeak::upgrade(self).map(|r| Box::new(r) as Box<dyn TellHandler<M>>)
    }

    fn clone_boxed(&self) -> Box<dyn WeakTellHandler<M>> {
        Box::new(self.clone())
    }

    fn identity(&self) -> Identity {
        ActorWeak::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorWeak::is_alive(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakTellHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl<T, M> WeakAskHandler<M, <T as Message<M>>::Reply> for ActorWeak<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn upgrade(&self) -> Option<Box<dyn AskHandler<M, <T as Message<M>>::Reply>>> {
        ActorWeak::upgrade(self)
            .map(|r| Box::new(r) as Box<dyn AskHandler<M, <T as Message<M>>::Reply>>)
    }

    fn clone_boxed(&self) -> Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(self.clone())
    }

    fn identity(&self) -> Identity {
        ActorWeak::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorWeak::is_alive(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakAskHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// ============================================================================
// From trait implementations
// ============================================================================

// === Strong Handler From implementations ===

// From ActorRef (ownership transfer) to Box<dyn TellHandler<M>>
impl<T, M> From<ActorRef<T>> for Box<dyn TellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_ref: ActorRef<T>) -> Self {
        Box::new(actor_ref)
    }
}

// From &ActorRef (clone) to Box<dyn TellHandler<M>>
impl<T, M> From<&ActorRef<T>> for Box<dyn TellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_ref: &ActorRef<T>) -> Self {
        Box::new(actor_ref.clone())
    }
}

// From ActorRef (ownership transfer) to Box<dyn AskHandler<M, R>>
impl<T, M> From<ActorRef<T>> for Box<dyn AskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_ref: ActorRef<T>) -> Self {
        Box::new(actor_ref)
    }
}

// From &ActorRef (clone) to Box<dyn AskHandler<M, R>>
impl<T, M> From<&ActorRef<T>> for Box<dyn AskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_ref: &ActorRef<T>) -> Self {
        Box::new(actor_ref.clone())
    }
}

// === Weak Handler From implementations ===

// From ActorWeak (ownership transfer) to Box<dyn WeakTellHandler<M>>
impl<T, M> From<ActorWeak<T>> for Box<dyn WeakTellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_weak: ActorWeak<T>) -> Self {
        Box::new(actor_weak)
    }
}

// From &ActorWeak (clone) to Box<dyn WeakTellHandler<M>>
impl<T, M> From<&ActorWeak<T>> for Box<dyn WeakTellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_weak: &ActorWeak<T>) -> Self {
        Box::new(actor_weak.clone())
    }
}

// From ActorWeak (ownership transfer) to Box<dyn WeakAskHandler<M, R>>
impl<T, M> From<ActorWeak<T>> for Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_weak: ActorWeak<T>) -> Self {
        Box::new(actor_weak)
    }
}

// From &ActorWeak (clone) to Box<dyn WeakAskHandler<M, R>>
impl<T, M> From<&ActorWeak<T>> for Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
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
    fn test_handler_traits_are_send_sync() {
        // These will fail to compile if traits are not Send + Sync
        assert_send_sync::<Box<dyn TellHandler<()>>>();
        assert_send_sync::<Box<dyn AskHandler<(), ()>>>();
        assert_send_sync::<Box<dyn WeakTellHandler<()>>>();
        assert_send_sync::<Box<dyn WeakAskHandler<(), ()>>>();
    }
}
