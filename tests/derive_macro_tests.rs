// Tests for the Actor derive macro

use rsactor::{impl_message_handler, spawn, Actor, ActorRef, Message};

#[derive(Actor, Debug, PartialEq)]
struct TestActor {
    value: i32,
    name: String,
}

// Test messages
struct GetValue;
struct GetName;
struct SetValue(i32);

impl Message<GetValue> for TestActor {
    type Reply = i32;

    async fn handle(&mut self, _msg: GetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.value
    }
}

impl Message<GetName> for TestActor {
    type Reply = String;

    async fn handle(&mut self, _msg: GetName, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.name.clone()
    }
}

impl Message<SetValue> for TestActor {
    type Reply = ();

    async fn handle(&mut self, msg: SetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.value = msg.0;
    }
}

impl_message_handler!(TestActor, [GetValue, GetName, SetValue]);

#[tokio::test]
async fn test_derive_actor_basic_functionality() {
    let actor_instance = TestActor {
        value: 42,
        name: "test_actor".to_string(),
    };

    let (actor_ref, _join_handle) = spawn::<TestActor>(actor_instance);

    // Test that we can get the initial values
    let value = actor_ref.ask(GetValue).await.unwrap();
    assert_eq!(value, 42);

    let name = actor_ref.ask(GetName).await.unwrap();
    assert_eq!(name, "test_actor");

    // Test that we can modify the state
    actor_ref.tell(SetValue(100)).await.unwrap();
    let new_value = actor_ref.ask(GetValue).await.unwrap();
    assert_eq!(new_value, 100);

    // Stop the actor
    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_error_type() {
    let actor_instance = TestActor {
        value: 0,
        name: "error_test".to_string(),
    };

    // The derive macro should set Error type to Infallible
    // This test ensures the actor can be spawned without error handling
    let (actor_ref, join_handle) = spawn::<TestActor>(actor_instance);

    actor_ref.stop().await.unwrap();

    // The join handle should complete successfully
    let result = join_handle.await.unwrap();
    assert!(result.is_completed());
}

#[tokio::test]
async fn test_derive_actor_args_type() {
    // Test that Args type is Self, meaning we pass the actor instance directly
    let actor_instance = TestActor {
        value: 123,
        name: "args_test".to_string(),
    };

    let original_value = actor_instance.value;
    let original_name = actor_instance.name.clone();

    let (actor_ref, _join_handle) = spawn::<TestActor>(actor_instance);

    // Verify the actor was created with the correct values
    let value = actor_ref.ask(GetValue).await.unwrap();
    let name = actor_ref.ask(GetName).await.unwrap();

    assert_eq!(value, original_value);
    assert_eq!(name, original_name);

    actor_ref.stop().await.unwrap();
}

// Test with a unit struct
#[derive(Actor, Debug)]
struct UnitActor;

struct Ping;

impl Message<Ping> for UnitActor {
    type Reply = String;

    async fn handle(&mut self, _msg: Ping, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        "pong".to_string()
    }
}

impl_message_handler!(UnitActor, [Ping]);

#[tokio::test]
async fn test_derive_actor_unit_struct() {
    let (actor_ref, _join_handle) = spawn::<UnitActor>(UnitActor);

    let response = actor_ref.ask(Ping).await.unwrap();
    assert_eq!(response, "pong");

    actor_ref.stop().await.unwrap();
}

// Test with a tuple struct
#[derive(Actor, Debug)]
struct TupleActor(String, i32);

struct GetFirst;
struct GetSecond;

impl Message<GetFirst> for TupleActor {
    type Reply = String;

    async fn handle(&mut self, _msg: GetFirst, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.0.clone()
    }
}

impl Message<GetSecond> for TupleActor {
    type Reply = i32;

    async fn handle(&mut self, _msg: GetSecond, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.1
    }
}

impl_message_handler!(TupleActor, [GetFirst, GetSecond]);

#[tokio::test]
async fn test_derive_actor_tuple_struct() {
    let actor_instance = TupleActor("hello".to_string(), 456);
    let (actor_ref, _join_handle) = spawn::<TupleActor>(actor_instance);

    let first = actor_ref.ask(GetFirst).await.unwrap();
    let second = actor_ref.ask(GetSecond).await.unwrap();

    assert_eq!(first, "hello");
    assert_eq!(second, 456);

    actor_ref.stop().await.unwrap();
}
