## Blocking Task Actors

While Tokio and `rsActor` are designed for asynchronous operations, there are scenarios where you need to integrate with blocking code, such as CPU-bound computations, synchronous I/O libraries, or FFI calls.

Running blocking code directly within an actor's message handler (which runs on a Tokio worker thread) can stall the Tokio runtime, preventing other asynchronous tasks from making progress. To handle this, Tokio provides `tokio::task::spawn_blocking`.

`rsActor` facilitates interaction with tasks running in `spawn_blocking` by providing blocking message sending methods: `blocking_ask` and `blocking_tell` on `ActorRef`.

### When to Use `spawn_blocking` with Actors

1.  **CPU-Bound Work**: For lengthy computations that would otherwise block an async task for too long.
2.  **Synchronous Libraries**: When interacting with libraries that use blocking I/O or do not have an async API.
3.  **FFI Calls**: If foreign function interface calls are blocking.

### Pattern: Actor Managing a Blocking Task

A common pattern is an actor that either:
*   Offloads specific blocking operations to `spawn_blocking` within its message handlers.
*   Manages a dedicated, long-running blocking task that communicates with the actor.

The `examples/actor_blocking_task.rs` file demonstrates an actor that spawns a dedicated synchronous background task in its `on_start` method.

**Key elements:**

1.  **Spawning the Blocking Task (in `on_start`)**:

    ```rust
    // Inside SyncDataProcessorActor::on_start
    let task_actor_ref = actor_ref.clone(); // For task -> actor communication

    let handle = task::spawn_blocking(move || {
        loop {
            thread::sleep(interval); // Blocking sleep
            let raw_value = rand::random::<f64>() * 100.0;

            // Send data back to the actor using blocking_tell
            if let Err(e) = task_actor_ref.blocking_tell(ProcessedData { /* ... */ }, None) {
                break; // Exit task on error
            }
        }
    });
    ```

2.  **Communication from Blocking Task to Actor (`blocking_tell`)**:
    The blocking task uses `task_actor_ref.blocking_tell(...)` to send messages back to the actor.

3.  **Communication from Actor to Blocking Task**:
    The actor can send commands to the blocking task using standard Tokio MPSC channels.

### `blocking_tell` and `blocking_ask`

These methods on `ActorRef` are designed for use from any thread, including non-async contexts:

*   **`blocking_tell(message, timeout: Option<Duration>)`**: Sends a message and blocks the current thread until enqueued. Fire-and-forget.
*   **`blocking_ask(message, timeout: Option<Duration>)`**: Sends a message and blocks until the actor processes it and a reply is received.

#### Timeout Behavior

- **`timeout: None`**: Uses Tokio's `blocking_send` directly. Most efficient, but blocks indefinitely if the mailbox is full.
- **`timeout: Some(duration)`**: Spawns a separate thread with a temporary Tokio runtime. Has additional overhead (~50-200us for thread creation) but guarantees bounded waiting.

#### Thread Safety

These methods are safe to call from within an existing Tokio runtime context because the timeout implementation spawns a separate thread with its own runtime, avoiding the "cannot start a runtime from within a runtime" panic.

### Deprecated Methods

The older `ask_blocking` and `tell_blocking` methods are deprecated since v0.10.0. Use `blocking_ask` and `blocking_tell` instead:

```rust
// Old (deprecated):
// actor_ref.ask_blocking(msg, timeout);
// actor_ref.tell_blocking(msg, timeout);

// New:
actor_ref.blocking_ask(msg, timeout);
actor_ref.blocking_tell(msg, timeout);
```

### Considerations

*   **Thread Pool**: `spawn_blocking` uses a dedicated thread pool in Tokio. Be mindful of pool size if spawning many blocking tasks.
*   **Communication**: Use `ActorRef` with `blocking_*` methods for task-to-actor communication, and Tokio MPSC channels for actor-to-task communication.
*   **Shutdown**: Ensure graceful shutdown of blocking tasks when the managing actor stops.

By using `spawn_blocking` and the `blocking_*` methods, `rsActor` allows you to integrate synchronous, blocking code into your asynchronous actor system safely and efficiently.
