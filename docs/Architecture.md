# rsActor Architecture

This document outlines the architecture of the `rsActor` framework, detailing key processes such as actor creation, message passing, and termination. PlantUML sequence diagrams are used to illustrate these processes.

## 1. Actor Creation and Spawning Process

This section describes the sequence of events when a user defines an actor and spawns it using `rsactor::spawn`.

**Key Components:**
*   **User Code:** Defines the actor struct, its initialization arguments (`Actor::Args`), and its behavior (implementing `Actor` and `Message<M>` traits).
*   **`spawn(args: Actor::Args)` function:** The entry point for creating and starting an actor. It takes the initialization arguments.
*   **`ActorRef`:** A handle to the actor, used for sending messages and control signals.
*   **`Runtime`:** An internal struct that manages the actor's lifecycle and message loop.
*   **Tokio Channels:** `mpsc::channel` for the actor's mailbox and a dedicated `mpsc::channel` for termination signals.
*   **Tokio Task:** The actor runs within its own asynchronous Tokio task.

**Diagram:**

![Actor Creation and Spawning](./Actor%20Creation%20and%20Spawning.png)

**Description:**
1.  The user defines their actor structure, its `Args` type, and implements the necessary `Actor` and `Message<M>` traits.
2.  The user calls `rsactor::spawn(args)`, passing the initialization arguments for the actor.
3.  The `spawn()` function:
    a.  Generates a unique ID for the actor.
    b.  Creates two Tokio MPSC channels: one for regular messages (the mailbox) and another dedicated channel for termination signals. The termination channel has a small buffer (e.g., 1) to ensure kill signals can be delivered even if the main mailbox is full.
    c.  Instantiates an `ActorRef` containing the actor's ID and the sender ends of both channels.
    d.  Instantiates a `Runtime` object. This `Runtime` will hold the actor instance once it's created by `on_start`. It also stores a clone of the `ActorRef`, the receiver ends of both channels, and the `args` provided by the user.
    e.  Calls `tokio::spawn` to run the `Runtime::run_actor_lifecycle(args)` method in a new asynchronous task. This method contains the actor's main loop and receives the `args`.
    f.  Returns the `ActorRef` and a `tokio::task::JoinHandle<ActorResult<A>>` to the user. The `JoinHandle` can be used to await the actor's termination and retrieve the `ActorResult`.
4.  Inside the spawned Tokio task, `Runtime::run_actor_lifecycle(args)` begins execution:
    a.  It first calls the actor's `A::on_start(args, actor_ref.clone())` lifecycle hook, passing the user-provided `args` and a clone of the `ActorRef`. This method is responsible for creating and returning the actor instance (`Ok(Self)`).
    b.  If `on_start()` returns `Ok(actor_instance)`, this instance is stored in the `Runtime`. The runtime then calls the actor's `on_run()` lifecycle hook which contains the actor's main execution logic and runs for its lifetime.
    c.  Concurrently with `on_run()`, the runtime also processes messages from the mailbox using `tokio::select!`. This enables the actor to handle incoming messages while executing its `on_run()` logic.
    d.  If `on_start()` fails (returns `Err(e)`), the actor fails to start. The `run_actor_lifecycle` method will then ensure the `JoinHandle` resolves to `ActorResult::Failed { cause: e, phase: FailurePhase::OnStart, .. }`.
    e.  When `on_run()` completes (returns `Err(_)`), or if the actor is stopped/killed, the actor's lifecycle ends. The `JoinHandle` will resolve to an appropriate `ActorResult` (e.g., `ActorResult::Completed` or `ActorResult::Failed`).

## 2. Message Passing (tell/ask)

This section explains how messages are sent to an actor using `ActorRef::tell` (fire-and-forget) and `ActorRef::ask` (request-reply) and how they are processed.

**Key Components:**
*   **`ActorRef`:** Used by the client to send messages.
*   **`MailboxMessage::Envelope`:** Wraps the user's message and an optional `oneshot::Sender` for replies (used by `ask`).
*   **Mailbox Channel (`mpsc::channel`):** The primary channel for delivering messages to the actor.
*   **`Runtime`:** Receives messages from the mailbox and dispatches them.
*   **`MessageHandler` trait (via `impl_message_handler!` macro):** Dynamically dispatches the type-erased message to the actor's specific `Message<M>::handle` method.
*   **`Message<M>::handle()`:** The user-defined method that processes the message.
*   **`oneshot::channel`:** Used by `ask` to receive a reply from the actor.

**Diagram:**

![Message Passing (tell and ask)](./Message%20Passing.png)

**Description:**
1.  **`tell(message)` (Fire-and-Forget):**
    a.  The client calls `actor_ref.tell(message)`.
    b.  `ActorRef::tell` wraps the `message` into a `MailboxMessage::Envelope` with the `reply_channel` field set to `None`.
    c.  The envelope is sent asynchronously to the actor's mailbox via the `mpsc::Sender` held by the `ActorRef`.
    d.  The `Runtime`, in its message loop, receives the `MailboxMessage::Envelope` from its `mpsc::Receiver`.
    e.  The `Runtime` uses the `MessageHandler` trait (implemented for the actor by the `impl_message_handler!` macro) to downcast the type-erased message payload and call the appropriate `Message<M>::handle` method on the actor instance.
    f.  The actor processes the message. Since it was a `tell`, no reply is sent back through a dedicated channel.

2.  **`ask(message)` (Request-Reply):**
    a.  The client calls `actor_ref.ask(message).await`.
    b.  `ActorRef::ask` first creates a `tokio::sync::oneshot` channel for the reply.
    c.  It then wraps the `message` into a `MailboxMessage::Envelope`, placing the sender part of the `oneshot` channel into the `reply_channel` field.
    d.  The envelope is sent to the actor's mailbox (same as `tell`).
    e.  `ActorRef::ask` then `.await`s on the receiver part of the `oneshot` channel.
    f.  The `Runtime` receives and dispatches the message to the actor's `Message<M>::handle` method as described above.
    g.  After the actor's `handle` method completes, the `Runtime` takes the returned reply.
    h.  If a `reply_channel` (the `oneshot::Sender`) exists in the envelope, the `Runtime` sends the reply (or an error if processing failed) through this `oneshot` channel.
    i.  The `ActorRef::ask` method, awaiting on the `oneshot::Receiver`, receives the reply, downcasts it to the expected type, and returns it to the client.

## 3. Actor Termination (stop/kill)

This section details how an actor is terminated using `ActorRef::stop` (graceful shutdown) or `ActorRef::kill` (immediate termination), and how this relates to `ActorResult`.

**Key Components:**
*   **`ActorRef`:** Used by the client to initiate termination.
*   **`MailboxMessage::StopGracefully`:** A control message for graceful shutdown, sent via the main mailbox.
*   **`ControlSignal::Terminate`:** A control signal for immediate termination, sent via a dedicated, prioritized channel.
*   **Mailbox Channel & Terminate Channel:** Used to deliver these signals.
*   **`Runtime`:** Handles these signals in its `tokio::select!` loop.
*   **`ActorResult<A>`:** An enum (`Completed`, `Failed`) indicating the outcome of the actor's lifecycle. The `Completed` variant includes the final actor state and a `killed` flag. The `Failed` variant includes error details, failure phase, and optional actor state.
*   **`JoinHandle<ActorResult<A>>`:** Resolves when the actor's task completes, providing the `ActorResult`.

**Diagram:**

![Actor Termination (stop and kill)](./Actor%20Termination.png)

**Description:**
1.  **`stop()` (Graceful Shutdown):**
    a.  The client calls `actor_ref.stop().await`.
    b.  `ActorRef::stop` sends a `MailboxMessage::StopGracefully` message to the actor via its main mailbox channel.
    c.  The `Runtime`, in its message loop, receives `StopGracefully`.
    d.  The message loop is broken. The `on_run` method is expected to detect the shutdown signal (e.g., by checking a flag or if its communication channels close) and return.
    e.  Once `on_run` returns (typically `Ok(())` if it was just processing messages and the mailbox closes), the actor's task prepares to finish.
    f.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: false }`.

2.  **`kill()` (Immediate Termination):**
    a.  The client calls `actor_ref.kill()`.
    b.  `ActorRef::kill` sends a `ControlSignal::Terminate` message via the dedicated, prioritized termination channel (`terminate_sender`).
    c.  The `Runtime`'s `tokio::select!` loop is `biased` to prioritize checking the `terminate_receiver`.
    d.  Upon receiving `ControlSignal::Terminate`:
        i.  The `Runtime` immediately breaks out of the message processing loop, effectively ignoring any unprocessed messages in the main mailbox.
        ii. It calls `on_stop` with `killed: true`.
    e.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: true }`.

3.  **`on_run` returning `Err(_)`:**
    a.  If `on_run` returns `Err(e)`, it signals a runtime failure. The `JoinHandle` resolves with `ActorResult::Failed { actor: Some(final_actor_state), error: e, phase: FailurePhase::OnRun, killed: false }`.

4.  **All `ActorRef`s dropped:**
    a.  If all `ActorRef` instances for an actor are dropped, its mailbox channel will close.
    b.  The `Runtime`'s message receiving loop (`self.receiver.recv().await`) will eventually return `None`.
    c.  This triggers the actor termination sequence with `on_stop` being called.
    d.  The `JoinHandle` resolves with `ActorResult::Completed { actor: final_actor_state, killed: false }`.

In all scenarios, the `ActorResult` provides the final state of the actor if it completed or failed at runtime, allowing for potential recovery or inspection. The `on_stop` hook is called before the actor terminates to allow for cleanup.
