<!-- filepath: /Volumes/Workspace/rust/rsactor/docs/FAQ.md -->
# rsActor FAQ

This FAQ provides answers to common questions about the `rsActor` framework.

## General

**Q1: What is rsActor?**

A1: `rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing a simple and efficient actor model for local, in-process systems. It emphasizes clean message-passing semantics and straightforward actor lifecycle management while maintaining high performance for Rust applications.

**Q2: What are the main design goals or philosophy behind rsActor?**

A2: The primary goals are simplicity and efficiency for in-process actor systems. It leverages Tokio for its asynchronous runtime and provides core actor primitives with minimal boilerplate. Key features include:
*   **Type Safety**: Strong compile-time type safety through `ActorRef<T>`
*   **Performance**: Zero-cost abstractions with efficient message passing
*   **Simplicity**: Clean APIs with optional derive macros for reduced boilerplate
*   **Observability**: Optional tracing support for production debugging

**Q3: How does rsActor compare to other Rust actor frameworks like Actix or Kameo?**

A3:
*   **Scope:** `rsActor` is designed for local, in-process actors only and does not support remote actors or clustering, unlike some more comprehensive frameworks.
*   **Simplicity:** It aims for a smaller API surface and less complexity compared to frameworks like Actix, with a focus on essential actor model features.
*   **Type Safety:** Provides strong compile-time type safety through `ActorRef<T>` while maintaining flexibility.
*   **Features:** Compared to Kameo, `rsActor` provides:
    - Concrete `ActorRef<T>` with compile-time type safety
    - Optional tracing support for production observability
    - Straightforward lifecycle management with `on_start`, `on_run`, and `on_stop` hooks
    - Both `#[message_handlers]` macro and manual `Message<T>` trait implementation
*   **Error Handling:** Uses `ActorResult` enum to indicate startup or runtime failures with detailed error information and failure phases.

## Actor Definition and Usage

**Q4: How do I define an actor?**

A4: To define an actor, you can choose between two approaches:

### Option A: Using the Actor Derive Macro and Message Handlers Macro (Recommended)

1.  Create a struct for your actor's state and derive `Actor`.
2.  Define the message types your actor will handle.
3.  Use the `#[message_handlers]` attribute macro with `#[handler]` method attributes to automatically implement message handling.

### Option B: Manual Implementation (for complex initialization)

1.  Create a struct for your actor's state.
2.  Define a struct or tuple for your actor's initialization arguments (this will be `Actor::Args`).
3.  Implement the `Actor` trait for your state struct. This involves:
    *   Defining an associated type `Args` (the type of arguments your `on_start` method will take).
    *   Defining an associated type `Error` (the error type your actor's lifecycle methods can return).
    *   Implementing an `on_start` method which initializes your actor from the arguments.
    *   Implementing `on_run` and `on_stop` methods, which are optional and have default implementations.
4.  Define the message types your actor will handle.
5.  For each message type, implement the `Message<MessageType>` trait for your actor struct.
6.  Use the `#[message_handlers]` macro to implement message handling, or the deprecated `impl_message_handler!` macro.

**Q5: Is there a simple example of an actor?**

A5: Here are examples using both approaches:

### Recommended Approach (Using Derive and Message Handlers Macros)

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};

// Define actor struct with derive macro
#[derive(Actor)]
struct SimpleActor {
    counter: u32,
}

// Define message types
struct Increment(u32);
struct GetCount;

// Use message_handlers macro with handler attributes
#[message_handlers]
impl SimpleActor {
    #[handler]
    async fn handle_increment(&mut self, msg: Increment, _: &ActorRef<Self>) -> u32 {
        self.counter += msg.0;
        self.counter
    }

    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
        self.counter
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and spawn actor
    let actor = SimpleActor { counter: 0 };
    let (actor_ref, _handle) = spawn(actor);

    // Send messages
    let new_count = actor_ref.ask(Increment(5)).await?;
    println!("New count: {}", new_count);

    let current_count = actor_ref.ask(GetCount).await?;
    println!("Current count: {}", current_count);

    // Gracefully stop the actor
    actor_ref.stop().await?;
    Ok(())
}
```

### Manual Implementation Approach

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn, ActorResult};
use anyhow::Result;

// Define actor struct
struct SimpleActor {
    counter: u32,
}

// Implement Actor trait
impl Actor for SimpleActor {
    type Args = u32; // Starting counter value
    type Error = anyhow::Error;

    async fn on_start(initial_counter: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(SimpleActor { counter: initial_counter })
    }
}

// Define message type
struct Increment(u32);

// Implement message handler
impl Message<Increment> for SimpleActor {
    type Reply = u32; // Return new counter value

    async fn handle(&mut self, msg: Increment, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.counter += msg.0;
        self.counter
    }
}

// Use macro to implement MessageHandler trait (deprecated approach)
impl_message_handler!(SimpleActor, [Increment]);

#[tokio::main]
async fn main() -> Result<()> {
    // Spawn actor with initial counter value of 0
    let (actor_ref, _join_handle) = spawn::<SimpleActor>(0);

    // Send Increment message and await reply
    let new_value = actor_ref.ask(Increment(5)).await?;
    println!("New counter value: {}", new_value);

    Ok(())
}
```

**Q6: How do I spawn an actor?**

A6: To spawn an actor, you use the `spawn` function provided by `rsActor`:

```rust
let (actor_ref, join_handle) = spawn::<MyActor>(args);
```

This function returns a tuple containing:
*   An `ActorRef<MyActor>` which you can use to send messages to the actor.
*   A `JoinHandle<ActorResult<MyActor>>` which you can use to await the actor's completion and get its final state or error information.

The `args` parameter is of type `MyActor::Args` and will be passed to the actor's `on_start` method.

**Q7: How do I send messages to an actor?**

A7: `rsActor` provides several methods for sending messages to actors:

1.  **`ask`**: Send a message and await a reply.
    ```rust
    let result = actor_ref.ask(MyMessage).await?;
    ```

2.  **`ask_with_timeout`**: Send a message, await a reply with a specified timeout.
    ```rust
    let result = actor_ref.ask_with_timeout(MyMessage, Duration::from_secs(1)).await?;
    ```

3.  **`tell`**: Send a message without waiting for a reply (fire-and-forget).
    ```rust
    actor_ref.tell(MyMessage).await?;
    ```

4.  **`tell_with_timeout`**: Send a message without waiting for a reply, with a timeout.
    ```rust
    actor_ref.tell_with_timeout(MyMessage, Duration::from_secs(1)).await?;
    ```

5.  **`blocking_ask`**: Blocking version of `ask` for use from any thread (no runtime context required).
    ```rust
    // Without timeout (blocks indefinitely)
    let result = actor_ref.blocking_ask(MyMessage, None)?;

    // With timeout
    let result = actor_ref.blocking_ask(MyMessage, Some(Duration::from_secs(5)))?;
    ```

6.  **`blocking_tell`**: Blocking version of `tell` for use from any thread (no runtime context required).
    ```rust
    // Without timeout (blocks indefinitely)
    actor_ref.blocking_tell(MyMessage, None)?;

    // With timeout
    actor_ref.blocking_tell(MyMessage, Some(Duration::from_secs(5)))?;
    ```

**Note:** The `ask_blocking` and `tell_blocking` methods are deprecated since v0.10.0. Use `blocking_ask` and `blocking_tell` instead.

**Q8: How do I stop an actor?**

A8: To stop an actor, you can use:

1.  **Graceful Stop**:
    ```rust
    actor_ref.stop().await?;
    ```
    This sends a stop signal to the actor and waits for it to shut down cleanly. The actor will continue processing its current message, finish its current `on_run` execution, and then call `on_stop` before terminating.

2.  **Immediate Kill**:
    ```rust
    actor_ref.kill();
    ```
    This abruptly stops the actor. The actor will not finish processing its current message, but will call `on_stop(killed=true)` before terminating.

3.  **From within the actor**:
    An actor can stop itself by calling `actor_ref.stop()` or `actor_ref.kill()` within its own methods.

**Q9: How do I define message types?**

A9: Message types in `rsActor` are just regular Rust types (structs or enums) that can carry the data needed for the actor to process the request. Message types should be `Send + 'static` to be safely sent across threads.

```rust
// Simple message with no data
struct Ping;

// Message with data
struct AddUser {
    id: u64,
    name: String,
    email: Option<String>,
}

// Enum message type
enum DatabaseCommand {
    Insert(Record),
    Delete(u64),
    Query(QueryParams),
}
```

**Q10: How do I handle messages in an actor?**

A10: There are two approaches to handle messages:

### Recommended Approach: Using `#[message_handlers]` Macro

```rust
use rsactor::{Actor, ActorRef, message_handlers};

#[derive(Actor)]
struct UserManagerActor {
    users: std::collections::HashMap<u64, User>,
    next_id: u64,
}

#[message_handlers]
impl UserManagerActor {
    #[handler]
    async fn handle_add_user(&mut self, msg: AddUser, _: &ActorRef<Self>) -> Result<UserId, UserError> {
        let user = User {
            id: self.next_id,
            name: msg.name,
            email: msg.email,
        };

        self.next_id += 1;
        self.users.insert(user.id, user.clone());

        Ok(user.id)
    }

    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) -> String {
        "Pong".to_string()
    }
}
```

### Manual Implementation: Using `Message<T>` Trait

```rust
impl Message<AddUser> for UserManagerActor {
    // Define what this message handler returns
    type Reply = Result<UserId, UserError>;

    // Implement the message handler
    async fn handle(&mut self, msg: AddUser, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        let user = User {
            id: self.next_id,
            name: msg.name,
            email: msg.email,
        };

        self.next_id += 1;
        self.users.insert(user.id, user.clone());

        Ok(user.id)
    }
}

// Use macro to implement MessageHandler trait (deprecated approach)
impl_message_handler!(UserManagerActor, [Ping, AddUser, RemoveUser, GetUser]);
```

Then you must also use the `impl_message_handler!` macro to register all the message types your actor can handle:

```rust
impl_message_handler!(UserManagerActor, [Ping, AddUser, RemoveUser, GetUser]);
```

This macro implements the `MessageHandler` trait, which is what allows the actor runtime to dispatch messages to the appropriate handler methods.

**Note:** The `#[message_handlers]` approach is recommended as it automatically generates the `Message<T>` implementations and `MessageHandler` trait implementation, reducing boilerplate and potential errors.

## Actor Lifecycle

**Q11: What is the lifecycle of an actor?**

A11: The lifecycle of an actor in `rsActor` follows these stages:

1.  **Creation and Initialization**:
    *   Actor is spawned with arguments via `spawn::<Actor>(args)`.
    *   The framework calls `on_start(args, actor_ref)` to create the actor instance.
    *   If `on_start` returns `Ok(actor_instance)`, the actor enters the running state.
    *   If `on_start` returns `Err(e)`, the actor fails to start, and the `JoinHandle` resolves with an error.

2.  **Running**:
    *   The framework repeatedly calls the actor's `on_run` method, which defines the actor's main execution logic.
    *   Concurrently, the actor processes messages from its mailbox.
    *   This continues until the actor is stopped or encounters an error.

3.  **Termination**:
    *   When the actor is stopping (either due to `stop()`, `kill()`, or an error), the framework calls `on_stop(actor_ref, killed)`.
    *   After `on_stop` completes, the actor is destroyed, and the `JoinHandle` is resolved with an `ActorResult`.

The actor's lifecycle methods are:

*   `on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error>`: Called when the actor is starting. Creates and returns the actor instance.
*   `on_run(&mut self, actor_ref: &ActorRef<Self>) -> Result<(), Self::Error>`: Called after `on_start` succeeds. This method contains the main execution logic of the actor and runs concurrently with message handling.
    *   If `on_run` returns `Ok(())`, the actor continues running and `on_run` will be called again.
    *   If `on_run` returns `Err(e)`, the actor terminates due to a runtime error, resulting in `ActorResult::Failed` with `phase: FailurePhase::OnRun`.
    *   To stop the actor normally from within `on_run`, call `actor_ref.stop().await` or `actor_ref.kill()`.
*   `on_stop(&mut self, actor_ref: &ActorRef<Self>, killed: bool) -> Result<(), Self::Error>`: Called when the actor is stopping. The `killed` parameter is `true` if the actor was killed, and `false` if it was stopped gracefully.

**Q12: What is the `ActorResult` enum?**

A12: The `ActorResult` enum represents the outcome of an actor's lifecycle when awaiting its `JoinHandle`. It has two variants:

1.  **`ActorResult::Completed`**:
    *   Indicates that the actor completed successfully.
    *   Contains the final actor state (`actor: A`) and a boolean `killed` indicating whether the actor was killed or stopped gracefully.
    *   Returned when an actor is successfully stopped or killed.

2.  **`ActorResult::Failed`**:
    *   Indicates that the actor failed during its lifecycle.
    *   Contains the optional actor state (`actor: Option<A>`), the error that caused the failure (`error: E`), the phase in which the failure occurred (`phase: FailurePhase`), and a boolean `killed` indicating whether the actor was killed.
    *   The `FailurePhase` can be `OnStart`, `OnRun`, or `OnStop`.

**Q13: How do I handle errors in actors?**

A13: Error handling in `rsActor` happens at several levels:

*   **Lifecycle - `on_start`:** If `on_start` returns `Err(e)`, the actor never starts, and the `JoinHandle` will resolve to `ActorResult::Failed { actor: None, error: e, phase: FailurePhase::OnStart, killed: false }`. Since the actor wasn't created, the `actor` field is `None`.
*   **Lifecycle - `on_run`:** If `on_run` returns `Err(e)`, the actor will terminate, and the `JoinHandle` will resolve to `ActorResult::Failed { actor: Some(actor_state), error: e, phase: FailurePhase::OnRun, killed: false }`. The `actor` field may contain the actor's state.
*   **Panics:** If a message handler or `on_run` panics, the Tokio task hosting the actor will terminate. Awaiting the `JoinHandle` will then result in an `Err` (typically a `tokio::task::JoinError` indicating a panic). It's generally recommended to handle errors gracefully within your actor logic and return `Result` types from `on_start` and `on_run`, and use `Result` as reply types for messages where appropriate, rather than relying on panics.
*   **Message Handling:** For message handling, the `Message<T>::handle` method can return any type as its `Reply`, including a `Result` type. If your message handler might fail, it's a good practice to use a `Result` type as the `Reply` type.
*   **Sending Messages:** The methods for sending messages (`ask`, `tell`, etc.) return `Result<R, rsactor::Error>`, where `R` is the reply type of the message. These methods can fail if the actor has stopped, the mailbox is full, or a timeout occurs.

## Advanced Usage

**Q14: Can I use rsActor with blocking code?**

A14: Yes, `rsActor` provides mechanisms for working with blocking code:

1.  **Within Message Handlers:**
    If a message handler needs to perform blocking operations, you can use `tokio::task::spawn_blocking`:

    ```rust
    impl Message<ProcessFile> for FileProcessorActor {
        type Reply = Result<Stats, FileError>;

        async fn handle(&mut self, msg: ProcessFile, _actor_ref: &ActorRef<Self>) -> Self::Reply {
            let file_path = msg.path.clone();
            let result = tokio::task::spawn_blocking(move || {
                // Perform blocking file operations
                process_file_synchronously(&file_path)
            }).await??; // Unwrap both the JoinError and the inner Result

            Ok(result)
        }
    }
    ```

2.  **Sending Messages from Blocking Contexts:**
    If you need to send messages to actors from within a blocking context, use the `blocking_ask` and `blocking_tell` methods:

    ```rust
    tokio::task::spawn_blocking(move || {
        // Some blocking work...

        // Send message and get response, blocking until response is received
        let result = actor_ref.blocking_ask(
            Query { id: 123 },
            Some(Duration::from_secs(5))
        );

        // Process result...
    });
    ```

**Q15: How do I test actors?**

A15: Testing actors can be done in several ways:

1.  **Integration-style tests**:
    Spawn the actor and interact with it directly:

    ```rust
    #[tokio::test]
    async fn test_counter_actor() {
        let (actor_ref, _handle) = spawn::<CounterActor>(0);

        // Test increment
        let result = actor_ref.ask(Increment(5)).await.unwrap();
        assert_eq!(result, 5);

        // Test get count
        let count = actor_ref.ask(GetCount).await.unwrap();
        assert_eq!(count, 5);
    }
    ```

2.  **Test message handlers directly**:
    You can instantiate your actor struct and call the Message trait's handle method directly by spawning a temporary actor:

    ```rust
    #[tokio::test]
    async fn test_message_handlers() {
        // Spawn a real actor for testing
        let (actor_ref, _handle) = spawn::<CounterActor>(0);

        // Test increment handler via the actor
        let result = actor_ref.ask(Increment(5)).await.unwrap();
        assert_eq!(result, 5);

        // Test current count
        let count = actor_ref.ask(GetCount).await.unwrap();
        assert_eq!(count, 5);

        actor_ref.stop().await.unwrap();
    }
    ```

3.  **Test lifecycle methods**:
    For testing lifecycle hooks, spawn the actor and observe behavior:

    ```rust
    #[tokio::test]
    async fn test_lifecycle() {
        // Spawn actor - on_start is called automatically
        let (actor_ref, handle) = spawn::<CounterActor>(10); // Initial count = 10

        // Verify initialization worked
        let count = actor_ref.ask(GetCount).await.unwrap();
        assert_eq!(count, 10);

        // Stop actor gracefully - on_stop is called
        actor_ref.stop().await.unwrap();

        // Await handle to get ActorResult
        let result = handle.await.unwrap();
        assert!(result.is_completed());
    }
    ```

**Q16: Can I use custom error types?**

A16: Yes, you can use any error type that implements `Send + Debug + 'static` as the `Actor::Error` type:

```rust
#[derive(Debug, thiserror::Error)]
enum MyActorError {
    #[error("Database error: {0}")]
    DbError(#[from] sqlx::Error),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Network timeout")]
    NetworkTimeout,
}

struct MyActor {
    // ...
}

impl Actor for MyActor {
    type Args = Config;
    type Error = MyActorError;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // ...
    }
}
```

**Q17: How do I handle actor supervision?**

A17: `rsActor` does not have a built-in supervision system like some other actor frameworks (e.g., Akka). However, you can implement a simple supervision pattern by:

1.  Monitoring the `JoinHandle` of child actors:
    ```rust
    // Spawn child actor
    let (child_ref, child_handle) = spawn::<ChildActor>(child_args);

    // Monitor the child's JoinHandle in another task
    tokio::spawn(async move {
        match child_handle.await {
            Ok(ActorResult::Completed { .. }) => {
                // Child completed normally
            }
            Ok(ActorResult::Failed { error, .. }) => {
                // Child failed, take some action (e.g., restart)
                let (new_child_ref, new_child_handle) = spawn::<ChildActor>(child_args);
                // ...
            }
            Err(join_error) => {
                // Child panicked
            }
        }
    });
    ```

2.  Creating a supervisor actor that manages child actors:
    ```rust
    struct SupervisorActor {
        children: HashMap<ActorId, ChildInfo>,
    }

    impl SupervisorActor {
        async fn spawn_child(&mut self, args: ChildArgs) -> Result<ActorRef<ChildActor>> {
            let (child_ref, child_handle) = spawn::<ChildActor>(args.clone());
            let child_id = child_ref.identity();

            // Monitor child in background task
            let supervisor_ref = self.self_ref.clone();
            tokio::spawn(async move {
                let result = child_handle.await;
                // Notify supervisor about child termination
                supervisor_ref.tell(ChildTerminated {
                    id: child_id,
                    result,
                    args, // Keep args for possible restart
                }).await;
            });

            self.children.insert(child_id, ChildInfo { ref: child_ref.clone() });
            Ok(child_ref)
        }
    }
    ```

**Q18: How to communicate between actors?**

A18: Actors communicate by sending messages to each other:

```rust
impl Message<ProcessOrder> for OrderProcessorActor {
    type Reply = Result<OrderStatus, OrderError>;

    async fn handle(&mut self, msg: ProcessOrder, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        // Process order locally
        let order = self.validate_order(&msg.order)?;

        // Send message to another actor for inventory check
        let inventory_status = self.inventory_actor.ask(CheckInventory {
            items: order.items.clone(),
        }).await?;

        if !inventory_status.all_available {
            return Err(OrderError::ItemsOutOfStock(inventory_status.missing_items));
        }

        // Send message to payment actor
        let payment_result = self.payment_actor.ask(ProcessPayment {
            amount: order.total_amount,
            payment_method: msg.payment_method,
        }).await?;

        if let Err(e) = payment_result {
            return Err(OrderError::PaymentFailed(e));
        }

        // Update order status
        self.orders.insert(order.id, order.clone());

        Ok(OrderStatus::Completed(order))
    }
}
```

**Q19: How do I implement request-response patterns?**

A19: The request-response pattern is built into `rsActor` through the `ask` method and `Message<T>::Reply` type:

```rust
// Request message
struct GetUserDetails {
    user_id: UserId,
}

// Response is defined as the Reply type
impl Message<GetUserDetails> for UserManagerActor {
    type Reply = Result<UserDetails, UserError>;

    async fn handle(&mut self, msg: GetUserDetails, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        match self.users.get(&msg.user_id) {
            Some(user) => Ok(UserDetails::from(user)),
            None => Err(UserError::UserNotFound(msg.user_id)),
        }
    }
}

// Client code:
async fn get_user_profile(
    user_manager: &ActorRef<UserManagerActor>,
    user_id: UserId,
) -> Result<UserDetails, Error> {
    let user_details = user_manager.ask(GetUserDetails { user_id }).await??;
    Ok(user_details)
}
```

**Q20: How do I share actor references between actors?**

A20: Actor references can be shared by passing them during actor creation or via messages:

1.  **Via constructor arguments**:
    ```rust
    struct CoordinatorActor {
        worker_actors: Vec<ActorRef<WorkerActor>>,
    }

    impl Actor for CoordinatorActor {
        type Args = Vec<ActorRef<WorkerActor>>;
        type Error = anyhow::Error;

        async fn on_start(workers: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(CoordinatorActor {
                worker_actors: workers,
            })
        }
    }

    // Spawn workers first, then coordinator:
    let worker_refs: Vec<_> = (0..5)
        .map(|i| spawn::<WorkerActor>(WorkerArgs { id: i }).0)
        .collect();

    let (coordinator_ref, _) = spawn::<CoordinatorActor>(worker_refs);
    ```

2.  **Via messages**:
    ```rust
    struct RegisterWorker {
        worker: ActorRef<WorkerActor>,
    }

    impl Message<RegisterWorker> for CoordinatorActor {
        type Reply = ();

        async fn handle(&mut self, msg: RegisterWorker, _actor_ref: &ActorRef<Self>) -> Self::Reply {
            self.worker_actors.push(msg.worker);
        }
    }

    // Later, register a worker:
    let (worker_ref, _) = spawn::<WorkerActor>(worker_args);
    coordinator_ref.tell(RegisterWorker { worker: worker_ref }).await?;
    ```

**Q21: Can I use generics with actors?**

A21: Yes, you can define generic actors. Here's an example using both the recommended `#[message_handlers]` approach and the manual approach:

### Recommended Approach: Using `#[message_handlers]` Macro

```rust
use rsactor::{Actor, ActorRef, message_handlers};
use std::fmt::Debug;

// Define a generic actor struct
#[derive(Debug)]
struct GenericActor<T: Send + Debug + Clone + 'static> {
    data: Option<T>,
}

// Implement the Actor trait for the generic actor
impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> {
    type Args = Option<T>; // Initial value for data
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { data: args })
    }
}

// Define message types
#[derive(Debug)]
struct SetValue<T: Send + Debug + 'static>(pub T);

#[derive(Debug, Clone, Copy)]
struct GetValue;

#[derive(Debug, Clone, Copy)]
struct ClearValue;

// Use message_handlers macro for generic actors
#[message_handlers]
impl<T: Send + Debug + Clone + 'static> GenericActor<T> {
    #[handler]
    async fn handle_set_value(&mut self, msg: SetValue<T>, _: &ActorRef<Self>) -> () {
        self.data = Some(msg.0);
    }

    #[handler]
    async fn handle_get_value(&mut self, _msg: GetValue, _: &ActorRef<Self>) -> Option<T> {
        self.data.clone()
    }

    #[handler]
    async fn handle_clear_value(&mut self, _msg: ClearValue, _: &ActorRef<Self>) -> () {
        self.data = None;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Usage with String - works automatically!
    let (string_actor_ref, _s_handle) = spawn::<GenericActor<String>>(Some("hello".to_string()));
    string_actor_ref.tell(SetValue("world".to_string())).await?;
    let val_s: Option<String> = string_actor_ref.ask(GetValue).await?;
    println!("String actor value: {:?}", val_s); // Should be Some("world")

    // Usage with i32 - works automatically!
    let (int_actor_ref, _i_handle) = spawn::<GenericActor<i32>>(Some(42));
    int_actor_ref.tell(SetValue(100)).await?;
    let val_i: Option<i32> = int_actor_ref.ask(GetValue).await?;
    println!("Integer actor value: {:?}", val_i); // Should be Some(100)

    Ok(())
}
```

### Manual Implementation: Using `impl_message_handler!` Macro

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn, ActorResult};
use anyhow::Result;
use std::fmt::Debug;

// Define a generic actor struct
#[derive(Debug)]
struct GenericActor<T: Send + Debug + Clone + 'static> {
    data: Option<T>,
}

// Implement the Actor trait for the generic actor
impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> {
    type Args = Option<T>; // Initial value for data
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { data: args })
    }
}

// Define message types
// A generic message to set the value
#[derive(Debug)]
struct SetValue<T: Send + Debug + 'static>(pub T);

// A non-generic message to get the value
#[derive(Debug, Clone, Copy)]
struct GetValue;

// A message to clear the value
#[derive(Debug, Clone, Copy)]
struct ClearValue;

// Implement Message trait for SetValue<T>
impl<T: Send + Debug + Clone + 'static> Message<SetValue<T>> for GenericActor<T> {
    type Reply = ();

    async fn handle(&mut self, msg: SetValue<T>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data = Some(msg.0);
    }
}

// Implement Message trait for GetValue
impl<T: Send + Debug + Clone + 'static> Message<GetValue> for GenericActor<T> {
    type Reply = Option<T>;

    async fn handle(&mut self, _msg: GetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data.clone()
    }
}

// Implement Message trait for ClearValue
impl<T: Send + Debug + Clone + 'static> Message<ClearValue> for GenericActor<T> {
    type Reply = ();

    async fn handle(&mut self, _msg: ClearValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data = None;
    }
}

// ---- Unified Macro Usage for Generic Actors ----
// Use the unified syntax with generic constraints in square brackets
// This single macro call handles ALL generic instantiations of GenericActor<T>
impl_message_handler!([T: Send + Debug + Clone + 'static] for GenericActor<T>, [SetValue<T>, GetValue, ClearValue]);

/*
#[tokio::main]
async fn main() -> Result<()> {
    // Usage with String - no additional macro calls needed!
    let (string_actor_ref, _s_handle) = spawn::<GenericActor<String>>(Some("hello".to_string()));
    string_actor_ref.tell(SetValue("world".to_string())).await?;
    let val_s: Option<String> = string_actor_ref.ask(GetValue).await?;
    println!("String actor value: {:?}", val_s); // Should be Some("world")

    // Usage with i32 - works automatically!
    let (int_actor_ref, _i_handle) = spawn::<GenericActor<i32>>(Some(42));
    int_actor_ref.tell(SetValue(100)).await?;
    let val_i: Option<i32> = int_actor_ref.ask(GetValue).await?;
    println!("Integer actor value: {:?}", val_i); // Should be Some(100)

    // Clear the value
    int_actor_ref.tell(ClearValue).await?;
    let val_cleared: Option<i32> = int_actor_ref.ask(GetValue).await?;
    println!("Cleared value: {:?}", val_cleared); // Should be None

    Ok(())
}
*/
```

The `#[message_handlers]` macro automatically handles this for you, or you can use the deprecated `impl_message_handler!` macro which supports two syntax patterns:
- **Generic actors**: `impl_message_handler!([T: Send + Debug + Clone + 'static] for GenericActor<T>, [SetValue<T>, GetValue, ClearValue]);`
- **Non-generic actors**: `impl_message_handler!(MyActor, [MessageType1, MessageType2]);`

With the generic syntax, you specify the generic constraints in square brackets, followed by `for` and the generic actor type, then the list of message types. This single macro call generates message handling for all possible instantiations of the generic actor, eliminating the need for separate macro calls for each concrete type.

**Note:** The `#[message_handlers]` macro approach is recommended over `impl_message_handler!` as it provides better ergonomics and reduces boilerplate.

**Q22: How can I effectively use the `on_run` method in my actors?**

A22: The `on_run` method is a key part of the actor lifecycle in the `rsActor` framework that enables an actor to perform its primary processing work. It is called after `on_start` completes and continues running throughout the actor's lifetime. Here's how to use it effectively:

*   **Actor Behavior Implementation:** The `on_run` method is where you implement the actor's primary behavior and processing logic. Following the actor model principles, this processing happens concurrently with message handling, allowing the actor to maintain its core responsibilities while remaining responsive to messages.

*   **Periodic Tasks:** You can implement periodic tasks by using `tokio::time` utilities within the `on_run`. `tokio::select!` makes it easy to handle multiple timers concurrently. For example, first define your actor struct with interval fields:

    ```rust
    struct MyActor {
        // ... other fields ...
        fast_interval: tokio::time::Interval,
        slow_interval: tokio::time::Interval,
    }

    // Initialize intervals in on_start
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(MyActor {
            // ... initialize other fields ...
            fast_interval: tokio::time::interval(std::time::Duration::from_millis(500)),
            slow_interval: tokio::time::interval(std::time::Duration::from_secs(5)),
        })
    }

    // Use the intervals in on_run without loop
    async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<(), Self::Error> {
        tokio::select! {
            _ = self.fast_interval.tick() => {
                // Handle high-frequency tasks (every 500ms)
                self.process_high_frequency_work();
            }
            _ = self.slow_interval.tick() => {
                // Handle low-frequency tasks (every 5s)
                self.process_low_frequency_work();
            }
        }
        Ok(())
    }
    ```

*   **Consuming Events:** The `on_run` method is ideal for processing events from channels or streams:

    ```rust
    struct EventProcessorActor {
        events_rx: mpsc::Receiver<Event>,
    }

    impl Actor for EventProcessorActor {
        // ...

        async fn on_run(&mut self, _actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
            tokio::select! {
                Some(event) = self.events_rx.recv() => {
                    self.process_event(event)?;
                }
                else => {
                    // Channel closed, stop the actor
                    return Err(anyhow::anyhow!("Event channel closed"));
                }
            }
            Ok(())
        }
    }
    ```

*   **Background Processing:** Use `on_run` for continuous background processing tasks:

    ```rust
    async fn on_run(&mut self, _actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // Process one batch of work items
        if let Some(work_item) = self.queue.pop() {
            self.process_work_item(work_item)?;
        } else {
            // No work to do right now, add a small delay to avoid busy waiting
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
    ```

*   **Combine multiple sources with `tokio::select!`:** You can wait on multiple event sources concurrently:

    ```rust
    async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
        tokio::select! {
            Some(msg) = self.command_rx.recv() => {
                self.handle_command(msg)?;
            }
            _ = self.health_check_interval.tick() => {
                self.perform_health_check()?;
            }
            Ok(()) = self.check_resource_limits() => {
                // Resources are within limits, continue
            }
            else => {
                // All channels are closed
                actor_ref.stop().await?;
            }
        }
        Ok(())
    }
    ```

**Q23: How do I handle backpressure in actors?**

A23: Backpressure is important to prevent overwhelming actors with more messages than they can process. `rsActor` provides several techniques:

1.  **Mailbox Capacity**:
    When spawning an actor, you can specify the mailbox size:

    ```rust
    let mailbox_capacity = 100;
    let (actor_ref, handle) = spawn_with_mailbox_capacity::<MyActor>(args, mailbox_capacity);
    ```

    When the mailbox is full, `ask` and `tell` operations will return an error, allowing the sender to implement backpressure strategies.

2.  **Rate Limiting**:
    Implement rate limiting within the actor:

    ```rust
    struct RateLimitedActor {
        limiter: RateLimiter,
        // ...
    }

    impl Actor for RateLimitedActor {
        // ...

        async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Self {
                limiter: RateLimiter::new(args.rate),
                // ...
            })
        }
    }

    impl Message<ProcessRequest> for RateLimitedActor {
        type Reply = Result<Response, Error>;

        async fn handle(&mut self, msg: ProcessRequest, _actor_ref: &ActorRef<Self>) -> Self::Reply {
            // Wait for rate limiter to allow processing
            self.limiter.acquire_one().await;

            // Process the request
            self.process(msg)
        }
    }
    ```

3.  **Flow Control with Acknowledgments**:
    Use explicit acknowledgments to implement flow control:

    ```rust
    // Sender side
    for item in items {
        actor_ref.ask(ProcessItem { item }).await?;
        // Wait for acknowledgment before sending next item
    }

    // Actor side
    impl Message<ProcessItem> for ProcessingActor {
        type Reply = ();

        async fn handle(&mut self, msg: ProcessItem, _actor_ref: &ActorRef<Self>) -> Self::Reply {
            self.process(msg.item);
            // Return empty acknowledgment
        }
    }
    ```

4.  **Batching**:
    Process items in batches to reduce message overhead:

    ```rust
    // Instead of sending individual items:
    actor_ref.ask(ProcessBatch { items: batch_of_items }).await?;

    // Actor handles batches more efficiently:
    impl Message<ProcessBatch> for BatchProcessorActor {
        type Reply = BatchResult;

        async fn handle(&mut self, msg: ProcessBatch, _actor_ref: &ActorRef<Self>) -> Self::Reply {
            // Process entire batch at once
            self.process_batch(msg.items)
        }
    }
    ```

**Q24: How does rsActor handle type safety for messages and actors?**

A24: rsActor provides a comprehensive type safety system for actor messaging through two complementary approaches:

1. **Compile-time Type Safety with `ActorRef<T>`**:
   - The primary actor reference type you'll use in most cases
   - Fully leverages Rust's type system for static verification
   - Only allows sending messages that the actor has explicitly implemented handlers for
   - Automatically infers and enforces correct return types based on message handler implementations
   - Compiler errors occur if you attempt to send an unhandled message type
   - Zero runtime overhead for type checking
   - Example:
     ```rust
     // This will compile only if CounterActor implements Message<IncrementMsg>
     let new_count: u32 = actor_ref.ask(IncrementMsg(5)).await?;

     // This would be a compile-time error if CounterActor doesn't handle ResetMsg
     actor_ref.tell(ResetMsg).await?;
     ```

2. **Type-Erased Actor Management with Traits**:
   - `ActorControl` trait: Type-erased lifecycle management (stop, kill, is_alive)
   - `TellHandler<M>` / `AskHandler<M, R>`: Type-erased message sending for specific message types
   - Useful for storing different actor types in collections while maintaining message type safety
   - Example:
     ```rust
     use rsactor::{ActorControl, TellHandler, AskHandler};

     // Store different actor types with unified lifecycle control
     let controls: Vec<Box<dyn ActorControl>> = vec![
         (&actor1).into(),
         (&actor2).into(),
     ];

     // Stop all actors gracefully
     for control in &controls {
         control.stop().await?;
     }

     // Or store handlers for specific message types
     let handlers: Vec<Box<dyn TellHandler<PingMsg>>> = vec![
         (&actor1).into(),
         (&actor2).into(),
     ];

     // Send same message to all actors that handle it
     for handler in &handlers {
         handler.tell(PingMsg).await?;
     }
     ```

The combination of compile-time type safety with `ActorRef<T>` and type-erased traits provides both safety and flexibility, making it suitable for a wide range of actor system designs.

*This FAQ is based on the state of the `rsActor` project as of its `README.md` and `src/lib.rs` on May 25, 2025. Features and behaviors may change in future versions.*
