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
*   **Straightforward Actor Lifecycle**: Provides `on_start`, `on_run`, and `on_stop` hooks for managing actor behavior:
    *   `on_start`: `async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error>` - Initializes the actor's state. This method is required.
    *   `on_run`: `async fn on_run(&mut self, actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error>` - Contains the actor's main execution logic, which runs concurrently with message handling. This method is optional and has a default implementation.
    *   `on_stop`: `async fn on_stop(&mut self, actor_ref: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error>` - Performs cleanup before the actor terminates. The `killed` flag indicates whether the termination was graceful (`false`) or immediate (`true`). This method is optional and has a default implementation.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully or killed.
*   **`ActorResult`**: Enum representing the outcome of an actor's lifecycle (e.g., completed, failed).
*   **Macro-Assisted Message Handling**:
    *   `#[message_handlers]` attribute macro with `#[handler]` method attributes for automatic message handling
*   **Tokio-Native**: Built for the `tokio` asynchronous runtime.
*   **Strong Type Safety**: Provides both compile-time (`ActorRef<T>`) and runtime (`UntypedActorRef`) type safety options, ensuring message handling consistency while supporting flexible actor management patterns.
*   **Only `Send` Trait Required**: Actor structs only need to implement the `Send` trait (not `Sync`), enabling the use of interior mutability types like `std::cell::Cell` for internal state management without synchronization overhead. The `Actor` trait and `MessageHandler` trait (via `#[message_handlers]` macro) are also required, but they don't add any additional constraints on the actor's fields.
*   **Optional Tracing Support**: Built-in support for detailed observability using the `tracing` crate. When enabled via the `tracing` feature flag, provides comprehensive logging of actor lifecycle events, message handling, and performance metrics.

## Getting Started

### 1. Add Dependency

```toml
[dependencies]
rsactor = "0.9" # Check crates.io for the latest version

# Optional: Enable tracing support for detailed observability
# rsactor = { version = "0.9", features = ["tracing"] }
```

For using the derive macros, you'll also need the `message_handlers` attribute macro which is included by default.

### Optional Features

#### Tracing Support

rsActor provides optional tracing support for comprehensive observability into actor behavior. When enabled, the framework emits structured trace events for:

- Actor lifecycle events (start, stop, termination scenarios)
- Message sending and handling with timing information
- Reply processing and error handling
- Performance metrics (message processing duration)

To enable tracing support, add the `tracing` feature to your dependencies:

```toml
[dependencies]
rsactor = { version = "0.9", features = ["tracing"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

All examples include tracing support with feature detection. Here's the pattern used:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing if the feature is enabled
    #[cfg(feature = "tracing")]
    {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .init();
        println!("🚀 Demo: Tracing is ENABLED");
    }

    #[cfg(not(feature = "tracing"))]
    {
        env_logger::init();
        println!("📝 Demo: Tracing is DISABLED");
    }

    // Your actor code here...
    Ok(())
}
```

Run any example with tracing enabled:
```bash
RUST_LOG=debug cargo run --example basic --features tracing
```

### 2. Choose Your Implementation Approach

#### Option A: Using the Message Handlers Macro (Recommended)

The `#[message_handlers]` attribute macro with `#[handler]` method attributes automatically generates Message trait implementations, reducing boilerplate:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};

// 1. Define message types
struct Increment;
struct GetCount;

// 2. Define your actor struct and derive Actor
#[derive(Actor)]
struct CounterActor {
    count: u32,
}

// 3. Use the #[message_handlers] macro with #[handler] attributes to automatically generate Message trait implementations
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> () {
        self.count += 1;
    }

    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
        self.count
    }
}

// 4. Usage
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let actor = CounterActor { count: 0 };
    let (actor_ref, _join_handle) = rsactor::spawn::<CounterActor>(actor);

    actor_ref.tell(Increment).await?;
    let count = actor_ref.ask(GetCount).await?;
    println!("Count: {}", count); // Prints: Count: 1

    actor_ref.stop().await?;
    Ok(())
}
```

#### Option B: Complex Initialization with Custom Actor Implementation

For actors that need complex initialization logic with custom Actor trait implementation:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};
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

// Use message_handlers macro for message handling
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, msg: IncrementMsg, _actor_ref: &ActorRef<Self>) -> u32 {
        self.count += msg.0;
        self.count
    }
}

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

## Message Handling with `#[message_handlers]`

rsActor simplifies message handling by using the `#[message_handlers]` attribute macro combined with `#[handler]` attributes on methods. This approach offers several advantages:

- **Selective Processing**: Only methods marked with `#[handler]` are treated as message handlers.
- **Clean Separation**: Regular methods can coexist with message handlers within the same `impl` block.
- **Automatic Generation**: The macro automatically generates the necessary `Message` trait implementations and handler registrations.
- **Type Safety**: Message handler signatures are verified at compile time.
- **Reduced Boilerplate**: Eliminates the need to manually implement `Message` traits.

This is the recommended way to handle messages in rsActor for most use cases, as it leads to more concise and maintainable code.

## Examples

rsActor comes with several examples that demonstrate various features and use cases:

* **[basic](./examples/basic.rs)** - Simple counter actor demonstrating core actor model concepts with `#[message_handlers]` macro
* **[derive_macro_demo](./examples/derive_macro_demo.rs)** - Simple example using `#[message_handlers]` with `#[handler]` attributes
* **[message_macro_demo](./examples/message_macro_demo.rs)** - Demonstrates various message types with the new macro system
* **[unified_macro_demo](./examples/unified_macro_demo.rs)** - Combined usage of derive and message handler macros
* **[advanced_derive_demo](./examples/advanced_derive_demo.rs)** - Advanced usage patterns with derive macros
* **[actor_with_timeout](./examples/actor_with_timeout.rs)** - Using timeouts for actor communication
* **[actor_async_worker](./examples/actor_async_worker.rs)** - Inter-actor communication with async tasks
* **[actor_task](./examples/actor_task.rs)** - Background task communication with actors
* **[actor_blocking_task](./examples/actor_blocking_task.rs)** - Using blocking APIs with actors
* **[dining_philosophers](./examples/dining_philosophers.rs)** - Classic concurrency problem implementation
* **[weak_reference_demo](./examples/weak_reference_demo.rs)** - Working with weak actor references and lifecycle management

Run any example with:
```bash
cargo run --example <example_name>
```

All examples support tracing when enabled with the `tracing` feature:
```bash
RUST_LOG=debug cargo run --example <example_name> --features tracing
```

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

