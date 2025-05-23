# rsActor
[![CI](https://github.com/hiking90/rsactor/actions/workflows/rust.yml/badge.svg)](https://github.com/hiking90/rsactor/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/rsactor.svg)](https://crates.io/crates/rsactor)
[![Docs.rs](https://docs.rs/rsactor/badge.svg)](https://docs.rs/rsactor)
[![Rust Version](https://img.shields.io/badge/rustc-1.75+-blue.svg)](https://blog.rust-lang.org/)

A Lightweight Rust Actor Framework with Simple Yet Powerful Task Control.

`rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing simple yet powerful task control. It prioritizes simplicity and efficiency for local, in-process actor systems while giving developers complete control over their actors' execution lifecycle — define your own `run_loop`, control execution, control the lifecycle.

**Note:** This project is actively evolving. While core APIs are stable, some features may be refined in future releases.

## Core Features

*   **Minimalist Actor System**: Focuses on core actor model primitives.
*   **Message Passing**:
    *   `ask`/`ask_with_timeout`: Send a message and asynchronously await a reply.
    *   `tell`/`tell_with_timeout`: Send a message without waiting for a reply.
    *   `ask_blocking`/`tell_blocking`: Blocking versions for `tokio::task::spawn_blocking` contexts.
*   **Actor Lifecycle with Simple Yet Powerful Task Control**: `on_start`, `on_stop`, and `run_loop` hooks form the actor's lifecycle. The distinctive `run_loop` feature (added in v0.4.0) provides a dedicated task execution environment that users can control with simple yet powerful primitives, unlike other actor frameworks. This gives developers complete control over their actor's task logic while the framework manages the underlying execution, eliminating the need for separate `tokio::spawn` calls. All lifecycle hooks are optional and have default implementations.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully or killed.
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
use rsactor::{Actor, ActorRef, Message, ActorStopReason, impl_message_handler, spawn};
use anyhow::Result;
use log::info;

// Define actor struct
struct CounterActor {
    count: u32,
}

// Implement Actor trait
impl Actor for CounterActor {
    type Error = anyhow::Error;

    // on_start, on_stop, and run_loop are optional and have default implementations.
    // You can uncomment and implement them if needed.

    async fn on_start(&mut self, actor_ref: &ActorRef) -> Result<(), Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.id(), self.nt);
        Ok(())
    }

    // async fn on_stop(&mut self, actor_ref: &ActorRef, stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
    //     info!("CounterActor (id: {}) stopping. Final count: {}. Reason: {:?}", actor_ref.id(), self.count, stop_reason);
    //     Ok(())
    // }

    // The main execution loop for the actor.
    //
    // This method sets up two periodic timers and continuously processes their events:
    // - One timer fires every 300 milliseconds
    // - Another timer fires every 1 second
    //
    // The method employs Tokio's select mechanism to efficiently wait for either timer to ck,
    // and then performs the corresponding action. This creates a non-blocking event loop at
    // can handle multiple timing-based operations concurrently.
    //
    // Note: The actor will continue running until this method returns, either with Ok(())
    // with an error. As currently implemented, this loop will run indefinitely.
    async fn run_loop(&mut self, _actor_ref: &ActorRef) -> Result<(), Self::Error> {
        let mut tick_300ms = tokio::time::interval(std::time::Duration::from_millis));
        let mut tick_1s = tokio::time::interval(std::time::Duration::from_secs(1));
        // The actor will stop when this method returns Ok(()) or Err(_).
        loop {
            tokio::select! {
                _ = tick_300ms.tick() => {
                    println!("Tick: 300ms");
                }
                _ = tick_1s.tick() => {
                    println!("Tick: 1s");
                }
            }
        }
        // If you add a break to exit the loop, return Ok(()) here to satisfy the tion signature.
        // Ok(())
    }
}

// Define message types
struct IncrementMsg(u32);
struct GetCountMsg;

// Implement Message<T> for IncrementMsg
impl Message<IncrementMsg> for CounterActor {
    type Reply = u32; // New count

    async fn handle(&mut self, msg: IncrementMsg, _actor_ref: &ActorRef) -> Self::Reply {
        self.count += msg.0;
        self.count
    }
}

// Implement Message<T> for GetCountMsg
impl Message<GetCountMsg> for CounterActor {
    type Reply = u32; // Current count

    async fn handle(&mut self, _msg: GetCountMsg, _actor_ref: &ActorRef) -> Self::Reply {
        self.count
    }
}

// Use macro for message handling
impl_message_handler!(CounterActor, [IncrementMsg, GetCountMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize logger

    let counter_actor_instance = CounterActor { count: 0u32 };
    info!("Creating CounterActor");

    let (actor_ref, join_handle) = spawn(counter_actor_instance);
    info!("CounterActor spawned with ID: {}", actor_ref.id());

    let new_count: u32 = actor_ref.ask(IncrementMsg(5)).await?;
    info!("Incremented count: {}", new_count);

    let current_count: u32 = actor_ref.ask(GetCountMsg).await?;
    info!("Current count: {}", current_count);

    actor_ref.stop().await?;
    info!("Stop signal sent to CounterActor (ID: {})", actor_ref.id());

    let (stopped_actor, reason) = join_handle.await?;
    info!(
        "CounterActor (ID: {}) task completed. Final count: {}. Stop reason: {:?}",
        actor_ref.id(),
        stopped_actor.count,
        reason
    );

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
use rsactor::{ActorRef, Message}; // Assuming Actor is also in scope
use tokio::task;
use std::time::Duration;
use anyhow::Result;

// Dummy message and actor for context
struct MyMessage(String);
struct MyQuery;
struct MyActor;

impl Actor for MyActor {
    type Error = anyhow::Error;
    // on_start, on_stop, and run_loop are optional
}

impl Message<MyMessage> for MyActor {
    type Reply = ();
    async fn handle(&mut self, _msg: MyMessage, _actor_ref: &ActorRef) -> Self::Reply {}
}

impl Message<MyQuery> for MyActor {
    type Reply = String;
    async fn handle(&mut self, _msg: MyQuery, _actor_ref: &ActorRef) -> Self::Reply {
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
//     let (actor_ref, _join_handle) = rsactor::spawn(MyActor);
//     demonstrate_blocking_calls(actor_ref).await?;
//     Ok(())
// }
```

**Important**: These functions require an active Tokio runtime.

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

