// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::Actor;
use std::fmt::Debug;

/// Represents the phase during which an actor failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePhase {
    /// Actor failed during the `on_start` lifecycle hook.
    OnStart,
    /// Actor failed during execution.
    OnRun,
    /// Actor failed during the `on_stop` lifecycle hook.
    OnStop,
}

impl std::fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailurePhase::OnStart => write!(f, "OnStart"),
            FailurePhase::OnRun => write!(f, "OnRun"),
            FailurePhase::OnStop => write!(f, "OnStop"),
        }
    }
}

/// Result type returned when an actor's lifecycle completes.
#[derive(Debug)]
pub enum ActorResult<T: Actor> {
    /// Actor completed successfully and can be recovered.
    Completed {
        actor: T,
        killed: bool,
    },
    /// Actor failed during one of its lifecycle phases.
    Failed {
        actor: Option<T>,
        error: T::Error,
        phase: FailurePhase,
        killed: bool,
    },
}

impl<T: Actor> From<ActorResult<T>> for (Option<T>, Option<T::Error>) {
    fn from(result: ActorResult<T>) -> Self {
        match result {
            ActorResult::Completed { actor, .. } => (Some(actor), None),
            ActorResult::Failed { actor, error: cause, .. } => (actor, Some(cause)),
        }
    }
}


impl<T: Actor> ActorResult<T> {
    /// Returns `true` if the actor completed successfully.
    pub fn is_completed(&self) -> bool {
        matches!(self, ActorResult::Completed { .. })
    }

    /// Returns `true` if the actor was killed.
    pub fn was_killed(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: true, .. } | ActorResult::Failed { killed: true, .. })
    }

    /// Returns `true` if the actor stopped normally.
    pub fn stopped_normally(&self) -> bool {
        matches!(self, ActorResult::Completed { killed: false, .. })
    }

    /// Returns `true` if the actor failed to start.
    pub fn is_startup_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnStart, .. })
    }

    /// Returns `true` if the actor failed during runtime.
    pub fn is_runtime_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnRun, .. })
    }

    /// Returns `true` if the actor failed during the stop phase.
    pub fn is_stop_failed(&self) -> bool {
        matches!(self, ActorResult::Failed { phase: FailurePhase::OnStop, .. })
    }

    /// Returns the actor instance if available, regardless of the result type.
    pub fn actor(&self) -> Option<&T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { actor, .. } => actor.as_ref(),
        }
    }

    /// Consumes the result and returns the actor instance if available.
    pub fn into_actor(self) -> Option<T> {
        match self {
            ActorResult::Completed { actor, .. } => Some(actor),
            ActorResult::Failed { actor, .. } => actor,
        }
    }

    /// Returns the error if the result represents a failure.
    pub fn error(&self) -> Option<&T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
        }
    }

    /// Consumes the result and returns the error if it represents a failure.
    pub fn into_error(self) -> Option<T::Error> {
        match self {
            ActorResult::Completed { .. } => None,
            ActorResult::Failed { error: cause, .. } => Some(cause),
        }
    }

    /// Returns true if the result represents any kind of failure.
    pub fn is_failed(&self) -> bool {
        !self.is_completed()
    }

    /// Returns true if the result contains an actor instance.
    pub fn has_actor(&self) -> bool {
        self.actor().is_some()
    }

    /// Converts to a standard Result, preserving the actor on success
    pub fn to_result(self) -> std::result::Result<T, T::Error> {
        match self {
            ActorResult::Completed { actor, .. } => Ok(actor),
            ActorResult::Failed { error: cause, .. } => Err(cause),
        }
    }
}
