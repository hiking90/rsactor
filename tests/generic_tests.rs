// filepath: /Volumes/Workspace/rust/rsactor/tests/generic_tests.rs
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn, ActorResult};
use anyhow::Result;
use std::fmt::Debug;

// ---- Generic Actor Definition ----
// T must have all bounds required by any message handler and the Actor trait itself.
#[derive(Debug)]
struct GenericActor<T: Send + Debug + Clone + 'static> {
    data: Option<T>,
}

// ---- Actor Trait Implementation ----
impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> {
    type Args = Option<T>; // Initial value for data
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { data: args })
    }
}

// ---- Test Data Struct Definition (moved before impl_message_handler) ----
#[derive(Debug, Clone, PartialEq)]
struct MyTestData {
    id: i32,
    name: String,
}

impl MyTestData {
    fn new(id: i32, name: &str) -> Self {
        Self { id, name: name.to_string() }
    }
}

// ---- Message Definitions ----
#[derive(Debug)]
pub struct SetValue<T: Send + Debug + 'static>(pub T);

#[derive(Debug, Clone, Copy)]
pub struct GetValue;

#[derive(Debug, Clone, Copy)]
pub struct ClearValue;

// ---- Message Trait Implementations ----

// Handle SetValue<T>
impl<T: Send + Debug + Clone + 'static> Message<SetValue<T>> for GenericActor<T> {
    type Reply = ();

    async fn handle(&mut self, msg: SetValue<T>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data = Some(msg.0);
    }
}

// Handle GetValue
impl<T: Send + Debug + Clone + 'static> Message<GetValue> for GenericActor<T> {
    type Reply = Option<T>;

    async fn handle(&mut self, _msg: GetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data.clone()
    }
}

// Handle ClearValue
impl<T: Send + Debug + Clone + 'static> Message<ClearValue> for GenericActor<T> {
    type Reply = ();

    async fn handle(&mut self, _msg: ClearValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data = None;
    }
}

// ---- impl_message_handler Macro ----
// Specific implementations for concrete types used in tests
impl_message_handler!(GenericActor<String>, [SetValue<String>, GetValue, ClearValue]);
impl_message_handler!(GenericActor<i32>, [SetValue<i32>, GetValue, ClearValue]);
impl_message_handler!(GenericActor<MyTestData>, [SetValue<MyTestData>, GetValue, ClearValue]);

// ---- Test Cases ----
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generic_actor_string() {
        let initial_value: Option<String> = Some("initial_string".to_string());
        let (actor_ref, join_handle) = spawn::<GenericActor<String>>(initial_value.clone());

        // Get initial value
        let value: Option<String> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, initial_value);

        // Set a new value
        let new_value = "new_string".to_string();
        actor_ref.tell(SetValue(new_value.clone())).await.unwrap();

        // Get new value
        let value: Option<String> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, Some(new_value));

        // Clear value
        actor_ref.tell(ClearValue).await.unwrap();
        let value: Option<String> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, None);

        // Stop actor and wait
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
        if let ActorResult::Completed { actor, killed: _ } = result {
            assert_eq!(actor.data, None); // Check final state after stop
        } else {
            panic!("Actor did not complete as expected");
        }
    }

    #[tokio::test]
    async fn test_generic_actor_i32() {
        let initial_value: Option<i32> = Some(42);
        let (actor_ref, join_handle) = spawn::<GenericActor<i32>>(initial_value);

        // Get initial value
        let value: Option<i32> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, initial_value);

        // Set a new value
        let new_value = 100;
        actor_ref.tell(SetValue(new_value)).await.unwrap();

        // Get new value
        let value: Option<i32> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, Some(new_value));

        // Clear value
        actor_ref.tell(ClearValue).await.unwrap();
        let value: Option<i32> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, None);

        // Stop actor and wait
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
        if let ActorResult::Completed { actor, killed: _ } = result {
            assert_eq!(actor.data, None); // Check final state after stop
        } else {
            panic!("Actor did not complete as expected");
        }
    }

    #[tokio::test]
    async fn test_generic_actor_struct() {
        let initial_data = MyTestData::new(1, "initial_data");
        // Ensure MyTestData satisfies Send + Debug + Clone + 'static for GenericActor<MyTestData>
        let (actor_ref, join_handle) = spawn::<GenericActor<MyTestData>>(Some(initial_data.clone()));

        let value: Option<MyTestData> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, Some(initial_data.clone()));

        let new_data = MyTestData::new(2, "new_data");
        actor_ref.tell(SetValue(new_data.clone())).await.unwrap();

        let value: Option<MyTestData> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, Some(new_data.clone()));

        actor_ref.tell(ClearValue).await.unwrap();
        let value: Option<MyTestData> = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, None);

        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
        if let ActorResult::Completed { actor, killed: _ } = result {
            assert_eq!(actor.data, None); // Check final state after stop
        } else {
            panic!("Actor did not complete as expected");
        }
    }
}
