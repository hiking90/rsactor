// Advanced example showing different ways to use the Actor derive macro

use rsactor::{message_handlers, spawn, Actor, ActorRef};

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

#[message_handlers]
impl SimpleActor {
    #[handler]
    async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> String {
        self.name.clone()
    }

    #[handler]
    async fn handle_set_name(&mut self, msg: SetName, _: &ActorRef<Self>) {
        self.name = msg.0;
    }
}

// Messages for CounterActor
struct Increment;
struct Decrement;
struct GetCount;
struct IsAtMax;

#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> u32 {
        if self.count < self.max_count {
            self.count += 1;
        }
        self.count
    }

    #[handler]
    async fn handle_decrement(&mut self, _msg: Decrement, _: &ActorRef<Self>) -> u32 {
        if self.count > 0 {
            self.count -= 1;
        }
        self.count
    }

    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
        self.count
    }

    #[handler]
    async fn handle_is_at_max(&mut self, _msg: IsAtMax, _: &ActorRef<Self>) -> bool {
        self.count >= self.max_count
    }
}

// Messages for WorkerActor
struct DoWork(String);
struct GetTasksCompleted;
struct GetWorkerId;

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_do_work(&mut self, msg: DoWork, _: &ActorRef<Self>) -> String {
        // Simulate some work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        self.tasks_completed += 1;
        format!("Worker {} completed task: {}", self.id, msg.0)
    }

    #[handler]
    async fn handle_get_tasks_completed(
        &mut self,
        _msg: GetTasksCompleted,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.tasks_completed
    }

    #[handler]
    async fn handle_get_worker_id(&mut self, _msg: GetWorkerId, _: &ActorRef<Self>) -> u32 {
        self.id
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("🚀 Advanced Actor Derive Macro Demo");

    // Example 1: Simple Actor
    println!("\n📝 Example 1: Simple Actor");
    let simple_actor = SimpleActor {
        name: "Alice".to_string(),
    };
    let (simple_ref, _simple_handle) = spawn::<SimpleActor>(simple_actor);

    let name = simple_ref.ask(GetName).await?;
    println!("Initial name: {name}");

    simple_ref.tell(SetName("Bob".to_string())).await?;
    let new_name = simple_ref.ask(GetName).await?;
    println!("Updated name: {new_name}");

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
        println!("Increment {i}: count = {count}, at_max = {at_max}");
    }

    // Decrement
    let count = counter_ref.ask(Decrement).await?;
    println!("After decrement: {count}");

    // Example 3: Worker Actor
    println!("\n⚙️  Example 3: Worker Actor");
    let worker_actor = WorkerActor {
        id: 42,
        tasks_completed: 0,
    };
    let (worker_ref, _worker_handle) = spawn::<WorkerActor>(worker_actor);

    let worker_id = worker_ref.ask(GetWorkerId).await?;
    println!("Worker ID: {worker_id}");

    // Assign some tasks
    let tasks = vec![
        "Process data batch 1",
        "Generate report",
        "Send notifications",
        "Update database",
    ];

    for task in tasks {
        let result = worker_ref.ask(DoWork(task.to_string())).await?;
        println!("Result: {result}");
    }

    let total_tasks = worker_ref.ask(GetTasksCompleted).await?;
    println!("Total tasks completed: {total_tasks}");

    // Cleanup
    simple_ref.stop().await?;
    counter_ref.stop().await?;
    worker_ref.stop().await?;

    println!("\n✨ All actors stopped successfully!");

    Ok(())
}
