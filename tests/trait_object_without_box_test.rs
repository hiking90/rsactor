// Test for using trait objects without Box wrapper

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::sync::Arc;

#[derive(Debug)]
struct TraitObjectTestActor {
    counter: u32,
}

impl Actor for TraitObjectTestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(TraitObjectTestActor { counter: 0 })
    }
}

#[message_handlers]
impl TraitObjectTestActor {
    // Test 1: Try using Arc instead of Box
    #[handler]
    async fn handle_fn_arc(
        &mut self,
        msg: Arc<dyn Fn(u32) -> u32 + Send + Sync>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // Test 2: Keep the boxed version for comparison
    #[handler]
    async fn handle_fn_once_boxed(
        &mut self,
        msg: Box<dyn FnOnce(u32) -> u32 + Send>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // Test 3: Try with owned function pointer (not trait object)
    #[handler]
    async fn handle_fn_pointer(&mut self, msg: fn(u32) -> u32, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }
}

fn multiply_by_two(x: u32) -> u32 {
    x * 2u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_arc_instead_of_box() {
        let (actor_ref, _) = spawn::<TraitObjectTestActor>(());

        // Test with Arc - this should work
        let arc_closure = Arc::new(|x| x * 2u32) as Arc<dyn Fn(u32) -> u32 + Send + Sync>;
        let result: u32 = actor_ref.ask(arc_closure).await.unwrap();
        assert_eq!(result, 2);
    }

    #[tokio::test]
    async fn test_function_pointer_no_wrapper() {
        let (actor_ref, _) = spawn::<TraitObjectTestActor>(());

        // Test with function pointer - no wrapper needed
        let result: u32 = actor_ref
            .ask(multiply_by_two as fn(u32) -> u32)
            .await
            .unwrap();
        assert_eq!(result, 2);
    }

    #[tokio::test]
    async fn test_boxed_version_still_works() {
        let (actor_ref, _) = spawn::<TraitObjectTestActor>(());

        let boxed_closure = Box::new(|x| x * 2u32) as Box<dyn FnOnce(u32) -> u32 + Send>;
        let result: u32 = actor_ref.ask(boxed_closure).await.unwrap();
        assert_eq!(result, 2);
    }
}
