use futures::future::join;
use rsactor::{ActorRef, Actor, Message, ActorStopReason}; // MODIFIED: Added System
use anyhow::Result;
use log::{info, debug}; // ADDED

// Message types
struct Increment; // Message to increment the actor's counter
struct Decrement; // Message to decrement the actor's counter

// Define the actor struct
struct MyActor {
    count: u32, // Internal state of the actor
}

// Implement the Actor trait for MyActor
impl Actor for MyActor {
    type Error = anyhow::Error; // Define the error type for actor operations

    // Called when the actor is started
    async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
        self.count = 0; // Initialize count on start
        info!(
            "MyActor (id: {}) started. Initial count: {}.",
            actor_ref.id(),
            self.count
        );
        Ok(())
    }

    // Called when the actor is stopped
    async fn on_stop(&mut self, _actor_ref: ActorRef, _stop_reason: &ActorStopReason) -> Result<(), Self::Error> {
        info!("MyActor stopping. Final count: {}.", self.count);
        Ok(())
    }
}

// Implement message handling for Increment
impl Message<Increment> for MyActor {
    type Reply = u32; // Define the reply type for Increment messages

    // Handle the Increment message
    async fn handle(&mut self, _msg: Increment) -> Self::Reply {
        self.count += 1;
        debug!("MyActor handled Increment. Count is now {}.", self.count); // Changed to debug!
        self.count // Return the new count
    }
}

// Implement message handling for Decrement
impl Message<Decrement> for MyActor {
    type Reply = u32; // Define the reply type for Decrement messages

    // Handle the Decrement message
    async fn handle(&mut self, _msg: Decrement) -> Self::Reply {
        self.count -= 1;
        debug!("MyActor handled Decrement. Count is now {}.", self.count); // Changed to debug!
        self.count // Return the new count
    }
}

// A dummy message type for demonstration
struct DummyMessage;
impl Message<DummyMessage> for MyActor {
    type Reply = u32;

    async fn handle(&mut self, _msg: DummyMessage) -> Self::Reply {
        debug!("MyActor handled DummyMessage. Count is now {}.", self.count);
        self.count
    }
}

// Use the macro to implement ErasedMessageHandler for MyActor,
// enabling it to handle the specified message types.
rsactor::impl_message_handler!(MyActor, [Increment, Decrement, DummyMessage]);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init(); // Initialize the logger

    // Create an instance of MyActor.
    // Its `count` field is initialized here, but `on_start` will reset it to 0.
    let actor_instance = MyActor { count: 100 };

    println!("Spawning MyActor...");
    // Spawn the actor. This returns an ActorRef for sending messages
    // and a JoinHandle to await the actor's completion.
    let (actor_ref, join_handle) = rsactor::spawn(actor_instance); // MODIFIED: use system.spawn and await
    println!("MyActor spawned with ref: {:?}", actor_ref);

    println!("Sending Increment message...");
    // Send an Increment message and await the reply using `ask`.
    let count_after_inc: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment: {}", count_after_inc);

    println!("Sending Decrement message...");
    // Send a Decrement message and await the reply.
    let count_after_dec: u32 = actor_ref.ask(Decrement).await?;
    println!("Reply after Decrement: {}", count_after_dec);

    println!("Sending Increment message again...");
    // Send another Increment message.
    let count_after_inc_2: u32 = actor_ref.ask(Increment).await?;
    println!("Reply after Increment again: {}", count_after_inc_2);

    // Signal the actor to stop gracefully.
    // The actor will process any remaining messages in its mailbox before stopping.
    println!("Sending StopGracefully message to actor {}.", actor_ref.id());
    actor_ref.stop().await?; // Corrected method name

    // Wait for the actor's task to complete.
    // `join_handle.await` returns a Result containing a tuple:
    // - The actor instance (allowing access to its final state).
    // - The ActorStopReason indicating why it stopped.
    println!("Waiting for actor {} to stop...", actor_ref.id());
    let (stopped_actor, reason) = join_handle.await?;
    // Successfully retrieved the actor and its stop reason.
    println!(
        "Actor {} stopped. Final count: {}. Reason: {:?}",
        actor_ref.id(), // actor_ref is still in scope here
        stopped_actor.count, // Access the final count from the returned actor instance
        reason // The reason why the actor stopped
    );

    println!("Main function finished.");
    Ok(())
}
