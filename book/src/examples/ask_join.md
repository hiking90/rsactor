# Ask Join Demo

This example demonstrates the `ask_join` pattern where message handlers spawn long-running tasks and return `JoinHandle<T>`, while callers use `ask_join` to automatically await task completion.

## Key Concepts

- **`ask_join`**: Sends a message, receives a `JoinHandle<T>`, and automatically awaits it
- **Non-blocking handlers**: Handlers spawn `tokio::spawn` tasks instead of blocking the actor
- **Concurrency**: The actor remains responsive while spawned tasks run in the background

## Code Walkthrough

### Handler returning JoinHandle

```rust
#[derive(Actor)]
struct WorkerActor {
    task_counter: u32,
}

struct HeavyComputationTask {
    id: u32,
    duration_secs: u64,
    multiplier: u32,
}

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_heavy_computation(
        &mut self,
        msg: HeavyComputationTask,
        _: &ActorRef<Self>,
    ) -> JoinHandle<u64> {
        self.task_counter += 1;
        let task_id = msg.id;
        let multiplier = msg.multiplier;
        let duration = Duration::from_secs(msg.duration_secs);

        // Spawn task — actor remains free to process other messages
        tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            (task_id as u64) * (multiplier as u64)
        })
    }
}
```

### ask vs ask_join

```rust
// ask() returns JoinHandle — you await it manually
let join_handle: JoinHandle<u64> = worker_ref.ask(task).await?;
let result = join_handle.await?;

// ask_join() does both steps automatically
let result: u64 = worker_ref.ask_join(task).await?;
```

### Error Handling

`ask_join` returns `Error::Join` when the spawned task panics:

```rust
match worker_ref.ask_join(PanicTask).await {
    Ok(result) => println!("Success: {}", result),
    Err(rsactor::Error::Join { identity, source }) => {
        println!("Task panicked on actor {}: {}", identity, source);
    }
    Err(e) => println!("Other error: {}", e),
}
```

## Running

```bash
cargo run --example ask_join_demo
```
