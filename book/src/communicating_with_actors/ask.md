## Ask (Request-Response)

The "ask" pattern, accessed via `actor_ref.ask(message)`, is used to send a message to an actor and asynchronously await a reply. This is a form of two-way communication, suitable for request-response interactions.

### Characteristics of `ask`:

*   **Asynchronous Request, Asynchronous Reply**: `actor_ref.ask(message)` returns a `Future` that completes when the target actor has processed the message and produced a reply.
*   **Direct Reply**: The sender receives a typed value back from the actor, defined by the handler's return type.
*   **Error Handling**: Returns `Result<Reply, rsactor::Error>`. Errors can occur if:
    *   The actor is not alive (`Error::Send`)
    *   The reply channel was dropped (`Error::Receive`)
    *   A timeout occurs (`Error::Timeout`)

### Usage Example:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};
use anyhow::Result;
use tracing::info;

#[derive(Actor)]
struct CalculatorActor {
    last_result: i32,
}

struct AddMsg(i32, i32);
struct GetLastResult;

#[message_handlers]
impl CalculatorActor {
    #[handler]
    async fn handle_add(&mut self, msg: AddMsg, _: &ActorRef<Self>) -> i32 {
        let sum = msg.0 + msg.1;
        self.last_result = sum;
        sum
    }

    #[handler]
    async fn handle_get_last(&mut self, _: GetLastResult, _: &ActorRef<Self>) -> i32 {
        self.last_result
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let (calc_ref, jh) = spawn::<CalculatorActor>(CalculatorActor { last_result: 0 });

    // Send an AddMsg using ask and await the reply
    let sum: i32 = calc_ref.ask(AddMsg(10, 25)).await?;
    info!("Sum: {}", sum); // Sum: 35

    let last: i32 = calc_ref.ask(GetLastResult).await?;
    assert_eq!(sum, last);

    calc_ref.stop().await?;
    jh.await?;
    Ok(())
}
```

### `ask_with_timeout`

Specify a timeout for the entire request-response cycle:

```rust
use std::time::Duration;

match calc_ref.ask_with_timeout(AddMsg(5, 7), Duration::from_secs(1)).await {
    Ok(sum) => info!("Sum: {}", sum),
    Err(e) => info!("Failed: {:?}", e),
}
```

This is crucial for building resilient systems where you cannot wait indefinitely.

### `ask_join`

For handlers that return a `JoinHandle<R>` (spawning long-running tasks), `ask_join` automatically awaits the task completion:

```rust
// Handler returns JoinHandle<String>
let result: String = actor_ref.ask_join(HeavyTask { data: "input".into() }).await?;
```

This avoids manually awaiting the `JoinHandle` from a regular `ask` call.

### `blocking_ask`

For sending messages from non-async contexts:

```rust
// Without timeout
let result = actor_ref.blocking_ask(GetLastResult, None)?;

// With timeout
let result = actor_ref.blocking_ask(GetLastResult, Some(Duration::from_secs(5)))?;
```

### When to Use `ask`:

*   **Requesting Data**: When an actor needs to retrieve information from another actor.
*   **Performing Operations with Results**: When the caller needs the result to proceed.
*   **Synchronizing Operations**: To ensure one operation completes before another begins.

### Potential Pitfalls:

*   **Deadlocks**: If Actor A `ask`s Actor B, and Actor B concurrently `ask`s Actor A, a deadlock occurs. Design interaction flows carefully, or use `tell` to break cycles. rsActor provides a **runtime deadlock detection** feature that catches these cycles before they happen — see [Deadlock Detection](../advanced/deadlock_detection.md) for details.
*   **Performance**: Excessive `ask` where `tell` would suffice leads to performance bottlenecks. If a reply isn't strictly needed, prefer `tell`.

`ask` is a powerful tool for interactions requiring a response, but should be used with awareness of its blocking nature and potential complexities.
