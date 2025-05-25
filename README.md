# rsActor
A Lightweight Rust Actor Framework with Simple Yet Powerful Task Control.

`rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing simple yet powerful task control. It prioritizes simplicity and efficiency for local, in-process actor systems while giving developers complete control over their actors' execution lifecycle — define your own `on_run`, control execution, control the lifecycle.

**Note:** This project is actively evolving. While core APIs are stable, some features may be refined in future releases.

## Core Features

*   **Minimalist Actor System**: Focuses on core actor model primitives.
*   **Message Passing**:
    *   `ask`/`ask_with_timeout`: Send a message and asynchronously await a reply.
    *   `tell`/`tell_with_timeout`: Send a message without waiting for a reply.
    *   `ask_blocking`/`tell_blocking`: Blocking versions for `tokio::task::spawn_blocking` contexts.
*   **Actor Lifecycle with Simple Yet Powerful Task Control**: `on_start` and `on_run` hooks form the actor's lifecycle. The distinctive `on_run` feature provides a dedicated task execution environment that users can control with simple yet powerful primitives, unlike other actor frameworks. This gives developers complete control over their actor's task logic while the framework manages the underlying execution, eliminating the need for separate `tokio::spawn` calls. All lifecycle hooks are optional and have default implementations.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully or killed.
*   **`ActorResult`**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
*   **Macro-Assisted Message Handling**: `impl_message_handler!` macro simplifies routing messages.
*   **Tokio-Native**: Built for the `tokio` asynchronous runtime.
*   **Only `Send` Trait Required**: Actor structs only need to implement the `Send` trait (not `Sync`), enabling the use of interior mutability types like `std::cell::Cell` for internal state management without synchronization overhead.

## Getting Started

### 1. Add Dependency

```toml
[dependencies]
rsactor = "0.5" # Check crates.io for the latest version
```

### 2. Basic Usage Example

A simple counter actor:

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn, ActorResult};
use anyhow::Result;
use log::info;

// Define actor struct
#[derive(Debug)] // Added Debug for printing the actor in ActorResult
struct CounterActor {
    count: u32,
}

// Implement Actor trait
impl Actor for CounterActor {
    type Args = u32; // Define an args type for actor creation
    type Error = anyhow::Error;

    // on_start and on_run are optional and have default implementations.
    // You can uncomment and implement them if needed.

    async fn on_start(initial_count: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.identity(), initial_count);
        Ok(CounterActor { count: initial_count })
    }

    // The main execution loop for the actor.
    // This method is called after on_start. If it returns Ok(()), the actor continues running.
    // If it returns Err(_), the actor stops due to an error.
    async fn on_run(&mut self, _actor_ref: ActorRef<Self>) -> Result<(), Self::Error> {
        let mut tick_300ms = tokio::time::interval(std::time::Duration::from_millis(300));
        let mut tick_1s = tokio::time::interval(std::time::Duration::from_secs(1));
        // The actor will stop when this method returns Err(_) or when actor_ref.stop() is called.
        // For this example, we'll let it run for a short period then stop.
        for _ in 0..5 { // Run for 5 ticks of the 1s timer
            tokio::select! {
                _ = tick_300ms.tick() => {
                    println!("Tick: 300ms, Count: {}", self.count);
                }
                _ = tick_1s.tick() => {
                    println!("Tick: 1s, Count: {}", self.count);
                }
            }
        }
        // Return Ok(()) to continue running, or call actor_ref.stop() to gracefully stop
        Ok(())
    }
}

// Define message types
struct IncrementMsg(u32);
struct GetCountMsg;

// Implement Message<T> for IncrementMsg
impl Message<IncrementMsg> for CounterActor {
    type Reply = u32; // New count

    async fn handle(&mut self, msg: IncrementMsg, _actor_ref: ActorRef<Self>) -> Self::Reply {
        self.count += msg.0;
        self.count
    }
}

// Implement Message<T> for GetCountMsg
impl Message<GetCountMsg> for CounterActor {
    type Reply = u32; // Current count

    async fn handle(&mut self, _msg: GetCountMsg, _actor_ref: ActorRef<Self>) -> Self::Reply {
        self.count
    }
}

// Use macro for message handling
impl_message_handler!(CounterActor, [IncrementMsg, GetCountMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize logger

    info!("Creating CounterActor");

    let (actor_ref, join_handle) = spawn::<CounterActor>(0u32); // Pass initial count
    info!("CounterActor spawned with ID: {}", actor_ref.identity());

    let new_count: u32 = actor_ref.ask(IncrementMsg(5)).await?;
    info!("Incremented count: {}", new_count);

    let current_count: u32 = actor_ref.ask(GetCountMsg).await?;
    info!("Current count: {}", current_count);

    // Actor will stop itself based on on_run logic.
    // If you want to stop it explicitly:
    // actor_ref.stop().await?;
    // info!("Stop signal sent to CounterActor (ID: {})", actor_ref.identity());

    let actor_result = join_handle.await?;
    info!(
        "CounterActor (ID: {}) task completed. Result: {:?}",
        actor_ref.identity(),
        actor_result
    );

    // Example of how to inspect the ActorResult
    match actor_result {
        ActorResult::Completed { actor, killed } => {
            info!("Actor completed. Final count: {}. Killed: {}", actor.count, killed);
        }
        ActorResult::Failed { actor, error, phase, killed } => {
            info!("Actor failed during phase {:?}: {:?}. Actor state at failure: {:?}. Killed: {}", phase, error, actor, killed);
        }
    }

    info!("Example finished.");
    Ok(())
}
```

## Running the Example

Run the example from `examples/basic.rs`:

```bash
cargo run --example basic
```

## Using Blocking Functions with Tokio Tasks

`ask_blocking` and `tell_blocking` are for use within Tokio's blocking tasks (`tokio::task::spawn_blocking`).

### When to Use
- Inside a `tokio::task::spawn_blocking` task.

### Example

```rust
use rsactor::{ActorRef, Message, Actor, impl_message_handler}; // Assuming Actor is also in scope
use tokio::task;
use std::time::Duration;
use anyhow::Result;

// Dummy message and actor for context
struct MyMessage(String);
struct MyQuery;
#[derive(Debug)] // Added for ActorResult
struct MyActor;

impl Actor for MyActor {
    type Args = (); // Added Args
    type Error = anyhow::Error;
    async fn on_start(_args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> { // Updated on_start
        Ok(MyActor)
    }
    // on_run is optional
}

impl Message<MyMessage> for MyActor {
    type Reply = ();
    async fn handle(&mut self, _msg: MyMessage, _actor_ref: ActorRef<Self>) -> Self::Reply {}
}

impl Message<MyQuery> for MyActor {
    type Reply = String;
    async fn handle(&mut self, _msg: MyQuery, _actor_ref: ActorRef<Self>) -> Self::Reply {
        "response".to_string()
    }
}

rsactor::impl_message_handler!(MyActor, [MyMessage, MyQuery]);

async fn demonstrate_blocking_calls(actor_ref: ActorRef) -> Result<()> {
    // --- tell_blocking example ---
    // Clone ActorRef for the first blocking task (tell_blocking)
    let actor_ref_clone_tell = actor_ref.clone();
    // Spawn a blocking task for tell_blocking
    let blocking_task_tell = task::spawn_blocking(move || {
        // Send a message without waiting for a reply, without a timeout
        actor_ref_clone_tell.tell_blocking(MyMessage("notification".to_string()), None)
    });

    // --- ask_blocking example ---
    // Clone ActorRef for the second blocking task (ask_blocking)
    let actor_ref_clone_ask = actor_ref.clone();
    // Spawn another blocking task for ask_blocking
    let blocking_task_ask = task::spawn_blocking(move || {
        // Send a query and wait for a reply, with a timeout.
        // This call will block the current thread (managed by `spawn_blocking`)
        // until a response is received from the actor or the timeout occurs.
        // The type parameters for ask_blocking are:
        // M: The message type (MyQuery). This is the type of the message being sent.
        // R: The expected reply type (String). This is the type of the response we expect back.
        actor_ref_clone_ask.ask_blocking::<MyQuery, String>(
            MyQuery, Some(Duration::from_secs(2))
        )
    });

    // Wait for tasks to complete and handle results
    // Handle the result of the tell_blocking task
    match blocking_task_tell.await? {
        Ok(_) => println!("Tell blocking successful"),
        Err(e) => println!("Tell blocking failed: {:?}", e),
    }

    // Handle the result of the ask_blocking task
    match blocking_task_ask.await? {
        Ok(response) => println!("Ask blocking successful, response: {}", response),
        Err(e) => println!("Ask blocking failed: {:?}", e),
    }
    Ok(())
}

// To make this runnable, you'd need to spawn an actor and pass its ActorRef
// For example:
// #[tokio::main]
// async fn main() -> Result<()> {
//     let (actor_ref, _join_handle) = rsactor::spawn::<MyActor>(()); // Pass empty args
//     demonstrate_blocking_calls(actor_ref).await?;
//     Ok(())
// }
```

**Important**: These functions require an active Tokio runtime.

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

