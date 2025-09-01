// Tests for function/closure/future message types with the message_handlers macro

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::time::timeout;

// Test actor for function message handlers
#[derive(Debug)]
struct FunctionHandlerTestActor {
    counter: u32,
    results: Vec<String>,
}

impl Actor for FunctionHandlerTestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(
        _args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(FunctionHandlerTestActor {
            counter: 0,
            results: Vec::new(),
        })
    }
}

// Test message types that are function-like or futures

// 1. Function pointer type
type SimpleFunction = fn(u32) -> u32;

// 2. Boxed closure type (FnOnce)
type BoxedFnOnce = Box<dyn FnOnce(u32) -> u32 + Send>;

// 3. Boxed closure type (Fn)
type BoxedFn = Box<dyn Fn(u32) -> u32 + Send + Sync>;

// 4. Boxed async closure type
type BoxedAsyncFn = Box<dyn Fn(u32) -> Pin<Box<dyn Future<Output = u32> + Send>> + Send + Sync>;

// 5. Pinned future type
type PinnedFuture = Pin<Box<dyn Future<Output = String> + Send>>;

// 6. Arc wrapper for shared functions
type SharedFunction = Arc<dyn Fn(u32) -> u32 + Send + Sync>;

// 7. Complex function type with multiple parameters
type ComplexFunction = fn(&str, u32, bool) -> String;

// 8. Future trait object
type FutureTraitObject = Pin<Box<dyn Future<Output = i32> + Send>>;

// 9. Generic function wrapper
#[derive(Debug)]
struct FunctionWrapper<F> {
    func: F,
    name: String,
}

// 10. Command pattern with execute method
trait Command: Send + Sync {
    fn execute(&self, input: u32) -> u32;
}

#[derive(Debug)]
struct SimpleCommand {
    multiplier: u32,
}

impl Command for SimpleCommand {
    fn execute(&self, input: u32) -> u32 {
        input * self.multiplier
    }
}

type BoxedCommand = Box<dyn Command>;

// Message handlers implementation
#[message_handlers]
impl FunctionHandlerTestActor {
    #[handler]
    async fn handle_simple_function(&mut self, msg: SimpleFunction, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let result = msg(self.counter);
        self.results
            .push(format!("SimpleFunction: {} -> {}", self.counter, result));
        result
    }

    #[handler]
    async fn handle_boxed_fn_once(&mut self, msg: BoxedFnOnce, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let result = msg(self.counter);
        self.results
            .push(format!("BoxedFnOnce: {} -> {}", self.counter, result));
        result
    }

    #[handler]
    async fn handle_boxed_fn(&mut self, msg: BoxedFn, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let result = msg(self.counter);
        self.results
            .push(format!("BoxedFn: {} -> {}", self.counter, result));
        result
    }

    #[handler]
    async fn handle_boxed_async_fn(&mut self, msg: BoxedAsyncFn, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let future = msg(self.counter);
        let result = future.await;
        self.results
            .push(format!("BoxedAsyncFn: {} -> {}", self.counter, result));
        result
    }

    #[handler]
    async fn handle_pinned_future(&mut self, msg: PinnedFuture, _: &ActorRef<Self>) -> String {
        self.counter += 1;
        let result = msg.await;
        self.results.push(format!("PinnedFuture: {}", result));
        result
    }

    #[handler]
    async fn handle_shared_function(&mut self, msg: SharedFunction, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let result = msg(self.counter);
        self.results
            .push(format!("SharedFunction: {} -> {}", self.counter, result));
        result
    }

    #[handler]
    async fn handle_complex_function(
        &mut self,
        msg: ComplexFunction,
        _: &ActorRef<Self>,
    ) -> String {
        self.counter += 1;
        let result = msg("test", self.counter, true);
        self.results.push(format!("ComplexFunction: {}", result));
        result
    }

    #[handler]
    async fn handle_future_trait_object(
        &mut self,
        msg: FutureTraitObject,
        _: &ActorRef<Self>,
    ) -> i32 {
        self.counter += 1;
        let result = msg.await;
        self.results.push(format!("FutureTraitObject: {}", result));
        result
    }

    #[handler]
    async fn handle_function_wrapper_simple(
        &mut self,
        msg: FunctionWrapper<fn(u32) -> u32>,
        _: &ActorRef<Self>,
    ) -> String {
        self.counter += 1;
        let result = (msg.func)(self.counter);
        let response = format!("{}: {} -> {}", msg.name, self.counter, result);
        self.results.push(response.clone());
        response
    }

    #[handler]
    async fn handle_function_wrapper_boxed(
        &mut self,
        msg: FunctionWrapper<Box<dyn Fn(u32) -> u32 + Send + Sync>>,
        _: &ActorRef<Self>,
    ) -> String {
        self.counter += 1;
        let result = (msg.func)(self.counter);
        let response = format!("{}: {} -> {}", msg.name, self.counter, result);
        self.results.push(response.clone());
        response
    }

    #[handler]
    async fn handle_boxed_command(&mut self, msg: BoxedCommand, _: &ActorRef<Self>) -> u32 {
        self.counter += 1;
        let result = msg.execute(self.counter);
        self.results
            .push(format!("BoxedCommand: {} -> {}", self.counter, result));
        result
    }

    // Helper handler to get results for testing
    #[handler]
    async fn handle_get_results(&mut self, _msg: (), _: &ActorRef<Self>) -> (u32, Vec<String>) {
        (self.counter, self.results.clone())
    }
}

// Test helper functions
fn double(x: u32) -> u32 {
    x * 2
}

fn square(x: u32) -> u32 {
    x * x
}

fn format_complex(prefix: &str, num: u32, flag: bool) -> String {
    format!("{prefix}_{num}_{flag}")
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_simple_function_pointer() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with function pointer
        let result: u32 = actor_ref.ask(double as SimpleFunction).await.unwrap();
        assert_eq!(result, 2); // double(1) = 2

        let result: u32 = actor_ref.ask(square as SimpleFunction).await.unwrap();
        assert_eq!(result, 4); // square(2) = 4

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 2);
        assert_eq!(results.len(), 2);
        assert!(results[0].contains("SimpleFunction: 1 -> 2"));
        assert!(results[1].contains("SimpleFunction: 2 -> 4"));
    }

    #[tokio::test]
    async fn test_boxed_fn_once() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with boxed FnOnce
        let closure: BoxedFnOnce = Box::new(|x| x + 10);
        let result: u32 = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, 11); // 1 + 10 = 11

        let closure2: BoxedFnOnce = Box::new(|x| x * 3);
        let result: u32 = actor_ref.ask(closure2).await.unwrap();
        assert_eq!(result, 6); // 2 * 3 = 6

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 2);
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_boxed_fn() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with boxed Fn
        let closure: BoxedFn = Box::new(|x| x + 5);
        let result: u32 = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, 6); // 1 + 5 = 6

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("BoxedFn: 1 -> 6"));
    }

    #[tokio::test]
    async fn test_boxed_async_fn() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with boxed async Fn
        let async_closure: BoxedAsyncFn = Box::new(|x| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                x * 4
            })
        });

        let result: u32 = actor_ref.ask(async_closure).await.unwrap();
        assert_eq!(result, 4); // 1 * 4 = 4

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("BoxedAsyncFn: 1 -> 4"));
    }

    #[tokio::test]
    async fn test_pinned_future() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with pinned future
        let future: PinnedFuture = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            "async_result".to_string()
        });

        let result: String = actor_ref.ask(future).await.unwrap();
        assert_eq!(result, "async_result");

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("PinnedFuture: async_result"));
    }

    #[tokio::test]
    async fn test_shared_function() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with Arc-wrapped function
        let shared_fn: SharedFunction = Arc::new(|x| x + 100);
        let result: u32 = actor_ref.ask(shared_fn).await.unwrap();
        assert_eq!(result, 101); // 1 + 100 = 101

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("SharedFunction: 1 -> 101"));
    }

    #[tokio::test]
    async fn test_complex_function() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with complex function signature
        let result: String = actor_ref
            .ask(format_complex as ComplexFunction)
            .await
            .unwrap();
        assert_eq!(result, "test_1_true");

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("ComplexFunction: test_1_true"));
    }

    #[tokio::test]
    async fn test_future_trait_object() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with future trait object
        let future: FutureTraitObject = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            42
        });

        let result: i32 = actor_ref.ask(future).await.unwrap();
        assert_eq!(result, 42);

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("FutureTraitObject: 42"));
    }

    #[tokio::test]
    async fn test_function_wrapper_simple() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with function wrapper
        let wrapper = FunctionWrapper {
            func: double as fn(u32) -> u32,
            name: "double_function".to_string(),
        };

        let result: String = actor_ref.ask(wrapper).await.unwrap();
        assert_eq!(result, "double_function: 1 -> 2");

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_function_wrapper_boxed() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with boxed function wrapper
        let wrapper = FunctionWrapper {
            func: Box::new(|x| x * 7u32) as Box<dyn Fn(u32) -> u32 + Send + Sync>,
            name: "multiply_by_seven".to_string(),
        };

        let result: String = actor_ref.ask(wrapper).await.unwrap();
        assert_eq!(result, "multiply_by_seven: 1 -> 7");

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_boxed_command() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test with command pattern
        let command: BoxedCommand = Box::new(SimpleCommand { multiplier: 5 });
        let result: u32 = actor_ref.ask(command).await.unwrap();
        assert_eq!(result, 5); // 1 * 5 = 5

        let (counter, results): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("BoxedCommand: 1 -> 5"));
    }

    #[tokio::test]
    async fn test_multiple_function_types_concurrently() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test multiple function types concurrently
        let future1 = actor_ref.ask(double as SimpleFunction);
        let future2 = actor_ref.ask(Box::new(|x| x + 20u32) as BoxedFnOnce);
        let future3 = actor_ref.ask(Arc::new(|x| x * 10u32) as SharedFunction);

        let results = tokio::join!(future1, future2, future3);

        assert_eq!(results.0.unwrap(), 2); // double(1)
        assert_eq!(results.1.unwrap(), 22); // 2 + 20
        assert_eq!(results.2.unwrap(), 30); // 3 * 10

        let (counter, _): (u32, Vec<String>) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 3);
    }

    #[tokio::test]
    async fn test_timeout_with_async_functions() {
        let (actor_ref, _join_handle) = spawn::<FunctionHandlerTestActor>(());

        // Test timeout behavior with async functions
        let slow_async_fn: BoxedAsyncFn = Box::new(|x| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                x * 2
            })
        });

        let result = timeout(Duration::from_millis(200), actor_ref.ask(slow_async_fn)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), 2);

        // Test with very short timeout
        let very_slow_async_fn: BoxedAsyncFn = Box::new(|x| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                x * 3
            })
        });

        let result = timeout(Duration::from_millis(50), actor_ref.ask(very_slow_async_fn)).await;
        assert!(result.is_err()); // Should timeout
    }
}
