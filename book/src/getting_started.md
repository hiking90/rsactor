# Getting Started

This section will guide you through setting up `rsActor` in your project and running a basic example.

## 1. Add Dependency

To use `rsActor` in your Rust project, add it as a dependency in your `Cargo.toml` file:

```toml
[dependencies]
rsactor = "0.13" # Check crates.io for the latest version
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
```

Make sure to replace `"0.13"` with the latest version available on [crates.io](https://crates.io/crates/rsactor).

## 2. Basic Usage Example

Let's create a simple counter actor. This actor will maintain a count and increment it when it receives an `IncrementMsg`.

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

// Use message_handlers macro with handler attributes
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, msg: Increment, _: &ActorRef<Self>) -> u32 {
        self.count += msg.0;
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init(); // Initialize tracing

    info!("Creating CounterActor");

    let (actor_ref, join_handle) = spawn::<CounterActor>(0u32); // Pass initial count
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

### Key Points Demonstrated:

1. **Actor State Management**: `CounterActor` encapsulates its state (`count`) and provides controlled access through messages.
2. **Type-Safe Communication**: The `Message<T>` trait ensures compile-time verification of message handling.
3. **Lifecycle Control**: `spawn` creates the actor, `ask` communicates with it, and `stop` terminates it gracefully.
4. **Error Handling**: Proper use of `Result` types and `?` operator for clean error propagation.

This example shows the basic actor pattern that forms the foundation for more complex actor systems.
