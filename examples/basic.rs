use rsactor::{ActorRef, Actor, Message}; // MODIFIED: Added System
use anyhow::Result;
use log::{info, debug}; // ADDED

// Message types
struct Increment;
struct Decrement;

struct MyActor {
    count: u32,
}

impl Actor for MyActor {
    type Error = anyhow::Error;

    async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
        self.count = 0;
        info!(
            "MyActor (id: {}) started. Initial count: {}.",
            actor_ref.id(),
            self.count
        );
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<(), Self::Error> {
        info!("MyActor stopping. Final count: {}.", self.count);
        Ok(())
    }
}

// MyActor handles Increment messages
impl Message<Increment> for MyActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: Increment) -> Self::Reply {
        self.count += 1;
        debug!("MyActor handled Increment. Count is now {}.", self.count); // Changed to debug!
        self.count
    }
}

// MyActor handles Decrement messages
impl Message<Decrement> for MyActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: Decrement) -> Self::Reply {
        self.count -= 1;
        debug!("MyActor handled Decrement. Count is now {}.", self.count); // Changed to debug!
        self.count
    }
}

struct DummyMessage;
impl Message<DummyMessage> for MyActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: DummyMessage) -> Self::Reply {
        debug!("MyActor handled DummyMessage. Count is now {}.", self.count);
        self.count
    }
}

// Use the macro to implement ErasedMessageHandler for MyActor
rsactor::impl_message_handler!(MyActor, [Increment, Decrement, DummyMessage]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // ADDED: Initialize logger

    // MyActor's count field is initialized here, but on_start will reset it to 0.
    let actor_instance = MyActor { count: 100 };

    println!("Spawning MyActor...");
    let actor_ref = rsactor::spawn(actor_instance).await?; // MODIFIED: use system.spawn
    println!("MyActor spawned with ref: {:?}", actor_ref);

    println!("Sending Increment message...");
    let count_after_inc: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment: {}", count_after_inc);

    println!("Sending Decrement message...");
    let count_after_dec: u32 = actor_ref.ask(Decrement).await?;
    println!("Reply after Decrement: {}", count_after_dec);

    println!("Sending Increment message again...");
    let count_after_inc_2: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment again: {}", count_after_inc_2);

    // To observe the actor's on_stop message, we need to ensure its runtime loop exits.
    // Dropping the actor_ref (and any clones) will close its mailbox channel from the sender side.
    // The runtime's receiver will then yield None, causing the loop to terminate and on_stop to be called.
    println!("Dropping ActorRef to signal shutdown for actor {}.", actor_ref.id());
    drop(actor_ref);

    // Give some time for the actor's task to process the channel closure and shut down gracefully.
    // In a real application, you'd have a more robust system-wide shutdown mechanism.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    println!("Main function finished.");
    Ok(())
}
