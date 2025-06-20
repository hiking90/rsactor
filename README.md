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
*   **Actor Derive Macro**: `#[derive(Actor)]` for simple actors that don't need complex initialization.
*   **Message Passing**:
    *   `ask`/`ask_with_timeout`: Send a message and asynchronously await a reply.
    *   `tell`/`tell_with_timeout`: Send a message without waiting for a reply.
    *   `ask_blocking`/`tell_blocking`: Blocking versions for `tokio::task::spawn_blocking` contexts.
*   **Straightforward Actor Lifecycle**: `on_start`, `on_run`, and `on_stop` hooks provide a clean and intuitive actor lifecycle management system. The framework manages the execution flow while giving developers full control over actor behavior.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully or killed.
*   **`ActorResult`**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
*   **Macro-Assisted Message Handling**: `impl_message_handler!` macro simplifies routing messages.
*   **Tokio-Native**: Built for the `tokio` asynchronous runtime.
*   **Strong Type Safety**: Provides both compile-time (`ActorRef<T>`) and runtime (`UntypedActorRef`) type safety options, ensuring message handling consistency while supporting flexible actor management patterns.
*   **Only `Send` Trait Required**: Actor structs only need to implement the `Send` trait (not `Sync`), enabling the use of interior mutability types like `std::cell::Cell` for internal state management without synchronization overhead. The `Actor` trait and `MessageHandler` trait (via `impl_message_handler!` macro) are also required, but they don't add any additional constraints on the actor's fields.

## Getting Started

### 1. Add Dependency

```toml
[dependencies]
rsactor = "0.8" # Check crates.io for the latest version
```

### 2. Choose Your Implementation Approach

#### Option A: Using the Actor Derive Macro (Recommended for Simple Cases)

For simple actors that don't need complex initialization logic:

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};

// 1. Define your actor struct and derive Actor
#[derive(Actor)]
struct CounterActor {
    count: u32,
}

// 2. Define message types
struct Increment;
struct GetCount;

// 3. Implement message handlers
impl Message<Increment> for CounterActor {
    type Reply = ();
    async fn handle(&mut self, _: Increment, _: &ActorRef<Self>) -> Self::Reply {
        self.count += 1;
    }
}

impl Message<GetCount> for CounterActor {
    type Reply = u32;
    async fn handle(&mut self, _: GetCount, _: &ActorRef<Self>) -> Self::Reply {
        self.count
    }
}

// 4. Wire up message handlers
impl_message_handler!(CounterActor, [Increment, GetCount]);

// 5. Usage
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create actor instance and spawn
    let actor = CounterActor { count: 0 };
    let (actor_ref, _join_handle) = spawn::<CounterActor>(actor);

    // Send messages
    actor_ref.tell(Increment).await?;
    let count = actor_ref.ask(GetCount).await?;
    println!("Count: {}", count); // Prints: Count: 1

    actor_ref.stop().await?;
    Ok(())
}
```

#### Option B: Manual Implementation (For Complex Initialization)

For actors that need complex initialization logic:

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
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

    // on_start is required and must be implemented.
    // on_run and on_stop are optional and have default implementations.
    async fn on_start(initial_count: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.identity(), initial_count);
        Ok(CounterActor {
            count: initial_count,
        })
    }
}

// Define message types
struct IncrementMsg(u32);

// Implement Message<T> for IncrementMsg
impl Message<IncrementMsg> for CounterActor {
    type Reply = u32; // New count

    async fn handle(&mut self, msg: IncrementMsg, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count += msg.0;
        self.count
    }
}

// Use macro for message handling
impl_message_handler!(CounterActor, [IncrementMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize logger

    info!("Creating CounterActor");

    let (actor_ref, join_handle) = spawn::<CounterActor>(0u32); // Pass initial count
    info!("CounterActor spawned with ID: {}", actor_ref.identity());

    let new_count: u32 = actor_ref.ask(IncrementMsg(5)).await?;
    info!("Incremented count: {}", new_count);

    actor_ref.stop().await?;
    info!("Stop signal sent to CounterActor (ID: {})", actor_ref.identity());

    let actor_result = join_handle.await?;
    info!(
        "CounterActor (ID: {}) task completed. Result: {:?}",
        actor_ref.identity(),
        actor_result
    );

    Ok(())
}
```

## Examples

rsActor comes with several examples that demonstrate various features and use cases:

* **[basic](./examples/basic.rs)** - Simple counter actor demonstrating core actor model concepts
* **[actor_with_timeout](./examples/actor_with_timeout.rs)** - Using timeouts for actor communication
* **[actor_async_worker](./examples/actor_async_worker.rs)** - Inter-actor communication with async tasks
* **[actor_task](./examples/actor_task.rs)** - Background task communication with actors
* **[actor_blocking_task](./examples/actor_blocking_task.rs)** - Using blocking APIs with actors
* **[dining_philosophers](./examples/dining_philosophers.rs)** - Classic concurrency problem implementation
* **[unified_macro_demo](./examples/unified_macro_demo.rs)** - Message handling with macros

Run any example with:
```bash
cargo run --example <example_name>
```

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

