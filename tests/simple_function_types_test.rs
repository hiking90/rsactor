// Simple test to verify that message_handlers macro supports various message types
// including function pointers, closures, futures, and trait objects

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::sync::Arc;

#[derive(Debug)]
struct TestActor {
    count: u32,
}

impl Actor for TestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(TestActor { count: 0 })
    }
}

// Test various message types that are not traditional structs/enums
#[message_handlers]
impl TestActor {
    // 1. Function pointer as message
    #[handler]
    async fn handle_function_pointer(&mut self, msg: fn(u32) -> u32, _: &ActorRef<Self>) -> u32 {
        self.count += 1;
        msg(self.count)
    }

    // 2. Boxed closure (FnOnce) as message
    #[handler]
    async fn handle_boxed_closure(
        &mut self,
        msg: Box<dyn FnOnce(u32) -> u32 + Send>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.count += 1;
        msg(self.count)
    }

    // 3. Shared closure (Arc<Fn>) as message
    #[handler]
    async fn handle_shared_closure(
        &mut self,
        msg: Arc<dyn Fn(u32) -> u32 + Send + Sync>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.count += 1;
        msg(self.count)
    }

    // 4. Trait object as message
    #[handler]
    async fn handle_trait_object(
        &mut self,
        msg: Box<dyn Processor + Send>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.count += 1;
        msg.process(self.count)
    }
}

// Helper trait for trait object message
trait Processor {
    fn process(&self, input: u32) -> u32;
}

struct SimpleProcessor {
    multiplier: u32,
}

impl Processor for SimpleProcessor {
    fn process(&self, input: u32) -> u32 {
        input * self.multiplier
    }
}

// Helper functions
fn double(x: u32) -> u32 {
    x * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_function_pointer_message() {
        let (actor_ref, _) = spawn::<TestActor>(());

        let result: u32 = actor_ref.ask(double as fn(u32) -> u32).await.unwrap();
        assert_eq!(result, 2); // double(1) = 2
    }

    #[tokio::test]
    async fn test_boxed_closure_message() {
        let (actor_ref, _) = spawn::<TestActor>(());

        let closure = Box::new(|x| x + 10u32) as Box<dyn FnOnce(u32) -> u32 + Send>;
        let result: u32 = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, 11); // 1 + 10 = 11
    }

    #[tokio::test]
    async fn test_shared_closure_message() {
        let (actor_ref, _) = spawn::<TestActor>(());

        let closure = Arc::new(|x| x * 3u32) as Arc<dyn Fn(u32) -> u32 + Send + Sync>;
        let result: u32 = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, 3); // 1 * 3 = 3
    }

    #[tokio::test]
    async fn test_trait_object_message() {
        let (actor_ref, _) = spawn::<TestActor>(());

        let processor = Box::new(SimpleProcessor { multiplier: 7 }) as Box<dyn Processor + Send>;
        let result: u32 = actor_ref.ask(processor).await.unwrap();
        assert_eq!(result, 7); // 1 * 7 = 7
    }

    #[tokio::test]
    async fn test_all_message_types_work_together() {
        let (actor_ref, _) = spawn::<TestActor>(());

        // Send different types of messages to verify they all work
        let _: u32 = actor_ref.ask(double as fn(u32) -> u32).await.unwrap();
        let _: u32 = actor_ref
            .ask(Box::new(|x| x + 1u32) as Box<dyn FnOnce(u32) -> u32 + Send>)
            .await
            .unwrap();
        let _: u32 = actor_ref
            .ask(Arc::new(|x| x * 2u32) as Arc<dyn Fn(u32) -> u32 + Send + Sync>)
            .await
            .unwrap();

        // All should work without any issues
    }
}
