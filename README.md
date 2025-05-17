# rsActor: A Simplified Actor Framework for Rust

`rsActor` is a lightweight, Tokio-based actor framework in Rust, inspired by [Kameo](https://github.com/tqwewe/kameo). It prioritizes simplicity and ease of use for local, in-process actor systems by stripping away more complex features like remote actors and supervision trees.

**Note:** This project is in a very early stage of development. APIs are subject to change, and features are still being actively developed.

## Core Features

*   **Minimalist Actor System**: Focuses on the core actor model primitives.
*   **Message Passing**:
    *   `ask`: Send a message and asynchronously await a reply.
    *   `tell`: Send a message without waiting for a reply (fire-and-forget).
    *   `ask_blocking`: Blocking version of `ask` for use in `tokio::task::spawn_blocking` contexts.
    *   `tell_blocking`: Blocking version of `tell` for use in `tokio::task::spawn_blocking` contexts.
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
rsactor = "0.2" # Check crates.io for the latest version
```

### 2. Basic Usage Example

Here's a simple counter actor:

```rust
use rsactor::{Actor, ActorRef, Message, ActorStopReason, impl_message_handler, spawn};
use anyhow::Result;
use log::info;

// Define your actor struct
struct CounterActor {
    count: u32,
}

// Implement the Actor trait
impl Actor for CounterActor {
    type Error = anyhow::Error;

    async fn on_start(&mut self,
        actor_ref: ActorRef
    ) -> Result<(), Self::Error> {
        info!("CounterActor (id: {}) started. Initial count: {}", actor_ref.id(), self.count);
        Ok(())
    }

    async fn on_stop(&mut self,
        actor_ref: ActorRef,
        stop_reason: &ActorStopReason
    ) -> Result<(), Self::Error> {
        info!("CounterActor (id: {}) stopping. Final count: {}. Reason: {:?}", actor_ref.id(), self.count, stop_reason);
        Ok(())
    }
}

// Define message types
struct IncrementMsg(u32); // Message to increment the counter by a value
struct GetCountMsg;       // Message to get the current count

// Implement Message<T> for CounterActor to handle IncrementMsg
impl Message<IncrementMsg> for CounterActor {
    type Reply = u32; // This message expects a u32 reply (the new count)

    async fn handle(&mut self, msg: IncrementMsg) -> Self::Reply {
        self.count += msg.0;
        self.count // Return the new count
    }
}

// Implement Message<T> for CounterActor to handle GetCountMsg
impl Message<GetCountMsg> for CounterActor {
    type Reply = u32; // This message expects a u32 reply (the current count)

    async fn handle(&mut self, _msg: GetCountMsg) -> Self::Reply {
        self.count // Return the current count
    }
}

// Use the impl_message_handler! macro to generate boilerplate
// for routing IncrementMsg and GetCountMsg to their respective handlers.
impl_message_handler!(CounterActor, [IncrementMsg, GetCountMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger (e.g., env_logger) to see log messages.
    // Ensure you have `env_logger` and `log` in your Cargo.toml.
    env_logger::init();

    let initial_count = 0u32;
    let counter_actor_instance = CounterActor { count: initial_count };
    info!("Creating CounterActor with initial count: {}", initial_count);

    // Spawn the actor.
    // spawn returns a tuple:
    // 1. ActorRef: A handle to send messages to the actor.
    // 2. JoinHandle: A handle to await the actor's termination and retrieve its final state.
    info!("Spawning CounterActor...");
    let (actor_ref, join_handle) = spawn(counter_actor_instance);
    info!("CounterActor spawned with ID: {}", actor_ref.id());

    // Send an IncrementMsg using 'ask' to get a reply.
    let increment_value = 5u32;
    info!("Sending IncrementMsg({}) to CounterActor (ID: {})...", increment_value, actor_ref.id());
    let count_after_increment: u32 = actor_ref.ask(IncrementMsg(increment_value)).await?;
    info!("Received reply after increment: new count = {}", count_after_increment);

    // Send a GetCountMsg using 'ask'.
    info!("Sending GetCountMsg to CounterActor (ID: {})...", actor_ref.id());
    let current_count: u32 = actor_ref.ask(GetCountMsg).await?;
    info!("Received reply for GetCountMsg: current count = {}", current_count);

    // Stop the actor gracefully.
    // The actor will process all messages currently in its mailbox before stopping.
    // The on_stop hook will be called.
    info!("Sending stop signal to CounterActor (ID: {})...", actor_ref.id());
    actor_ref.stop().await?;
    info!("Stop signal sent. CounterActor (ID: {}) will shut down gracefully.", actor_ref.id());

    // Wait for the actor's task to complete.
    // This is important to ensure the actor has fully stopped and resources are cleaned up.
    // join_handle.await returns a Result containing the actor's final state and stop reason.
    info!("Waiting for CounterActor (ID: {}) to complete its task...", actor_ref.id());
    let (stopped_actor, reason) = join_handle.await?;
    info!(
        "CounterActor (ID: {}) task completed. Final count: {}. Stop reason: {:?}",
        actor_ref.id(), // Note: actor_ref.id() is still usable here
        stopped_actor.count,
        reason
    );

    info!("Example finished.");
    Ok(())
}
```

## Running the Example

The project includes a basic example in `examples/basic.rs`. You can run it using:

```bash
cargo run --example basic
```

This will demonstrate actor creation, message passing, and lifecycle logging.

## Using Blocking Functions with Tokio Tasks

rsactor provides blocking versions of the messaging functions (`ask_blocking` and `tell_blocking`), but these are specifically designed for use within Tokio's blocking tasks - not for general synchronous code.

### When to Use Blocking Functions

Use the blocking functions when:
- You're inside a `tokio::task::spawn_blocking` task
- You need to communicate with actors from CPU-bound code
- You need to interact with actors from synchronous code that runs within the Tokio runtime

### Example: Communicating from a Blocking Task

```rust
use rsactor::{Actor, ActorRef};
use tokio::task;
use std::time::Duration;

// Within an actor's implementation or anywhere with access to an ActorRef
let actor_ref_clone = actor_ref.clone();
let blocking_task = task::spawn_blocking(move || {
    // Inside the blocking task we can use the blocking variants

    // Send a message without waiting for a response
    actor_ref_clone.tell_blocking("notification", Some(Duration::from_secs(1)))
        .expect("Failed to send message");

    // Send a message and wait for a response
    let response: String = actor_ref_clone.ask_blocking("query", Some(Duration::from_secs(2)))
        .expect("Failed to get response");

    // Process the response...
    println!("Received response: {}", response);
});
```

**Important**: These functions require an active Tokio runtime and will return an error if used outside of a Tokio runtime context.

See the `examples/actor_blocking_task.rs` for a complete example.

## Motivation

The primary goal of this project is to provide a streamlined and efficient actor-based framework by focusing on core functionalities while reducing complexity. This makes it suitable for scenarios where a full-featured actor system like `actix` might be overkill, but the actor model's benefits (concurrency, state encapsulation) are still desired.

## License

This project is licensed under the **Apache License 2.0**. You can find a copy of the license in the LICENSE-APACHE file.

## Contribution

Contributions are welcome! Feel free to open issues and submit pull requests to improve the project.

