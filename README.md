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
rsactor = "0.6" # Check crates.io for the latest version
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

    // on_start and on_run are optional and have default implementations.
    // You can uncomment and implement them if needed.

    async fn on_start(initial_count: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.identity(), initial_count);
        Ok(CounterActor {
            count: initial_count,
            tick_300ms: tokio::time::interval(std::time::Duration::from_millis(300)),
            tick_1s: tokio::time::interval(std::time::Duration::from_secs(1)),
        })
    }

    // The main execution loop for the actor.
    // This method is called after on_start. If it returns Ok(()), the actor continues running.
    // If it returns Err(_), the actor stops due to an error.
    async fn on_run(&mut self, _actor_ref: ActorRef<Self>) -> Result<(), Self::Error> {
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

## Type Safety Features

rsActor provides two actor reference types that offer different levels of type safety:

### `ActorRef<T>` - Type-Safe Actor References

`ActorRef<T>` provides **compile-time type safety** for message passing:

- **Message Type Safety**: Only messages that the actor can handle (via `Message<M>` trait) are accepted at compile time
- **Reply Type Safety**: Return types are automatically inferred from the `Message<M>` trait implementation
- **Zero Runtime Overhead**: Type safety is enforced at compile time with no performance cost

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};

#[derive(Debug)]
struct Calculator;

impl Actor for Calculator {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Calculator)
    }
}

struct Add(i32, i32);
struct Multiply(i32, i32);

impl Message<Add> for Calculator {
    type Reply = i32;  // Compile-time guarantee of reply type

    async fn handle(&mut self, msg: Add, _actor_ref: ActorRef<Self>) -> Self::Reply {
        msg.0 + msg.1
    }
}

impl Message<Multiply> for Calculator {
    type Reply = i32;

    async fn handle(&mut self, msg: Multiply, _actor_ref: ActorRef<Self>) -> Self::Reply {
        msg.0 * msg.1
    }
}

impl_message_handler!(Calculator, [Add, Multiply]);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (calc_ref, _handle) = spawn::<Calculator>(());

    // ✅ Type-safe: Compiler knows the return type is i32
    let sum: i32 = calc_ref.ask(Add(5, 3)).await?;
    let product: i32 = calc_ref.ask(Multiply(4, 7)).await?;

    // ❌ Compile error: Calculator doesn't handle String messages
    // calc_ref.ask("invalid message").await?;

    println!("Sum: {}, Product: {}", sum, product);
    Ok(())
}
```

### `UntypedActorRef` - Type-Erased Actor References

`UntypedActorRef` provides **runtime type handling** for dynamic scenarios:

- **Collection Management**: Store different actor types in the same collection (Vec, HashMap, etc.)
- **Plugin Systems**: Handle actors loaded dynamically at runtime
- **Heterogeneous Actor Groups**: Manage actors of different types uniformly

⚠️ **Developer Responsibility**: When using `UntypedActorRef`, you are responsible for ensuring type safety at runtime.

```rust
use rsactor::{Actor, ActorRef, UntypedActorRef, Message, impl_message_handler, spawn};
use std::collections::HashMap;

// Different actor types
#[derive(Debug)]
struct Logger;

#[derive(Debug)]
struct Counter { count: u32 }

impl Actor for Logger {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Logger)
    }
}

impl Actor for Counter {
    type Args = u32;
    type Error = anyhow::Error;
    async fn on_start(initial: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Counter { count: initial })
    }
}

// Message types
struct LogMessage(String);
struct IncrementMessage;

impl Message<LogMessage> for Logger {
    type Reply = ();
    async fn handle(&mut self, msg: LogMessage, _actor_ref: ActorRef<Self>) -> Self::Reply {
        println!("LOG: {}", msg.0);
    }
}

impl Message<IncrementMessage> for Counter {
    type Reply = u32;
    async fn handle(&mut self, _msg: IncrementMessage, _actor_ref: ActorRef<Self>) -> Self::Reply {
        self.count += 1;
        self.count
    }
}

impl_message_handler!(Logger, [LogMessage]);
impl_message_handler!(Counter, [IncrementMessage]);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create different actor types
    let (logger_ref, _) = spawn::<Logger>(());
    let (counter_ref, _) = spawn::<Counter>(0);

    // Store in a collection using UntypedActorRef
    let mut actors: HashMap<String, UntypedActorRef> = HashMap::new();
    actors.insert("logger".to_string(), logger_ref.untyped_actor_ref().clone());
    actors.insert("counter".to_string(), counter_ref.untyped_actor_ref().clone());

    // ⚠️ Developer responsibility: Ensure correct message types at runtime
    if let Some(logger) = actors.get("logger") {
        logger.tell(LogMessage("Hello from untyped ref!".to_string())).await?;
    }

    if let Some(counter) = actors.get("counter") {
        let new_count: u32 = counter.ask(IncrementMessage).await?;
        println!("Counter: {}", new_count);
    }

    // ❌ Runtime error: Sending wrong message type to wrong actor
    // This will compile but fail at runtime!
    // actors.get("logger").unwrap().ask::<IncrementMessage, u32>(IncrementMessage).await?;

    Ok(())
}
```

### Type Safety Guidelines

1. **Use `ActorRef<T>` by default** for type safety and better development experience
2. **Use `UntypedActorRef` only when necessary** for:
   - Collections of heterogeneous actors
   - Dynamic actor loading/plugin systems
   - Interfacing with external APIs that require type erasure

3. **When using `UntypedActorRef`**:
   - Document expected message types clearly
   - Consider wrapping in higher-level abstractions
   - Use defensive programming (error handling for type mismatches)
   - Test thoroughly for runtime type errors

### Converting Between Reference Types

You can easily convert between typed and untyped actor references:

```rust
let (typed_ref, _) = spawn::<MyActor>(());

// Get untyped reference from typed reference
let untyped_ref: &UntypedActorRef = typed_ref.untyped_actor_ref();

// Note: You cannot safely convert UntypedActorRef back to ActorRef<T>
// without additional type information and validation
```

## Further Information

For more detailed questions and answers, please see the [FAQ](./docs/FAQ.md).

## License

This project is licensed under the Apache License 2.0. See the [LICENSE-APACHE](LICENSE-APACHE) file for details.

