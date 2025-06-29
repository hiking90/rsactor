// An example demonstrating how Actor A can request work from Actor B,
// which processes the work in a spawned async task and sends the results back to Actor A

use anyhow::Result;
use rsactor::{message_handlers, Actor, ActorRef};
use tokio::time::Duration;

// -------------------------------------------------------------------
// Actor A (Requester) - Sends requests to Actor B and handles results
// -------------------------------------------------------------------

struct RequesterActor {
    worker_ref: ActorRef<WorkerActor>,
    received_results: Vec<String>,
}

impl Actor for RequesterActor {
    type Args = ActorRef<WorkerActor>;
    type Error = anyhow::Error;

    async fn on_start(
        args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        println!("RequesterActor started");
        Ok(RequesterActor {
            worker_ref: args,
            received_results: Vec::new(),
        })
    }
}

// Message to request work from Worker
struct RequestWork {
    task_id: usize,
    data: String,
}

// Message received when work is completed
struct WorkCompleted {
    task_id: usize,
    result: String,
}

// Message to get all results received so far
struct GetResults;

#[message_handlers]
impl RequesterActor {
    #[handler]
    async fn handle_request_work(&mut self, msg: RequestWork, actor_ref: &ActorRef<Self>) {
        println!(
            "RequesterActor sending work request for task {}",
            msg.task_id
        );

        // Send request to the worker actor
        let requester = actor_ref.clone();

        // Request the worker to process our task
        self.worker_ref
            .tell(ProcessTask {
                task_id: msg.task_id,
                data: msg.data,
                requester,
            })
            .await
            .expect("Failed to send task to worker");
    }

    #[handler]
    async fn handle_work_completed(&mut self, msg: WorkCompleted, _actor_ref: &ActorRef<Self>) {
        println!(
            "RequesterActor received result for task {}: {}",
            msg.task_id, msg.result
        );
        self.received_results.push(msg.result);
    }

    #[handler]
    async fn handle_get_results(
        &mut self,
        _msg: GetResults,
        _actor_ref: &ActorRef<Self>,
    ) -> Vec<String> {
        self.received_results.clone()
    }
}

// -------------------------------------------------------------------
// Actor B (Worker) - Processes work in tokio tasks and replies back
// -------------------------------------------------------------------

struct WorkerActor;

impl Actor for WorkerActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(
        _args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        println!("WorkerActor started");
        Ok(WorkerActor)
    }
}

// Message to process a task
struct ProcessTask {
    task_id: usize,
    data: String,
    requester: ActorRef<RequesterActor>,
}

#[message_handlers]
impl WorkerActor {
    #[handler]
    async fn handle_process_task(&mut self, msg: ProcessTask, _actor_ref: &ActorRef<Self>) {
        let task_id = msg.task_id;
        let data = msg.data;
        let requester = msg.requester;

        println!("WorkerActor received task {task_id}: {data}");

        // Spawn a task to do the processing asynchronously
        tokio::spawn(async move {
            // Simulate some processing time
            let processing_time = (task_id % 3 + 1) as u64;
            println!(
                "Processing task {task_id} will take {processing_time} seconds"
            );
            tokio::time::sleep(Duration::from_secs(processing_time)).await;

            // Generate a result
            let result = format!(
                "Result of task {task_id} with data '{data}' (took {processing_time}s)"
            );

            // Send the result back to the requester
            match requester.tell(WorkCompleted { task_id, result }).await {
                Ok(_) => println!("Worker sent back result for task {task_id}"),
                Err(e) => eprintln!("Failed to send result for task {task_id}: {e:?}"),
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Create the actors
    let (worker_ref, worker_handle) = rsactor::spawn::<WorkerActor>(());
    let (requester_ref, requester_handle) = rsactor::spawn::<RequesterActor>(worker_ref.clone());

    // Send multiple work requests
    for i in 1..=5 {
        requester_ref
            .tell(RequestWork {
                task_id: i,
                data: format!("Task data {i}"),
            })
            .await?;
    }

    // Wait a bit for all tasks to complete
    println!("Waiting for all tasks to complete...");
    tokio::time::sleep(Duration::from_secs(6)).await;

    // Get all results from the requester
    let results = requester_ref.ask(GetResults).await?;
    println!("\nAll received results:");
    for (i, result) in results.iter().enumerate() {
        println!("{}: {}", i + 1, result);
    }

    // Gracefully stop the actors
    requester_ref.stop().await?;
    worker_ref.stop().await?;

    // Wait for actors to complete
    let _requester_result = requester_handle.await?;
    let _worker_result = worker_handle.await?;

    println!("Example completed successfully");
    Ok(())
}
