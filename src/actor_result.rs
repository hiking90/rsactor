// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::Actor;
use std::fmt::Debug;

/// Represents the phase during which an actor failure occurred.
///
/// This enum is used to identify which lifecycle method of an actor
/// caused a failure, enabling more precise error handling and debugging.
/// Each phase corresponds to a specific method in the [`Actor`](crate::Actor) trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePhase {
    /// Actor failed during the [`on_start`](crate::Actor::on_start) lifecycle hook.
    OnStart,
    /// Actor failed during execution in the [`on_idle`](crate::Actor::on_idle) lifecycle hook.
    OnIdle,
    /// Actor failed during the [`on_stop`](crate::Actor::on_stop) lifecycle hook.
    OnStop,
    /// Actor failed during [`on_idle`](crate::Actor::on_idle), and then
    /// [`on_stop`](crate::Actor::on_stop) also failed during cleanup.
    /// The primary `on_idle` error is exposed via [`ActorFailure::error`]; the
    /// `on_stop` error is exposed via [`ActorFailure::stop_error`].
    OnIdleThenOnStop,
}

/// Implements Display for FailurePhase to provide human-readable error messages.
///
/// The human-readable form is the variant name, so this delegates to the
/// derived `Debug` impl — one source of truth when variants are added.
impl std::fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// Phase-specific payload of a failed actor lifecycle.
///
/// Each variant carries exactly the data that can exist in that phase, so the
/// invariants that were previously documentation-only are enforced by the type
/// system:
///
/// - Only [`OnStart`](ActorFailure::OnStart) lacks an actor instance (the actor
///   was never constructed).
/// - Only [`OnIdleThenOnStop`](ActorFailure::OnIdleThenOnStop) carries a second
///   error (the `on_stop` cleanup failure after the primary `on_idle` failure).
///
/// Use the accessors ([`phase`](Self::phase), [`error`](Self::error),
/// [`actor`](Self::actor), [`stop_error`](Self::stop_error)) for uniform access
/// across variants, or match directly when handling a specific phase.
#[derive(Debug)]
pub enum ActorFailure<T: Actor> {
    /// [`on_start`](crate::Actor::on_start) returned an error; the actor was
    /// never constructed, so no instance is available.
    OnStart {
        /// The error returned by `on_start`.
        error: T::Error,
    },
    /// [`on_idle`](crate::Actor::on_idle) returned an error and the subsequent
    /// `on_stop` cleanup succeeded.
    OnIdle {
        /// The actor instance, recovered for inspection or restart.
        actor: T,
        /// The error returned by `on_idle`.
        error: T::Error,
    },
    /// [`on_stop`](crate::Actor::on_stop) returned an error during shutdown.
    OnStop {
        /// The actor instance, recovered for inspection or restart.
        actor: T,
        /// The error returned by `on_stop`.
        error: T::Error,
    },
    /// [`on_idle`](crate::Actor::on_idle) returned an error, and then
    /// [`on_stop`](crate::Actor::on_stop) also failed while cleaning up.
    OnIdleThenOnStop {
        /// The actor instance, recovered for inspection or restart.
        actor: T,
        /// The primary error returned by `on_idle`.
        error: T::Error,
        /// The secondary error returned by `on_stop` during cleanup.
        stop_error: T::Error,
    },
}

impl<T: Actor> ActorFailure<T> {
    /// Returns the lifecycle phase this failure occurred in.
    pub fn phase(&self) -> FailurePhase {
        match self {
            ActorFailure::OnStart { .. } => FailurePhase::OnStart,
            ActorFailure::OnIdle { .. } => FailurePhase::OnIdle,
            ActorFailure::OnStop { .. } => FailurePhase::OnStop,
            ActorFailure::OnIdleThenOnStop { .. } => FailurePhase::OnIdleThenOnStop,
        }
    }

    /// Returns the primary error of this failure.
    ///
    /// For [`OnIdleThenOnStop`](Self::OnIdleThenOnStop) this is the original
    /// `on_idle` error; the `on_stop` cleanup error is available via
    /// [`stop_error`](Self::stop_error).
    pub fn error(&self) -> &T::Error {
        match self {
            ActorFailure::OnStart { error }
            | ActorFailure::OnIdle { error, .. }
            | ActorFailure::OnStop { error, .. }
            | ActorFailure::OnIdleThenOnStop { error, .. } => error,
        }
    }

    /// Consumes the failure and returns the primary error.
    ///
    /// Drops the recovered actor instance (if any) and the `on_stop` cleanup
    /// error (for [`OnIdleThenOnStop`](Self::OnIdleThenOnStop)).
    pub fn into_error(self) -> T::Error {
        match self {
            ActorFailure::OnStart { error }
            | ActorFailure::OnIdle { error, .. }
            | ActorFailure::OnStop { error, .. }
            | ActorFailure::OnIdleThenOnStop { error, .. } => error,
        }
    }

    /// Returns the recovered actor instance, if one exists.
    ///
    /// `None` only for [`OnStart`](Self::OnStart) failures, where the actor was
    /// never constructed.
    pub fn actor(&self) -> Option<&T> {
        match self {
            ActorFailure::OnStart { .. } => None,
            ActorFailure::OnIdle { actor, .. }
            | ActorFailure::OnStop { actor, .. }
            | ActorFailure::OnIdleThenOnStop { actor, .. } => Some(actor),
        }
    }

    /// Consumes the failure and returns the recovered actor instance, if any.
    pub fn into_actor(self) -> Option<T> {
        match self {
            ActorFailure::OnStart { .. } => None,
            ActorFailure::OnIdle { actor, .. }
            | ActorFailure::OnStop { actor, .. }
            | ActorFailure::OnIdleThenOnStop { actor, .. } => Some(actor),
        }
    }

    /// Returns the `on_stop` cleanup error, present only for
    /// [`OnIdleThenOnStop`](Self::OnIdleThenOnStop).
    pub fn stop_error(&self) -> Option<&T::Error> {
        match self {
            ActorFailure::OnIdleThenOnStop { stop_error, .. } => Some(stop_error),
            _ => None,
        }
    }

    /// Consumes the failure and returns the `on_stop` cleanup error, present
    /// only for [`OnIdleThenOnStop`](Self::OnIdleThenOnStop).
    pub fn into_stop_error(self) -> Option<T::Error> {
        match self {
            ActorFailure::OnIdleThenOnStop { stop_error, .. } => Some(stop_error),
            _ => None,
        }
    }
}

/// Result type returned when an actor's lifecycle completes.
///
/// `ActorResult` encapsulates the final state of an actor after its lifecycle has ended,
/// whether it completed successfully or failed. This is typically obtained when awaiting the
/// `JoinHandle` returned by [`spawn`](crate::spawn) or [`spawn_with_mailbox_capacity`](crate::spawn_with_mailbox_capacity).
/// It provides detailed information about the actor's termination state, including:
///
/// - Whether the actor completed successfully or failed
/// - Whether the actor was killed forcefully or stopped gracefully
/// - The phase in which a failure occurred (if applicable)
/// - The actor instance itself (if recoverable)
/// - The error that caused a failure (if applicable)
///
/// # Usage Patterns
///
/// ## Basic Error Handling
/// ```rust,no_run
/// # use rsactor::{Actor, ActorRef, spawn, ActorResult};
/// # use anyhow::Result;
/// # struct MyActor;
/// # impl Actor for MyActor {
/// #     type Args = ();
/// #     type Error = anyhow::Error;
/// #     type IdleEvent = ();
/// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self> { Ok(MyActor) }
/// # }
/// # async fn example() -> Result<()> {
/// let (actor_ref, join_handle) = spawn::<MyActor>(());
///
/// match join_handle.await? {
///     ActorResult::Completed { actor, killed } => {
///         println!("Actor completed successfully, killed: {}", killed);
///         // Use the recovered actor instance
///     }
///     ActorResult::Failed { failure, .. } => {
///         eprintln!("Actor failed in phase {}: {}", failure.phase(), failure.error());
///         // Handle the error appropriately
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Actor Supervision
/// ```rust,no_run
/// # use rsactor::{Actor, ActorRef, spawn, ActorResult, FailurePhase};
/// # use anyhow::Result;
/// # struct MyActor;
/// # impl Actor for MyActor {
/// #     type Args = ();
/// #     type Error = anyhow::Error;
/// #     type IdleEvent = ();
/// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self> { Ok(MyActor) }
/// # }
/// # async fn restart_actor() -> Result<(ActorRef<MyActor>, tokio::task::JoinHandle<ActorResult<MyActor>>)> {
/// #     Ok(spawn::<MyActor>(()))
/// # }
/// # async fn supervision_example() -> Result<()> {
/// let (actor_ref, join_handle) = spawn::<MyActor>(());
///
/// match join_handle.await? {
///     result if result.is_startup_failed() => {
///         // Actor failed to start - may need different initialization
///         eprintln!("Startup failed, checking configuration...");
///     }
///     result if result.is_runtime_failed() => {
///         // Runtime failure - restart the actor
///         if let Some(actor) = result.into_actor() {
///             println!("Restarting actor after runtime failure...");
///             let (new_ref, new_handle) = restart_actor().await?;
///         }
///     }
///     result if result.was_killed() => {
///         // Actor was forcefully terminated
///         println!("Actor was killed - no restart needed");
///     }
///     _ => {
///         // Normal completion
///         println!("Actor completed normally");
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// This enum is typically returned by actor supervision systems or when awaiting the
/// completion of an actor's task.
#[derive(Debug)]
pub enum ActorResult<T: Actor> {
    /// Actor completed successfully and can be recovered.
    ///
    /// This variant indicates that the actor finished its lifecycle without errors.
    Completed {
        /// The successfully completed actor instance
        actor: T,
        /// Whether the actor was killed (`true`) or stopped gracefully (`false`)
        killed: bool,
    },
    /// Actor failed during one of its lifecycle phases.
    ///
    /// The phase, the error(s), and the recovered actor instance (when one
    /// exists) live in the phase-specific [`ActorFailure`] payload.
    Failed {
        /// What failed, where, and what could be recovered.
        failure: ActorFailure<T>,
        /// Whether the actor was killed (`true`) or was attempting to stop gracefully (`false`)
        killed: bool,
    },
}

/// Conversion from ActorResult to a tuple of (`Option<Actor>`, `Option<Error>`)
///
/// This allows extracting both the actor instance and error (if any) in a single operation.
/// Useful for pattern matching and destructuring in supervision contexts.
///
/// **Lossy**: this conversion discards the failure phase, the `killed` flag,
/// and — for [`FailurePhase::OnIdleThenOnStop`] — the secondary `on_stop`
/// cleanup error. Use the accessors on [`ActorResult`] / [`ActorFailure`] when
/// any of those matter.
///
/// # Example
/// ```ignore
/// let (maybe_actor, maybe_error) = actor_result.into();
/// if let Some(actor) = maybe_actor {
///     // The actor is available (either completed or recovered after failure)
/// }
/// if let Some(error) = maybe_error {
///     // An error occurred
/// }
/// ```
impl<T: Actor> From<ActorResult<T>> for (Option<T>, Option<T::Error>) {
    fn from(result: ActorResult<T>) -> Self {
        match result {
            ActorResult::Completed { actor, .. } => (Some(actor), None),
            ActorResult::Failed { failure, .. } => match failure {
                ActorFailure::OnStart { error } => (None, Some(error)),
                ActorFailure::OnIdle { actor, error }
                | ActorFailure::OnStop { actor, error }
                | ActorFailure::OnIdleThenOnStop { actor, error, .. } => (Some(actor), Some(error)),
            },
        }
    }
}

impl<T: Actor> ActorResult<T> {
    /// Returns `true` if the actor completed successfully.
    ///
    /// This method checks if the actor finished its lifecycle without any errors,
    /// regardless of whether it was killed or stopped normally.
    pub fn is_completed(&self) -> bool {
        matches!(self, ActorResult::Completed { .. })
    }

    /// Returns `true` if the actor was killed.
    ///
    /// An actor is considered killed if it was terminated forcefully via the [`kill()`](crate::ActorRef::kill()) method,
    /// regardless of whether it completed successfully or failed. Both `ActorResult::Completed`
    /// and `ActorResult::Failed` can have `killed: true`.
    pub fn was_killed(&self) -> bool {
        matches!(
            self,
            ActorResult::Completed { killed: true, .. } | ActorResult::Failed { killed: true, .. }
        )
    }

    /// Returns `true` if the actor stopped normally.
    ///
    /// An actor stopped normally if it completed successfully without being killed,
    /// typically by processing a `StopGracefully` message or reaching the end of its lifecycle.
    pub fn stopped_normally(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: false, .. })
    }

    /// Returns the failure phase, or `None` if the actor completed successfully.
    pub fn phase(&self) -> Option<FailurePhase> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { failure, .. } => Some(failure.phase()),
        }
    }

    /// Returns `true` if the actor failed to start.
    ///
    /// This indicates that the actor failed during the [`on_start`](crate::Actor::on_start) lifecycle phase,
    /// which means it couldn't initialize properly.
    pub fn is_startup_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                failure: ActorFailure::OnStart { .. },
                ..
            }
        )
    }

    /// Returns `true` if the actor failed during runtime.
    ///
    /// This indicates that the actor started successfully but encountered an error
    /// during its normal operation in the [`on_idle`](crate::Actor::on_idle) lifecycle phase.
    /// Also returns `true` when `on_stop` additionally failed during cleanup
    /// ([`FailurePhase::OnIdleThenOnStop`]).
    pub fn is_runtime_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                failure: ActorFailure::OnIdle { .. } | ActorFailure::OnIdleThenOnStop { .. },
                ..
            }
        )
    }

    /// Returns `true` if `on_stop` also failed after an `on_idle` error.
    ///
    /// The primary `on_idle` error is exposed via [`error()`](Self::error); the
    /// `on_stop` cleanup error is exposed via [`secondary_error()`](Self::secondary_error).
    pub fn is_cleanup_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                failure: ActorFailure::OnIdleThenOnStop { .. },
                ..
            }
        )
    }

    /// Returns `true` if [`on_stop`](crate::Actor::on_stop) failed.
    ///
    /// This covers both a failure during a normal shutdown
    /// ([`FailurePhase::OnStop`]) and an `on_stop` failure while cleaning up
    /// after an `on_idle` error ([`FailurePhase::OnIdleThenOnStop`]) —
    /// mirroring how [`is_runtime_failed()`](Self::is_runtime_failed) includes
    /// the composite phase. Use [`is_cleanup_failed()`](Self::is_cleanup_failed)
    /// to distinguish the composite case.
    pub fn is_stop_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                failure: ActorFailure::OnStop { .. } | ActorFailure::OnIdleThenOnStop { .. },
                ..
            }
        )
    }

    /// Returns the actor instance if available, regardless of the result type.
    ///
    /// If the actor completed successfully, it will always return `Some(actor)`.
    /// If the actor failed, it may return `Some(actor)` or `None` depending on
    /// when the failure occurred and if the actor instance could be recovered.
    ///
    /// If the failure occurred during the [`on_start`](crate::Actor::on_start) phase, this will return `None`
    /// since the actor was not successfully initialized.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use rsactor::{ActorResult, Actor, ActorRef};
    /// # struct MyActor;
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     type IdleEvent = ();
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> { Ok(MyActor) }
    /// # }
    /// # fn example(result: ActorResult<MyActor>) {
    /// if let Some(actor) = result.actor() {
    ///     // Actor instance is available - can inspect its state
    ///     println!("Actor instance recovered");
    /// } else {
    ///     // Actor failed during initialization
    ///     println!("Actor not available (startup failure)");
    /// }
    /// # }
    /// ```
    pub fn actor(&self) -> Option<&T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { failure, .. } => failure.actor(),
        }
    }

    /// Consumes the result and returns the actor instance if available.
    ///
    /// This method is similar to [`actor()`](ActorResult::actor()) but it consumes the `ActorResult`,
    /// giving ownership of the actor to the caller if available.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use rsactor::{ActorResult, Actor, ActorRef};
    /// # struct MyActor { state: String }
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     type IdleEvent = ();
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
    /// #         Ok(MyActor { state: "ready".to_string() })
    /// #     }
    /// # }
    /// # fn example(result: ActorResult<MyActor>) {
    /// if let Some(actor) = result.into_actor() {
    ///     // Take ownership of the actor for reuse or state inspection
    ///     println!("Actor final state: {}", actor.state);
    /// }
    /// # }
    /// ```
    pub fn into_actor(self) -> Option<T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { failure, .. } => failure.into_actor(),
        }
    }

    /// Returns the error if the result represents a failure.
    ///
    /// If the actor completed successfully, this returns `None`.
    /// If the actor failed, this returns `Some(error)` containing the error that caused the failure.
    pub fn error(&self) -> Option<&T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { failure, .. } => Some(failure.error()),
        }
    }

    /// Consumes the result and returns the error if it represents a failure.
    ///
    /// This method is similar to [`error()`](ActorResult::error()) but it consumes the `ActorResult`,
    /// giving ownership of the error to the caller if available.
    pub fn into_error(self) -> Option<T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { failure, .. } => Some(failure.into_error()),
        }
    }

    /// Returns the secondary error if a cleanup hook also failed.
    ///
    /// Currently populated only when [`is_cleanup_failed()`](Self::is_cleanup_failed)
    /// is `true` (phase [`FailurePhase::OnIdleThenOnStop`]). In that case, the value
    /// is the error returned by `on_stop` during cleanup; the primary `on_idle` error
    /// remains accessible via [`error()`](Self::error).
    pub fn secondary_error(&self) -> Option<&T::Error> {
        match self {
            ActorResult::Failed { failure, .. } => failure.stop_error(),
            ActorResult::Completed { .. } => None,
        }
    }

    /// Consumes the result and returns the secondary error if a cleanup hook also failed.
    ///
    /// See [`secondary_error()`](Self::secondary_error) for semantics.
    pub fn into_secondary_error(self) -> Option<T::Error> {
        match self {
            ActorResult::Failed { failure, .. } => failure.into_stop_error(),
            ActorResult::Completed { .. } => None,
        }
    }

    /// Returns true if the result represents any kind of failure.
    ///
    /// This is the logical opposite of [`is_completed()`](ActorResult::is_completed()).
    pub fn is_failed(&self) -> bool {
        !self.is_completed()
    }

    /// Returns true if the result contains an actor instance.
    ///
    /// This checks if the actor instance is available, regardless of
    /// whether the actor completed successfully or failed.
    pub fn has_actor(&self) -> bool {
        self.actor().is_some()
    }

    /// Converts to a standard Result, preserving the actor on success
    ///
    /// This transforms the `ActorResult<T>` into a `Result<T, T::Error>`,
    /// which is useful for integrating with Rust's standard error handling patterns.
    ///
    /// **Lossy**: this discards whether the actor was killed and the failure
    /// phase, and on failure it also drops the recovered actor instance (when
    /// one exists) and the secondary `on_stop` cleanup error. Use the
    /// individual query methods if you need any of that information.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use rsactor::{ActorResult, Actor, ActorRef};
    /// # struct MyActor;
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     type IdleEvent = ();
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> { Ok(MyActor) }
    /// # }
    /// # fn example(result: ActorResult<MyActor>) -> Result<MyActor, anyhow::Error> {
    /// // Convert to standard Result for use with ? operator
    /// let actor = result.into_result()?;
    /// Ok(actor)
    /// # }
    /// ```
    pub fn into_result(self) -> std::result::Result<T, T::Error> {
        match self {
            ActorResult::Completed { actor, .. } => Ok(actor),
            ActorResult::Failed { failure, .. } => Err(failure.into_error()),
        }
    }
}
