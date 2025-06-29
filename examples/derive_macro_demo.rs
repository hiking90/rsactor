// Example of using the Actor derive macro

use rsactor::{message_handlers, spawn, Actor, ActorRef};

#[derive(Actor)]
struct MyActor {
    name: String,
    count: u32,
}

// Define some messages
struct GetName;
struct Increment;
struct GetCount;

// Implement message handlers using the #[message_handlers] macro with #[handler] attributes
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> String {
        self.name.clone()
    }

    #[handler]
    async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) {
        self.count += 1;
    }

    #[handler]
    async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
        self.count
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing if the feature is enabled
    #[cfg(feature = "tracing")]
    {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .init();
        println!("🚀 Derive Macro Demo: Tracing is ENABLED");
        println!("You should see detailed trace logs for all actor operations\n");
    }

    #[cfg(not(feature = "tracing"))]
    {
        env_logger::init();
        println!("📝 Derive Macro Demo: Tracing is DISABLED\n");
    }

    // Create an actor instance
    let actor_instance = MyActor {
        name: "TestActor".to_string(),
        count: 0,
    };

    // Spawn the actor
    let (actor_ref, _join_handle) = spawn::<MyActor>(actor_instance);

    // Test the actor
    let name = actor_ref.ask(GetName).await?;
    println!("Actor name: {name}");

    let initial_count = actor_ref.ask(GetCount).await?;
    println!("Initial count: {initial_count}");

    actor_ref.tell(Increment).await?;
    actor_ref.tell(Increment).await?;

    let final_count = actor_ref.ask(GetCount).await?;
    println!("Final count: {final_count}");

    // Stop the actor
    actor_ref.stop().await?;

    Ok(())
}
