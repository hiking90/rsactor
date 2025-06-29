// Tests for the Actor derive macro (structs and enums)
#![allow(deprecated)]

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

// Test with an enum
#[derive(Actor, Debug, Clone)]
enum StateActor {
    Idle,
    Processing(String),
    Completed(i32),
}

struct GetState;
struct SetState(StateActor);
struct IsIdle;
struct GetProcessingData;
struct GetCompletedValue;

impl Message<GetState> for StateActor {
    type Reply = StateActor;

    async fn handle(&mut self, _msg: GetState, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.clone()
    }
}

impl Message<SetState> for StateActor {
    type Reply = ();

    async fn handle(&mut self, msg: SetState, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        *self = msg.0;
    }
}

impl Message<IsIdle> for StateActor {
    type Reply = bool;

    async fn handle(&mut self, _msg: IsIdle, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        matches!(self, StateActor::Idle)
    }
}

impl Message<GetProcessingData> for StateActor {
    type Reply = Option<String>;

    async fn handle(
        &mut self,
        _msg: GetProcessingData,
        _actor_ref: &ActorRef<Self>,
    ) -> Self::Reply {
        match self {
            StateActor::Processing(data) => Some(data.clone()),
            _ => None,
        }
    }
}

impl Message<GetCompletedValue> for StateActor {
    type Reply = Option<i32>;

    async fn handle(
        &mut self,
        _msg: GetCompletedValue,
        _actor_ref: &ActorRef<Self>,
    ) -> Self::Reply {
        match self {
            StateActor::Completed(value) => Some(*value),
            _ => None,
        }
    }
}

impl_message_handler!(
    StateActor,
    [
        GetState,
        SetState,
        IsIdle,
        GetProcessingData,
        GetCompletedValue
    ]
);

#[tokio::test]
async fn test_derive_actor_enum_basic() {
    let actor_instance = StateActor::Idle;
    let (actor_ref, _join_handle) = spawn::<StateActor>(actor_instance);

    // Test initial state
    let is_idle = actor_ref.ask(IsIdle).await.unwrap();
    assert!(is_idle);

    let state = actor_ref.ask(GetState).await.unwrap();
    assert!(matches!(state, StateActor::Idle));

    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_enum_state_transitions() {
    let actor_instance = StateActor::Idle;
    let (actor_ref, _join_handle) = spawn::<StateActor>(actor_instance);

    // Transition to Processing state
    actor_ref
        .tell(SetState(StateActor::Processing("working".to_string())))
        .await
        .unwrap();

    let is_idle = actor_ref.ask(IsIdle).await.unwrap();
    assert!(!is_idle);

    let processing_data = actor_ref.ask(GetProcessingData).await.unwrap();
    assert_eq!(processing_data, Some("working".to_string()));

    // Transition to Completed state
    actor_ref
        .tell(SetState(StateActor::Completed(42)))
        .await
        .unwrap();

    let completed_value = actor_ref.ask(GetCompletedValue).await.unwrap();
    assert_eq!(completed_value, Some(42));

    let processing_data = actor_ref.ask(GetProcessingData).await.unwrap();
    assert_eq!(processing_data, None);

    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_enum_variant_data() {
    // Test with Processing variant
    let actor_instance = StateActor::Processing("initial_task".to_string());
    let (actor_ref, _join_handle) = spawn::<StateActor>(actor_instance);

    let processing_data = actor_ref.ask(GetProcessingData).await.unwrap();
    assert_eq!(processing_data, Some("initial_task".to_string()));

    let is_idle = actor_ref.ask(IsIdle).await.unwrap();
    assert!(!is_idle);

    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_enum_completed_variant() {
    // Test with Completed variant
    let actor_instance = StateActor::Completed(100);
    let (actor_ref, _join_handle) = spawn::<StateActor>(actor_instance);

    let completed_value = actor_ref.ask(GetCompletedValue).await.unwrap();
    assert_eq!(completed_value, Some(100));

    let processing_data = actor_ref.ask(GetProcessingData).await.unwrap();
    assert_eq!(processing_data, None);

    let is_idle = actor_ref.ask(IsIdle).await.unwrap();
    assert!(!is_idle);

    actor_ref.stop().await.unwrap();
}

// Test with a simple enum without data
#[derive(Actor, Debug, Clone, PartialEq)]
enum DirectionActor {
    North,
    South,
    East,
    West,
}

struct GetDirection;
struct TurnRight;
struct TurnLeft;

impl Message<GetDirection> for DirectionActor {
    type Reply = DirectionActor;

    async fn handle(&mut self, _msg: GetDirection, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.clone()
    }
}

impl Message<TurnRight> for DirectionActor {
    type Reply = ();

    async fn handle(&mut self, _msg: TurnRight, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        *self = match self {
            DirectionActor::North => DirectionActor::East,
            DirectionActor::East => DirectionActor::South,
            DirectionActor::South => DirectionActor::West,
            DirectionActor::West => DirectionActor::North,
        };
    }
}

impl Message<TurnLeft> for DirectionActor {
    type Reply = ();

    async fn handle(&mut self, _msg: TurnLeft, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        *self = match self {
            DirectionActor::North => DirectionActor::West,
            DirectionActor::West => DirectionActor::South,
            DirectionActor::South => DirectionActor::East,
            DirectionActor::East => DirectionActor::North,
        };
    }
}

impl_message_handler!(DirectionActor, [GetDirection, TurnRight, TurnLeft]);

#[tokio::test]
async fn test_derive_actor_simple_enum() {
    let actor_instance = DirectionActor::North;
    let (actor_ref, _join_handle) = spawn::<DirectionActor>(actor_instance);

    // Test initial direction
    let direction = actor_ref.ask(GetDirection).await.unwrap();
    assert_eq!(direction, DirectionActor::North);

    // Turn right: North -> East
    actor_ref.tell(TurnRight).await.unwrap();
    let direction = actor_ref.ask(GetDirection).await.unwrap();
    assert_eq!(direction, DirectionActor::East);

    // Turn right: East -> South
    actor_ref.tell(TurnRight).await.unwrap();
    let direction = actor_ref.ask(GetDirection).await.unwrap();
    assert_eq!(direction, DirectionActor::South);

    // Turn left: South -> East
    actor_ref.tell(TurnLeft).await.unwrap();
    let direction = actor_ref.ask(GetDirection).await.unwrap();
    assert_eq!(direction, DirectionActor::East);

    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_enum_full_rotation() {
    let actor_instance = DirectionActor::North;
    let (actor_ref, _join_handle) = spawn::<DirectionActor>(actor_instance);

    // Full right rotation
    for expected in [
        DirectionActor::East,
        DirectionActor::South,
        DirectionActor::West,
        DirectionActor::North,
    ] {
        actor_ref.tell(TurnRight).await.unwrap();
        let direction = actor_ref.ask(GetDirection).await.unwrap();
        assert_eq!(direction, expected);
    }

    // Full left rotation
    for expected in [
        DirectionActor::West,
        DirectionActor::South,
        DirectionActor::East,
        DirectionActor::North,
    ] {
        actor_ref.tell(TurnLeft).await.unwrap();
        let direction = actor_ref.ask(GetDirection).await.unwrap();
        assert_eq!(direction, expected);
    }

    actor_ref.stop().await.unwrap();
}
