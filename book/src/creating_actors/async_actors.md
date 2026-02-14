## Async Actors

Actors in `rsActor` are inherently asynchronous, thanks to their integration with Tokio. However, the term "Async Actor" here refers to actors that, within their message handlers or lifecycle methods, perform operations that involve `.await`ing other asynchronous tasks. This is a common pattern for I/O-bound work or when an actor needs to coordinate with other asynchronous services.

`rsActor` fully supports this: message handlers (`handle` method) and lifecycle hooks (`on_start`, `on_run`, `on_stop`) are already `async fn`.

### Example: Actor Performing Async Work in `handle`

Consider an actor that, upon receiving a message, needs to perform an asynchronous HTTP request or a database query.

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
use anyhow::Result;
use log::info;
use tokio::time::{sleep, Duration};

#[derive(Debug)]
struct AsyncWorkerActor {
    worker_id: u32,
}

impl Actor for AsyncWorkerActor {
    type Args = u32; // Worker ID
    type Error = anyhow::Error;

    async fn on_start(worker_id: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("AsyncWorkerActor (id: {}, worker_id: {}) starting", actor_ref.identity(), worker_id);
        Ok(AsyncWorkerActor { worker_id })
    }
}

// Message to perform an async task
struct PerformAsyncTaskMsg {
    task_data: String,
    delay_ms: u64,
}

impl Message<PerformAsyncTaskMsg> for AsyncWorkerActor {
    type Reply = String; // Result of the async task

    async fn handle(&mut self, msg: PerformAsyncTaskMsg, actor_ref: &ActorRef<Self>) -> Self::Reply {
        info!(
            "Actor (id: {}, worker_id: {}) received async task: {}. Will delay for {}ms",
            actor_ref.identity(),
            self.worker_id,
            msg.task_data,
            msg.delay_ms
        );

        // Simulate an asynchronous operation (e.g., an HTTP call, DB query)
        sleep(Duration::from_millis(msg.delay_ms)).await;

        let result = format!(
            "Task '{}' completed by worker {} after {}ms",
            msg.task_data,
            self.worker_id,
            msg.delay_ms
        );
        info!("Actor (id: {}) finished async task: {}", actor_ref.identity(), result);
        result
    }
}

impl_message_handler!(AsyncWorkerActor, [PerformAsyncTaskMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let (actor_ref, jh) = spawn::<AsyncWorkerActor>(101); // Spawn with worker_id 101

    let task1 = PerformAsyncTaskMsg { task_data: "Process Payment".to_string(), delay_ms: 100 };
    let task2 = PerformAsyncTaskMsg { task_data: "Fetch User Data".to_string(), delay_ms: 50 };

    // Send messages and await replies
    // Since message handling is sequential for an actor, task2 will only start after task1's handle completes.
    let result1 = actor_ref.ask(task1).await?;
    info!("Main: Result 1: {}", result1);

    let result2 = actor_ref.ask(task2).await?;
    info!("Main: Result 2: {}", result2);

    actor_ref.stop().await?;
    jh.await??;
    Ok(())
}

```

In this example:
- The `handle` method for `PerformAsyncTaskMsg` is `async`.
- It uses `tokio::time::sleep(...).await` to simulate a non-CPU-bound asynchronous operation.
- While the actor is `await`ing inside `handle`, its specific Tokio task yields control, allowing other Tokio tasks (including other actors or other asynchronous operations in the system) to run. However, this specific actor instance will not process further messages from its mailbox until the current `handle` method completes.

### Spawning Additional Tokio Tasks from an Actor

Sometimes, an actor might need to initiate multiple asynchronous operations concurrently or offload work to separate Tokio tasks without blocking its own message processing loop for the entire duration of that work. This is a common pattern for achieving higher concurrency within the scope of a single actor's responsibilities.

The `actor_async_worker.rs` example in the `examples/` directory demonstrates this pattern:
- A `RequesterActor` sends tasks to a `WorkerActor`.
- The `WorkerActor`, in its `handle` method for `ProcessTask`, uses `tokio::spawn` to launch a new asynchronous task for the actual work.
- This allows the `WorkerActor`'s `handle` method to return quickly, making the `WorkerActor` available to process new messages while the spawned Tokio tasks run in the background.
- The spawned tasks, upon completion, send their results back to the `RequesterActor` (or another designated actor) using messages.

**Key snippet from `examples/actor_async_worker.rs` (`WorkerActor`):**

```rust
// In WorkerActor's impl Message<ProcessTask>
async fn handle(&mut self, msg: ProcessTask, _actor_ref: &ActorRef<Self>) -> Self::Reply {
    let task_id = msg.task_id;
    let data = msg.data;
    let requester = msg.requester; // ActorRef to send the result back

    println!("WorkerActor received task {}: {}", task_id, data);

    // Spawn a new Tokio task to do the processing asynchronously
    tokio::spawn(async move {
        // Simulate some processing time
        let processing_time = (task_id % 3 + 1) as u64;
        println!("Processing task {} will take {} seconds", task_id, processing_time);
        tokio::time::sleep(Duration::from_secs(processing_time)).await;

        let result = format!("Result of task {} ...", task_id, /* ... */);

        // Send the result back to the requester
        match requester.tell(WorkCompleted { task_id, result }).await {
            Ok(_) => println!("Worker sent back result for task {}", task_id),
            Err(e) => eprintln!("Failed to send result for task {}: {:?}", task_id, e),
        }
    });
    // The handle method returns quickly, allowing WorkerActor to process other messages.
}
```

This pattern is powerful for actors that manage or delegate multiple concurrent operations.

### Considerations:

*   **Sequential Message Processing**: Even if an actor spawns other Tokio tasks, its own messages are still processed one by one from its mailbox. The `handle` method for a given message must complete before the next message is taken from the mailbox.
*   **State Management**: If spawned tasks need to interact with the actor's state, they must do so by sending messages back to the actor. Direct mutable access to `self` is not possible from a separately spawned `tokio::spawn` task that outlives the `handle` method's scope (unless using `Arc<Mutex<...>>` or similar, which moves away from the actor model's state encapsulation benefits for that specific piece of state).
*   **Error Handling**: Errors in spawned tasks need to be handled appropriately, potentially by sending an error message back to the parent actor or another designated error-handling actor.
*   **Graceful Shutdown**: If an actor spawns long-running tasks, consider how these tasks should behave when the actor itself is stopped. They might need to be cancelled or allowed to complete. `rsActor` itself doesn't automatically manage tasks spawned via `tokio::spawn` from within an actor; this is the developer's responsibility.

Async actors are fundamental to building responsive, I/O-bound applications with `rsActor`.
