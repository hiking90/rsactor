# Frequently Asked Questions

This FAQ provides answers to common questions about the rsActor framework, covering basic concepts, practical usage patterns, and advanced scenarios.

## Getting Started

### What is rsActor?

rsActor is a lightweight, async-first Rust actor framework that provides a simple and type-safe way to build concurrent applications using the actor model. It leverages Tokio for async runtime and provides both compile-time and runtime type safety for message passing.

### How do I create my first actor?

To create an actor, you need to implement the `Actor` trait and define message handlers:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};

// Define your actor struct
#[derive(Debug)]
struct CounterActor {
    count: u32,
}

// Implement the Actor trait
impl Actor for CounterActor {
    type Args = u32; // Initial count value
    type Error = anyhow::Error;

    async fn on_start(initial_count: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(CounterActor { count: initial_count })
    }
}

// Define a message
#[derive(Debug)]
struct Increment(u32);

// Use message_handlers macro with handler attributes
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, msg: Increment, _: &ActorRef<Self>) -> u32 {
        self.count += msg.0;
        self.count
    }
}

// Usage
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (actor_ref, _handle) = spawn::<CounterActor>(0);
    let new_count = actor_ref.ask(Increment(5)).await?;
    println!("Count: {}", new_count); // Prints: Count: 5
    Ok(())
}
```

### What is the difference between `ask` and `tell`?

- **`ask`**: Sends a message and waits for a response. Returns the reply value.
- **`tell`**: Sends a message without waiting for a response. Fire-and-forget style.

```rust
// Ask - waits for response
let result = actor_ref.ask(GetValue).await?;

// Tell - no response expected
actor_ref.tell(LogMessage("Hello".to_string())).await?;
```

### How do I handle actor failures?

Actors can fail during their lifecycle. The `JoinHandle` returns an `ActorResult` that indicates success or failure:

```rust
let (actor_ref, handle) = spawn::<MyActor>(args);

match handle.await {
    Ok(ActorResult::Completed { actor, killed }) => {
        println!("Actor completed successfully");
    }
    Ok(ActorResult::Failed { error, phase, .. }) => {
        println!("Actor failed during {:?}: {}", phase, error);
    }
    Err(join_error) => {
        println!("Actor panicked: {}", join_error);
    }
}
```

## Actor System

### How do I spawn actors?

Use the `spawn` function to create and start actors:

```rust
use rsactor::spawn;

// Spawn with default mailbox size
let (actor_ref, handle) = spawn::<MyActor>(actor_args);

// Spawn with custom mailbox size
let (actor_ref, handle) = spawn_with_mailbox_capacity::<MyActor>(actor_args, 1000);
```

### How do I stop an actor?

Actors can be stopped gracefully or killed immediately:

```rust
// Graceful stop - allows ongoing operations to complete
actor_ref.stop().await?;

// Immediate kill - stops the actor immediately
actor_ref.kill()?;
```

### Can I send messages to stopped actors?

No, sending messages to stopped actors will return an `Error::Send`. You can check if an actor is alive, or handle the potential error:

```rust
if actor_ref.is_alive() {
    actor_ref.tell(msg).await?;
}
```

## Message Handling

### How do I define message types?

Messages are regular Rust structs:

```rust
#[derive(Debug, Clone)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Debug)]
struct GetUser { id: u64 }

#[derive(Debug)]
struct DeleteUser { id: u64 }
```

### How do I implement message handlers?

Use the `#[message_handlers]` macro with `#[handler]` attributes:

```rust
#[message_handlers]
impl UserManagerActor {
    #[handler]
    async fn handle_create_user(&mut self, msg: CreateUser, _: &ActorRef<Self>) -> Result<User, UserError> {
        let user = User { id: self.next_id(), name: msg.name, email: msg.email };
        self.users.insert(user.id, user.clone());
        Ok(user)
    }

    #[handler]
    async fn handle_get_user(&mut self, msg: GetUser, _: &ActorRef<Self>) -> Option<User> {
        self.users.get(&msg.id).cloned()
    }

    #[handler]
    async fn handle_delete_user(&mut self, msg: DeleteUser, _: &ActorRef<Self>) -> bool {
        self.users.remove(&msg.id).is_some()
    }
}
```

### What happens if I forget to add a handler?

If you forget to add a `#[handler]` attribute to a method that should handle messages, you'll get a compile-time error when trying to send that message type to the actor.

## Actor Lifecycle

### What is the lifecycle of an actor?

Actors follow a three-stage lifecycle:

1. **Creation and Initialization**:
   - Actor is spawned with `spawn::<Actor>(args)`
   - Framework calls `on_start(args, &ActorRef<Self>)` to create the actor instance
   - If successful, actor enters the running state

2. **Running**:
   - Framework repeatedly calls `on_run(&mut self, &ActorWeak<Self>)` for continuous processing
   - Actor concurrently processes messages from its mailbox
   - Continues until stopped, killed, or error occurs

3. **Termination**:
   - Framework calls `on_stop(&mut self, &ActorWeak<Self>, killed: bool)`
   - Actor is destroyed and `JoinHandle` resolves with `ActorResult`

### What are the lifecycle methods?

```rust
impl Actor for MyActor {
    type Args = MyArgs;
    type Error = MyError;

    // Called during initialization — creates and returns the actor instance
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(MyActor::new(args))
    }

    // Called repeatedly when idle — returns Ok(true) to continue, Ok(false) to stop idle processing
    async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        // Main processing logic
        Ok(false)
    }

    // Called during termination — cleanup and finalization
    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        // Cleanup logic
        Ok(())
    }
}
```

Note: `on_run` and `on_stop` receive `&ActorWeak<Self>` (not `&ActorRef<Self>`) to prevent the actor from holding a strong reference to itself.

### What is the `ActorResult` enum?

`ActorResult` represents the outcome of an actor's lifecycle:

- **`ActorResult::Completed { actor, killed }`**: Actor completed successfully
- **`ActorResult::Failed { actor, error, phase, killed }`**: Actor failed with error details including the failure phase (`OnStart`, `OnRun`, or `OnStop`)

## Error Handling

### How do I handle errors in actors?

Error handling occurs at multiple levels:

```rust
impl Actor for MyActor {
    type Error = MyError;

    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // Return Err to prevent actor creation
        Ok(MyActor::new(args)?)
    }

    async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        // Return Err to terminate actor
        self.do_work()?;
        Ok(false)
    }
}

// Message handling errors
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_process(&mut self, msg: ProcessData, _: &ActorRef<Self>) -> Result<Output, ProcessingError> {
        self.process(msg.data)
    }
}
```

### How do I observe tell() errors?

Use the `on_tell_result` hook. When using `#[handler]`, the macro auto-generates a default implementation that logs errors via `tracing::warn!`. Use `#[handler(no_log)]` to suppress this.

For manual `Message<T>` implementations, override `on_tell_result`:

```rust
impl Message<Command> for MyActor {
    type Reply = Result<(), CommandError>;

    async fn handle(&mut self, msg: Command, _: &ActorRef<Self>) -> Self::Reply {
        self.execute(msg)
    }

    fn on_tell_result(result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        if let Err(e) = result {
            tracing::error!("Command failed: {e:?}");
        }
    }
}
```

### Can I use custom error types?

Yes, any error type that implements `Send + Debug + 'static` can be used:

```rust
#[derive(Debug, thiserror::Error)]
enum MyActorError {
    #[error("Database error: {0}")]
    DbError(#[from] sqlx::Error),

    #[error("Network timeout")]
    NetworkTimeout,
}

impl Actor for MyActor {
    type Error = MyActorError;
    // ...
}
```

## Advanced Usage

### Can I use rsActor with blocking code?

Yes, use `tokio::task::spawn_blocking` for blocking operations within handlers. For sending messages from blocking contexts, use `blocking_ask` and `blocking_tell`:

```rust
tokio::task::spawn_blocking(move || {
    // blocking_ask replaces the deprecated ask_blocking (since v0.10.0)
    let result = actor_ref.blocking_ask::<Query, QueryResult>(
        Query { id: 123 },
        Some(Duration::from_secs(5))
    );

    // blocking_tell replaces the deprecated tell_blocking (since v0.10.0)
    actor_ref.blocking_tell(LogEvent("done".into()), None);
});
```

### How do I test actors?

```rust
#[tokio::test]
async fn test_counter_actor() {
    let (actor_ref, _handle) = spawn::<CounterActor>(0);
    let result = actor_ref.ask(Increment(5)).await.unwrap();
    assert_eq!(result, 5);
}
```

For dead letter testing, enable the `test-utils` feature:

```rust
use rsactor::{dead_letter_count, reset_dead_letter_count};

#[tokio::test]
async fn test_dead_letters() {
    reset_dead_letter_count();
    let (actor_ref, handle) = spawn::<MyActor>(args);
    actor_ref.stop().await.unwrap();
    handle.await.unwrap();

    let _ = actor_ref.tell(MyMessage).await;
    assert_eq!(dead_letter_count(), 1);
}
```

### How do I implement actor supervision?

While rsActor doesn't have built-in supervision, you can implement supervision patterns:

```rust
let (child_ref, child_handle) = spawn::<ChildActor>(args);

tokio::spawn(async move {
    match child_handle.await {
        Ok(ActorResult::Completed { .. }) => { /* normal */ }
        Ok(ActorResult::Failed { error, .. }) => {
            // Restart the child
            let (_new_ref, _) = spawn::<ChildActor>(args);
        }
        Err(_) => { /* panicked */ }
    }
});
```

### How do I implement periodic tasks?

Use the `on_run` method with Tokio's time utilities:

```rust
struct PeriodicActor {
    interval: tokio::time::Interval,
}

impl Actor for PeriodicActor {
    type Args = Duration;
    type Error = anyhow::Error;

    async fn on_start(period: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Ok(Self { interval })
    }

    async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        self.interval.tick().await;
        self.perform_periodic_work();
        Ok(false)
    }
}
```

### How do I use handler traits for heterogeneous collections?

Handler traits enable type-erased message sending to different actor types:

```rust
use rsactor::{TellHandler, AskHandler};

// Store different actors that handle the same message type
let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
    (&counter_ref).into(),
    (&logger_ref).into(),
];

// Send to all
for handler in &handlers {
    handler.tell(Ping).await?;
}
```

See [Handler Traits](advanced/handler_traits.md) for details.

### How do I manage actors of different types?

Use `ActorControl` for type-erased lifecycle management:

```rust
use rsactor::ActorControl;

let controls: Vec<Box<dyn ActorControl>> = vec![
    (&worker_ref).into(),
    (&logger_ref).into(),
];

// Stop all actors
for control in &controls {
    control.stop().await?;
}
```

See [Actor Control](advanced/actor_control.md) for details.

### Can I use generics with actors?

Yes, the `#[message_handlers]` macro supports generic actors:

```rust
#[derive(Debug)]
struct GenericActor<T: Send + Debug + Clone + 'static> {
    data: Option<T>,
}

impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> {
    type Args = Option<T>;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { data: args })
    }
}

#[message_handlers]
impl<T: Send + Debug + Clone + 'static> GenericActor<T> {
    #[handler]
    async fn handle_set_value(&mut self, msg: SetValue<T>, _: &ActorRef<Self>) {
        self.data = Some(msg.0);
    }

    #[handler]
    async fn handle_get_value(&mut self, _: GetValue, _: &ActorRef<Self>) -> Option<T> {
        self.data.clone()
    }
}
```

### How does rsActor handle type safety?

rsActor provides comprehensive type safety:

1. **`ActorRef<T>`**: Compile-time type safety — only allows sending messages the actor can handle
2. **Handler traits**: Type-erased at runtime while maintaining message type safety
3. **Automatic inference**: Return types are inferred from handler signatures

```rust
// Compile-time safety
let count: u32 = actor_ref.ask(IncrementMsg(5)).await?; // Type-safe
```

---

*For more detailed information, refer to the complete documentation and examples.*
