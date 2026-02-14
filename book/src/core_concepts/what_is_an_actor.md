## What is an Actor?

In `rsActor`, an actor is an independent computational entity that encapsulates:

1.  **State**: Actors can have internal state that they manage exclusively. No other part of the system can directly access or modify an actor's state.
2.  **Behavior**: Actors define how they react to messages they receive. This behavior is implemented through message handlers.
3.  **Mailbox**: Each actor has a mailbox where incoming messages are queued. Messages are processed one at a time, ensuring that an actor's state is modified sequentially and avoiding data races.

### Key Characteristics of Actors in `rsActor`:

*   **Isolation**: An actor's internal state is protected from direct external access. Communication happens solely through messages.
*   **Concurrency**: Actors can run concurrently with each other. `rsActor` leverages Tokio to manage these concurrent tasks.
*   **Asynchronous Operations**: Actors primarily operate asynchronously, allowing them to perform non-blocking I/O and other long-running tasks efficiently.
*   **Lifecycle**: Actors have a defined lifecycle, including startup, execution, and shutdown phases, which developers can hook into.

### The `Actor` Trait

To define an actor in `rsActor`, you implement the `Actor` trait for your struct:

```rust
use rsactor::{Actor, ActorRef, ActorWeak};
use anyhow::Result;

#[derive(Debug)]
struct MyActor {
    // ... internal state
}

impl Actor for MyActor {
    type Args = (); // Arguments needed to create the actor
    type Error = anyhow::Error; // Error type for lifecycle methods

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // Initialize and return the actor instance
        println!("MyActor (id: {}) started with args: {:?}", actor_ref.identity(), args);
        Ok(MyActor { /* ... initial state ... */ })
    }

    async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        // Optional: Idle handler called when the message queue is empty.
        // Return Ok(true) to continue calling on_run, Ok(false) to stop idle processing.
        println!("MyActor (id: {}) idle processing.", actor_weak.identity());
        Ok(false) // Stop idle processing, only handle messages
    }

    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        // Optional: Called when the actor is stopping.
        // `killed` indicates whether the actor was stopped gracefully (false) or killed (true).
        if killed {
            println!("MyActor (id: {}) was killed.", actor_weak.identity());
        } else {
            println!("MyActor (id: {}) is stopping gracefully.", actor_weak.identity());
        }
        Ok(())
    }
}
```

*   **`Args`**: An associated type defining the arguments required by the `on_start` method to initialize the actor.
*   **`Error`**: An associated type for the error type that lifecycle methods can return. Must implement `Send + Debug`.
*   **`on_start`**: A required asynchronous method called when the actor is first created and started. It receives the initialization arguments and an `ActorRef` to itself. It should return a `Result` containing the initialized actor instance or an error.
*   **`on_run`**: An optional idle handler called when the message queue is empty. It receives an `ActorWeak` (weak reference). Return `Ok(true)` to continue calling `on_run`, or `Ok(false)` to stop idle processing. Defaults to `Ok(false)`.
*   **`on_stop`**: An optional cleanup method. It receives an `ActorWeak` and a `killed` boolean indicating whether termination was graceful (`false`) or forced via `kill()` (`true`).

By encapsulating state and behavior, actors provide a powerful model for building concurrent and resilient applications.
