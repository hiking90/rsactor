# rsActor: A Simplified Actor Framework for Rust

`rsActor` is a lightweight, Tokio-based actor framework in Rust, inspired by [Kameo](https://github.com/Nugine/kameo). It prioritizes simplicity and ease of use for local, in-process actor systems by stripping away more complex features like remote actors and supervision trees.

**Note:** This project is in a very early stage of development. APIs are subject to change, and features are still being actively developed.

## Core Features

*   **Minimalist Actor System**: Focuses on the core actor model primitives.
*   **Asynchronous Message Passing**:
    *   `ask`: Send a message and asynchronously await a reply.
    *   `tell`: Send a message without waiting for a reply (fire-and-forget).
*   **Actor Lifecycle**: Actors implement `on_start` and `on_stop` hooks.
*   **Graceful & Immediate Termination**: Actors can be stopped gracefully (processing remaining messages) or killed immediately.
*   **Macro-Assisted Message Handling**: The `impl_message_handler!` macro simplifies routing different message types to their respective handlers within an actor.
*   **Tokio-Native**: Built exclusively for the `tokio` asynchronous runtime.

## Comparison with Kameo

While `rsActor` shares the goal of providing an actor system, it makes different design choices compared to Kameo:

*   **No Remote Actor Support**: `rsActor` is for local actors only.
*   **Non-Generic `ActorRef`**: `rsActor`'s `ActorRef` is a concrete type. Messages are dynamically typed (`Box<dyn Any + Send>`), with runtime type checking for replies in `ask` calls. Kameo uses a generic `ActorRef<A: Actor>`.
*   **No Actor Linking or Supervision**: `rsActor` does not include built-in support for linking actor lifecycles or supervision strategies.
*   **Tokio-Specific**: `rsActor` is tightly coupled with Tokio. Kameo is designed for broader async runtime compatibility.
*   **`impl_message_handler!` Macro**: `rsActor` uses a macro to generate the boilerplate for handling multiple message types, whereas Kameo might use generic trait implementations per message.

## Getting Started

### 1. Add Dependency

Add `rsActor` to your `Cargo.toml`:

```toml
[dependencies]
rsActor = { git = "https://your-repo-url/rsActor.git" } # Or path, or crates.io version when published
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
log = "0.4"
env_logger = "0.11" # Optional, for logging examples
```

*(Note: Update the dependency source once `rsActor` is published or if you're using a local path.)*

### 2. Basic Usage Example

Here's a simple counter actor:

```rust
use rsActor::{Actor, ActorRef, Message, spawn, impl_message_handler};
use anyhow::Result;
use log::info;

// Define your actor struct
struct CounterActor {
    count: i32,
}

// Implement the Actor trait
impl Actor for CounterActor {
    type Error = anyhow::Error; // Define an error type

    async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.id(), self.count);
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<(), Self::Error> {
        info!("CounterActor stopping. Final count: {}.", self.count);
        Ok(())
    }
}

// Define message types
struct IncrementMsg(i32);
struct GetCountMsg;

// Implement Message<T> for each message your actor handles
impl Message<IncrementMsg> for CounterActor {
    type Reply = i32; // The type of reply this message expects

    async fn handle(&mut self, msg: IncrementMsg) -> Self::Reply {
        self.count += msg.0;
        self.count
    }
}

impl Message<GetCountMsg> for CounterActor {
    type Reply = i32;

    async fn handle(&mut self, _msg: GetCountMsg) -> Self::Reply {
        self.count
    }
}

// Use the macro to implement the MessageHandler for your actor and its messages
impl_message_handler!(CounterActor, [IncrementMsg, GetCountMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize logger to see on_start/on_stop messages

    let counter_actor = CounterActor { count: 0 };
    let actor_ref: ActorRef = spawn(counter_actor).await?;

    // Send an IncrementMsg (fire-and-forget using tell, though ask is used here for demo)
    let new_count: i32 = actor_ref.ask(IncrementMsg(5)).await?;
    info!("Count after increment: {}", new_count); // Expected: 5

    // Send GetCountMsg
    let current_count: i32 = actor_ref.ask(GetCountMsg).await?;
    info!("Current count: {}", current_count); // Expected: 5

    // Stop the actor (it will process on_stop)
    // Dropping the ActorRef also signals the actor to stop after processing existing messages.
    // actor_ref.stop().await?; // or actor_ref.kill().await?;
    drop(actor_ref);

    // Give a moment for the actor to shut down and log its on_stop message
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    Ok(())
}
```

## Running the Example

The project includes a basic example in `examples/basic.rs`. You can run it using:

```bash
cargo run --example basic
```

This will demonstrate actor creation, message passing, and lifecycle logging.

## Motivation

The primary goal of this project is to provide a streamlined and efficient actor-based framework by focusing on core functionalities while reducing complexity. This makes it suitable for scenarios where a full-featured actor system like `actix` might be overkill, but the actor model's benefits (concurrency, state encapsulation) are still desired.

## License

This project is licensed under the **Apache License 2.0**. You can find a copy of the license in the LICENSE-APACHE file.

## Contribution

Contributions are welcome! Feel free to open issues and submit pull requests to improve the project.

