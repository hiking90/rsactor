# rsActor FAQ

This FAQ provides answers to common questions about the `rsActor` framework.

## General

**Q1: What is rsActor?**

A1: `rsActor` is a lightweight, Tokio-based actor framework for Rust. It aims to provide a simple and easy-to-use solution for building concurrent applications using the actor model, focusing on local, in-process actor systems.

**Q2: What are the main design goals or philosophy behind rsActor?**

A2: The primary goal is simplicity and ease of use for in-process actor systems. It leverages Tokio for its asynchronous runtime and provides core actor primitives without extensive boilerplate or features typically found in larger, distributed actor systems.

**Q3: How does rsActor compare to other Rust actor frameworks like Actix or Kameo?**

A3:
*   **Scope:** `rsActor` is designed for local, in-process actors only and does not support remote actors or clustering, unlike some more comprehensive frameworks.
*   **Simplicity:** It aims for a smaller API surface and less complexity compared to frameworks like Actix.
*   **Features:** As mentioned in the `README.md`, compared to Kameo, `rsActor` uses a concrete `ActorRef` with runtime type checking for replies, does not include built-in actor linking or supervision, is tightly coupled with Tokio, and uses the `impl_message_handler!` macro to simplify message handler boilerplate.
*   **Error Handling:** Error handling is primarily through the framework's own `Result<T>` type (which uses `rsactor::Error`) and the `ActorStopReason::Error` variant containing a `rsactor::Error`.

## Actor Definition and Usage

**Q4: How do I define an actor?**

A4: To define an actor, you need to:
1.  Create a struct for your actor's state.
2.  Implement the `Actor` trait for this struct. This involves defining an `Error` type and optionally implementing `on_start`, `run_loop`, and `on_stop` lifecycle hooks.
3.  For each specific message type (e.g., `PingRequest`) your actor will handle, you need to implement the `Message<PingRequest>` trait *for your actor struct*. This trait implementation involves defining an associated `Reply` type (e.g., `PongResponse`) and the `async fn handle(&mut self, message: PingRequest, ...)` method that dictates how the actor processes this specific message type.
4.  Use the `impl_message_handler!(YourActorType, [MessageType1, MessageType2]);` macro to generate the necessary boilerplate for routing messages to their respective handlers.

**Q5: How do I create and start an actor?**

A5: You use the `rsactor::spawn(your_actor_instance)` function. This function takes your actor instance, spawns a new Tokio task for it, and returns an `ActorRef` (to send messages to the actor) and a `tokio::task::JoinHandle` (to await the actor's termination).

**Q6: What is an `ActorRef`?**

A6: An `ActorRef` is a handle to an actor. It allows you to send messages to the actor (`ask`, `tell`, `ask_blocking`, `tell_blocking`), stop it (`stop`), or kill it (`kill`) without having direct access to the actor's instance or its state. `ActorRef`s are cloneable and can be shared across tasks.

**Q7: Is cloning an `ActorRef` expensive?**

A7: No, cloning an `ActorRef` is cheap. It involves cloning internal `mpsc::Sender` channels, which are designed for this purpose.

## Message Passing

**Q8: How do I send messages to an actor?**

A8: You use the methods on `ActorRef`:
*   `tell(message)`: Sends a message asynchronously without waiting for a reply (fire-and-forget).
*   `ask(message)`: Sends a message asynchronously and waits for a reply.
*   `tell_blocking(message, timeout)`: Sends a message synchronously from a blocking context (e.g., `tokio::task::spawn_blocking`).
*   `ask_blocking(message, timeout)`: Sends a message synchronously from a blocking context and waits for a reply.

**Q9: What's the difference between `ask`/`tell` and `ask_blocking`/`tell_blocking`? When should I use which?**

A9:
*   `ask` and `tell` are asynchronous methods. They should be used in `async` functions and tasks.
*   `ask_blocking` and `tell_blocking` are synchronous (blocking) methods. They are specifically designed for use within code running in a `tokio::task::spawn_blocking` task. This is useful when you need to interact with actors from CPU-bound code or synchronous code that is itself running within Tokio's blocking thread pool.

**Q10: How does message handling work? What is the role of `impl_message_handler!`?**

A10: When you send a message, it goes into the actor's mailbox. The actor's internal `Runtime` processes messages one by one. The `impl_message_handler!` macro generates code that implements the `MessageHandler` trait for your actor. This trait has a method that takes a type-erased message (`Box<dyn Any + Send>`), attempts to downcast it to one of the concrete message types your actor understands, and then calls the appropriate `Message::handle` method you defined.

**Q11: Are messages processed in order?**

A11: Yes, for a given actor instance, messages sent to its mailbox are processed sequentially in the order they are received.

**Q12: What happens if I send a message to a stopped or killed actor?**

A12: Sending a message to an actor whose mailbox channel is closed (which happens when it's stopped or killed) will result in an error. For example, `tell` would return `Err(...)` and `ask` would also return `Err(...)` indicating the failure to send or receive a reply.

**Q: Do the `tell()` or `ask()` methods in `ActorRef` not have a timeout feature? If needed, how can it be implemented?**

A: Yes, the standard `tell()` and `ask()` methods in `rsActor`'s `ActorRef` do not have direct timeout parameters.

If you need to apply a timeout to an `ask()` call, you can use the `tokio::time::timeout` function to wrap the `ask()` call.

Here is a simple example of applying a timeout to `ask()`:

```rust
use rsactor::{ActorRef, Message}; // ... (other necessary imports)
use std::time::Duration;
use anyhow::Result;

// ... (actor and message definitions)

async fn ask_with_timeout<M, R>(
    actor_ref: &ActorRef,
    message: M,
    timeout_duration: Duration,
) -> Result<R, anyhow::Error>
where
    M: Send + 'static,
    R: Send + 'static,
{
    match tokio::time::timeout(timeout_duration, actor_ref.ask(message)).await {
        Ok(Ok(reply)) => Ok(reply), // Successfully received response within timeout
        Ok(Err(e)) => Err(e), // Error occurred in ask itself (actor sent an error in response, or communication failed)
        Err(_) => Err(anyhow::anyhow!("Request timed out after {:?}", timeout_duration)), // Timeout occurred
    }
}

// Usage example:
// let reply: Result<MyReplyType, _> = ask_with_timeout(
//     &my_actor_ref,
//     MyMessage,
//     Duration::from_secs(5)
// ).await;
```

In the case of `tell()`, it's a "fire-and-forget" method, so a timeout is generally not necessary. The `tell()` call itself is an operation to send a message to the actor's mailbox, and it's rare for this operation to take a very long time. If you want to control the possibility of a `tell()` call blocking (e.g., if the mailbox is full), you might need to directly use `ActorRef`'s `sender.send_timeout()` (if `mpsc::Sender` provides that functionality) or similar Tokio features, but this is not exposed in `rsActor`'s standard `tell()` interface.

If you need to communicate with an actor within `tokio::task::spawn_blocking` and require a timeout, you can use `ActorRef`'s `ask_blocking(message, timeout)` and `tell_blocking(message, timeout)` methods. These methods directly support a timeout parameter.

## Actor Lifecycle and Termination

**Q13: How do I manage an actor's lifecycle?**

A13: The `Actor` trait provides three lifecycle hooks:
*   `on_start(&mut self, actor_ref: ActorRef)`: Called when the actor is started, before it begins processing messages. Useful for initialization.
*   `run_loop(&mut self, actor_ref: ActorRef)`: Called after `on_start` and contains the main execution logic of the actor. It runs for the entire lifetime of the actor. If this method returns `Ok(())`, the actor will stop normally. If it returns `Err(_)`, the actor will stop due to an error.
*   `on_stop(&mut self, actor_ref: ActorRef, reason: &ActorStopReason)`: Called when the actor is about to stop. Useful for cleanup. The `reason` argument indicates why the actor is stopping.

**Q14: How do I stop an actor? What's the difference between `stop()` and `kill()`?**

A14:
*   `actor_ref.stop().await`: Sends a `StopGracefully` signal. The actor will process all messages currently in its mailbox and then shut down. Its `on_stop` hook will be called with `ActorStopReason::Normal` (if no errors occurred).
*   `actor_ref.kill()`: Sends an immediate `Terminate` signal. The actor will attempt to stop as soon as possible, potentially interrupting its current task and not processing further messages from its main mailbox. Its `on_stop` hook will be called with `ActorStopReason::Killed`.

The `kill` signal is sent over a dedicated, prioritized channel to ensure it can be delivered even if the actor's main mailbox is full.

**Q15: What is `ActorStopReason`?**

A15: `ActorStopReason` is an enum that indicates why an actor stopped. Its variants are:
*   `Normal`: The actor stopped gracefully (e.g., via `stop()` or if its `ActorRef` was dropped and no more messages could arrive).
*   `Killed`: The actor was terminated by a `kill()` signal.
*   `Error(rsactor::Error)`: The actor stopped due to an error (e.g., an error returned from `on_start`, `on_stop`, or `run_loop`).

**Q16: What is the purpose of the `JoinHandle<(T, ActorStopReason)>` returned by `spawn`?**

A16: The `JoinHandle` allows you to await the termination of the actor's task. When the actor stops, the `JoinHandle` resolves to a tuple containing the actor's final state (instance `T`) and the `ActorStopReason` indicating why it stopped. This is useful for cleanup, retrieving final state, or ensuring actors have shut down properly.

## Error Handling

**Q17: How are errors handled in actors?**

A17:
*   **Message Handling:** The `Message<M>::handle` method returns a value of type `Self::Reply` which is not necessarily a `Result`. If the `MessageHandler::handle` implementation (generated by the `impl_message_handler!` macro) encounters an error, such as when trying to handle an unhandled message type, this error will be propagated back to the caller of `ask`. For `tell`, errors from message handling are logged but not returned to the sender since `tell` doesn't wait for a reply.
*   **Lifecycle Hooks:** If `on_start` returns an error, the actor will fail to start, and the actor will immediately stop with `ActorStopReason::Error` containing a `Lifecycle` error that wraps the original error (note that `on_stop` isn't called in this case). If `run_loop` returns an error, the actor will stop with `ActorStopReason::Error` and `on_stop` will be called. If `on_stop` itself returns an error, this will be reflected in the final `ActorStopReason::Error` (wrapping the original reason, if there was one).
*   **Panics:** If a message handler panics, the `rsActor` runtime does not currently have explicit panic handling that converts panics into `ActorStopReason::Error`. A panic in an actor task will typically cause that task to terminate, and the `JoinHandle` would yield an `Err` with a panic error when awaited. In debug builds, the framework will deliberately panic when encountering unhandled message types to help developers identify errors. It's generally recommended to handle errors gracefully within your actor logic and return `Result` types.

## Configuration and Advanced Topics

**Q18: What is the actor mailbox capacity and can I configure it?**

A18: The mailbox is an MPSC channel that holds incoming messages for an actor.
*   There's a default capacity (`DEFAULT_MAILBOX_CAPACITY`, which is 32).
*   You can set a global default mailbox capacity once using `set_default_mailbox_capacity(size)`.
*   You can also specify a custom mailbox capacity for an individual actor when spawning it using `spawn_with_mailbox_capacity(actor, capacity)`.

**Q19: Does rsActor support actor supervision or linking?**

A19: No, `rsActor` currently does not have built-in support for actor supervision hierarchies or direct linking of actors in the way some other actor systems (like Erlang/OTP or Akka) do. You would need to implement such patterns manually if required, for example, by having one actor monitor the `JoinHandle` of another.

**Q20: Can actors communicate with each other?**

A20: Yes. Actors can hold `ActorRef`s to other actors and send messages to them just like any other part of your application can.

**Q21: Is rsActor suitable for distributed systems?**

A21: No, `rsActor` is designed for local, in-process actor systems only. It does not provide features for network transparency or communication between actors in different processes or on different machines.

**Q22: Can I use generic types with actors?**

A22: Yes, `rsActor` supports generic actors. You can define an actor with generic type parameters:

```rust
struct GenericActor<T: Send + 'static> {
    value: T,
}

impl<T: Send + 'static> Actor for GenericActor<T> {
    type Error = anyhow::Error;
}

impl<T: Send + Clone + 'static> Message<GetValueMsg> for GenericActor<T> {
    type Reply = T;
    async fn handle(&mut self, _msg: GetValueMsg, _: &ActorRef) -> Self::Reply {
        self.value.clone()
    }
}
```

However, due to the nature of Rust's macro system, the `impl_message_handler!` macro must be called with concrete type instances rather than with type parameters:

```rust
// This won't work:
// impl_message_handler!(GenericActor<T>, [GetValueMsg]);

// Instead, do this for each concrete type you need:
impl_message_handler!(GenericActor<u32>, [GetValueMsg]);
impl_message_handler!(GenericActor<String>, [GetValueMsg]);
```

The limitation exists because macros in Rust are expanded at compile time before type checking occurs, so they can't directly work with generic type parameters.

To use generic actors:
1. Define your actor struct with appropriate type parameters
2. Implement the `Actor` trait generically
3. Implement message handling for the generic actor
4. Call `impl_message_handler!` specifically for each concrete type instantiation you need
5. When creating and spawning actors, use concrete types

```rust
// Example usage:
let actor = GenericActor::new(123u32);
let (actor_ref, handle) = spawn(actor);
let reply: u32 = actor_ref.ask(GetValueMsg).await.expect("ask failed");
```

**Q23: How can I effectively use the `run_loop` method in my actors?**

A23: The `run_loop` method is a powerful feature of the `rsActor` framework that provides a clean way to implement continuous or periodic tasks within an actor. It is called after `on_start` and runs for the entire lifetime of the actor. Here's how to use it effectively:

*   **Continuous Processing:** The `run_loop` method is ideal for implementing continuous processing logic that should run throughout the actor's lifetime. Unlike spawning separate tasks, work in the `run_loop` is part of the actor's main execution flow.

*   **Periodic Tasks:** You can implement periodic tasks by using `tokio::time` utilities within the `run_loop`. For example:

    ```rust
    async fn run_loop(&mut self, actor_ref: &ActorRef) -> Result<(), Self::Error> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            // Perform periodic work here
        }
    }
    ```

*   **Graceful Termination:** When you want the actor to stop normally, you can break out of the loop in `run_loop` or simply return `Ok(())`. This will trigger the normal shutdown sequence, calling `on_stop` with `ActorStopReason::Normal`.

*   **Error Handling:** If the `run_loop` returns an error (`Err(_)`), the actor will stop and `on_stop` will be called with `ActorStopReason::Error`.

*   **Integration with Message Handling:** The `run_loop` runs concurrently with message processing. The actor will continue to handle messages from its mailbox while the `run_loop` is executing. This allows for a nice separation of concerns where long-running tasks are in the `run_loop` and message-specific logic is in the message handlers.

**Important Considerations:**

*   **Responsiveness:** The `run_loop` and message handlers share the same execution context. This means that if your `run_loop` doesn't yield control by using `.await` points, it can block message processing. Always ensure your loop includes sufficient `.await` points.

*   **State Sharing:** Both the `run_loop` and message handlers have access to the actor's state (`self`), allowing them to share data without additional synchronization mechanisms.

*   **CPU-Bound or Blocking Work:**
    *   For CPU-bound or blocking I/O operations that would otherwise block the Tokio runtime, you **must** use `tokio::task::spawn_blocking` even within the `run_loop`.
    *   Directly performing blocking operations in the `run_loop` will block the Tokio worker thread that the actor is running on, making the actor unresponsive to messages and potentially affecting other tasks.
    *   When using `spawn_blocking` from the `run_loop`, you can `.await` its result directly:

        ```rust
        async fn run_loop(&mut self, actor_ref: &ActorRef) -> Result<(), Self::Error> {
            loop {
                // Offload CPU-intensive or blocking I/O work
                let result = tokio::task::spawn_blocking(|| {
                    // Perform CPU-bound or blocking I/O work
                    // Return the result
                }).await?;

                // Process the result
            }
        }
        ```

*   **Coordination with `on_stop`:** If your `run_loop` spawns tasks or acquires resources, ensure they are properly managed when the actor stops. You can use flags (like `self.running` in the example above) to signal the `run_loop` to gracefully terminate when the actor receives a stop signal.

In summary, the `run_loop` method provides an elegant way to implement continuous or periodic tasks within an actor without spawning separate Tokio tasks. It runs as part of the actor's main execution flow, has access to the actor's state, and can use the full range of async features available in the Tokio ecosystem. By using the `run_loop`, you often eliminate the need to spawn separate tasks from an actor's methods, resulting in cleaner and more manageable actor implementations.

---

*This FAQ is based on the state of the `rsActor` project as of its `README.md` and `src/lib.rs` on May 17, 2025. Features and behaviors may change in future versions.*
