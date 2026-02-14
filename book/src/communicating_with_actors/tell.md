## Tell (Fire and Forget)

The "tell" pattern, accessed via `actor_ref.tell(message)`, is used to send a message to an actor without expecting a direct reply. This is a form of asynchronous, one-way communication.

### Characteristics of `tell`:

*   **Asynchronous**: `actor_ref.tell(message)` returns a `Future` that completes once the message is enqueued into the actor's mailbox. It does *not* wait for the actor to process the message.
*   **No Direct Reply**: The sender does not receive a value back from the actor.
*   **`Reply` Type**: For messages intended for `tell`, their handler typically returns `()`.
*   **Error Handling**: Returns `Result<(), rsactor::Error>`. An error occurs if the actor is no longer alive.

### Usage Example:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};
use anyhow::Result;
use tracing::info;

#[derive(Actor)]
struct LoggerActor;

// Message to log a string
struct LogMessage(String);

#[message_handlers]
impl LoggerActor {
    #[handler]
    async fn handle_log(&mut self, msg: LogMessage, actor_ref: &ActorRef<Self>) -> () {
        info!("LoggerActor (id: {}): {}", actor_ref.identity(), msg.0);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let (logger_ref, jh) = spawn::<LoggerActor>(LoggerActor);

    // Send a log message using tell
    logger_ref.tell(LogMessage("Application started successfully".to_string())).await?;
    logger_ref.tell(LogMessage("Processing item #123".to_string())).await?;

    logger_ref.stop().await?;
    jh.await?;
    Ok(())
}
```

### `tell_with_timeout`

`rsActor` also provides `actor_ref.tell_with_timeout(message, timeout)`.

*   Similar to `tell`, but allows specifying a `Duration` as a timeout for enqueuing the message.
*   Returns a `Timeout` error if the message cannot be enqueued within the given duration.

```rust
use std::time::Duration;

match logger_ref.tell_with_timeout(
    LogMessage("Critical event".to_string()),
    Duration::from_millis(100)
).await {
    Ok(_) => info!("Log message sent."),
    Err(e) => info!("Failed to send log message: {:?}", e),
}
```

### `blocking_tell`

For sending messages from non-async contexts (e.g., `spawn_blocking` tasks):

```rust
// Without timeout (most efficient)
actor_ref.blocking_tell(LogMessage("from blocking".into()), None)?;

// With timeout
actor_ref.blocking_tell(LogMessage("from blocking".into()), Some(Duration::from_secs(1)))?;
```

### When to Use `tell`:

*   **Notifications/Events**: When an actor needs to notify another without needing an immediate response.
*   **Commands**: Issuing commands where the sender doesn't need to wait for completion.
*   **Decoupling**: When you want to minimize coupling between actors.
*   **Avoiding Deadlocks**: In complex actor interactions, `tell` can break circular dependency cycles that `ask` might create.

`tell` is a fundamental communication pattern for building reactive and event-driven systems with actors.
