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
    /// Actor failed during execution in the [`on_run`](crate::Actor::on_run) lifecycle hook.
    OnRun,
    /// Actor failed during the [`on_stop`](crate::Actor::on_stop) lifecycle hook.
    OnStop,
    /// Actor failed during [`on_run`](crate::Actor::on_run), and then
    /// [`on_stop`](crate::Actor::on_stop) also failed during cleanup.
    /// The primary error is from `on_run`; the `on_stop` error is logged.
    OnRunThenOnStop,
}

/// Implements Display for FailurePhase to provide human-readable error messages.
///
/// This allows `FailurePhase` to be easily converted to strings for logging,
/// error reporting, and debugging purposes.
impl std::fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailurePhase::OnStart => write!(f, "OnStart"),
            FailurePhase::OnRun => write!(f, "OnRun"),
            FailurePhase::OnStop => write!(f, "OnStop"),
            FailurePhase::OnRunThenOnStop => write!(f, "OnRunThenOnStop"),
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
///     ActorResult::Failed { error, phase, .. } => {
///         eprintln!("Actor failed in phase {:?}: {}", phase, error);
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
    /// This variant indicates that the actor encountered an error during execution.
    Failed {
        /// The actor instance (if recoverable), or None if not recoverable.
        /// This will be `None` specifically when the failure occurred during the [`on_start`](Actor::on_start) phase,
        /// as the actor wasn't fully initialized.
        actor: Option<T>,
        /// The error that caused the failure
        error: T::Error,
        /// The lifecycle phase during which the failure occurred
        phase: FailurePhase,
        /// Whether the actor was killed (`true`) or was attempting to stop gracefully (`false`)
        killed: bool,
    },
}

/// Conversion from ActorResult to a tuple of (`Option<Actor>`, `Option<Error>`)
///
/// This allows extracting both the actor instance and error (if any) in a single operation.
/// Useful for pattern matching and destructuring in supervision contexts.
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
            ActorResult::Failed {
                actor,
                error: cause,
                ..
            } => (actor, Some(cause)),
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

    /// Returns `true` if the actor failed to start.
    ///
    /// This indicates that the actor failed during the [`on_start`](crate::Actor::on_start) lifecycle phase,
    /// which means it couldn't initialize properly.
    pub fn is_startup_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                phase: FailurePhase::OnStart,
                ..
            }
        )
    }

    /// Returns `true` if the actor failed during runtime.
    ///
    /// This indicates that the actor started successfully but encountered an error
    /// during its normal operation in the [`on_run`](crate::Actor::on_run) lifecycle phase.
    /// Also returns `true` when `on_stop` additionally failed during cleanup
    /// ([`FailurePhase::OnRunThenOnStop`]).
    pub fn is_runtime_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                phase: FailurePhase::OnRun | FailurePhase::OnRunThenOnStop,
                ..
            }
        )
    }

    /// Returns `true` if `on_stop` also failed after an `on_run` error.
    ///
    /// The primary error is from `on_run`; the `on_stop` error is logged separately.
    pub fn is_cleanup_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                phase: FailurePhase::OnRunThenOnStop,
                ..
            }
        )
    }

    /// Returns `true` if the actor failed during the stop phase.
    ///
    /// This indicates that the actor encountered an error while trying to shut down
    /// in the [`on_stop`](crate::Actor::on_stop) lifecycle phase.
    pub fn is_stop_failed(&self) -> bool {
        matches!(
            self,
            ActorResult::Failed {
                phase: FailurePhase::OnStop,
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
            ActorResult::Failed { actor, .. } => actor.as_ref(),
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
            ActorResult::Failed { actor, .. } => actor,
        }
    }

    /// Returns the error if the result represents a failure.
    ///
    /// If the actor completed successfully, this returns `None`.
    /// If the actor failed, this returns `Some(error)` containing the error that caused the failure.
    pub fn error(&self) -> Option<&T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
        }
    }

    /// Consumes the result and returns the error if it represents a failure.
    ///
    /// This method is similar to [`error()`](ActorResult::error()) but it consumes the `ActorResult`,
    /// giving ownership of the error to the caller if available.
    pub fn into_error(self) -> Option<T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
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
    /// **Note**: This method discards information about whether the actor was killed
    /// and the failure phase. Use the individual query methods if you need this information.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use rsactor::{ActorResult, Actor, ActorRef};
    /// # struct MyActor;
    /// # impl Actor for MyActor {
    /// #     type Args = ();
    /// #     type Error = anyhow::Error;
    /// #     async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> { Ok(MyActor) }
    /// # }
    /// # fn example(result: ActorResult<MyActor>) -> Result<MyActor, anyhow::Error> {
    /// // Convert to standard Result for use with ? operator
    /// let actor = result.to_result()?;
    /// Ok(actor)
    /// # }
    /// ```
    pub fn to_result(self) -> std::result::Result<T, T::Error> {
        match self {
            ActorResult::Completed { actor, .. } => Ok(actor),
            ActorResult::Failed { error: cause, .. } => Err(cause),
        }
    }
}
