# Getting Started with rsActor

This guide will help you get started with rsActor, a lightweight, Tokio-based actor framework for Rust.

## Installation

Add rsActor to your `Cargo.toml`:

```toml
[dependencies]
rsactor = "0.9"  # Check crates.io for the latest version

# Optional: Enable tracing support for detailed observability
# rsactor = { version = "0.9", features = ["tracing"] }
```

## Your First Actor

Let's create a simple counter actor that demonstrates the core concepts:

### Option A: Using the Actor Derive Macro (Recommended for Simple Actors)

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

// 3. Use the message_handlers macro for automatic message handling
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
    let (actor_ref, _join_handle) = spawn::<CounterActor>(actor);

    // Send messages to the actor
    actor_ref.tell(Increment).await?;
    let count = actor_ref.ask(GetCount).await?;
    println!("Count: {}", count); // Prints: Count: 1

    actor_ref.stop().await?;
    Ok(())
}
```

### Option B: Manual Actor Implementation (for Complex Initialization)

For actors that need complex initialization logic:

```rust
use rsactor::{Actor, ActorRef, ActorWeak, message_handlers, spawn};
use anyhow::Result;

// Define actor struct
struct CounterActor {
    count: u32,
    name: String,
}

// Implement Actor trait for complex initialization
impl Actor for CounterActor {
    type Args = (u32, String); // Tuple of initial count and name
    type Error = anyhow::Error;

    // on_start is required - this is where the actor instance is created
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let (initial_count, name) = args;
        println!("CounterActor '{}' (ID: {}) starting with count: {}",
                 name, actor_ref.identity(), initial_count);

        Ok(CounterActor {
            count: initial_count,
            name,
        })
    }

    // on_run is optional - defines the actor's main processing loop
    async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // This runs concurrently with message handling
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        println!("Actor '{}' heartbeat - current count: {}", self.name, self.count);
        Ok(()) // Returning Ok(()) means on_run will be called again
    }

    // on_stop is optional - called when the actor is terminating
    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        println!("Actor '{}' stopping (killed: {}), final count: {}",
                 self.name, killed, self.count);
        Ok(())
    }
}

// Define message types
struct IncrementMsg(u32);
struct GetCountMsg;

// Use message_handlers macro
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, msg: IncrementMsg, _: &ActorRef<Self>) -> u32 {
        self.count += msg.0;
        println!("Actor '{}' incremented by {}, new count: {}",
                 self.name, msg.0, self.count);
        self.count
    }

    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCountMsg, _: &ActorRef<Self>) -> u32 {
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Spawn actor with initialization arguments
    let (actor_ref, join_handle) = spawn::<CounterActor>((10, "MyCounter".to_string()));

    // Allow actor to run and emit heartbeats
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    // Send messages
    let new_count = actor_ref.ask(IncrementMsg(5)).await?;
    println!("Received count: {}", new_count);

    let current_count = actor_ref.ask(GetCountMsg).await?;
    println!("Current count: {}", current_count);

    // Gracefully stop the actor
    actor_ref.stop().await?;

    // Wait for actor to complete and check the result
    match join_handle.await? {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed successfully. Final count: {}, killed: {}",
                     actor.count, killed);
        }
        rsactor::ActorResult::Failed { error, phase, .. } => {
            println!("Actor failed during {:?}: {}", phase, error);
        }
    }

    Ok(())
}
```

## Core Concepts

### Actor Lifecycle

1. **`on_start`** (required): Creates the actor instance from initialization arguments
2. **`on_run`** (optional): The actor's main processing loop, runs concurrently with message handling
3. **`on_stop`** (optional): Cleanup logic when the actor terminates

### Message Passing

- **`tell`**: Fire-and-forget messaging
- **`ask`**: Request-reply messaging
- **`tell_with_timeout`/`ask_with_timeout`**: Variants with timeout support
- **`tell_blocking`/`ask_blocking`**: Blocking variants for `spawn_blocking` contexts

### Actor Termination

- **`stop()`**: Graceful shutdown - processes remaining messages
- **`kill()`**: Immediate termination - stops processing immediately

### Type Safety

rsActor provides strong compile-time type safety:

```rust
// This will compile - CounterActor handles IncrementMsg
let count: u32 = actor_ref.ask(IncrementMsg(5)).await?;

// This will NOT compile - CounterActor doesn't handle this message type
// let result = actor_ref.ask("invalid message").await?; // Compile error!
```

## Next Steps

- Learn about [advanced features](./Architecture.md) like actor lifecycle management
- Explore [tracing support](./tracing.md) for production observability
- Check out the [examples](../examples/) directory for more complex use cases
- Read the [FAQ](./FAQ.md) for common questions and best practices

## Optional Features

### Tracing Support

Enable comprehensive observability:

```toml
[dependencies]
rsactor = { version = "0.9", features = ["tracing"] }
tracing-subscriber = "0.3"
```

Initialize tracing in your application:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Your actor code here...
    Ok(())
}
```

This provides detailed logs of actor lifecycle events, message handling, and performance metrics.
