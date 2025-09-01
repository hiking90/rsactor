// Comprehensive test showing all possible alternatives to Box<dyn Trait>

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::sync::Arc;

#[derive(Debug)]
struct AlternativesTestActor {
    counter: u32,
}

impl Actor for AlternativesTestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(AlternativesTestActor { counter: 0 })
    }
}

// Custom trait for testing
trait Calculator: Send + Sync {
    fn calculate(&self, x: u32) -> u32;
}

struct Doubler;
impl Calculator for Doubler {
    fn calculate(&self, x: u32) -> u32 {
        x * 2
    }
}

struct Adder {
    value: u32,
}
impl Calculator for Adder {
    fn calculate(&self, x: u32) -> u32 {
        x + self.value
    }
}

#[message_handlers]
impl AlternativesTestActor {
    // 1. Box<dyn Trait> - Traditional way ✅
    #[handler]
    async fn handle_boxed_trait(&mut self, msg: Box<dyn Calculator>, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        msg.calculate(self.counter)
    }

    // 2. Arc<dyn Trait> - Shared ownership alternative ✅
    #[handler]
    async fn handle_arc_trait(&mut self, msg: Arc<dyn Calculator>, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        msg.calculate(self.counter)
    }

    // 3. Function pointer - No wrapper needed ✅
    #[handler]
    async fn handle_function_pointer(&mut self, msg: fn(u32) -> u32, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // 4. Box<dyn Fn> - Closure in Box ✅
    #[handler]
    async fn handle_boxed_fn(
        &mut self,
        msg: Box<dyn Fn(u32) -> u32 + Send + Sync>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // 5. Arc<dyn Fn> - Closure in Arc ✅
    #[handler]
    async fn handle_arc_fn(
        &mut self,
        msg: Arc<dyn Fn(u32) -> u32 + Send + Sync>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // 6. Box<dyn FnOnce> - One-time use closure ✅
    #[handler]
    async fn handle_boxed_fn_once(
        &mut self,
        msg: Box<dyn FnOnce(u32) -> u32 + Send>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        msg(self.counter)
    }

    // 7. Concrete type - Direct struct as message ✅
    #[handler]
    async fn handle_concrete_calculator(&mut self, msg: Doubler, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        msg.calculate(self.counter)
    }

    // 8. Enum dispatch - Alternative to trait objects
    #[handler]
    async fn handle_operation_enum(&mut self, msg: Operation, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        match msg {
            Operation::Add(n) => self.counter + n,
            Operation::Multiply(n) => self.counter * n,
        }
    }
}

// Enum as alternative to trait objects
#[derive(Debug, Clone)]
enum Operation {
    Add(u32),
    Multiply(u32),
}

// Helper functions
fn triple(x: u32) -> u32 {
    x * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_all_alternatives_work() {
        let (actor_ref, _) = spawn::<AlternativesTestActor>(());

        // 1. Box<dyn Trait>
        let boxed_calc: Box<dyn Calculator> = Box::new(Doubler);
        let result1: u32 = actor_ref.ask(boxed_calc).await.unwrap();
        assert_eq!(result1, 2); // counter=1, 1*2=2

        // 2. Arc<dyn Trait>
        let arc_calc: Arc<dyn Calculator> = Arc::new(Adder { value: 10 });
        let result2: u32 = actor_ref.ask(arc_calc).await.unwrap();
        assert_eq!(result2, 12); // counter=2, 2+10=12

        // 3. Function pointer
        let result3: u32 = actor_ref.ask(triple as fn(u32) -> u32).await.unwrap();
        assert_eq!(result3, 9); // counter=3, 3*3=9

        // 4. Box<dyn Fn>
        let boxed_fn: Box<dyn Fn(u32) -> u32 + Send + Sync> = Box::new(|x| x + 5);
        let result4: u32 = actor_ref.ask(boxed_fn).await.unwrap();
        assert_eq!(result4, 9); // counter=4, 4+5=9

        // 5. Arc<dyn Fn>
        let arc_fn: Arc<dyn Fn(u32) -> u32 + Send + Sync> = Arc::new(|x| x * 10);
        let result5: u32 = actor_ref.ask(arc_fn).await.unwrap();
        assert_eq!(result5, 50); // counter=5, 5*10=50

        // 6. Box<dyn FnOnce>
        let fn_once: Box<dyn FnOnce(u32) -> u32 + Send> = Box::new(|x| x - 1);
        let result6: u32 = actor_ref.ask(fn_once).await.unwrap();
        assert_eq!(result6, 5); // counter=6, 6-1=5

        // 7. Concrete type (direct struct)
        let doubler = Doubler;
        let result7: u32 = actor_ref.ask(doubler).await.unwrap();
        assert_eq!(result7, 14); // counter=7, 7*2=14

        // 8. Enum dispatch
        let operation = Operation::Add(100);
        let result8: u32 = actor_ref.ask(operation).await.unwrap();
        assert_eq!(result8, 108); // counter=8, 8+100=108
    }

    #[tokio::test]
    async fn test_reusable_arc_trait_objects() {
        let (actor_ref, _) = spawn::<AlternativesTestActor>(());

        // Arc<dyn Trait> can be cloned and reused - trait objects only
        let shared_calc: Arc<dyn Calculator> = Arc::new(Doubler);

        let calc1 = Arc::clone(&shared_calc);
        let result1: u32 = actor_ref.ask(calc1).await.unwrap();
        assert_eq!(result1, 2);

        let calc2 = Arc::clone(&shared_calc);
        let result2: u32 = actor_ref.ask(calc2).await.unwrap();
        assert_eq!(result2, 4);
    }

    #[tokio::test]
    async fn test_enum_vs_trait_object_performance() {
        let (actor_ref, _) = spawn::<AlternativesTestActor>(());

        // Enum dispatch - compile-time known, potentially faster
        let enum_op = Operation::Multiply(5);
        let result1: u32 = actor_ref.ask(enum_op).await.unwrap();

        // Trait object - runtime dispatch
        let trait_obj: Box<dyn Calculator> = Box::new(Adder { value: 4 });
        let result2: u32 = actor_ref.ask(trait_obj).await.unwrap();

        // Both should work, enum might be faster
        assert!(result1 > 0 && result2 > 0);
    }
}
