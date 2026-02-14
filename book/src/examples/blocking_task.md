# Blocking Task

This example demonstrates how actors can interact with CPU-intensive or blocking operations without blocking the async runtime. It shows the proper way to integrate blocking tasks with the actor system using `tokio::task::spawn_blocking` and the blocking communication APIs.

## Overview

The blocking task pattern is essential when you need to:
- **CPU-intensive computations** that would block the async executor
- **Synchronous I/O operations** with legacy APIs
- **Bridge synchronous and asynchronous code**
- **Prevent blocking the main actor runtime**

## Key Concepts

### Why Use Blocking Tasks?

Tokio's async runtime is designed for I/O-bound operations. CPU-intensive or truly blocking operations can:
- Block the entire async executor
- Prevent other actors from processing messages
- Degrade overall system performance

### Solution: `spawn_blocking`

Tokio provides `spawn_blocking` to run blocking operations on a dedicated thread pool, keeping the main async runtime responsive.

## Architecture

```
Actor ←→ [tokio channels] ←→ Blocking Task
  ↑                              ↓
  └──[blocking API]──────────────┘
     (ask_blocking/tell_blocking)
```

## Implementation

### Message Types

```rust
// Messages for actor communication
struct GetState;
struct SetFactor(f64);
struct ProcessedData {
    value: f64,
    timestamp: std::time::Instant,
}

// Commands for the blocking task
enum TaskCommand {
    ChangeInterval(Duration),
    Stop,
}
```

### Actor with Blocking Task

```rust
struct SyncDataProcessorActor {
    factor: f64,
    latest_value: Option<f64>,
    task_sender: Option<mpsc::UnboundedSender<TaskCommand>>,
    task_handle: Option<task::JoinHandle<()>>,
}

impl Actor for SyncDataProcessorActor {
    type Args = f64; // Initial factor
    type Error = anyhow::Error;

    async fn on_start(factor: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("SyncDataProcessorActor started with factor: {}", factor);

        // Create communication channels
        let (task_sender, task_receiver) = mpsc::unbounded_channel();

        // Spawn the blocking task
        let actor_ref_clone = actor_ref.clone();
        let task_handle = task::spawn_blocking(move || {
            sync_background_task(factor, task_receiver, actor_ref_clone)
        });

        Ok(Self {
            factor,
            latest_value: None,
            task_sender: Some(task_sender),
            task_handle: Some(task_handle),
        })
    }

    async fn on_stop(&mut self, _: &ActorRef<Self>) -> Result<(), Self::Error> {
        info!("SyncDataProcessorActor stopping - sending stop command to background task");

        // Signal the background task to stop
        if let Some(sender) = &self.task_sender {
            if sender.send(TaskCommand::Stop).is_err() {
                debug!("Background task already stopped or receiver dropped");
            }
        }

        // Wait for the background task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(()) => info!("Background task completed successfully"),
                Err(e) => info!("Background task completed with error: {}", e),
            }
        }

        Ok(())
    }
}
```

### Blocking Task Implementation

```rust
fn sync_background_task(
    mut factor: f64,
    mut task_receiver: mpsc::UnboundedReceiver<TaskCommand>,
    actor_ref: ActorRef<SyncDataProcessorActor>,
) {
    info!("Sync background task started");
    let mut interval = Duration::from_millis(500);
    let mut counter = 0.0;

    loop {
        // Check for commands from the actor (non-blocking)
        match task_receiver.try_recv() {
            Ok(TaskCommand::ChangeInterval(new_interval)) => {
                info!("Background task: changing interval to {:?}", new_interval);
                interval = new_interval;
            }
            Ok(TaskCommand::Stop) => {
                info!("Background task: received stop command");
                break;
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                // No command available, continue with normal processing
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                info!("Background task: actor disconnected, stopping");
                break;
            }
        }

        // Simulate CPU-intensive work
        counter += 1.0;
        let processed_value = expensive_calculation(counter, factor);

        // Send result back to actor using blocking API
        let message = ProcessedData {
            value: processed_value,
            timestamp: std::time::Instant::now(),
        };

        if let Err(e) = actor_ref.tell_blocking(message) {
            info!("Background task: failed to send data to actor: {}", e);
            break;
        }

        // Sleep (blocking operation)
        thread::sleep(interval);
    }

    info!("Sync background task finished");
}

fn expensive_calculation(input: f64, factor: f64) -> f64 {
    // Simulate CPU-intensive calculation
    let mut result = input * factor;
    for _ in 0..1000000 {
        result = result.sin().cos().tan();
    }
    result
}
```

### Message Handlers

```rust
impl Message<ProcessedData> for SyncDataProcessorActor {
    type Reply = ();

    async fn handle(&mut self, msg: ProcessedData, _: &ActorRef<Self>) -> Self::Reply {
        debug!("Actor received processed data: value={:.6}, timestamp={:?}",
               msg.value, msg.timestamp);
        self.latest_value = Some(msg.value);
    }
}

impl Message<SetFactor> for SyncDataProcessorActor {
    type Reply = f64; // Return the new factor

    async fn handle(&mut self, msg: SetFactor, _: &ActorRef<Self>) -> Self::Reply {
        info!("Actor: changing factor from {} to {}", self.factor, msg.0.0);
        self.factor = msg.0.0;

        // Could send a command to the background task if needed
        // if let Some(sender) = &self.task_sender {
        //     sender.send(TaskCommand::UpdateFactor(self.factor)).ok();
        // }

        self.factor
    }
}

impl Message<GetState> for SyncDataProcessorActor {
    type Reply = (f64, Option<f64>); // (factor, latest_value)

    async fn handle(&mut self, _: GetState, _: &ActorRef<Self>) -> Self::Reply {
        (self.factor, self.latest_value)
    }
}
```

## Key Patterns

### 1. **Proper Blocking API Usage**
- Use `tell_blocking` and `ask_blocking` only within `spawn_blocking` tasks
- These APIs are designed for Tokio's blocking thread pool
- NOT for use in `std::thread::spawn` or general sync code

### 2. **Communication Channels**
- Use `tokio::sync::mpsc` for actor → blocking task communication
- Use actor messages for blocking task → actor communication
- Separate concerns: commands vs. data

### 3. **Lifecycle Management**
- Spawn blocking tasks in `on_start`
- Clean up in `on_stop` by sending stop commands
- Await task completion to ensure proper cleanup

### 4. **Error Handling**
- Handle channel disconnections gracefully
- Propagate errors appropriately between sync and async contexts
- Use try_recv for non-blocking command checking

## Benefits

### **Runtime Protection**
- Blocking operations don't block the async executor
- Other actors continue processing messages
- Maintains system responsiveness

### **Resource Management**
- Tokio manages the blocking thread pool
- Automatic scaling based on workload
- Proper cleanup when actors stop

### **Flexibility**
- Integrate legacy synchronous code
- Handle CPU-intensive algorithms
- Bridge different execution models

## Usage Example

```rust
#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Spawn the actor with initial factor
    let (actor_ref, join_handle) = rsactor::spawn::<SyncDataProcessorActor>(2.0);

    // Let it run for a bit
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get current state
    let (factor, latest) = actor_ref.ask(GetState).await?;
    println!("Current state: factor={}, latest_value={:?}", factor, latest);

    // Change the factor
    let new_factor = actor_ref.ask(SetFactor(3.5)).await?;
    println!("Changed factor to: {}", new_factor);

    // Let it run with new factor
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Stop the actor
    actor_ref.stop().await?;
    let result = join_handle.await?;
    println!("Actor stopped: {:?}", result);

    Ok(())
}
```

## Running the Example

```bash
cargo run --example actor_blocking_task
```

Output shows:
- Background task processing data continuously
- Actor receiving processed values
- Factor changes affecting calculations
- Proper cleanup on shutdown

## When to Use This Pattern

- **Mathematical Computations**: Heavy algorithms, simulations
- **Legacy Integration**: Wrapping synchronous libraries
- **File Processing**: Large file operations, compression
- **Database Operations**: Synchronous database drivers
- **System Calls**: Direct OS interactions

This pattern enables seamless integration of blocking operations while maintaining the benefits of the actor model and async runtime efficiency.
