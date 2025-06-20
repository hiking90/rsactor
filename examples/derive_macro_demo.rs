// Example of using the Actor derive macro

use rsactor::{impl_message_handler, spawn, Actor, ActorRef, Message};

#[derive(Actor)]
struct MyActor {
    name: String,
    count: u32,
}

// Define some messages
struct GetName;
struct Increment;
struct GetCount;

// Implement message handlers
impl Message<GetName> for MyActor {
    type Reply = String;

    async fn handle(&mut self, _msg: GetName, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.name.clone()
    }
}

impl Message<Increment> for MyActor {
    type Reply = ();

    async fn handle(&mut self, _msg: Increment, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count += 1;
    }
}

impl Message<GetCount> for MyActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: GetCount, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.count
    }
}

// Wire up the message handlers
impl_message_handler!(MyActor, [GetName, Increment, GetCount]);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an actor instance
    let actor_instance = MyActor {
        name: "TestActor".to_string(),
        count: 0,
    };

    // Spawn the actor
    let (actor_ref, _join_handle) = spawn::<MyActor>(actor_instance);

    // Test the actor
    let name = actor_ref.ask(GetName).await?;
    println!("Actor name: {}", name);

    let initial_count = actor_ref.ask(GetCount).await?;
    println!("Initial count: {}", initial_count);

    actor_ref.tell(Increment).await?;
    actor_ref.tell(Increment).await?;

    let final_count = actor_ref.ask(GetCount).await?;
    println!("Final count: {}", final_count);

    // Stop the actor
    actor_ref.stop().await?;

    Ok(())
}
