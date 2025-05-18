# rsActor Architecture

This document outlines the architecture of the `rsActor` framework, detailing key processes such as actor creation, message passing, and termination. PlantUML sequence diagrams are used to illustrate these processes.

## 1. Actor Creation and Spawning Process

This section describes the sequence of events when a user defines an actor and spawns it using `rsactor::spawn`.

**Key Components:**
*   **User Code:** Defines the actor struct and its behavior (implementing `Actor` and `Message<M>` traits).
*   **`spawn()` function:** The entry point for creating and starting an actor.
*   **`ActorRef`:** A handle to the actor, used for sending messages and control signals.
*   **`Runtime`:** An internal struct that manages the actor's lifecycle and message loop.
*   **Tokio Channels:** `mpsc::channel` for the actor's mailbox and a dedicated `mpsc::channel` for termination signals.
*   **Tokio Task:** The actor runs within its own asynchronous Tokio task.

**Diagram:**

![Actor Creation and Spawning](./Actor%20Cration%20and%20Spawning.png)

**Description:**
1.  The user defines their actor structure and implements the necessary `Actor` and `Message<M>` traits.
2.  The user calls `rsactor::spawn(actor_instance)`, passing their actor instance.
3.  The `spawn()` function:
    a.  Generates a unique ID for the actor.
    b.  Creates two Tokio MPSC channels: one for regular messages (the mailbox) and another dedicated channel for termination signals. The termination channel has a small buffer (e.g., 1) to ensure kill signals can be delivered even if the main mailbox is full.
    c.  Instantiates an `ActorRef` containing the actor's ID and the sender ends of both channels.
    d.  Instantiates a `Runtime` object, moving the user's actor instance, a clone of the `ActorRef`, and the receiver ends of both channels into it.
    e.  Calls `tokio::spawn` to run the `Runtime::run_actor_lifecycle` method in a new asynchronous task. This method contains the actor's main loop.
    f.  Returns the `ActorRef` and a `tokio::task::JoinHandle` to the user. The `JoinHandle` can be used to await the actor's termination.
4.  Inside the spawned Tokio task, `Runtime::run_actor_lifecycle()` begins execution:
    a.  It first calls the actor's `on_start()` lifecycle hook.
    b.  If `on_start()` is successful, the runtime calls the actor's `run_loop()` lifecycle hook which contains the actor's main execution logic and runs for its lifetime.
    c.  Concurrently with `run_loop()`, the runtime also processes messages from the mailbox.
    d.  If `on_start()` fails, or when `run_loop()` completes or fails, the actor stops, and `on_stop()` is called with the appropriate stop reason.

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

This section details how an actor is terminated using `ActorRef::stop` (graceful shutdown) or `ActorRef::kill` (immediate termination).

**Key Components:**
*   **`ActorRef`:** Used by the client to initiate termination.
*   **`MailboxMessage::StopGracefully`:** A control message for graceful shutdown, sent via the main mailbox.
*   **`ControlSignal::Terminate`:** A control signal for immediate termination, sent via a dedicated, prioritized channel.
*   **Mailbox Channel & Terminate Channel:** Used to deliver these signals.
*   **`Runtime`:** Handles these signals in its `tokio::select!` loop.
*   **`Actor::on_stop()`:** The actor's lifecycle hook for cleanup.
*   **`ActorStopReason`:** An enum indicating why the actor stopped.
*   **`JoinHandle`:** Resolves when the actor's task completes, providing the final actor state and stop reason.

**Diagram:**

![Actor Termination (stop and kill)](./Actor%20Termination.png)

**Description:**
1.  **`stop()` (Graceful Shutdown):**
    a.  The client calls `actor_ref.stop().await`.
    b.  `ActorRef::stop` sends a `MailboxMessage::StopGracefully` message to the actor via its main mailbox channel.
    c.  The `Runtime`, in its message loop, receives `StopGracefully`.
    d.  It sets a flag indicating graceful shutdown (`gracefully_stopping = true`) and sets the `final_reason` to `ActorStopReason::Normal`.
    e.  Crucially, it closes its main mailbox receiver (`self.receiver.close()`). This prevents new messages from being accepted, but the loop continues to process any messages already queued in the mailbox.
    f.  Once all existing messages are processed, `self.receiver.recv()` will return `None`, causing the message loop to break.
    g.  After the loop, `Runtime` calls the actor's `on_stop()` lifecycle hook with `ActorStopReason::Normal`.
    h.  The actor's task then finishes, and the `JoinHandle` resolves.

2.  **`kill()` (Immediate Termination):**
    a.  The client calls `actor_ref.kill()`.
    b.  `ActorRef::kill` sends a `ControlSignal::Terminate` message via the dedicated, prioritized termination channel (`terminate_sender`). Using `try_send` ensures this is a non-blocking attempt to signal.
    c.  The `Runtime`'s `tokio::select!` loop is `biased` to prioritize checking the `terminate_receiver`.
    d.  Upon receiving `ControlSignal::Terminate`:
        i.  The `Runtime` sets the `final_reason` to `ActorStopReason::Killed`.
        ii. It closes both the main mailbox receiver and its own terminate signal receiver.
        iii. It immediately breaks out of the message processing loop, effectively ignoring any unprocessed messages in the main mailbox.
    e.  After the loop, `Runtime` calls the actor's `on_stop()` lifecycle hook with `ActorStopReason::Killed`.
    f.  The actor's task then finishes, and the `JoinHandle` resolves.

In both termination scenarios, if `on_stop()` itself returns an error, this error is incorporated into the final `ActorStopReason::Error` that the `JoinHandle` will resolve with.
