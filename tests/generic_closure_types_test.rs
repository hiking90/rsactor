// Test for generic closure types as messages

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};

#[derive(Debug)]
struct GenericClosureTestActor {
    counter: u32,
    results: Vec<String>,
}

impl Actor for GenericClosureTestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericClosureTestActor {
            counter: 0,
            results: Vec::new(),
        })
    }
}

// Test different ways to represent generic closure types
#[message_handlers]
impl GenericClosureTestActor {
    // Test 1: Box<dyn FnOnce() -> Result<u32, E> + Send>
    #[handler]
    async fn handle_boxed_fn_once_result(
        &mut self,
        msg: Box<dyn FnOnce() -> Result<u32, String> + Send>,
        _: &ActorRef<Self>,
    ) -> Result<u32, String> {
        self.counter += 1;
        let result = msg();
        self.results
            .push(format!("BoxedFnOnceResult: {:?}", result));
        result
    }

    // Test 2: Box<dyn FnOnce() -> u32 + Send> - simpler version
    #[handler]
    async fn handle_boxed_fn_once_simple(
        &mut self,
        msg: Box<dyn FnOnce() -> u32 + Send>,
        _: &ActorRef<Self>,
    ) -> u32 {
        self.counter += 1;
        let result = msg();
        self.results.push(format!("BoxedFnOnceSimple: {}", result));
        result
    }

    // Test 3: Box<dyn Fn() -> Result<u32, E> + Send + Sync>
    #[handler]
    async fn handle_boxed_fn_result(
        &mut self,
        msg: Box<dyn Fn() -> Result<u32, String> + Send + Sync>,
        _: &ActorRef<Self>,
    ) -> Result<u32, String> {
        self.counter += 1;
        let result = msg();
        self.results.push(format!("BoxedFnResult: {:?}", result));
        result
    }

    // Test 4: Try with anyhow::Result
    #[handler]
    async fn handle_boxed_fn_anyhow(
        &mut self,
        msg: Box<dyn FnOnce() -> anyhow::Result<u32> + Send>,
        _: &ActorRef<Self>,
    ) -> anyhow::Result<u32> {
        self.counter += 1;
        let result = msg();
        self.results.push(format!("BoxedFnAnyhow: {:?}", result));
        result
    }

    // Test 5: Function that takes parameters and returns Result
    #[handler]
    async fn handle_boxed_fn_with_params(
        &mut self,
        msg: Box<dyn FnOnce(u32) -> Result<String, String> + Send>,
        _: &ActorRef<Self>,
    ) -> Result<String, String> {
        self.counter += 1;
        let result = msg(self.counter);
        self.results
            .push(format!("BoxedFnWithParams: {:?}", result));
        result
    }

    // Helper to get results
    #[handler]
    async fn handle_get_state(&mut self, _: (), _: &ActorRef<Self>) -> (u32, Vec<String>) {
        (self.counter, self.results.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_boxed_fn_once_result() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Test successful closure
        let success_closure =
            Box::new(|| Ok(42u32)) as Box<dyn FnOnce() -> Result<u32, String> + Send>;
        let result = actor_ref.ask(success_closure).await.unwrap();
        assert_eq!(result, Ok(42));

        // Test failing closure
        let fail_closure = Box::new(|| Err("Something went wrong".to_string()))
            as Box<dyn FnOnce() -> Result<u32, String> + Send>;
        let result = actor_ref.ask(fail_closure).await.unwrap();
        assert_eq!(result, Err("Something went wrong".to_string()));

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 2);
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_boxed_fn_once_simple() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        let closure = Box::new(|| 100u32) as Box<dyn FnOnce() -> u32 + Send>;
        let result = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, 100);

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert!(results[0].contains("BoxedFnOnceSimple: 100"));
    }

    #[tokio::test]
    async fn test_boxed_fn_result() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Fn can be called multiple times (unlike FnOnce)
        let closure = Box::new(|| Ok(200u32)) as Box<dyn Fn() -> Result<u32, String> + Send + Sync>;
        let result = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, Ok(200));

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert!(results[0].contains("BoxedFnResult: Ok(200)"));
    }

    #[tokio::test]
    async fn test_boxed_fn_anyhow() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Test with anyhow::Result
        let success_closure =
            Box::new(|| anyhow::Ok(300u32)) as Box<dyn FnOnce() -> anyhow::Result<u32> + Send>;
        let result = actor_ref.ask(success_closure).await.unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 300);

        // Test with anyhow error
        let error_closure = Box::new(|| anyhow::bail!("Anyhow error"))
            as Box<dyn FnOnce() -> anyhow::Result<u32> + Send>;
        let result = actor_ref.ask(error_closure).await.unwrap();
        assert!(result.is_err());

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 2);
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_boxed_fn_with_params() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Closure that takes parameters
        let closure = Box::new(|n: u32| {
            if n > 0 {
                Ok(format!("Number: {}", n))
            } else {
                Err("Number must be positive".to_string())
            }
        }) as Box<dyn FnOnce(u32) -> Result<String, String> + Send>;

        let result = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, Ok("Number: 1".to_string()));

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert!(results[0].contains("Number: 1"));
    }

    #[tokio::test]
    async fn test_closure_with_captured_values() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Closure that captures values from environment
        let multiplier = 5u32;
        let base_value = 10u32;

        let closure = Box::new(move || {
            let result = base_value * multiplier;
            if result > 40 {
                Ok(result)
            } else {
                Err(format!("Result {} is too small", result))
            }
        }) as Box<dyn FnOnce() -> Result<u32, String> + Send>;

        let result = actor_ref.ask(closure).await.unwrap();
        assert_eq!(result, Ok(50));

        let (counter, results) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 1);
        assert!(results[0].contains("Ok(50)"));
    }

    #[tokio::test]
    async fn test_multiple_result_types() {
        let (actor_ref, _) = spawn::<GenericClosureTestActor>(());

        // Test different combinations
        let _ = actor_ref
            .ask(Box::new(|| Ok(1u32)) as Box<dyn FnOnce() -> Result<u32, String> + Send>)
            .await
            .unwrap();
        let _ = actor_ref
            .ask(Box::new(|| 2u32) as Box<dyn FnOnce() -> u32 + Send>)
            .await
            .unwrap();
        let _ = actor_ref
            .ask(Box::new(|| anyhow::Ok(3u32)) as Box<dyn FnOnce() -> anyhow::Result<u32> + Send>)
            .await
            .unwrap();

        let (counter, _) = actor_ref.ask(()).await.unwrap();
        assert_eq!(counter, 3);
    }
}
