# rsActor
[![CI](https://github.com/hiking90/rsactor/actions/workflows/rust.yml/badge.svg)](https://github.com/hiking90/rsactor/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/rsactor.svg)](https://crates.io/crates/rsactor)
[![Docs.rs](https://docs.rs/rsactor/badge.svg)](https://docs.rs/rsactor)
[![Rust Version](https://img.shields.io/badge/rustc-1.75+-blue.svg)](https://blog.rust-lang.org/)

A Simple and Efficient In-Process Actor Model Implementation for Rust.

`rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing a simple and efficient actor model for local, in-process systems. It emphasizes clean message-passing semantics and straightforward actor lifecycle management while maintaining high performance for Rust applications.

**Note:** This project is actively evolving. While core APIs are stable, some features may be refined in future releases.

## Core Features

### Actor System
- **Minimalist Design**: Focuses on core actor model primitives with a clean API
- **Tokio-Native**: Built for the `tokio` asynchronous runtime
- **Actor Derive Macro**: `#[derive(Actor)]` for simple actors that don't need complex initialization

### Message Passing
| Method | Description |
|--------|-------------|
| `ask` / `ask_with_timeout` | Send a message and asynchronously await a reply |
| `tell` / `tell_with_timeout` | Send a message without waiting for a reply (fire-and-forget) |
| `blocking_ask` / `blocking_tell` | Blocking versions for `tokio::task::spawn_blocking` contexts |

- **Macro-Assisted Handlers**: `#[message_handlers]` attribute macro with `#[handler]` method attributes for automatic message handling

### Actor Lifecycle
Three well-defined hooks for managing actor behavior:
- `on_start`: Initializes the actor's state (required)
- `on_run`: Main execution logic, runs concurrently with message handling (optional)
- `on_stop`: Cleanup before termination, with `killed` flag for graceful vs immediate (optional)

Supports **graceful termination** (`stop()`) and **immediate termination** (`kill()`), with `ActorResult` enum representing lifecycle outcomes.

### Type Safety
- **Compile-Time Safety**: `ActorRef<T>` ensures message handling consistency and prevents type-related runtime errors
- **Handler Traits**: `TellHandler<M>` and `AskHandler<M, R>` enable unified management of different actor types in a single collection
- **Actor Control Traits**: `ActorControl` and `WeakActorControl` provide type-erased lifecycle management
- **Only `Send` Required**: Actor structs only need `Send` trait (not `Sync`), enabling interior mutability types like `std::cell::Cell`

### Observability
- **Optional Tracing**: Built-in support via `tracing` feature flag for actor lifecycle events, message handling, and performance metrics
- **Metrics Support**: Optional `metrics` feature for monitoring message counts, processing times, and actor uptime

## Why rsActor?

### Focused Scope
Unlike broader frameworks like Actix, rsActor specializes exclusively in **local, in-process actor systems**. This focused approach eliminates complexity from unused features like remote actors or clustering, resulting in a cleaner API and smaller footprint.

### Comparison with Other Frameworks

| Feature | rsActor | Actix | Kameo |
|---------|:-------:|:-----:|:-----:|
| In-Process Focus | Yes | No (distributed) | Yes |
| `ActorRef<T>` Type Safety | Yes | No | No |
| Explicit Lifecycle Hooks | Yes | No | Yes |
| Built-in Tracing | Yes | No | No |
| Metrics Support | Yes | Limited | No |
| Learning Curve | Easy | Moderate | Easy |

### Key Advantages

- **Simplicity First**: Minimal API surface with sensible defaults
- **Type-Safe by Default**: `ActorRef<T>` ensures compile-time message validation with zero runtime overhead
- **Production-Ready Observability**: Integrated tracing and metrics support
- **Deadlock-Free Design**: Message-passing architecture naturally prevents deadlocks

## Getting Started

### 1. Add Dependency

```toml
[dependencies]
rsactor = "0.12" # Check crates.io for the latest version

# Optional: Enable tracing support for detailed observability
# rsactor = { version = "0.12", features = ["tracing"] }
```

For using the derive macros, you'll also need the `message_handlers` attribute macro which is included by default.

### 2. Message Handling with `#[message_handlers]`

rsActor uses the `#[message_handlers]` attribute macro combined with `#[handler]` method attributes for message handling. This is **required** for all actors and offers several advantages:

- **Selective Processing**: Only methods marked with `#[handler]` are treated as message handlers.
- **Clean Separation**: Regular methods can coexist with message handlers within the same `impl` block.
- **Automatic Generation**: The macro automatically generates the necessary `Message` trait implementations and handler registrations.
- **Type Safety**: Message handler signatures are verified at compile time.
- **Reduced Boilerplate**: Eliminates the need to manually implement `Message` traits.

### 3. Choose Your Actor Creation Approach

#### Option A: Simple Actor with `#[derive(Actor)]`

For simple actors that don't need complex initialization logic, use the `#[derive(Actor)]` macro:

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
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) {
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
    let (actor_ref, _join_handle) = spawn::<CounterActor>(actor);

    actor_ref.tell(Increment).await?;
    let count = actor_ref.ask(GetCount).await?;
    println!("Count: {}", count); // Prints: Count: 1

    actor_ref.stop().await?;
    Ok(())
}
```

#### Option B: Custom Actor Implementation with Manual Initialization

For actors that need custom initialization logic, implement the `Actor` trait manually:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};
use anyhow::Result;
use tracing::info;

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
struct Increment(u32);

// Use message_handlers macro for message handling
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, msg: Increment, _actor_ref: &ActorRef<Self>) -> u32 {
        self.count += msg.0;
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init(); // Initialize tracing

    info!("Creating CounterActor");

    let (actor_ref, join_handle) = spawn::<CounterActor>(0u32); // Pass initial count as Args
    info!("CounterActor spawned with ID: {}", actor_ref.identity());

    let new_count: u32 = actor_ref.ask(Increment(5)).await?;
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

* **[basic](./examples/basic.rs)** - Simple counter actor demonstrating core concepts with `#[message_handlers]` macro
* **[actor_with_timeout](./examples/actor_with_timeout.rs)** - Using timeouts for actor communication
* **[actor_async_worker](./examples/actor_async_worker.rs)** - Inter-actor communication with async tasks
* **[actor_blocking_task](./examples/actor_blocking_task.rs)** - Using blocking APIs with actors
* **[dining_philosophers](./examples/dining_philosophers.rs)** - Classic concurrency problem implementation
* **[weak_reference_demo](./examples/weak_reference_demo.rs)** - Working with weak actor references and lifecycle
* **[handler_demo](./examples/handler_demo.rs)** - Using handler traits for unified actor management
* **[ask_join_demo](./examples/ask_join_demo.rs)** - Using `ask_join` for CPU/IO-bound operations
* **[metrics_demo](./examples/metrics_demo.rs)** - Actor performance monitoring (requires `metrics` feature)
* **[tracing_demo](./examples/tracing_demo.rs)** - Structured logging and actor lifecycle tracing

Run any example with:
```bash
cargo run --example <example_name>
```

All examples support tracing when enabled with the `tracing` feature:
```bash
RUST_LOG=debug cargo run --example <example_name> --features tracing
```

## Optional Features

### Tracing Support

rsActor provides optional tracing support for comprehensive observability into actor behavior. When enabled, the framework emits structured trace events for:

- Actor lifecycle events (start, stop, termination scenarios)
- Message sending and handling with timing information
- Reply processing and error handling
- Performance metrics (message processing duration)

To enable tracing support, add the `tracing` feature to your dependencies:

```toml
[dependencies]
rsactor = { version = "0.12", features = ["tracing"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

All examples include tracing support. Here's the recommended initialization pattern:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Your actor code here...
    Ok(())
}
```

Run any example with tracing enabled:
```bash
RUST_LOG=debug cargo run --example basic --features tracing
```

## Handler Traits

Handler traits (`TellHandler`, `AskHandler`, `WeakTellHandler`, `WeakAskHandler`) enable unified management of different actor types handling the same message in a single collection. See the [Handler Traits Documentation](./docs/handler_traits_design.md) for details.

## Actor Control Traits

Actor control traits (`ActorControl`, `WeakActorControl`) provide type-erased lifecycle management for different actor types in a single collection. Handler traits provide `as_control()` and `as_weak_control()` methods to access lifecycle operations.

## Documentation

- **[Debugging Guide](./docs/debugging_guide.md)** - Error handling, dead letter tracking, and troubleshooting
- **[Metrics Guide](./docs/metrics.md)** - Actor performance monitoring
- **[Tracing Guide](./docs/tracing.md)** - Detailed observability with tracing
- **[FAQ](./docs/FAQ.md)** - Common questions and answers

## Contributing

We welcome contributions! Here's how to get started:

### Development Setup

```bash
git clone https://github.com/hiking90/rsactor.git
cd rsactor

# Run tests
cargo test --all-features

# Run examples
cargo run --example basic

# With tracing
RUST_LOG=debug cargo run --example basic --features tracing
```

### Code Quality

Before submitting a PR, ensure:

```bash
cargo fmt                                                    # Format code
cargo clippy --all-targets --all-features -- -D warnings    # Lint check
cargo test --all-features                                    # All tests pass
```

### Ways to Contribute

- Bug reports and fixes
- Documentation improvements
- New examples
- Performance optimizations
- Feature requests

## Claude Code Skills

rsActor provides [Claude Code](https://claude.ai/code) skills to help AI assistants write correct rsactor code.

### Installation

```bash
# Global installation (recommended)
curl -sSL https://raw.githubusercontent.com/hiking90/rsactor/main/install-skills.sh | bash

# Project-local installation
curl -sSL https://raw.githubusercontent.com/hiking90/rsactor/main/install-skills.sh | bash -s -- --local
```

### Available Skills

- **rsactor-actor**: Create new actors with proper patterns
- **rsactor-handler**: Add message handlers to existing actors
- **rsactor-guide**: API reference and troubleshooting guide

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

