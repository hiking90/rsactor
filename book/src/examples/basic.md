# Basic Usage

This example demonstrates the fundamental concepts of rsActor through a simple counter actor that maintains internal state and processes messages.

## Overview

The basic example shows:
- How to define an actor with internal state
- Implementing the `Actor` trait with lifecycle methods
- Creating and handling different message types
- Using the `#[message_handlers]` macro
- Spawning actors and managing their lifecycle
- Periodic tasks within actors using `on_run`

## Code Example

```rust
use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};
use tokio::time::{interval, Duration};
use tracing::info;

// Message types
struct Increment; // Message to increment the actor's counter
struct Decrement; // Message to decrement the actor's counter

// Define the actor struct
struct MyActor {
    count: u32,                        // Internal state of the actor
    start_up: std::time::Instant,      // Track the start time
    tick_300ms: tokio::time::Interval, // Interval for 300ms ticks
    tick_1s: tokio::time::Interval,    // Interval for 1s ticks
}

// Implement the Actor trait for MyActor
impl Actor for MyActor {
    type Args = Self;
    type Error = anyhow::Error;

    // Called when the actor is started
    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("MyActor started. Initial count: {}.", args.count);
        Ok(args)
    }

    // Called repeatedly when the message queue is empty (idle handler).
    // Returns Ok(true) to continue calling on_run, Ok(false) to stop idle processing.
    // Note: receives &ActorWeak<Self>, not &ActorRef<Self>.
    async fn on_run(&mut self, _actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        // Use tokio::select! to handle multiple async operations
        tokio::select! {
            _ = self.tick_300ms.tick() => {
                println!("300ms tick. Elapsed: {:?}", self.start_up.elapsed());
            }
            _ = self.tick_1s.tick() => {
                println!("1s tick. Elapsed: {:?}", self.start_up.elapsed());
            }
        }
        Ok(true) // Continue calling on_run
    }
}

// Message handling using the #[message_handlers] macro with #[handler] attributes
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> u32 {
        self.count += 1;
        println!("MyActor handled Increment. Count is now {}.", self.count);
        self.count
    }

    #[handler]
    async fn handle_decrement(&mut self, _msg: Decrement, _: &ActorRef<Self>) -> u32 {
        self.count -= 1;
        println!("MyActor handled Decrement. Count is now {}.", self.count);
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Create actor instance with initial state
    let my_actor = MyActor {
        count: 100,
        start_up: std::time::Instant::now(),
        tick_300ms: interval(Duration::from_millis(300)),
        tick_1s: interval(Duration::from_secs(1)),
    };

    // Spawn the actor
    let (actor_ref, join_handle) = rsactor::spawn::<MyActor>(my_actor);
    println!("MyActor spawned with ID: {}", actor_ref.identity());

    // Wait a bit to see some ticks
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Send messages and await replies
    println!("Sending Increment message...");
    let count_after_inc: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment: {}", count_after_inc);

    println!("Sending Decrement message...");
    let count_after_dec: u32 = actor_ref.ask(Decrement).await?;
    println!("Reply after Decrement: {}", count_after_dec);

    // Wait for more ticks
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Stop the actor gracefully
    println!("Stopping actor...");
    actor_ref.stop().await?;

    // Wait for completion and check result
    let result = join_handle.await?;
    match result {
        rsactor::ActorResult::Completed { actor, killed } => {
            println!("Actor completed. Final count: {}. Killed: {}",
                     actor.count, killed);
        }
        rsactor::ActorResult::Failed { actor, error, phase, killed } => {
            println!("Actor failed: {}. Phase: {:?}, Killed: {}",
                     error, phase, killed);
            if let Some(actor) = actor {
                println!("Final count: {}", actor.count);
            }
        }
    }

    Ok(())
}
```

## Key Concepts Demonstrated

### 1. Actor State Management
The `MyActor` struct encapsulates:
- `count`: The main business state
- `start_up`: Timing information
- `tick_300ms` and `tick_1s`: Periodic timers

### 2. Lifecycle Methods
- **`on_start`**: Initializes the actor when spawned
- **`on_run`**: Runs continuously, handling periodic tasks with `tokio::select!`

### 3. Message Handling
- `Increment` and `Decrement` messages modify the actor's state
- Each message handler returns the new count value
- Messages are processed sequentially, ensuring thread safety

### 4. Actor Communication
- **`ask`**: Send a message and wait for a reply
- **`tell`**: Send a message without waiting (fire-and-forget)
- **`stop`**: Gracefully shutdown the actor

### 5. Error Handling
The example demonstrates proper error handling throughout the actor lifecycle and shows how to access the final actor state after completion.

## Running the Example

```bash
cargo run --example basic
```

You'll see output showing the periodic ticks, message processing, and graceful shutdown.

## Next Steps

This basic example forms the foundation for more complex actor patterns. Next, explore:
- [Async Worker](async_worker.md) for I/O-intensive tasks
- [Blocking Task](blocking_task.md) for CPU-intensive operations
- [Actor with Timeout](actor_with_timeout.md) for timeout handling
