# rsActor Architecture

This document outlines the architecture of `rsActor`, a lightweight, Tokio-based actor framework in Rust focused on providing a simple and efficient actor model for local, in-process systems. It details key processes such as actor creation, message passing, and termination, with PlantUML sequence diagrams illustrating these processes.

## 1. Actor Creation and Spawning Process

This section describes the sequence of events when a user defines an actor and spawns it using `rsactor::spawn`.

**Key Components:**
*   **User Code:** Defines the actor struct, its initialization arguments (`Actor::Args`), and its behavior (implementing `Actor` and `Message<M>` traits).
*   **`spawn(args: Actor::Args)` function:** The entry point for creating and starting an actor. It takes the initialization arguments.
*   **`ActorRef<T>`:** A typed handle to the actor, used for sending messages and control signals with compile-time type safety.
*   **`ActorWeak<T>`:** A weak reference to the actor that doesn't prevent the actor from being terminated.
*   **`run_actor_lifecycle` function:** Manages the actor's complete lifecycle including initialization, message processing, and termination.
*   **Tokio Channels:** `mpsc::channel` for the actor's mailbox and a dedicated `mpsc::channel` for termination signals.
*   **Tokio Task:** The actor runs within its own asynchronous Tokio task.

**Diagram:**

![Actor Creation and Spawning](./Actor%20Creation%20and%20Spawning.png)

**Description:**
1.  The user defines their actor structure, its `Args` type, and implements the necessary `Actor` and `Message<M>` traits using either the derive macro or manual implementation.
2.  The user calls `rsactor::spawn(args)`, passing the initialization arguments for the actor.
3.  The `spawn()` function:
    a.  Generates a unique ID for the actor using atomic operations.
    b.  Creates two Tokio MPSC channels: one for regular messages (the mailbox) and another dedicated channel for termination signals. The termination channel has a small buffer (e.g., 1) to ensure kill signals can be delivered even if the main mailbox is full.
    c.  Instantiates an `ActorRef<T>` containing the actor's ID and the sender ends of both channels.
    d.  Calls `tokio::spawn` to run the `run_actor_lifecycle` function in a new asynchronous task, passing the actor arguments, `ActorRef`, and receiver ends of both channels.
    e.  Returns the `ActorRef<T>` and a `tokio::task::JoinHandle<ActorResult<A>>` to the user. The `JoinHandle` can be used to await the actor's termination and retrieve the `ActorResult`.
4.  Inside the spawned Tokio task, `run_actor_lifecycle` begins execution:
    a.  It first calls the actor's `A::on_start(args, &actor_ref)` lifecycle hook, passing the user-provided `args` and a reference to the `ActorRef`. This method is responsible for creating and returning the actor instance (`Ok(Self)`).
    b.  If `on_start()` returns `Ok(actor_instance)`, this instance is stored. The runtime then enters the main processing loop using `tokio::select!` to handle:
        - Messages from the mailbox
        - Termination signals (with bias priority)
        - The actor's `on_run()` method execution
    c.  The actor's `on_run()` method runs concurrently with message processing, implementing the actor's primary logic throughout its lifetime.
    d.  If `on_start()` fails (returns `Err(e)`), the actor fails to start. The `run_actor_lifecycle` function will return `ActorResult::Failed { actor: None, error: e, phase: FailurePhase::OnStart, killed: false }`.
    e.  When the actor terminates (due to `on_run()` errors, explicit stop/kill, or all references being dropped), the actor's `on_stop()` method is called before the `JoinHandle` resolves to an appropriate `ActorResult`.

## 2. Message Passing (tell/ask)

This section explains how messages are sent to an actor using `ActorRef::tell` (fire-and-forget) and `ActorRef::ask` (request-reply) and how they are processed.

**Key Components:**
*   **`ActorRef<T>`:** Used by the client to send messages with compile-time type safety.
*   **`MailboxMessage::Envelope`:** Wraps the user's message and an optional `oneshot::Sender` for replies (used by `ask`).
*   **Mailbox Channel (`mpsc::channel`):** The primary channel for delivering messages to the actor.
*   **`MessageHandler` trait (via `#[message_handlers]` macro):** Dynamically dispatches the type-erased message to the actor's specific `Message<M>::handle` method.
*   **`Message<M>::handle()`:** The user-defined method that processes the message.
*   **`oneshot::channel`:** Used by `ask` to receive a reply from the actor.

**Diagram:**

![Message Passing (tell and ask)](./Message%20Passing.png)

**Description:**
1.  **`tell(message)` (Fire-and-Forget):**
    a.  The client calls `actor_ref.tell(message)`.
    b.  `ActorRef::tell` wraps the `message` into a `MailboxMessage::Envelope` with the `reply_channel` field set to `None`.
    c.  The envelope is sent asynchronously to the actor's mailbox via the `mpsc::Sender` held by the `ActorRef`.
    d.  The actor's message loop, in its `tokio::select!` construct, receives the `MailboxMessage::Envelope` from its `mpsc::Receiver`.
    e.  The message loop uses the `MessageHandler` trait (implemented for the actor by the `#[message_handlers]` macro) to downcast the type-erased message payload and call the appropriate `Message<M>::handle` method on the actor instance.
    f.  The actor processes the message. Since it was a `tell`, no reply is sent back through a dedicated channel.

2.  **`ask(message)` (Request-Reply):**
    a.  The client calls `actor_ref.ask(message).await`.
    b.  `ActorRef::ask` first creates a `tokio::sync::oneshot` channel for the reply.
    c.  It then wraps the `message` into a `MailboxMessage::Envelope`, placing the sender part of the `oneshot` channel into the `reply_channel` field.
    d.  The envelope is sent to the actor's mailbox (same as `tell`).
    e.  `ActorRef::ask` then `.await`s on the receiver part of the `oneshot` channel.
    f.  The actor's message loop receives and dispatches the message to the actor's `Message<M>::handle` method as described above.
    g.  After the actor's `handle` method completes, the message loop takes the returned reply.
    h.  If a `reply_channel` (the `oneshot::Sender`) exists in the envelope, the message loop sends the reply (or an error if processing failed) through this `oneshot` channel.
    i.  The `ActorRef::ask` method, awaiting on the `oneshot::Receiver`, receives the reply, downcasts it to the expected type, and returns it to the client.

## 3. Actor Termination (stop/kill)

This section details how an actor is terminated using `ActorRef::stop` (graceful shutdown) or `ActorRef::kill` (immediate termination), and how this relates to `ActorResult`.

**Key Components:**
*   **`ActorRef<T>`:** Used by the client to initiate termination.
*   **`MailboxMessage::StopGracefully`:** A control message for graceful shutdown, sent via the main mailbox.
*   **`ControlSignal::Terminate`:** A control signal for immediate termination, sent via a dedicated, prioritized channel.
*   **Mailbox Channel & Terminate Channel:** Used to deliver these signals.
*   **Message Loop:** Handles these signals in its `tokio::select!` loop with biased priority for termination.
*   **`ActorResult<A>`:** An enum (`Completed`, `Failed`) indicating the outcome of the actor's lifecycle. The `Completed` variant includes the final actor state and a `killed` flag. The `Failed` variant includes error details, failure phase, and optional actor state.
*   **`JoinHandle<ActorResult<A>>`:** Resolves when the actor's task completes, providing the `ActorResult`.

**Diagram:**

![Actor Termination (stop and kill)](./Actor%20Termination.png)

**Description:**
1.  **`stop()` (Graceful Shutdown):**
    a.  The client calls `actor_ref.stop().await`.
    b.  `ActorRef::stop` sends a `MailboxMessage::StopGracefully(actor_ref)` message to the actor via its main mailbox channel.
    c.  The actor's message loop, in its `tokio::select!` construct, receives `StopGracefully` or `None` (when the mailbox is closed).
    d.  The message loop calls the actor's `on_stop(&actor_weak, false)` method to allow cleanup.
    e.  The message loop is terminated, and the actor task prepares to finish.
    f.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: false }`.

2.  **`kill()` (Immediate Termination):**
    a.  The client calls `actor_ref.kill()`.
    b.  `ActorRef::kill` sends a `ControlSignal::Terminate` message via the dedicated, prioritized termination channel (`terminate_sender`).
    c.  The actor's `tokio::select!` loop is `biased` to prioritize checking the `terminate_receiver`.
    d.  Upon receiving `ControlSignal::Terminate`:
        i.  The message loop immediately breaks out of the message processing loop, effectively ignoring any unprocessed messages in the main mailbox.
        ii. It calls `on_stop(&actor_weak, true)` with `killed: true`.
    e.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: true }`.

3.  **`on_run` returning `Err(_)`:**
    a.  If `on_run` returns `Err(e)`, it signals a runtime failure. The message loop calls `on_stop(&actor_weak, false)` and the `JoinHandle` resolves with `ActorResult::Failed { actor: Some(final_actor_state), error: e, phase: FailurePhase::OnRun, killed: false }`.

4.  **All `ActorRef`s dropped:**
    a.  If all `ActorRef` instances for an actor are dropped, its mailbox channel will close.
    b.  The message loop's message receiving (`receiver.recv().await`) will eventually return `None`.
    c.  This triggers the actor termination sequence with `on_stop(&actor_weak, false)` being called.
    d.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: false }`.

In all scenarios, the `ActorResult` provides the final state of the actor if it completed or failed at runtime, allowing for potential recovery or inspection. The `on_stop` hook is called before the actor terminates to allow for cleanup.

## 4. Type Safety System

rsActor implements a comprehensive type safety system through compile-time guarantees, providing both safety and performance.

**Key Components:**
*   **`ActorRef<T>`:** The primary reference type providing compile-time type safety.
*   **`MessageHandler` trait:** Implemented by the `#[message_handlers]` macro, enables efficient message dispatch.
*   **`Message<M>` trait:** Defines the handler method for a specific message type along with its reply type.

**Diagram:**

![Type Safety System](./type_safety.png)

**Description:**
1.  **Compile-time Type Safety with `ActorRef<T>`:**
    a.  An `ActorRef<T>` can only send messages that the actor type `T` explicitly implements handlers for.
    b.  The Rust compiler enforces that message types match what the actor can handle.
    c.  Reply types from messages are also statically checked, ensuring type consistency across the entire message-passing chain.
    d.  This approach prevents message-related errors at compile time with zero runtime overhead.

2.  **Zero-Cost Abstractions:**
    a.  No runtime type checking is required since types are verified at compile time.
    b.  Direct method dispatch without boxing or dynamic dispatch overhead.
    c.  The `#[message_handlers]` macro generates efficient code with no runtime penalties.

This approach provides complete type safety with maximum performance, ensuring that message-related errors are caught during compilation rather than at runtime.

## 5. Actor Lifecycle Management

This section provides a comprehensive view of the actor lifecycle from creation to termination, including all possible states and transitions.

**Diagram:**

![Actor Lifecycle Management](./lifecycle.png)

**Description:**
The actor lifecycle consists of three main phases:

1. **Initialization Phase**: The actor is created via `on_start()`. If this fails, no actor instance exists.
2. **Processing Phase**: The actor processes messages and executes `on_run()` repeatedly until termination.
3. **Termination Phase**: The actor is cleaned up via `on_stop()` before the task completes.

Each phase can result in different `ActorResult` outcomes, providing detailed information about the actor's final state.

## 6. Error Handling Scenarios

This section details how different types of errors are handled throughout the actor lifecycle.

**Diagram:**

![Error Handling Scenarios](./error_handling.png)

**Description:**
rsActor provides comprehensive error handling across all lifecycle phases:

1. **Initialization Errors**: Failed `on_start()` results in no actor instance being created.
2. **Runtime Errors**: Failed `on_run()` terminates the actor but preserves the final state.
3. **Cleanup Errors**: Failed `on_stop()` is reported but doesn't prevent termination.
4. **Message Handler Errors**: Should be handled within handlers using error reply types.

The `FailurePhase` enum helps identify where errors occurred, enabling appropriate recovery strategies.
