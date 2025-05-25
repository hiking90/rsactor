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
*   **Error Handling:** Error handling is primarily through the framework's own `Result<T>` type (which uses `rsactor::Error`) and the `ActorResult` enum which indicates startup or runtime failures.

## Actor Definition and Usage

**Q4: How do I define an actor?**

A4: To define an actor, you need to:
1.  Create a struct for your actor's state.
2.  Define a struct or tuple for your actor's initialization arguments (this will be `Actor::Args`).
3.  Implement the `Actor` trait for your state struct. This involves:
    *   Defining an associated type `Args` (the type of arguments your `on_start` method will take).
    *   Defining an associated type `Error` for errors that can occur during the actor's lifecycle.
    *   Implementing `async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error>`: This method is called when the actor is spawned. It receives the initialization arguments and an `ActorRef` to itself. It's responsible for creating and returning the actor instance (`Ok(Self)`) or an error if initialization fails.
    *   Implementing `async fn on_run(&mut self, actor_ref: ActorRef<Self>) -> Result<(), Self::Error>`: This method contains the main execution logic of the actor and runs for its lifetime after `on_start` succeeds. If it returns `Ok(())`, the actor continues running. If it returns `Err(_)`, the actor stops due to an error.
4.  For each specific message type (e.g., `PingRequest`) your actor will handle, you need to implement the `Message<PingRequest>` trait *for your actor struct*. This trait implementation involves defining an associated `Reply` type (e.g., `PongResponse`) and the `async fn handle(&mut self, message: PingRequest, ...)` method that dictates how the actor processes this specific message type.
5.  Use the `impl_message_handler!(YourActorType, [MessageType1, MessageType2]);` macro to generate the necessary boilerplate for routing messages to their respective handlers.

**Q5: How do I create and start an actor?**

A5: You use the `rsactor::spawn(args)` function. This function takes the arguments needed for your actor\'s `on_start` method, spawns a new Tokio task for it, and returns an `ActorRef` (to send messages to the actor) and a `tokio::task::JoinHandle<ActorResult<YourActorType>>` (to await the actor\'s termination and get the `ActorResult`).

**Q5a: Why is the `Actor` instance created inside `on_start` instead of being passed to `spawn` directly?**

A5a: Delaying the creation of the `Actor` instance until the `on_start` method offers a significant advantage in terms of struct design and ergonomics.

*   **Problem with Pre-Creation:** If you were to create the `Actor` instance *before* calling `spawn`, any member fields that can only be initialized *during* the actor\'s startup phase (e.g., resources allocated, connections established, or data derived from `ActorRef` which isn\'t available pre-spawn) would need to be declared as `Option<T>`. This is because their values wouldn\'t be known at the moment of the initial struct instantiation. This can lead to a proliferation of `Option<T>` fields and require frequent unwrapping or matching throughout the actor\'s logic, making the code more verbose and error-prone.

*   **Benefit of `on_start` Creation:** By creating the actual `Actor` instance *inside* `on_start`, you have access to the `ActorRef<Self>` (if needed for initialization) and can perform all necessary setup logic *before* the struct is fully constructed. This means that member fields can be initialized with their concrete values directly, reducing the need for `Option<T>` for state that is determined at startup. The `on_start` method effectively becomes the true constructor of the actor, ensuring that by the time the actor instance exists, it is in a fully initialized and valid state. This leads to cleaner, more straightforward actor struct definitions and more convenient development.

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

A12: Sending a message to an actor whose mailbox channel is closed (which happens when it's stopped or killed) will result in an error. For example, `tell` would return `Err(rsactor::Error::MailboxClosed)` and `ask` would also return an `Err` (e.g., `rsactor::Error::MailboxClosed` or `rsactor::Error::AskTimeout` if a timeout occurs) indicating the failure to send or receive a reply.

## Actor Lifecycle and Termination

**Q13: How do I manage an actor's lifecycle?**

A13: The `Actor` trait provides two main lifecycle hooks:
*   `on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error>`: Called when the actor is spawned. It receives initialization arguments (`Self::Args`) and is responsible for creating and returning the actor instance (`Self`). If it returns an `Err`, the actor fails to start, resulting in `ActorResult::Failed` with `phase: FailurePhase::OnStart`.
*   `on_run(&mut self, actor_ref: ActorRef<Self>) -> Result<(), Self::Error>`: Called after `on_start` succeeds. This method contains the main execution logic of the actor and runs concurrently with message handling.
    *   If `on_run` returns `Ok(())`, the actor continues running and `on_run` will be called again.
    *   If `on_run` returns `Err(e)`, the actor terminates due to a runtime error, resulting in `ActorResult::Failed` with `phase: FailurePhase::OnRun`.
    *   To stop the actor normally from within `on_run`, call `actor_ref.stop().await` or `actor_ref.kill()`.

There is also an `on_stop` hook: `on_stop(&mut self, actor_ref: ActorRef<Self>, killed: bool) -> Result<(), Self::Error>` which is called before the actor terminates, with the `killed` parameter indicating whether the actor was killed (true) or stopped gracefully (false).

**Q14: How do I stop an actor? What's the difference between `stop()` and `kill()`?**

A14:
*   `actor_ref.stop().await`: Sends a `StopGracefully` signal. The actor will process all messages currently in its mailbox, and then `on_stop` will be called with `killed: false`, after which the actor terminates. The `JoinHandle` will then resolve to `ActorResult::Completed { actor, killed: false }`.
*   `actor_ref.kill()`: Sends an immediate `Terminate` signal via a prioritized channel. The actor will attempt to stop as soon as possible. `on_stop` will be called with `killed: true`, after which the actor terminates. Any unprocessed messages in the main mailbox may be discarded. The `JoinHandle` will resolve to `ActorResult::Completed { actor, killed: true }`.

**Q15: What is `ActorResult`?**

A15: `ActorResult<T: Actor>` is an enum that indicates how an actor's lifecycle concluded. Its variants are:
*   `Completed { actor: T, killed: bool }`: The actor finished its execution. `actor` contains the final state of the actor. `killed` is `true` if termination was due to `actor_ref.kill()`, `false` otherwise (e.g. graceful stop or all `ActorRef`s dropped).
*   `Failed { actor: Option<T>, error: T::Error, phase: FailurePhase, killed: bool }`: The actor failed during one of its lifecycle phases. `actor` may contain the actor's state at the time of failure (if it's retrievable), `error` contains the error, `phase` indicates which lifecycle phase failed (OnStart, OnRun, or OnStop), and `killed` indicates if the actor was being killed when the failure occurred.

**Q16: What is the purpose of the `JoinHandle<ActorResult<T>>` returned by `spawn`?**

A16: The `JoinHandle` allows you to await the termination of the actor's task. When the actor stops, the `JoinHandle` resolves to an `ActorResult<T>` (where `T` is your actor type). This `ActorResult` provides the final state of the actor (if applicable) and information about how and why it stopped. This is useful for cleanup, retrieving final state, or ensuring actors have shut down properly.

## Error Handling

**Q17: How are errors handled in actors?**

A17:
*   **Message Handling:** The `Message<M>::handle` method returns a value of type `Self::Reply`. If this reply type is a `Result`, errors can be propagated back to the caller of `ask`. If the `MessageHandler::handle` implementation (generated by the `impl_message_handler!` macro) encounters an error (e.g., trying to handle an unhandled message type), this error will be propagated back to the caller of `ask`. For `tell`, errors from message handling are typically logged by the actor itself if necessary, as `tell` doesn't wait for a reply.
*   **Lifecycle - `on_start`:** If `on_start` returns `Err(e)`, the actor will not start, and the `JoinHandle` will resolve to `ActorResult::Failed { cause: e, phase: FailurePhase::OnStart, .. }`.
*   **Lifecycle - `on_run`:** If `on_run` returns `Err(e)`, the actor will terminate, and the `JoinHandle` will resolve to `ActorResult::Failed { actor, cause: e, phase: FailurePhase::OnRun, .. }`. The `actor` field may contain the actor's state.
*   **Panics:** If a message handler or `on_run` panics, the Tokio task hosting the actor will terminate. Awaiting the `JoinHandle` will then result in an `Err` (typically a `tokio::task::JoinError` indicating a panic). It's generally recommended to handle errors gracefully within your actor logic and return `Result` types from `on_start` and `on_run`, and use `Result` as reply types for messages where appropriate, rather than relying on panics.

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
use rsactor::{Actor, ActorRef, Message, ActorResult, spawn}; // Added ActorResult and spawn
use std::fmt::Debug; // For deriving Debug on actor state if needed for ActorResult

#[derive(Debug)] // Example: derive Debug if T is Debug and you want to see it in ActorResult
struct GenericActor<T: Send + Debug + 'static> {
    value: T,
}

// Define Args for the generic actor
struct GenericActorArgs<T> {
    initial_value: T,
}

impl<T: Send + Debug + 'static> Actor for GenericActor<T> {
    type Args = GenericActorArgs<T>; // Use the new Args struct
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { value: args.initial_value })
    }

    async fn on_run(&mut self, _actor_ref: ActorRef<Self>) -> Result<(), Self::Error> {
        // Basic on_run, keeps actor alive until stopped
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(())
    }
}

// Define a sample message
struct GetValueMsg;

impl<T: Send + Clone + Debug + 'static> Message<GetValueMsg> for GenericActor<T> {
    type Reply = Result<T, anyhow::Error>; // Reply is often a Result
    async fn handle(&mut self, _msg: GetValueMsg, _actor_ref: ActorRef<Self>) -> Self::Reply {
        Ok(self.value.clone())
    }
}
```

To use generic actors:
1. Define your actor struct with appropriate type parameters.
2. Define an `Args` struct for its initialization, possibly also generic.
3. Implement the `Actor` trait generically, including `on_start` and `on_run`.
4. Implement message handling for the generic actor.
5. Call `impl_message_handler!` specifically for each concrete type instantiation you need.
6. When creating and spawning actors, provide the concrete `Args` type.

```rust
// Example usage:
// Assuming GetValueMsg and impl_message_handler!(GenericActor<u32>, [GetValueMsg]); are defined
async fn run_generic_actor() {
    let actor_args = GenericActorArgs { initial_value: 123u32 };
    let actor_ref_join_handle = spawn(actor_args).await;

    if let Ok(actor_ref) = actor_ref_join_handle {
        let reply_result = actor_ref.ask(GetValueMsg).await;
        match reply_result {
            Ok(Ok(value)) => println!("GenericActor<u32> replied with: {}", value),
            Ok(Err(e)) => println!("GenericActor<u32> handler error: {}", e),
            Err(e) => println!("Failed to ask GenericActor<u32>: {}", e),
        }
        // Stop the actor
        let _ = actor_ref.stop().await;
    } else {
        println!("Failed to spawn GenericActor<u32>");
    }
}
```

**Q23: How can I effectively use the `on_run` method in my actors?**

A23: The `on_run` method is a powerful feature of the `rsActor` framework that provides a clean way to implement continuous or periodic tasks within an actor. It is called after `on_start` and runs for the entire lifetime of the actor. Here's how to use it effectively:

*   **Continuous Processing:** The `on_run` method is ideal for implementing continuous processing logic that should run throughout the actor's lifetime. Unlike spawning separate tasks, work in the `on_run` is part of the actor's main execution flow.

*   **Periodic Tasks:** You can implement periodic tasks by using `tokio::time` utilities within the `on_run`. `tokio::select!` makes it easy to handle multiple timers concurrently. For example, first define your actor struct with interval fields:

    ```rust
    struct MyActor {
        // ... other fields ...
        fast_interval: tokio::time::Interval,
        slow_interval: tokio::time::Interval,
    }

    // Initialize intervals in on_start
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(MyActor {
            // ... initialize other fields ...
            fast_interval: tokio::time::interval(std::time::Duration::from_millis(500)),
            slow_interval: tokio::time::interval(std::time::Duration::from_secs(5)),
        })
    }

    // Use the intervals in on_run without loop
    async fn on_run(&mut self, actor_ref: ActorRef<Self>) -> Result<(), Self::Error> {
        tokio::select! {
            _ = self.fast_interval.tick() => {
                // Handle high-frequency tasks (every 500ms)
                self.process_high_frequency_work();
            }
            _ = self.slow_interval.tick() => {
                // Handle low-frequency tasks (every 5 seconds)
                self.process_low_frequency_work().await?;
            }
            // You can add more branches as needed, including channels, futures, etc.
        }
        Ok(())
    }
    ```

*   **Graceful Termination:** When you want the actor to stop normally, you can call `actor_ref.stop().await` from within the `on_run` method. This will trigger the normal shutdown sequence, and the `JoinHandle` will resolve to `ActorResult::Completed { killed: false, .. }`. The `on_run` method returns `Ok(())` to continue running.

*   **Error Handling:** If the `on_run` returns an error (`Err(e)`), the actor will stop, and the `JoinHandle` will resolve to `ActorResult::Failed { cause: e, .. }`.

*   **Integration with Message Handling:** The `on_run` runs concurrently with message processing. The actor will continue to handle messages from its mailbox while the `on_run` is executing. This allows for a nice separation of concerns where long-running tasks are in the `on_run` and message-specific logic is in the message handlers.

**Important Considerations:**

*   **Responsiveness:** The `on_run` and message handlers share the same execution context. This means that if your `on_run` doesn't yield control by using `.await` points, it can block message processing. The framework will call `on_run` repeatedly, so ensure each call includes sufficient `.await` points.

*   **State Sharing:** Both the `on_run` and message handlers have access to the actor's state (`self`), allowing them to share data without additional synchronization mechanisms.

*   **CPU-Bound or Blocking Work:**
    *   For CPU-bound or blocking I/O operations that would otherwise block the Tokio runtime, you **must** use `tokio::task::spawn_blocking` even within the `on_run`.
    *   Directly performing blocking operations in the `on_run` will block the Tokio worker thread that the actor is running on, making the actor unresponsive to messages and potentially affecting other tasks.
    *   When using `spawn_blocking` from the `on_run`, you can `.await` its result directly:

        ```rust
        async fn on_run(&mut self, actor_ref: ActorRef<Self>) -> Result<(), Self::Error> {
            // Offload CPU-intensive or blocking I/O work
            let result = tokio::task::spawn_blocking(|| {
                // Perform CPU-bound or blocking I/O work
                // Return the result
            }).await?;

            // Process the result
            // Return Ok(()) to continue running - the framework will call on_run again
            Ok(())
        }
        ```

*   **Coordination with Actor Termination:** If your `on_run` spawns tasks or acquires resources, ensure they are properly managed when the actor stops. The `on_run` method should be designed to exit cleanly when it detects that the actor is shutting down (e.g., its mailbox closes, or it receives a specific signal if you implement one). There is no `on_stop` hook; all cleanup must happen within `on_run` before it returns, or by the code that awaits the `JoinHandle` and processes the `ActorResult`.

In summary, the `on_run` method provides an elegant way to implement continuous or periodic tasks within an actor without spawning separate Tokio tasks. It runs as part of the actor's main execution flow, has access to the actor's state, and can use the full range of async features available in the Tokio ecosystem. By using the `on_run`, you often eliminate the need to spawn separate tasks from an actor's methods, resulting in cleaner and more manageable actor implementations.

## Type Safety

**Q26: What's the difference between `ActorRef<T>` and `UntypedActorRef`?**

A26: rsActor provides two types of actor references with different levels of type safety:

- **`ActorRef<T>`**: Provides compile-time type safety. The compiler ensures that only valid message types can be sent, and reply types are automatically inferred. This is the recommended default for most use cases.
- **`UntypedActorRef`**: Provides runtime type handling through type erasure. This allows storing different actor types in collections but requires developer responsibility for ensuring type safety at runtime.

**Q27: When should I use `UntypedActorRef`?**

A27: Use `UntypedActorRef` only when you specifically need type erasure:
- **Collections**: Storing different actor types in the same `Vec`, `HashMap`, etc.
- **Plugin Systems**: Managing actors loaded dynamically at runtime
- **Heterogeneous Actor Groups**: When you need to manage actors of different types uniformly

For normal actor communication, always prefer `ActorRef<T>` for its compile-time safety guarantees.

**Q28: What are the risks of using `UntypedActorRef`?**

A28: When using `UntypedActorRef`, you lose compile-time type safety:
- **Runtime Errors**: Sending wrong message types will result in runtime errors instead of compile-time errors
- **Developer Responsibility**: You must ensure message types match the target actor
- **Less IDE Support**: You lose autocomplete and type checking benefits

**Q29: How do I convert between `ActorRef<T>` and `UntypedActorRef`?**

A29: You can easily get an `UntypedActorRef` from an `ActorRef<T>`:

```rust
let (typed_ref, _) = spawn::<MyActor>(());
let untyped_ref: &UntypedActorRef = typed_ref.untyped_actor_ref();
```

**Note**: You cannot safely convert `UntypedActorRef` back to `ActorRef<T>` without additional type information and validation.

**Q30: What happens if I send the wrong message type to an `UntypedActorRef`?**

A30: Sending an incorrect message type will result in a runtime error. The actor's message handler will attempt to downcast the message to one of its supported types, and if no match is found, it will return an `Error::UnhandledMessageType` error. This error includes:
- The actor's identity
- List of expected message types
- The actual type that was sent

---

*This FAQ is based on the state of the `rsActor` project as of its `README.md` and `src/lib.rs` on May 25, 2025. Features and behaviors may change in future versions.*
