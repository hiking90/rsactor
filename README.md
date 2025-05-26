# rsActor
[![CI](https://github.com/hiking90/rsactor/actions/workflows/rust.yml/badge.svg)](https://github.com/hiking90/rsactor/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/rsactor.svg)](https://crates.io/crates/rsactor)
[![Docs.rs](https://docs.rs/rsactor/badge.svg)](https://docs.rs/rsactor)
[![Rust Version](https://img.shields.io/badge/rustc-1.75+-blue.svg)](https://blog.rust-lang.org/)

A Simple and Efficient In-Process Actor Model Implementation for Rust.

`rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing a simple and efficient actor model for local, in-process systems. It emphasizes clean message-passing semantics and straightforward actor lifecycle management while maintaining high performance for Rust applications.

**Note:** This project is actively evolving. While core APIs are stable, some features may be refined in future releases.

## Core Features

*   **Minimalist Actor System**: Focuses on core actor model primitives.
*   **Message Passing**:
    *   `ask`/`ask_with_timeout`: Send a message and asynchronously await a reply.
    *   `tell`/`tell_with_timeout`: Send a message without waiting for a reply.
    *   `ask_blocking`/`tell_blocking`: Blocking versions for `tokio::task::spawn_blocking` contexts.
*   **Straightforward Actor Lifecycle**: `on_start`, `on_run`, and `on_stop` hooks provide a clean and intuitive actor lifecycle management system. The framework manages the execution flow while giving developers full control over actor behavior.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully or killed.
*   **`ActorResult`**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
*   **Macro-Assisted Message Handling**: `impl_message_handler!` macro simplifies routing messages.
*   **Tokio-Native**: Built for the `tokio` asynchronous runtime.
*   **Only `Send` Trait Required**: Actor structs only need to implement the `Send` trait (not `Sync`), enabling the use of interior mutability types like `std::cell::Cell` for internal state management without synchronization overhead. The `Actor` trait and `MessageHandler` trait (via `impl_message_handler!` macro) are also required, but they don't add any additional constraints on the actor's fields.

## Getting Started

### 1. Add Dependency

```toml
[dependencies]
rsactor = "0.7" # Check crates.io for the latest version
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
    tick_300ms: tokio::time::Interval,
    tick_1s: tokio::time::Interval,
}

// Implement Actor trait
impl Actor for CounterActor {
    type Args = u32; // Define an args type for actor creation
    type Error = anyhow::Error;

    // on_start is required and must be implemented.
    // on_run and on_stop are optional and have default implementations.

    async fn on_start(initial_count: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.identity(), initial_count);
        Ok(CounterActor {
            count: initial_count,
            tick_300ms: tokio::time::interval(std::time::Duration::from_millis(300)),
            tick_1s: tokio::time::interval(std::time::Duration::from_secs(1)),
        })
    }

    // The main processing loop for the actor.
    // This method is called repeatedly after on_start completes. If it returns Ok(()), the actor continues running.
    // If it returns Err(_), the actor stops with an error.
    async fn on_run(&mut self, _actor_ref: &ActorRef<Self>) -> Result<(), Self::Error> {
        // Use tokio::select! to handle multiple interval ticks concurrently
        tokio::select! {
            _ = self.tick_300ms.tick() => {
                println!("Tick: 300ms, Count: {}", self.count);
            }
            _ = self.tick_1s.tick() => {
                println!("Tick: 1s, Count: {}", self.count);
            }
        }
        // Return Ok(()) to continue running, or call actor_ref.stop() to gracefully stop
        Ok(())
    }

    // Called when the actor is stopping, either gracefully or due to being killed.
    // This provides an opportunity for cleanup before the actor terminates.
    // The 'killed' parameter is true if the actor was terminated via the 'kill' method,
    // and false if it was stopped gracefully via the 'stop' method.
    async fn on_stop(&mut self, _actor_ref: &ActorRef<Self>, killed: bool) -> Result<(), Self::Error> {
        info!("CounterActor stopping. Final count: {}. Killed: {}",
              self.count, killed);
        Ok(())
    }
}

// Define message types
struct IncrementMsg(u32);
struct GetCountMsg;

// Implement Message<T> for IncrementMsg
impl Message<IncrementMsg> for CounterActor {
    type Reply = u32; // New count

    async fn handle(&mut self, msg: IncrementMsg, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count += msg.0;
        self.count
    }
}

// Implement Message<T> for GetCountMsg
impl Message<GetCountMsg> for CounterActor {
    type Reply = u32; // Current count

    async fn handle(&mut self, _msg: GetCountMsg, _actor_ref: &ActorRef<Self>) -> Self::Reply {
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
            if let Some(actor_state) = actor {
                info!("Actor failed during phase {:?}: {:?}. Actor state at failure: {:?}. Killed: {}", phase, error, actor_state, killed);
            } else {
                info!("Actor failed during phase {:?}: {:?}. No actor state available. Killed: {}", phase, error, killed);
            }
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
    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> { // Updated on_start
        Ok(MyActor)
    }
    // on_run is optional
}

impl Message<MyMessage> for MyActor {
    type Reply = ();
    async fn handle(&mut self, _msg: MyMessage, _actor_ref: &ActorRef<Self>) -> Self::Reply {}
}

impl Message<MyQuery> for MyActor {
    type Reply = String;
    async fn handle(&mut self, _msg: MyQuery, _actor_ref: &ActorRef<Self>) -> Self::Reply {
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

## Type Safety Features

rsActor provides two actor reference types:

### `ActorRef<T>` - Compile-Time Type Safety
- **Recommended for most use cases**
- Only accepts messages the actor can handle (compile-time validation)
- Return types automatically inferred from trait implementations
- Zero runtime overhead

### `UntypedActorRef` - Runtime Type Handling
- **Use only when type erasure is needed**
- Enables storing different actor types in collections
- Supports dynamic actor management and plugin systems
- Developer responsible for ensuring type safety at runtime

**Conversion**: You can get an `UntypedActorRef` from `ActorRef<T>` using `typed_ref.untyped_actor_ref()`

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

