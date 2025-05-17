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
*   **Error Handling:** Error handling is primarily through `anyhow::Result` and the `ActorStopReason::Error` variant.

## Actor Definition and Usage

**Q4: How do I define an actor?**

A4: To define an actor, you need to:
1.  Create a struct for your actor's state.
2.  Implement the `Actor` trait for this struct. This involves defining an `Error` type and optionally implementing `on_start` and `on_stop` lifecycle hooks.
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

## Actor Lifecycle and Termination

**Q13: How do I manage an actor's lifecycle?**

A13: The `Actor` trait provides two lifecycle hooks:
*   `on_start(&mut self, actor_ref: ActorRef)`: Called when the actor is started, before it begins processing messages. Useful for initialization.
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
*   `Error(anyhow::Error)`: The actor stopped due to an error (e.g., a panic in a message handler, or an error returned from `on_start` or `on_stop`).

**Q16: What is the purpose of the `JoinHandle<(T, ActorStopReason)>` returned by `spawn`?**

A16: The `JoinHandle` allows you to await the termination of the actor's task. When the actor stops, the `JoinHandle` resolves to a tuple containing the actor's final state (instance `T`) and the `ActorStopReason` indicating why it stopped. This is useful for cleanup, retrieving final state, or ensuring actors have shut down properly.

## Error Handling

**Q17: How are errors handled in actors?**

A17:
*   **Message Handling:** If the `handle` method of your `Message<M>` trait implementation returns an `Err` (assuming its `Reply` type is a `Result`), this error will be propagated back to the caller of `ask`. For `tell`, the error is effectively ignored by the sender but will be logged by the actor runtime if it's an `anyhow::Error` from the `MessageHandler` trait itself.
*   **Lifecycle Hooks:** If `on_start` returns an error, the actor will fail to start, and `on_stop` will be called with `ActorStopReason::Error`. If `on_stop` itself returns an error, this will also be reflected in the final `ActorStopReason` (potentially wrapping the original reason).
*   **Panics:** If a message handler panics, the `rsActor` runtime does not currently have explicit panic handling that converts panics into `ActorStopReason::Error`. A panic in an actor task will typically cause that task to terminate, and the `JoinHandle` would yield an error when awaited. It's generally recommended to handle errors gracefully within your actor logic and return `Result` types.

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

**Q22: When might I need to spawn a new Tokio task from within an actor (e.g., in `on_start` or a message handler `async fn handle`)?**

A22: While an actor itself runs within a dedicated Tokio task, and its message handlers (`async fn handle`) or lifecycle methods (`on_start`, `on_stop`) are executed as part of this main actor task, there are scenarios where you might want to spawn additional Tokio tasks from within these actor methods:

*   **Performing Long-Running, Non-Blocking Operations Concurrently:** If an actor method (like `on_start` for initial setup, or `handle` for processing a message) needs to initiate a long-running operation that is itself asynchronous and non-blocking (e.g., making multiple independent network requests, complex calculations that can be broken down), spawning new tasks for these operations allows the actor method to return quickly. This is crucial for `handle` to keep the actor's mailbox from being blocked, enabling responsiveness. For `on_start`, it might mean the actor becomes "ready" (from `on_start`'s perspective) sooner, while background initialization continues.
*   **Offloading Work to Avoid Blocking Actor Methods:** If an operation within `on_start` or `handle` might take a significant amount of time, even if asynchronous, spawning it in a separate task ensures the method itself completes swiftly. The actor can then proceed (e.g., start processing messages after `on_start`, or process other messages if in `handle`). The spawned task can later send a message back to the actor (or another actor) with its result or status.
*   **Fire-and-Forget Background Tasks:** For operations initiated from any actor method where the actor doesn't need to wait for a direct reply before continuing its primary function, spawning a task is a good way to offload the work.
*   **Parallelism within an Actor Method:** If a single message or an initialization step requires performing several independent asynchronous sub-tasks, `tokio::spawn` can be used to run them concurrently. Tools like `tokio::join!` or `futures::future::join_all` can then be used to await their collective completion if the actor method needs these results before proceeding.

**Important Considerations:**
*   **Task Lifecycle Management:** If you spawn tasks, consider how their lifecycle is managed. Do they need to be cancelled if the actor stops? How are their results or errors handled? Often, spawned tasks might send a new message back to the originating actor (or another designated actor) upon completion or error. This is particularly important for tasks spawned from `on_start` – if they are critical for the actor's function, their failure might necessitate stopping the actor.
*   **Resource Management:** Spawning an excessive number of tasks without control can lead to resource exhaustion. Ensure your design doesn't lead to unbounded task creation.
*   **`ActorRef` Cloning:** Remember to clone the `ActorRef` if the spawned task needs to communicate back with the actor or other actors.
*   **Error Handling:** Errors occurring in tasks spawned via `tokio::spawn` operate in their own context and won't automatically propagate to the actor's main error handling mechanisms or influence its `ActorStopReason`. You must explicitly design how these errors are managed. Common patterns include having the spawned task send an error message back to the originating actor (or another designated error-handling actor), or if a critical task spawned from `on_start` fails, the actor might be designed to stop itself upon receiving notification of this failure.
*   **CPU-Bound or Blocking I/O Work (Using `spawn_blocking`):**
    *   If the work you need to perform from an actor method is CPU-bound (e.g., complex calculations that don\'t yield control to the Tokio runtime) or involves blocking I/O operations (e.g., traditional file system operations, some database drivers not designed for async), you **must** offload this work to Tokio\'s blocking thread pool using `tokio::task::spawn_blocking`.
    *   Directly performing such blocking operations within an actor\'s asynchronous method (`on_start`, `handle`, `on_stop`) will block the Tokio worker thread that the actor is running on. This can lead to the actor becoming unresponsive and can even stall other tasks sharing the same worker thread, potentially degrading the performance of the entire Tokio runtime.
    *   `spawn_blocking` moves the blocking code to a separate thread pool designed for such tasks, allowing the main Tokio worker threads to continue processing other asynchronous operations, including the actor\'s mailbox.
    *   Once its blocking work is complete, the task spawned with `spawn_blocking` can communicate its result back to an actor. If the blocking task itself needs to send a message synchronously, it can use `actor_ref.tell_blocking` or `actor_ref.ask_blocking`. Alternatively, and more commonly, the `Future` returned by `spawn_blocking` is `.await`ed in an asynchronous context (e.g., within the actor method that called `spawn_blocking`, or a new task), and then a standard asynchronous message (`actor_ref.tell` or `actor_ref.ask`) is sent with the result.

In summary, spawn new Tokio tasks (`tokio::spawn`) from an actor\'s methods for concurrent, non-blocking asynchronous operations. Use `tokio::task::spawn_blocking` for CPU-Bound or blocking I/O operations to prevent stalling the Tokio runtime and ensure actor responsiveness.

---

*This FAQ is based on the state of the `rsActor` project as of its `README.md` and `src/lib.rs` on May 17, 2025. Features and behaviors may change in future versions.*
