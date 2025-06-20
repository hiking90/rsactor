// Advanced example showing different ways to use the Actor derive macro

use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};

// Simple actor with derive
#[derive(Actor, Debug)]
struct SimpleActor {
    name: String,
}

// Counter actor with derive
#[derive(Actor, Debug)]
struct CounterActor {
    count: u32,
    max_count: u32,
}

// Worker actor with derive
#[derive(Actor, Debug)]
struct WorkerActor {
    id: u32,
    tasks_completed: u32,
}

// Messages for SimpleActor
struct GetName;
struct SetName(String);

impl Message<GetName> for SimpleActor {
    type Reply = String;

    async fn handle(&mut self, _msg: GetName, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.name.clone()
    }
}

impl Message<SetName> for SimpleActor {
    type Reply = ();

    async fn handle(&mut self, msg: SetName, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.name = msg.0;
    }
}

impl_message_handler!(SimpleActor, [GetName, SetName]);

// Messages for CounterActor
struct Increment;
struct Decrement;
struct GetCount;
struct IsAtMax;

impl Message<Increment> for CounterActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: Increment, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        if self.count < self.max_count {
            self.count += 1;
        }
        self.count
    }
}

impl Message<Decrement> for CounterActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: Decrement, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        if self.count > 0 {
            self.count -= 1;
        }
        self.count
    }
}

impl Message<GetCount> for CounterActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: GetCount, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count
    }
}

impl Message<IsAtMax> for CounterActor {
    type Reply = bool;

    async fn handle(&mut self, _msg: IsAtMax, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count >= self.max_count
    }
}

impl_message_handler!(CounterActor, [Increment, Decrement, GetCount, IsAtMax]);

// Messages for WorkerActor
struct DoWork(String);
struct GetTasksCompleted;
struct GetWorkerId;

impl Message<DoWork> for WorkerActor {
    type Reply = String;

    async fn handle(&mut self, msg: DoWork, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        // Simulate some work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        self.tasks_completed += 1;
        format!("Worker {} completed task: {}", self.id, msg.0)
    }
}

impl Message<GetTasksCompleted> for WorkerActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: GetTasksCompleted, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.tasks_completed
    }
}

impl Message<GetWorkerId> for WorkerActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: GetWorkerId, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.id
    }
}

impl_message_handler!(WorkerActor, [DoWork, GetTasksCompleted, GetWorkerId]);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Advanced Actor Derive Macro Demo");

    // Example 1: Simple Actor
    println!("\n📝 Example 1: Simple Actor");
    let simple_actor = SimpleActor {
        name: "Alice".to_string(),
    };
    let (simple_ref, _simple_handle) = spawn::<SimpleActor>(simple_actor);

    let name = simple_ref.ask(GetName).await?;
    println!("Initial name: {}", name);

    simple_ref.tell(SetName("Bob".to_string())).await?;
    let new_name = simple_ref.ask(GetName).await?;
    println!("Updated name: {}", new_name);

    // Example 2: Counter Actor
    println!("\n🔢 Example 2: Counter Actor");
    let counter_actor = CounterActor {
        count: 0,
        max_count: 5,
    };
    let (counter_ref, _counter_handle) = spawn::<CounterActor>(counter_actor);

    println!("Initial count: {}", counter_ref.ask(GetCount).await?);

    // Increment several times
    for i in 1..=7 {
        let count = counter_ref.ask(Increment).await?;
        let at_max = counter_ref.ask(IsAtMax).await?;
        println!("Increment {}: count = {}, at_max = {}", i, count, at_max);
    }

    // Decrement
    let count = counter_ref.ask(Decrement).await?;
    println!("After decrement: {}", count);

    // Example 3: Worker Actor
    println!("\n⚙️  Example 3: Worker Actor");
    let worker_actor = WorkerActor {
        id: 42,
        tasks_completed: 0,
    };
    let (worker_ref, _worker_handle) = spawn::<WorkerActor>(worker_actor);

    let worker_id = worker_ref.ask(GetWorkerId).await?;
    println!("Worker ID: {}", worker_id);

    // Assign some tasks
    let tasks = vec![
        "Process data batch 1",
        "Generate report",
        "Send notifications",
        "Update database",
    ];

    for task in tasks {
        let result = worker_ref.ask(DoWork(task.to_string())).await?;
        println!("Result: {}", result);
    }

    let total_tasks = worker_ref.ask(GetTasksCompleted).await?;
    println!("Total tasks completed: {}", total_tasks);

    // Cleanup
    simple_ref.stop().await?;
    counter_ref.stop().await?;
    worker_ref.stop().await?;

    println!("\n✨ All actors stopped successfully!");

    Ok(())
}
