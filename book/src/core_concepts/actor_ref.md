## Actor References (`ActorRef`)

An `ActorRef<A>` is a handle or a reference to an actor of type `A`. It is the primary way to interact with an actor from outside. You cannot directly access an actor's state or call its methods. Instead, you send messages to it via its `ActorRef`.

### Key Features of `ActorRef`:

*   **Message Sending**: `ActorRef` provides methods like `ask`, `tell`, `ask_with_timeout`, and `tell_with_timeout` to send messages to the associated actor.
*   **Type Safety**: `ActorRef<A>` is generic over the actor type `A`. This ensures that you can only send messages that the actor `A` is defined to handle, providing compile-time safety.
*   **Decoupling**: It decouples the sender of a message from the actor itself. Senders don't need to know the actor's internal implementation details, only the messages it accepts.
*   **Location Transparency (Conceptual)**: While `rsActor` is currently focused on in-process actors, the `ActorRef` concept is fundamental to actor systems and can be extended to support remote actors in the future. The reference itself abstracts away the actor's actual location.
*   **Lifecycle Management**: `ActorRef` also provides methods to manage the actor's lifecycle, such as `stop()` and `kill()`.
*   **Identity**: Each `ActorRef` has a unique `identity()` that can be used for logging or tracking purposes.

### Obtaining an `ActorRef`

An `ActorRef` is typically obtained when an actor is spawned:

```rust
use rsactor::{spawn, Actor, ActorRef, Message, impl_message_handler};
use anyhow::Result;

#[derive(Debug)]
struct MyActor;
impl Actor for MyActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(MyActor)
    }
}

// Dummy message for demonstration
struct PingMsg;
impl Message<PingMsg> for MyActor {
    type Reply = ();
    async fn handle(&mut self, _msg: PingMsg, _actor_ref: &ActorRef<Self>) -> Self::Reply {}
}
impl_message_handler!(MyActor, [PingMsg]);

#[tokio::main]
async fn main() {
    let (actor_ref, join_handle) = spawn::<MyActor>(());
    // actor_ref is an ActorRef<MyActor>
    // You can now use actor_ref to send messages to MyActor

    actor_ref.tell(PingMsg).await.unwrap();
    actor_ref.stop().await.unwrap();
    join_handle.await.unwrap();
}
```

### Type Safety

`rsActor` provides compile-time type safety through `ActorRef<A>`. This ensures that only messages that the actor can handle are sent, and reply types are correctly typed at compile time.

The type-safe approach prevents runtime type errors and provides better IDE support with autocomplete and type checking. All actor communication should use `ActorRef<A>` for the best development experience and runtime safety.

`ActorRef<A>` is cloneable and can be safely shared across tasks.
