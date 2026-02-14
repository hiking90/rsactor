# ActorResult

The `ActorResult<T: Actor>` enum is returned when an actor's lifecycle completes. This is typically what you get when you `.await` the `JoinHandle` returned by `rsactor::spawn()`.

It provides detailed information about how the actor terminated.

```rust
/// Represents the phase during which an actor failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePhase {
    /// Actor failed during the `on_start` lifecycle hook.
    OnStart,
    /// Actor failed during execution in the `on_run` lifecycle hook.
    OnRun,
    /// Actor failed during the `on_stop` lifecycle hook.
    OnStop,
}

/// Result type returned when an actor's lifecycle completes.
#[derive(Debug)]
pub enum ActorResult<T: Actor> {
    /// Actor completed successfully and can be recovered.
    Completed {
        /// The successfully completed actor instance
        actor: T,
        /// Whether the actor was killed (`true`) or stopped gracefully (`false`)
        killed: bool,
    },
    /// Actor failed during one of its lifecycle phases.
    Failed {
        /// The actor instance (if recoverable), or None if not recoverable.
        /// This will be `None` specifically when the failure occurred during `on_start`,
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
```

### Variants

1.  **`ActorResult::Completed { actor: T, killed: bool }`**
    *   Indicates that the actor finished its lifecycle without any errors defined by `T::Error`.
    *   `actor`: The instance of the actor. You can retrieve it if you need to inspect its final state.
    *   `killed`: A boolean indicating if the actor was terminated via a `kill()` signal (`true`) or if it stopped gracefully (e.g., via `stop()` or by naturally ending its run loop) (`false`).

2.  **`ActorResult::Failed { actor: Option<T>, error: T::Error, phase: FailurePhase, killed: bool }`**
    *   Indicates that the actor encountered an error (`T::Error`) during its operation.
    *   `actor`: An `Option<T>` containing the actor instance if it was recoverable. This will be `None` if the failure occurred during the `OnStart` phase, as the actor instance wouldn't have been fully created.
    *   `error`: The specific error of type `T::Error` (defined in your `impl Actor for YourActor`) that caused the failure.
    *   `phase`: A `FailurePhase` enum (`OnStart`, `OnRun`, `OnStop`) indicating when the error occurred in the actor's lifecycle.
    *   `killed`: A boolean indicating if the actor was attempting to stop due to a `kill()` signal when the failure occurred.

### Utility Methods

`ActorResult` provides several helpful methods:

*   `is_completed()`: Returns `true` if the actor completed successfully.
*   `is_failed()`: Returns `true` if the actor failed.
*   `was_killed()`: Returns `true` if the actor was killed, regardless of completion or failure.
*   `stopped_normally()`: Returns `true` if `Completed` and not `killed`.
*   `is_startup_failed()`: Returns `true` if failed during `OnStart`.
*   `is_runtime_failed()`: Returns `true` if failed during `OnRun`.
*   `is_stop_failed()`: Returns `true` if failed during `OnStop`.
*   `actor()`: Returns `Option<&T>` to the actor instance if available.
*   `into_actor()`: Consumes self and returns `Option<T>`.
*   `error()`: Returns `Option<&T::Error>` if failed.
*   `into_error()`: Consumes self and returns `Option<T::Error>`.
*   `has_actor()`: Returns `true` if the result contains an actor instance.
*   `to_result()`: Converts `ActorResult<T>` into `std::result::Result<T, T::Error>`.

### Example Usage

```rust
use rsactor::{spawn, Actor, ActorRef, ActorWeak, ActorResult, FailurePhase, message_handlers};
use anyhow::{Result, anyhow};

#[derive(Debug)]
struct MyErrActor { should_fail_on_start: bool, should_fail_on_run: bool }

impl Actor for MyErrActor {
    type Args = (bool, bool); // (should_fail_on_start, should_fail_on_run)
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, ar: &ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!("MyErrActor (id: {}) on_start called", ar.identity());
        if args.0 {
            return Err(anyhow!("Deliberate failure in on_start"));
        }
        Ok(MyErrActor { should_fail_on_start: args.0, should_fail_on_run: args.1 })
    }

    async fn on_run(&mut self, ar: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        tracing::info!("MyErrActor (id: {}) on_run called", ar.identity());
        if self.should_fail_on_run {
            return Err(anyhow!("Deliberate failure in on_run"));
        }
        Ok(true) // Return true to indicate run loop should stop
    }
}

// Message type
struct Ping;

#[message_handlers]
impl MyErrActor {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {}
}


async fn check_actor_termination(fail_on_start: bool, fail_on_run: bool) {
    let (actor_ref, join_handle) = spawn::<MyErrActor>((fail_on_start, fail_on_run));
    let id = actor_ref.identity();

    match join_handle.await {
        Ok(actor_result) => match actor_result {
            ActorResult::Completed { actor, killed } => {
                tracing::info!(
                    "Actor (id: {}) COMPLETED. Killed: {}. Final state: {:?}",
                    id, killed, actor
                );
            }
            ActorResult::Failed { actor, error, phase, killed } => {
                tracing::info!(
                    "Actor (id: {}) FAILED. Phase: {:?}, Killed: {}. Error: {}. State: {:?}",
                    id, phase, killed, error, actor
                );
                if fail_on_start {
                    assert!(matches!(phase, FailurePhase::OnStart));
                    assert!(actor.is_none());
                }
                if fail_on_run && !fail_on_start {
                     assert!(matches!(phase, FailurePhase::OnRun));
                     assert!(actor.is_some());
                }
            }
        },
        Err(join_error) => {
            tracing::info!("Actor (id: {}) task JOIN ERROR: {:?}", id, join_error);
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("--- Checking normal completion ---");
    check_actor_termination(false, false).await;
    tracing::info!("--- Checking failure in on_start ---");
    check_actor_termination(true, false).await;
    tracing::info!("--- Checking failure in on_run ---");
    check_actor_termination(false, true).await;
}
```

This `ActorResult` provides a comprehensive way to understand the termination reason and final state of an actor, which is crucial for supervision strategies and debugging.
