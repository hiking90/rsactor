# Async Worker

This example demonstrates a more complex actor interaction pattern where one actor (Requester) delegates asynchronous work to another actor (Worker). The Worker actor spawns async tasks to process requests and sends results back to the Requester.

## Overview

The async worker pattern is useful when you need:
- **Work delegation** between actors
- **Parallel processing** of multiple requests
- **Async task management** within actors
- **Bidirectional communication** between actors

## Architecture

```
RequesterActor ──[RequestWork]──> WorkerActor
      ↑                              │
      │                              ↓
      └──[WorkResult]──────── [spawns async task]
```

### Key Components

1. **RequesterActor**: Initiates work requests and handles results
2. **WorkerActor**: Receives requests, spawns async tasks, and sends back results
3. **Async Tasks**: Background tasks that perform the actual work

## Implementation

### Message Types

```rust
// Message to request work from Worker
struct RequestWork {
    task_id: usize,
    data: String,
}

// Message containing work results
struct WorkResult {
    task_id: usize,
    result: String,
}
```

### RequesterActor

```rust
struct RequesterActor {
    worker_ref: ActorRef<WorkerActor>,
    received_results: Vec<String>,
}

impl Actor for RequesterActor {
    type Args = ActorRef<WorkerActor>;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        println!("RequesterActor started");
        Ok(RequesterActor {
            worker_ref: args,
            received_results: Vec::new(),
        })
    }
}

impl Message<RequestWork> for RequesterActor {
    type Reply = ();

    async fn handle(&mut self, msg: RequestWork, actor_ref: &ActorRef<Self>) -> Self::Reply {
        println!("RequesterActor sending work request for task {}", msg.task_id);

        // Send request to the worker actor
        let requester = actor_ref.clone();
        let work_msg = ProcessTask {
            task_id: msg.task_id,
            data: msg.data,
            callback: requester,
        };

        if let Err(e) = self.worker_ref.tell(work_msg).await {
            eprintln!("Failed to send work to worker: {}", e);
        }
    }
}

impl Message<WorkResult> for RequesterActor {
    type Reply = ();

    async fn handle(&mut self, msg: WorkResult, _: &ActorRef<Self>) -> Self::Reply {
        println!("RequesterActor received result for task {}: {}",
                 msg.task_id, msg.result);
        self.received_results.push(msg.result);
    }
}
```

### WorkerActor

```rust
struct WorkerActor {
    active_tasks: usize,
}

impl Actor for WorkerActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        println!("WorkerActor started");
        Ok(WorkerActor { active_tasks: 0 })
    }
}

impl Message<ProcessTask> for WorkerActor {
    type Reply = ();

    async fn handle(&mut self, msg: ProcessTask, _: &ActorRef<Self>) -> Self::Reply {
        self.active_tasks += 1;
        println!("WorkerActor processing task {} (active: {})",
                 msg.task_id, self.active_tasks);

        let task_id = msg.task_id;
        let data = msg.data;
        let callback = msg.callback;

        // Spawn an async task to do the work
        tokio::spawn(async move {
            // Simulate some async work (e.g., network request, database query)
            tokio::time::sleep(Duration::from_millis(100 + task_id as u64 * 50)).await;

            let result = format!("Processed: {}", data);
            println!("Task {} completed with result: {}", task_id, result);

            // Send result back to requester
            let work_result = WorkResult { task_id, result };
            if let Err(e) = callback.tell(work_result).await {
                eprintln!("Failed to send result back: {}", e);
            }
        });

        self.active_tasks -= 1;
    }
}
```

## Key Patterns Demonstrated

### 1. **Actor-to-Actor Communication**
- Requester sends work requests to Worker
- Worker sends results back to Requester
- Bidirectional message flow

### 2. **Async Task Spawning**
- Worker spawns `tokio::spawn` tasks for parallel processing
- Tasks run independently of the actor's message processing loop
- Results are sent back via actor references

### 3. **Callback Pattern**
- Work requests include a callback reference (ActorRef)
- Spawned tasks use the callback to return results
- Enables loose coupling between work requesters and processors

### 4. **State Management**
- Each actor maintains its own state independently
- Worker tracks active tasks
- Requester accumulates results

## Benefits

### **Scalability**
- Multiple work requests can be processed in parallel
- Worker actor doesn't block while tasks are running
- Easy to scale by adding more worker actors

### **Fault Isolation**
- Failed tasks don't crash the actors
- Actor supervision can restart failed workers
- Error handling is isolated per task

### **Resource Management**
- Tasks are managed by Tokio's runtime
- Automatic cleanup when tasks complete
- Can implement backpressure by limiting concurrent tasks

## Usage Example

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Spawn worker actor
    let (worker_ref, worker_handle) = rsactor::spawn::<WorkerActor>(());

    // Spawn requester actor with worker reference
    let (requester_ref, requester_handle) = rsactor::spawn::<RequesterActor>(worker_ref);

    // Send multiple work requests
    for i in 0..5 {
        let request = RequestWork {
            task_id: i,
            data: format!("Task data {}", i),
        };
        requester_ref.tell(request).await?;

        // Small delay between requests
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Let the work complete
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Cleanup
    requester_ref.stop().await?;
    worker_ref.stop().await?;

    Ok(())
}
```

## Running the Example

```bash
cargo run --example actor_async_worker
```

You'll see output showing:
- Work requests being sent
- Tasks being processed in parallel
- Results being received back
- Task completion timing

## When to Use This Pattern

- **API Gateways**: Distribute requests to worker services
- **Data Processing**: Parallel processing of data batches
- **I/O Operations**: Handle multiple network/database requests
- **Background Jobs**: Queue and process background tasks
- **Microservices**: Inter-service communication patterns

This pattern demonstrates how actors can coordinate complex asynchronous workflows while maintaining clean separation of concerns and fault tolerance.
