// Tests for the Actor derive macro (structs and enums)

use rsactor::{spawn, Actor, ActorRef, Message};

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

// =============================================================================
// Tests for Enhanced Derive Macro Validation Features
// =============================================================================

// Test compilation errors for invalid handler signatures
// These tests check that the improved error messages work correctly

use rsactor::message_handlers;

// Valid actor with message handlers macro
#[derive(Actor, Debug)]
struct ValidatorTestActor {
    value: i32,
}

struct TestMessage;

// Test that message_handlers macro works correctly with valid handlers
#[message_handlers]
impl ValidatorTestActor {
    #[handler]
    async fn handle_test_message(&mut self, _msg: TestMessage, _: &ActorRef<Self>) -> i32 {
        self.value
    }

    // Test that non-handler methods are left unchanged
    #[allow(dead_code)]
    async fn non_handler_method(&mut self, value: i32) -> i32 {
        self.value = value;
        self.value
    }
}

#[tokio::test]
async fn test_message_handlers_macro_basic() {
    let actor_instance = ValidatorTestActor { value: 42 };
    let (actor_ref, _join_handle) = spawn::<ValidatorTestActor>(actor_instance);

    let result = actor_ref.ask(TestMessage).await.unwrap();
    assert_eq!(result, 42);

    actor_ref.stop().await.unwrap();
}

// Test for generic support in derive macro
#[derive(Actor, Debug)]
struct GenericActor<T>
where
    T: Send + 'static + Clone + std::fmt::Debug,
{
    data: T,
}

struct GetData;
struct SetData<T>(T);

#[message_handlers]
impl<T> GenericActor<T>
where
    T: Send + 'static + Clone + std::fmt::Debug,
{
    #[handler]
    async fn handle_get_data(&mut self, _msg: GetData, _: &ActorRef<Self>) -> T {
        self.data.clone()
    }

    #[handler]
    async fn handle_set_data(&mut self, msg: SetData<T>, _: &ActorRef<Self>) -> T {
        self.data = msg.0;
        self.data.clone()
    }
}

#[tokio::test]
async fn test_derive_actor_with_generics() {
    // Test with String type
    let actor_instance = GenericActor {
        data: "hello".to_string(),
    };
    let (actor_ref, _join_handle) = spawn::<GenericActor<String>>(actor_instance);

    let data = actor_ref.ask(GetData).await.unwrap();
    assert_eq!(data, "hello");

    let new_data = actor_ref.ask(SetData("world".to_string())).await.unwrap();
    assert_eq!(new_data, "world");

    actor_ref.stop().await.unwrap();
}

#[tokio::test]
async fn test_derive_actor_with_generics_numeric() {
    // Test with numeric type
    let actor_instance = GenericActor { data: 123i32 };
    let (actor_ref, _join_handle) = spawn::<GenericActor<i32>>(actor_instance);

    let data = actor_ref.ask(GetData).await.unwrap();
    assert_eq!(data, 123);

    let new_data = actor_ref.ask(SetData(456)).await.unwrap();
    assert_eq!(new_data, 456);

    actor_ref.stop().await.unwrap();
}

// Test actor with complex where clauses and multiple generic parameters
#[derive(Actor, Debug)]
struct MultiGenericActor<T, U>
where
    T: Send + 'static + Clone + std::fmt::Debug,
    U: Send + 'static + Clone + std::fmt::Debug + std::fmt::Display,
{
    first: T,
    second: U,
}

struct MultiGetFirst;
struct MultiGetSecond;
struct MultiGetBoth;
struct MultiSetBoth<T, U>(T, U);

#[message_handlers]
impl<T, U> MultiGenericActor<T, U>
where
    T: Send + 'static + Clone + std::fmt::Debug,
    U: Send + 'static + Clone + std::fmt::Debug + std::fmt::Display,
{
    #[handler]
    async fn handle_get_first(&mut self, _msg: MultiGetFirst, _: &ActorRef<Self>) -> T {
        self.first.clone()
    }

    #[handler]
    async fn handle_get_second(&mut self, _msg: MultiGetSecond, _: &ActorRef<Self>) -> U {
        self.second.clone()
    }

    #[handler]
    async fn handle_get_both(&mut self, _msg: MultiGetBoth, _: &ActorRef<Self>) -> (T, U) {
        (self.first.clone(), self.second.clone())
    }

    #[handler]
    async fn handle_set_both(&mut self, msg: MultiSetBoth<T, U>, _: &ActorRef<Self>) -> String {
        self.first = msg.0;
        self.second = msg.1;
        format!("Updated to: {:?}, {}", self.first, self.second)
    }
}

#[tokio::test]
async fn test_derive_actor_multi_generics() {
    let actor_instance = MultiGenericActor {
        first: vec![1, 2, 3],
        second: "test".to_string(),
    };
    let (actor_ref, _join_handle) = spawn::<MultiGenericActor<Vec<i32>, String>>(actor_instance);

    let first = actor_ref.ask(MultiGetFirst).await.unwrap();
    assert_eq!(first, vec![1, 2, 3]);

    let second = actor_ref.ask(MultiGetSecond).await.unwrap();
    assert_eq!(second, "test");

    let both = actor_ref.ask(MultiGetBoth).await.unwrap();
    assert_eq!(both, (vec![1, 2, 3], "test".to_string()));

    let result = actor_ref
        .ask(MultiSetBoth(vec![4, 5], "updated".to_string()))
        .await
        .unwrap();
    assert!(result.contains("Updated to"));

    let new_both = actor_ref.ask(MultiGetBoth).await.unwrap();
    assert_eq!(new_both, (vec![4, 5], "updated".to_string()));

    actor_ref.stop().await.unwrap();
}

// Test edge cases for handler method validation
#[derive(Actor, Debug)]
struct EdgeCaseActor {
    value: i32,
}

struct EdgeMessage;
struct EdgeExplicitMessage;

#[message_handlers]
impl EdgeCaseActor {
    // Test handler with unit return type (no explicit return type)
    #[handler]
    async fn handle_edge_message(&mut self, _msg: EdgeMessage, _: &ActorRef<Self>) {
        self.value += 1;
    }

    // Test handler with explicit unit return type
    #[handler]
    async fn handle_edge_explicit_unit(
        &mut self,
        _msg: EdgeExplicitMessage,
        _: &ActorRef<Self>,
    ) -> () {
        self.value += 2;
    }
}

struct EdgeGetValue;

impl Message<EdgeGetValue> for EdgeCaseActor {
    type Reply = i32;

    async fn handle(&mut self, _msg: EdgeGetValue, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.value
    }
}

#[tokio::test]
async fn test_handler_edge_cases() {
    let actor_instance = EdgeCaseActor { value: 0 };
    let (actor_ref, _join_handle) = spawn::<EdgeCaseActor>(actor_instance);

    // Test unit return (implicit)
    actor_ref.tell(EdgeMessage).await.unwrap();
    let value = actor_ref.ask(EdgeGetValue).await.unwrap();
    assert_eq!(value, 1);

    // Test explicit unit return
    actor_ref.tell(EdgeExplicitMessage).await.unwrap();
    let value = actor_ref.ask(EdgeGetValue).await.unwrap();
    assert_eq!(value, 3); // 1 + 2

    actor_ref.stop().await.unwrap();
}

// Test that the derive macro works with complex struct patterns
#[derive(Actor, Debug)]
struct ComplexStructActor {
    pub id: u64,
    pub(crate) internal_state: String,
    private_data: Vec<i32>,
    #[allow(dead_code)]
    unused_field: Option<String>,
}

struct ComplexGetId;
struct ComplexGetInternalState;
struct ComplexGetPrivateData;
struct ComplexAddToPrivateData(i32);

#[message_handlers]
impl ComplexStructActor {
    #[handler]
    async fn handle_get_id(&mut self, _msg: ComplexGetId, _: &ActorRef<Self>) -> u64 {
        self.id
    }

    #[handler]
    async fn handle_get_internal_state(
        &mut self,
        _msg: ComplexGetInternalState,
        _: &ActorRef<Self>,
    ) -> String {
        self.internal_state.clone()
    }

    #[handler]
    async fn handle_get_private_data(
        &mut self,
        _msg: ComplexGetPrivateData,
        _: &ActorRef<Self>,
    ) -> Vec<i32> {
        self.private_data.clone()
    }

    #[handler]
    async fn handle_add_to_private_data(
        &mut self,
        msg: ComplexAddToPrivateData,
        _: &ActorRef<Self>,
    ) -> usize {
        self.private_data.push(msg.0);
        self.private_data.len()
    }
}

#[tokio::test]
async fn test_complex_struct_actor() {
    let actor_instance = ComplexStructActor {
        id: 12345,
        internal_state: "initialized".to_string(),
        private_data: vec![1, 2, 3],
        unused_field: None,
    };

    let (actor_ref, _join_handle) = spawn::<ComplexStructActor>(actor_instance);

    let id = actor_ref.ask(ComplexGetId).await.unwrap();
    assert_eq!(id, 12345);

    let state = actor_ref.ask(ComplexGetInternalState).await.unwrap();
    assert_eq!(state, "initialized");

    let data = actor_ref.ask(ComplexGetPrivateData).await.unwrap();
    assert_eq!(data, vec![1, 2, 3]);

    let len = actor_ref.ask(ComplexAddToPrivateData(4)).await.unwrap();
    assert_eq!(len, 4);

    let new_data = actor_ref.ask(ComplexGetPrivateData).await.unwrap();
    assert_eq!(new_data, vec![1, 2, 3, 4]);

    actor_ref.stop().await.unwrap();
}

// Test that lifetimes and references work correctly in messages
#[derive(Actor, Debug)]
struct RefTestActor {
    data: String,
}

// Message that contains owned data
struct UpdateData(String);

// Message for getting a reference to internal data
struct GetDataRef;

#[message_handlers]
impl RefTestActor {
    #[handler]
    async fn handle_update_data(&mut self, msg: UpdateData, _: &ActorRef<Self>) -> usize {
        self.data = msg.0;
        self.data.len()
    }

    #[handler]
    async fn handle_get_data_ref(&mut self, _msg: GetDataRef, _: &ActorRef<Self>) -> String {
        // Return owned data since we can't return references from async handlers
        self.data.clone()
    }
}

#[tokio::test]
async fn test_reference_handling() {
    let actor_instance = RefTestActor {
        data: "initial".to_string(),
    };

    let (actor_ref, _join_handle) = spawn::<RefTestActor>(actor_instance);

    let len = actor_ref
        .ask(UpdateData("updated_data".to_string()))
        .await
        .unwrap();
    assert_eq!(len, 12); // "updated_data".len()

    let data = actor_ref.ask(GetDataRef).await.unwrap();
    assert_eq!(data, "updated_data");

    actor_ref.stop().await.unwrap();
}

// =============================================================================
// Compile-time Error Validation Tests
// These tests ensure that the macro correctly rejects invalid code patterns
// =============================================================================

// Note: These are documentation tests that should fail compilation
// They demonstrate that the macro provides good error messages

/// Test that async is required for handler methods
/// ```compile_fail
/// use rsactor::{Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct BadActor;
///
/// struct BadMessage;
///
/// #[message_handlers]
/// impl BadActor {
///     #[handler]
///     fn handle_bad_message(&mut self, _msg: BadMessage, _: &ActorRef<Self>) -> i32 {
///         42
///     }
/// }
/// ```
pub fn test_non_async_handler_fails() {}

/// Test that correct parameter count is enforced
/// ```compile_fail
/// use rsactor::{Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct BadActor;
///
/// struct BadMessage;
///
/// #[message_handlers]
/// impl BadActor {
///     #[handler]
///     async fn handle_bad_message(&mut self, _msg: BadMessage) -> i32 {
///         42
///     }
/// }
/// ```
pub fn test_wrong_parameter_count_fails() {}

/// Test that first parameter must be &mut self
/// ```compile_fail
/// use rsactor::{Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct BadActor;
///
/// struct BadMessage;
///
/// #[message_handlers]
/// impl BadActor {
///     #[handler]
///     async fn handle_bad_message(&self, _msg: BadMessage, _: &ActorRef<Self>) -> i32 {
///         42
///     }
/// }
/// ```
pub fn test_immutable_self_fails() {}

/// Test that #[handler] attribute doesn't accept arguments
/// ```compile_fail
/// use rsactor::{Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct BadActor;
///
/// struct BadMessage;
///
/// #[message_handlers]
/// impl BadActor {
///     #[handler(timeout = "5s")]
///     async fn handle_bad_message(&mut self, _msg: BadMessage, _: &ActorRef<Self>) -> i32 {
///         42
///     }
/// }
/// ```
pub fn test_handler_with_arguments_fails() {}

// =============================================================================
// Integration Tests for New Features
// =============================================================================

// Test that the improved error messages actually work in practice
#[tokio::test]
async fn test_enhanced_macro_features_integration() {
    // This test demonstrates that all the enhanced features work together

    // 1. Generic actor with complex constraints
    let generic_actor = GenericActor {
        data: "integration_test".to_string(),
    };
    let (generic_ref, _) = spawn::<GenericActor<String>>(generic_actor);

    // 2. Multi-generic actor
    let multi_actor = MultiGenericActor {
        first: vec![1, 2, 3],
        second: 42i32,
    };
    let (multi_ref, _) = spawn::<MultiGenericActor<Vec<i32>, i32>>(multi_actor);

    // 3. Complex struct actor
    let complex_actor = ComplexStructActor {
        id: 999,
        internal_state: "complex_test".to_string(),
        private_data: vec![10, 20],
        unused_field: Some("test".to_string()),
    };
    let (complex_ref, _) = spawn::<ComplexStructActor>(complex_actor);

    // Test that all actors work correctly
    let generic_data = generic_ref.ask(GetData).await.unwrap();
    assert_eq!(generic_data, "integration_test");

    let multi_first = multi_ref.ask(MultiGetFirst).await.unwrap();
    assert_eq!(multi_first, vec![1, 2, 3]);

    let complex_id = complex_ref.ask(ComplexGetId).await.unwrap();
    assert_eq!(complex_id, 999);

    // Stop all actors
    generic_ref.stop().await.unwrap();
    multi_ref.stop().await.unwrap();
    complex_ref.stop().await.unwrap();
}

// Test macro resilience with edge cases
#[tokio::test]
async fn test_macro_edge_cases() {
    // Test with zero-sized types
    let unit_actor = UnitActor;
    let (unit_ref, _) = spawn::<UnitActor>(unit_actor);

    let response = unit_ref.ask(Ping).await.unwrap();
    assert_eq!(response, "pong");

    // Test with tuple structs
    let tuple_actor = TupleActor("edge_test".to_string(), 123);
    let (tuple_ref, _) = spawn::<TupleActor>(tuple_actor);

    let first = tuple_ref.ask(GetFirst).await.unwrap();
    assert_eq!(first, "edge_test");

    // Test with enums
    let enum_actor = StateActor::Processing("edge_processing".to_string());
    let (enum_ref, _) = spawn::<StateActor>(enum_actor);

    let processing_data = enum_ref.ask(GetProcessingData).await.unwrap();
    assert_eq!(processing_data, Some("edge_processing".to_string()));

    // Stop all actors
    unit_ref.stop().await.unwrap();
    tuple_ref.stop().await.unwrap();
    enum_ref.stop().await.unwrap();
}

// Performance test to ensure macros don't introduce significant overhead
#[tokio::test]
async fn test_macro_performance() {
    use std::time::Instant;

    let start = Instant::now();

    // Create multiple actors quickly
    let mut actors = Vec::new();
    for i in 0..100 {
        let actor = TestActor {
            value: i,
            name: format!("perf_test_{i}"),
        };
        let (actor_ref, _) = spawn::<TestActor>(actor);
        actors.push(actor_ref);
    }

    // Test that message passing is still fast
    for (i, actor_ref) in actors.iter().enumerate() {
        let value = actor_ref.ask(GetValue).await.unwrap();
        assert_eq!(value, i as i32);
    }

    // Clean up
    for actor_ref in actors {
        actor_ref.stop().await.unwrap();
    }

    let duration = start.elapsed();
    println!("Created and tested 100 actors in {duration:?}");

    // This is a rough performance check - adjust as needed
    assert!(
        duration.as_millis() < 1000,
        "Macro performance regression detected"
    );
}
