## Messages

Messages are the sole means of communication between actors in `rsActor`. They are plain Rust structs or enums that carry data from a sender to a recipient actor.

### Defining Messages

A message can be any struct or enum. It's a common practice to define messages specific to the interactions an actor supports.

```rust
// Example message structs
struct GetData { id: u32 }
struct UpdateData { id: u32, value: String }

// Example message enum
enum CounterCommand {
    Increment(u32),
    Decrement(u32),
    GetValue,
}
```

### The `Message<T>` Trait

For an actor to handle a specific message type `T`, the actor must implement the `Message<T>` trait. This trait defines how the actor processes the message and what kind of reply (if any) it produces.

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler};
use anyhow::Result;

#[derive(Debug)]
struct DataStore { content: String }

impl Actor for DataStore {
    type Args = String;
    type Error = anyhow::Error;
    async fn on_start(initial_content: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(DataStore { content: initial_content })
    }
}

// Message to retrieve current content
struct GetContent;

impl Message<GetContent> for DataStore {
    type Reply = String;

    async fn handle(&mut self, _: GetContent, _: &ActorRef<Self>) -> Self::Reply {
        self.content.clone()
    }
}

// Message to update content
struct SetContent(String);

impl Message<SetContent> for DataStore {
    type Reply = (); // No direct reply needed

    async fn handle(&mut self, msg: SetContent, _: &ActorRef<Self>) -> Self::Reply {
        self.content = msg.0;
    }
}

impl_message_handler!(DataStore, [GetContent, SetContent]);
```

Key components of the `Message<T>` trait implementation:

*   **`type Reply`**: An associated type that specifies the type of the value the actor will send back to the sender if the message was sent using `ask`. If the message doesn't warrant a direct reply (e.g., for `tell` operations), `()` can be used.
*   **`async fn handle(&mut self, msg: T, actor_ref: &ActorRef<Self>) -> Self::Reply`**: This asynchronous method contains the logic for processing the message `msg` of type `T`.
    *   It takes a mutable reference to the actor's state (`&mut self`), allowing it to modify the actor's internal data.
    *   It also receives a reference to the actor's own `ActorRef`, which can be useful for various purposes, such as spawning child actors or sending messages to itself.
    *   It must return a value of type `Self::Reply`.

### Message Immutability

Once a message is sent, it should be considered immutable by the sender. The actor receiving the message effectively takes ownership of the data within the message (or a copy of it).

### Message Design Principles:

*   **Clarity**: Messages should clearly represent the intended operation or event.
*   **Granularity**: Design messages that are neither too coarse (bundling unrelated operations) nor too fine-grained (leading to chatty communication).
*   **Immutability**: Prefer messages that are immutable or contain immutable data to avoid shared mutable state issues.
*   **Serializability (for future/distributed systems)**: While `rsActor` is in-process, if you ever plan to distribute your actors, designing messages that can be serialized (e.g., using Serde) is a good practice.

By using well-defined messages and implementing the `Message<T>` trait, you create a clear and type-safe communication protocol for your actors.
