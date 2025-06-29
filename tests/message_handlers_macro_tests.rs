// Tests for the message_handlers macro with various message types

use anyhow::Error as AnyError;
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

// Test actor for message handlers macro
#[derive(Debug)]
struct MessageHandlerTestActor {
    counter: u32,
    messages_log: Vec<String>,
    data_store: HashMap<String, i32>,
}

impl Actor for MessageHandlerTestActor {
    type Args = ();
    type Error = AnyError;

    async fn on_start(
        _args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(MessageHandlerTestActor {
            counter: 0,
            messages_log: Vec::new(),
            data_store: HashMap::new(),
        })
    }
}

// Various message types to test different scenarios

// 1. Simple unit struct
#[derive(Debug, Clone)]
struct Ping;

// 2. Tuple struct with single value
#[derive(Debug, Clone)]
struct SetCounter(u32);

// 3. Named struct with multiple fields
#[derive(Debug, Clone)]
struct StoreData {
    key: String,
    value: i32,
}

// 4. Generic struct
#[derive(Debug, Clone)]
struct GenericMessage<T> {
    data: T,
    label: String,
}

// 5. Struct with Vec
#[derive(Debug, Clone)]
struct BatchUpdate {
    updates: Vec<(String, i32)>,
}

// 6. Struct with Option
#[derive(Debug, Clone)]
struct OptionalMessage {
    required_field: String,
    optional_field: Option<i32>,
}

// 7. Enum message
#[derive(Debug, Clone)]
enum Operation {
    Add(i32),
    Subtract(i32),
    Multiply(i32),
    Reset,
}

// 8. Simple struct without external dependencies
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SimpleMessage {
    id: u64,
    content: String,
    metadata: HashMap<String, String>,
}

// 9. Complex nested struct
#[derive(Debug, Clone)]
struct ComplexMessage {
    header: MessageHeader,
    payload: MessagePayload,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MessageHeader {
    timestamp: u64,
    sender: String,
    priority: u8,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MessagePayload {
    data: Vec<u8>,
    format: String,
}

// 10. Query message that returns complex data
#[derive(Debug, Clone)]
struct GetStatus;

#[derive(Debug, Clone)]
struct StatusResponse {
    counter: u32,
    messages_count: usize,
    data_store_size: usize,
    last_messages: Vec<String>,
}

// Additional complex generic message types

// 11. Multi-generic struct
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MultiGenericMessage<T, U> {
    primary: T,
    secondary: U,
    context: String,
}

// 12. Generic with constraints
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ConstrainedGeneric<T>
where
    T: Clone + std::fmt::Debug,
{
    data: T,
    metadata: Vec<String>,
}

// 13. Nested generics
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NestedGeneric<T> {
    outer: GenericContainer<T>,
    fallback: Option<T>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct GenericContainer<T> {
    items: Vec<T>,
    capacity: usize,
}

// 14. Generic with lifetime (using owned types for simplicity)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct GenericWithOwnership<T> {
    owned_data: Box<T>,
    reference_data: String, // Simulating lifetime with owned String
}

// 15. Complex nested generic with multiple type parameters
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ComplexNested<K, V, M>
where
    K: Clone + std::fmt::Debug,
    V: Clone + std::fmt::Debug,
    M: Clone + std::fmt::Debug,
{
    map_data: HashMap<K, V>,
    metadata: M,
    processing_queue: Vec<(K, V)>,
}

// 16. Generic enum
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum GenericOperation<T> {
    Process(T),
    Transform { input: T, factor: f64 },
    Batch(Vec<T>),
    None,
}

// 17. Recursive generic (with Box to avoid infinite size)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct RecursiveGeneric<T> {
    value: T,
    children: Vec<Box<RecursiveGeneric<T>>>,
    depth: usize,
}

// 18. Generic with associated types simulation
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct GenericProcessor<T, R> {
    input: T,
    processor_id: String,
    expected_output_type: std::marker::PhantomData<R>,
}

// 19. Generic tuple struct
#[derive(Debug, Clone)]
struct GenericTuple<A, B, C>(A, B, C);

// 20. Complex generic with Result and Option
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ComplexGenericResult<T, E> {
    results: Vec<Result<T, E>>,
    summary: Option<String>,
    retry_count: u32,
}

// Additional complex generic message types with advanced trait bounds

// 21. Generic with multiple complex trait bounds
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AdvancedConstrainedGeneric<T>
where
    T: Send + Sync + std::fmt::Debug + Clone + 'static + PartialEq + Eq + std::hash::Hash + Default,
{
    data: T,
    hash_map: HashMap<T, String>,
    default_value: T,
}

// 22. Generic with lifetime and function bounds
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FunctionBoundGeneric<T>
where
    T: Clone + std::fmt::Debug + Send + 'static,
{
    data: T,
    transformer_id: String,
    cache: Vec<String>,
}

// 23. Generic with iterator and collection traits (simplified)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct IteratorBoundGeneric<T>
where
    T: Clone + std::fmt::Debug + Send + 'static,
{
    source: Vec<T>,
    iterator_config: String,
}

// 24. Generic with serialization bounds (simulated)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SerializationBoundGeneric<T>
where
    T: std::fmt::Debug + Clone + Send + Sync + 'static + PartialEq + std::fmt::Display,
{
    data: T,
    serialized_cache: String,
    metadata: HashMap<String, String>,
}

// 25. Generic with mathematical operation bounds
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MathBoundGeneric<T>
where
    T: std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + Copy
        + Clone
        + std::fmt::Debug
        + Send
        + 'static
        + PartialOrd
        + Default,
{
    values: Vec<T>,
    operations_log: Vec<String>,
}

// 26. Generic with associated types and complex bounds
trait CustomProcessor {
    type Input: Clone + std::fmt::Debug + Send + 'static;
    type Output: Clone + std::fmt::Debug + Send + 'static;
    type Error: std::error::Error + Send + Sync + 'static;

    #[allow(dead_code)]
    fn process(&self, input: Self::Input) -> Result<Self::Output, Self::Error>;
}

// Simple implementation for testing
#[derive(Debug, Clone)]
struct CustomProcessorImpl;

#[derive(Debug, Clone)]
struct SimpleError(String);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SimpleError: {}", self.0)
    }
}

impl std::error::Error for SimpleError {}

impl CustomProcessor for CustomProcessorImpl {
    type Input = String;
    type Output = i32;
    type Error = SimpleError;

    fn process(&self, input: Self::Input) -> Result<Self::Output, Self::Error> {
        input
            .len()
            .try_into()
            .map_err(|_| SimpleError("conversion failed".to_string()))
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AssociatedTypeBoundGeneric<P>
where
    P: CustomProcessor + Clone + Send + 'static,
    P::Input: PartialEq + std::hash::Hash,
    P::Output: std::fmt::Display,
{
    processor: P,
    input_cache: HashMap<P::Input, P::Output>,
    error_log: Vec<String>,
}

// 27. Simplified async bound generic
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AsyncBoundGeneric<T, R>
where
    T: Clone + std::fmt::Debug + Send + Sync + 'static,
    R: Clone + std::fmt::Debug + Send + Sync + 'static,
{
    input: T,
    processor_id: String,
    result_cache: Option<R>,
}

// 28. Generic with nested trait bounds and where clauses
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NestedTraitBoundGeneric<T, U, V>
where
    T: Into<U> + Clone + std::fmt::Debug + Send + 'static,
    U: From<T> + Into<V> + Clone + std::fmt::Debug + Send + 'static,
    V: From<U> + Clone + std::fmt::Debug + Send + 'static + std::fmt::Display,
{
    original: T,
    intermediate: Option<U>,
    final_result: Option<V>,
}

// 29. Generic with error handling bounds
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CustomError {
    message: String,
    code: i32,
}

impl std::fmt::Display for CustomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CustomError({}): {}", self.code, self.message)
    }
}

impl std::error::Error for CustomError {}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ErrorHandlingGeneric<T, E>
where
    T: Clone + std::fmt::Debug + Send + 'static,
    E: std::error::Error + Clone + std::fmt::Debug + Send + Sync + 'static,
{
    operations: Vec<Result<T, E>>,
    error_recovery: HashMap<String, T>,
}

// 30. Generic with phantom data and complex bounds
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PhantomComplexGeneric<T, U, V>
where
    T: Clone + std::fmt::Debug + Send + 'static + PartialEq + Eq + std::hash::Hash,
    U: Clone + std::fmt::Debug + Send + 'static + std::fmt::Display,
    V: Clone + std::fmt::Debug + Send + 'static,
{
    data: HashMap<T, U>,
    phantom: std::marker::PhantomData<V>,
    type_info: String,
}

// Message handlers implementation using the macro
#[message_handlers]
impl MessageHandlerTestActor {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) -> String {
        self.counter += 1;
        self.messages_log.push("Ping received".to_string());
        format!("Pong #{}", self.counter)
    }

    #[handler]
    async fn handle_set_counter(&mut self, msg: SetCounter, _: &ActorRef<Self>) -> u32 {
        let old_value = self.counter;
        self.counter = msg.0;
        self.messages_log
            .push(format!("Counter set from {} to {}", old_value, msg.0));
        old_value
    }

    #[handler]
    async fn handle_store_data(&mut self, msg: StoreData, _: &ActorRef<Self>) -> bool {
        let existed = self.data_store.contains_key(&msg.key);
        self.data_store.insert(msg.key.clone(), msg.value);
        self.messages_log
            .push(format!("Stored {}={}", msg.key, msg.value));
        !existed // return true if it's a new key
    }

    #[handler]
    async fn handle_generic_string(
        &mut self,
        msg: GenericMessage<String>,
        _: &ActorRef<Self>,
    ) -> String {
        self.messages_log
            .push(format!("Generic string: {} - {}", msg.label, msg.data));
        format!("Processed: {}", msg.data)
    }

    #[handler]
    async fn handle_generic_number(&mut self, msg: GenericMessage<i32>, _: &ActorRef<Self>) -> i32 {
        self.messages_log
            .push(format!("Generic number: {} - {}", msg.label, msg.data));
        msg.data * 2
    }

    #[handler]
    async fn handle_batch_update(&mut self, msg: BatchUpdate, _: &ActorRef<Self>) -> usize {
        let count = msg.updates.len();
        for (key, value) in msg.updates {
            self.data_store.insert(key, value);
        }
        self.messages_log
            .push(format!("Batch updated {count} items"));
        count
    }

    #[handler]
    async fn handle_optional_message(
        &mut self,
        msg: OptionalMessage,
        _: &ActorRef<Self>,
    ) -> String {
        let response = match msg.optional_field {
            Some(value) => format!("{}: {}", msg.required_field, value),
            None => format!("{}: no value", msg.required_field),
        };
        self.messages_log
            .push(format!("Optional message: {response}"));
        response
    }

    #[handler]
    async fn handle_operation(&mut self, msg: Operation, _: &ActorRef<Self>) -> u32 {
        let old_value = self.counter;
        match msg {
            Operation::Add(n) => self.counter += n as u32,
            Operation::Subtract(n) => self.counter = self.counter.saturating_sub(n as u32),
            Operation::Multiply(n) => self.counter *= n as u32,
            Operation::Reset => self.counter = 0,
        }
        self.messages_log
            .push(format!("Operation: {} -> {}", old_value, self.counter));
        self.counter
    }

    #[handler]
    async fn handle_simple_message(&mut self, msg: SimpleMessage, _: &ActorRef<Self>) -> u64 {
        self.messages_log.push(format!(
            "Simple message: id={}, content={}",
            msg.id, msg.content
        ));
        msg.id
    }

    #[handler]
    async fn handle_complex_message(&mut self, msg: ComplexMessage, _: &ActorRef<Self>) -> usize {
        let payload_size = msg.payload.data.len();
        self.messages_log.push(format!(
            "Complex message from {} at {}, payload size: {}",
            msg.header.sender, msg.header.timestamp, payload_size
        ));
        payload_size
    }

    #[handler]
    async fn handle_get_status(&mut self, _msg: GetStatus, _: &ActorRef<Self>) -> StatusResponse {
        let last_messages = self
            .messages_log
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        StatusResponse {
            counter: self.counter,
            messages_count: self.messages_log.len(),
            data_store_size: self.data_store.len(),
            last_messages,
        }
    }

    // Handlers for additional complex generic message types

    #[handler]
    async fn handle_multi_generic_message(
        &mut self,
        msg: MultiGenericMessage<String, i32>,
        _: &ActorRef<Self>,
    ) -> String {
        self.messages_log.push(format!(
            "Multi-generic: {} + {} in context '{}'",
            msg.primary, msg.secondary, msg.context
        ));
        format!("Combined: {} + {}", msg.primary, msg.secondary)
    }

    #[handler]
    async fn handle_multi_generic_vec_bool(
        &mut self,
        msg: MultiGenericMessage<Vec<u8>, bool>,
        _: &ActorRef<Self>,
    ) -> usize {
        self.messages_log.push(format!(
            "Multi-generic vec+bool: {} bytes, flag: {}, context: '{}'",
            msg.primary.len(),
            msg.secondary,
            msg.context
        ));
        msg.primary.len()
    }

    #[handler]
    async fn handle_constrained_generic_string(
        &mut self,
        msg: ConstrainedGeneric<String>,
        _: &ActorRef<Self>,
    ) -> usize {
        self.messages_log.push(format!(
            "Constrained generic: {:?} with {} metadata items",
            msg.data,
            msg.metadata.len()
        ));
        msg.metadata.len()
    }

    #[handler]
    async fn handle_constrained_generic_number(
        &mut self,
        msg: ConstrainedGeneric<i64>,
        _: &ActorRef<Self>,
    ) -> i64 {
        self.messages_log.push(format!(
            "Constrained generic number: {:?} with metadata",
            msg.data
        ));
        msg.data * 10
    }

    #[handler]
    async fn handle_nested_generic_string(
        &mut self,
        msg: NestedGeneric<String>,
        _: &ActorRef<Self>,
    ) -> usize {
        let item_count = msg.outer.items.len();
        self.messages_log.push(format!(
            "Nested generic: {} items, fallback: {:?}",
            item_count, msg.fallback
        ));
        item_count
    }

    #[handler]
    async fn handle_nested_generic_number(
        &mut self,
        msg: NestedGeneric<i32>,
        _: &ActorRef<Self>,
    ) -> i32 {
        let sum: i32 = msg.outer.items.iter().sum();
        self.messages_log.push(format!(
            "Nested generic numbers: sum={}, fallback: {:?}",
            sum, msg.fallback
        ));
        sum
    }

    #[handler]
    async fn handle_generic_with_ownership(
        &mut self,
        msg: GenericWithOwnership<f64>,
        _: &ActorRef<Self>,
    ) -> f64 {
        self.messages_log.push(format!(
            "Generic with ownership: value={}, ref_data='{}'",
            *msg.owned_data, msg.reference_data
        ));
        *msg.owned_data * 2.0
    }

    #[handler]
    async fn handle_complex_nested(
        &mut self,
        msg: ComplexNested<String, i32, bool>,
        _: &ActorRef<Self>,
    ) -> usize {
        let queue_size = msg.processing_queue.len();
        self.messages_log.push(format!(
            "Complex nested: {} items in queue, metadata: {}",
            queue_size, msg.metadata
        ));
        queue_size
    }

    #[handler]
    async fn handle_generic_operation_string(
        &mut self,
        msg: GenericOperation<String>,
        _: &ActorRef<Self>,
    ) -> String {
        let result = match msg {
            GenericOperation::Process(s) => format!("Processed: {s}"),
            GenericOperation::Transform { input, factor } => {
                format!("Transformed: {input} (factor: {factor})")
            }
            GenericOperation::Batch(items) => format!("Batch: {} items", items.len()),
            GenericOperation::None => "None operation".to_string(),
        };
        self.messages_log
            .push(format!("Generic operation result: {result}"));
        result
    }

    #[handler]
    async fn handle_generic_operation_number(
        &mut self,
        msg: GenericOperation<i32>,
        _: &ActorRef<Self>,
    ) -> i32 {
        let result = match msg {
            GenericOperation::Process(n) => n * 2,
            GenericOperation::Transform { input, factor } => (input as f64 * factor) as i32,
            GenericOperation::Batch(items) => items.iter().sum(),
            GenericOperation::None => 0,
        };
        self.messages_log
            .push(format!("Generic operation number result: {result}"));
        result
    }

    #[handler]
    async fn handle_recursive_generic(
        &mut self,
        msg: RecursiveGeneric<String>,
        _: &ActorRef<Self>,
    ) -> usize {
        self.messages_log.push(format!(
            "Recursive generic: value='{}', depth={}, children={}",
            msg.value,
            msg.depth,
            msg.children.len()
        ));
        msg.depth
    }

    #[handler]
    async fn handle_generic_processor(
        &mut self,
        msg: GenericProcessor<String, i32>,
        _: &ActorRef<Self>,
    ) -> i32 {
        self.messages_log.push(format!(
            "Generic processor: input='{}', processor_id='{}'",
            msg.input, msg.processor_id
        ));
        msg.input.len() as i32
    }

    #[handler]
    async fn handle_generic_tuple(
        &mut self,
        msg: GenericTuple<String, i32, bool>,
        _: &ActorRef<Self>,
    ) -> String {
        self.messages_log
            .push(format!("Generic tuple: ({}, {}, {})", msg.0, msg.1, msg.2));
        "Tuple processed".to_string() // Fixed clippy warning
    }

    #[handler]
    async fn handle_complex_generic_result(
        &mut self,
        msg: ComplexGenericResult<String, i32>,
        _: &ActorRef<Self>,
    ) -> usize {
        let success_count = msg.results.iter().filter(|r| r.is_ok()).count();
        self.messages_log.push(format!(
            "Complex generic result: {}/{} successful, retry_count: {}",
            success_count,
            msg.results.len(),
            msg.retry_count
        ));
        success_count
    }

    // Handlers for additional complex generic message types with advanced trait bounds

    #[handler]
    async fn handle_advanced_constrained_generic(
        &mut self,
        msg: AdvancedConstrainedGeneric<String>,
        _: &ActorRef<Self>,
    ) -> String {
        let hash_size = msg.hash_map.len();
        self.messages_log.push(format!(
            "Advanced constrained: data='{}', hash_size={}, default='{}'",
            msg.data, hash_size, msg.default_value
        ));
        format!("processed_{}_{}", msg.data, hash_size)
    }

    #[handler]
    async fn handle_math_bound_generic(
        &mut self,
        msg: MathBoundGeneric<f64>,
        _: &ActorRef<Self>,
    ) -> f64 {
        let sum: f64 = msg.values.iter().sum();
        self.messages_log.push(format!(
            "Math bound: {} values, sum={}, operations: {:?}",
            msg.values.len(),
            sum,
            msg.operations_log
        ));
        sum
    }

    #[handler]
    async fn handle_phantom_complex_generic(
        &mut self,
        msg: PhantomComplexGeneric<String, String, i32>,
        _: &ActorRef<Self>,
    ) -> usize {
        let data_size = msg.data.len();
        self.messages_log.push(format!(
            "Phantom complex: {} items, type_info='{}'",
            data_size, msg.type_info
        ));
        data_size
    }

    #[handler]
    async fn handle_function_bound_generic(
        &mut self,
        msg: FunctionBoundGeneric<String>,
        _: &ActorRef<Self>,
    ) -> String {
        let transformed = format!("transformed_{}", msg.data);
        self.messages_log.push(format!(
            "Function bound: data='{}', transformer_id='{}'",
            msg.data, msg.transformer_id
        ));
        transformed
    }

    #[handler]
    async fn handle_iterator_bound_generic(
        &mut self,
        msg: IteratorBoundGeneric<String>,
        _: &ActorRef<Self>,
    ) -> String {
        let first_item = msg.source.first().cloned().unwrap_or_default();
        self.messages_log.push(format!(
            "Iterator bound: first_item='{}', config='{}'",
            first_item, msg.iterator_config
        ));
        first_item
    }

    #[handler]
    async fn handle_serialization_bound_generic(
        &mut self,
        msg: SerializationBoundGeneric<String>,
        _: &ActorRef<Self>,
    ) -> String {
        self.messages_log.push(format!(
            "Serialization bound: data='{}', cached='{}'",
            msg.data, msg.serialized_cache
        ));
        msg.data
    }

    #[handler]
    async fn handle_associated_type_bound_generic(
        &mut self,
        msg: AssociatedTypeBoundGeneric<CustomProcessorImpl>,
        _: &ActorRef<Self>,
    ) -> String {
        let cache_size = msg.input_cache.len();
        self.messages_log.push(format!(
            "Associated type bound: cache_size={}, errors={}",
            cache_size,
            msg.error_log.len()
        ));
        format!("processed_with_cache_size_{cache_size}")
    }

    #[handler]
    async fn handle_nested_trait_bound_generic(
        &mut self,
        msg: NestedTraitBoundGeneric<String, String, String>,
        _: &ActorRef<Self>,
    ) -> String {
        let original = msg.original.clone();
        let result = msg
            .final_result
            .unwrap_or_else(|| msg.intermediate.unwrap_or(msg.original));
        self.messages_log.push(format!(
            "Nested trait bound: original='{original}', final='{result}'"
        ));
        result
    }

    #[handler]
    async fn handle_async_bound_generic(
        &mut self,
        msg: AsyncBoundGeneric<String, String>,
        _: &ActorRef<Self>,
    ) -> String {
        let result = format!("async_processed_{}", msg.input);
        self.messages_log.push(format!(
            "Async bound: input='{}', processor_id='{}'",
            msg.input, msg.processor_id
        ));
        result
    }

    #[handler]
    async fn handle_error_handling_generic(
        &mut self,
        msg: ErrorHandlingGeneric<String, CustomError>,
        _: &ActorRef<Self>,
    ) -> String {
        let mut success_count = 0;
        for result in &msg.operations {
            if result.is_ok() {
                success_count += 1;
            }
        }

        let response = format!(
            "Processed {} successful operations out of {}",
            success_count,
            msg.operations.len()
        );
        self.messages_log.push(format!(
            "Error handling: successes={}/{}, recovery_options={}",
            success_count,
            msg.operations.len(),
            msg.error_recovery.len()
        ));
        response
    }
}

#[tokio::test]
async fn test_simple_unit_struct_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let response: String = actor_ref.ask(Ping).await.unwrap();
    assert_eq!(response, "Pong #1");

    let response: String = actor_ref.ask(Ping).await.unwrap();
    assert_eq!(response, "Pong #2");
}

#[tokio::test]
async fn test_tuple_struct_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let old_value: u32 = actor_ref.ask(SetCounter(42)).await.unwrap();
    assert_eq!(old_value, 0);

    let old_value: u32 = actor_ref.ask(SetCounter(100)).await.unwrap();
    assert_eq!(old_value, 42);
}

#[tokio::test]
async fn test_named_struct_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let is_new: bool = actor_ref
        .ask(StoreData {
            key: "test_key".to_string(),
            value: 123,
        })
        .await
        .unwrap();
    assert!(is_new);

    let is_new: bool = actor_ref
        .ask(StoreData {
            key: "test_key".to_string(),
            value: 456,
        })
        .await
        .unwrap();
    assert!(!is_new); // same key, so not new
}

#[tokio::test]
async fn test_generic_messages() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test generic with String
    let response: String = actor_ref
        .ask(GenericMessage {
            data: "Hello World".to_string(),
            label: "greeting".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(response, "Processed: Hello World");

    // Test generic with i32
    let response: i32 = actor_ref
        .ask(GenericMessage {
            data: 21,
            label: "number".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(response, 42);
}

#[tokio::test]
async fn test_batch_update_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let updates = vec![
        ("key1".to_string(), 1),
        ("key2".to_string(), 2),
        ("key3".to_string(), 3),
    ];

    let count: usize = actor_ref.ask(BatchUpdate { updates }).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_optional_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test with Some value
    let response: String = actor_ref
        .ask(OptionalMessage {
            required_field: "test".to_string(),
            optional_field: Some(42),
        })
        .await
        .unwrap();
    assert_eq!(response, "test: 42");

    // Test with None value
    let response: String = actor_ref
        .ask(OptionalMessage {
            required_field: "test2".to_string(),
            optional_field: None,
        })
        .await
        .unwrap();
    assert_eq!(response, "test2: no value");
}

#[tokio::test]
async fn test_enum_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Set initial value
    let _: u32 = actor_ref.ask(SetCounter(10)).await.unwrap();

    let result: u32 = actor_ref.ask(Operation::Add(5)).await.unwrap();
    assert_eq!(result, 15);

    let result: u32 = actor_ref.ask(Operation::Multiply(2)).await.unwrap();
    assert_eq!(result, 30);

    let result: u32 = actor_ref.ask(Operation::Subtract(10)).await.unwrap();
    assert_eq!(result, 20);

    let result: u32 = actor_ref.ask(Operation::Reset).await.unwrap();
    assert_eq!(result, 0);
}

#[tokio::test]
async fn test_simple_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut metadata = HashMap::new();
    metadata.insert("version".to_string(), "1.0".to_string());
    metadata.insert("source".to_string(), "test".to_string());

    let msg = SimpleMessage {
        id: 12345,
        content: "Test message content".to_string(),
        metadata,
    };

    let response: u64 = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, 12345);
}

#[tokio::test]
async fn test_complex_nested_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = ComplexMessage {
        header: MessageHeader {
            timestamp: 1234567890,
            sender: "test_sender".to_string(),
            priority: 1,
        },
        payload: MessagePayload {
            data: vec![1, 2, 3, 4, 5],
            format: "binary".to_string(),
        },
    };

    let payload_size: usize = actor_ref.ask(msg).await.unwrap();
    assert_eq!(payload_size, 5);
}

#[tokio::test]
async fn test_status_query_message() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Send some messages first
    let _: String = actor_ref.ask(Ping).await.unwrap();
    let _: u32 = actor_ref.ask(SetCounter(42)).await.unwrap();
    let _: bool = actor_ref
        .ask(StoreData {
            key: "test".to_string(),
            value: 123,
        })
        .await
        .unwrap();

    // Get status
    let status: StatusResponse = actor_ref.ask(GetStatus).await.unwrap();

    assert_eq!(status.counter, 42);
    assert_eq!(status.data_store_size, 1);
    assert!(status.messages_count >= 3);
    assert!(!status.last_messages.is_empty());
}

#[tokio::test]
async fn test_multiple_message_types_concurrently() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Send multiple different message types concurrently
    let ping_task = actor_ref.ask(Ping);
    let counter_task = actor_ref.ask(SetCounter(100));
    let store_task = actor_ref.ask(StoreData {
        key: "concurrent_test".to_string(),
        value: 999,
    });

    let (ping_result, counter_result, store_result) =
        tokio::try_join!(ping_task, counter_task, store_task).unwrap();

    // Note: Due to concurrent execution, the ping might be processed after SetCounter
    // so we just check that all operations completed successfully
    assert!(ping_result.starts_with("Pong #"));
    // counter_result could be 0 or 1 depending on execution order
    assert!(counter_result <= 1);
    assert!(store_result); // new key
}

#[tokio::test]
async fn test_message_with_timeout() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test that messages complete within reasonable time
    let result = timeout(Duration::from_millis(100), actor_ref.ask(Ping)).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().unwrap(), "Pong #1");
}

#[tokio::test]
async fn test_large_batch_processing() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Create a large batch of updates
    let updates: Vec<(String, i32)> = (0..1000).map(|i| (format!("key_{i}"), i)).collect();

    let count: usize = actor_ref.ask(BatchUpdate { updates }).await.unwrap();
    assert_eq!(count, 1000);

    // Verify the data was stored
    let status: StatusResponse = actor_ref.ask(GetStatus).await.unwrap();
    assert_eq!(status.data_store_size, 1000);
}

// Complex Generic Type Tests

#[tokio::test]
async fn test_multi_generic_messages() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test String + i32 combination
    let msg1 = MultiGenericMessage {
        primary: "Hello".to_string(),
        secondary: 42,
        context: "test_context".to_string(),
    };
    let response1: String = actor_ref.ask(msg1).await.unwrap();
    assert_eq!(response1, "Combined: Hello + 42");

    // Test Vec<u8> + bool combination
    let msg2 = MultiGenericMessage {
        primary: vec![1, 2, 3, 4, 5],
        secondary: true,
        context: "binary_context".to_string(),
    };
    let response2: usize = actor_ref.ask(msg2).await.unwrap();
    assert_eq!(response2, 5);
}

#[tokio::test]
async fn test_constrained_generic_messages() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test with String
    let msg1 = ConstrainedGeneric {
        data: "test_data".to_string(),
        metadata: vec!["meta1".to_string(), "meta2".to_string()],
    };
    let response1: usize = actor_ref.ask(msg1).await.unwrap();
    assert_eq!(response1, 2);

    // Test with i64
    let msg2 = ConstrainedGeneric {
        data: 123i64,
        metadata: vec!["tag1".to_string()],
    };
    let response2: i64 = actor_ref.ask(msg2).await.unwrap();
    assert_eq!(response2, 1230);
}

#[tokio::test]
async fn test_nested_generic_messages() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test with String
    let msg1 = NestedGeneric {
        outer: GenericContainer {
            items: vec![
                "item1".to_string(),
                "item2".to_string(),
                "item3".to_string(),
            ],
            capacity: 10,
        },
        fallback: Some("fallback".to_string()),
    };
    let response1: usize = actor_ref.ask(msg1).await.unwrap();
    assert_eq!(response1, 3);

    // Test with i32
    let msg2 = NestedGeneric {
        outer: GenericContainer {
            items: vec![10, 20, 30, 40],
            capacity: 5,
        },
        fallback: None,
    };
    let response2: i32 = actor_ref.ask(msg2).await.unwrap();
    assert_eq!(response2, 100); // sum of 10+20+30+40
}

#[tokio::test]
async fn test_generic_with_ownership() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = GenericWithOwnership {
        owned_data: Box::new(std::f64::consts::PI),
        reference_data: "pi_reference".to_string(),
    };

    let response: f64 = actor_ref.ask(msg).await.unwrap();
    assert!((response - std::f64::consts::TAU).abs() < 0.01); // PI * 2 = TAU
}

#[tokio::test]
async fn test_complex_nested_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut map_data = HashMap::new();
    map_data.insert("key1".to_string(), 100);
    map_data.insert("key2".to_string(), 200);

    let msg = ComplexNested {
        map_data,
        metadata: true,
        processing_queue: vec![
            ("queue1".to_string(), 1),
            ("queue2".to_string(), 2),
            ("queue3".to_string(), 3),
        ],
    };

    let response: usize = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, 3);
}

#[tokio::test]
async fn test_generic_operation_enum() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    // Test with String operations
    let response1: String = actor_ref
        .ask(GenericOperation::Process("test_string".to_string()))
        .await
        .unwrap();
    assert_eq!(response1, "Processed: test_string");

    let response2: String = actor_ref
        .ask(GenericOperation::Transform {
            input: "transform_me".to_string(),
            factor: 2.5,
        })
        .await
        .unwrap();
    assert_eq!(response2, "Transformed: transform_me (factor: 2.5)");

    let response3: String = actor_ref
        .ask(GenericOperation::Batch(vec![
            "item1".to_string(),
            "item2".to_string(),
        ]))
        .await
        .unwrap();
    assert_eq!(response3, "Batch: 2 items");

    // Test with i32 operations
    let response4: i32 = actor_ref.ask(GenericOperation::Process(21)).await.unwrap();
    assert_eq!(response4, 42); // 21 * 2

    let response5: i32 = actor_ref
        .ask(GenericOperation::Transform {
            input: 10,
            factor: 3.5,
        })
        .await
        .unwrap();
    assert_eq!(response5, 35); // 10 * 3.5 = 35

    let response6: i32 = actor_ref
        .ask(GenericOperation::Batch(vec![1, 2, 3, 4, 5]))
        .await
        .unwrap();
    assert_eq!(response6, 15); // sum
}

#[tokio::test]
async fn test_recursive_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let child1 = RecursiveGeneric {
        value: "child1".to_string(),
        children: vec![],
        depth: 2,
    };

    let child2 = RecursiveGeneric {
        value: "child2".to_string(),
        children: vec![],
        depth: 2,
    };

    let msg = RecursiveGeneric {
        value: "root".to_string(),
        children: vec![Box::new(child1), Box::new(child2)],
        depth: 1,
    };

    let response: usize = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, 1); // depth of root
}

#[tokio::test]
async fn test_generic_processor() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = GenericProcessor {
        input: "process_this_string".to_string(),
        processor_id: "processor_v2".to_string(),
        expected_output_type: std::marker::PhantomData::<i32>,
    };

    let response: i32 = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, 19); // length of "process_this_string"
}

#[tokio::test]
async fn test_generic_tuple() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = GenericTuple("tuple_string".to_string(), 123, true);

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "Tuple processed");
}

#[tokio::test]
async fn test_complex_generic_result() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = ComplexGenericResult {
        results: vec![
            Ok("success1".to_string()),
            Err(404),
            Ok("success2".to_string()),
            Err(500),
            Ok("success3".to_string()),
        ],
        summary: Some("Processing results".to_string()),
        retry_count: 2,
    };

    let response: usize = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, 3); // 3 successful results
}

// Advanced Complex Generic Type Tests

#[tokio::test]
async fn test_advanced_constrained_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut hash_map = HashMap::new();
    hash_map.insert("key1".to_string(), "value1".to_string());
    hash_map.insert("key2".to_string(), "value2".to_string());

    let msg = AdvancedConstrainedGeneric {
        data: "test_data".to_string(),
        hash_map,
        default_value: String::default(),
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "processed_test_data_2");
}

#[tokio::test]
async fn test_function_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = FunctionBoundGeneric {
        data: "input_data".to_string(),
        transformer_id: "transformer_v1".to_string(),
        cache: vec!["cached1".to_string(), "cached2".to_string()],
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "transformed_input_data");
}

#[tokio::test]
async fn test_iterator_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = IteratorBoundGeneric {
        source: vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string(),
        ],
        iterator_config: "parallel_config".to_string(),
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "item1");
}

#[tokio::test]
async fn test_serialization_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut metadata = HashMap::new();
    metadata.insert("format".to_string(), "json".to_string());
    metadata.insert("version".to_string(), "2.0".to_string());

    let msg = SerializationBoundGeneric {
        data: "serializable_data".to_string(),
        serialized_cache: "{\"data\":\"serializable_data\"}".to_string(),
        metadata,
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "serializable_data");
}

#[tokio::test]
async fn test_math_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = MathBoundGeneric {
        values: vec![1.0, 2.5, 3.7, 4.2],
        operations_log: vec!["add".to_string(), "multiply".to_string()],
    };

    let response: f64 = actor_ref.ask(msg).await.unwrap();
    assert!((response - 11.4).abs() < 0.01); // sum of values
}

#[tokio::test]
async fn test_associated_type_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut input_cache = HashMap::new();
    input_cache.insert("test_input".to_string(), 10);

    let msg = AssociatedTypeBoundGeneric {
        processor: CustomProcessorImpl,
        input_cache,
        error_log: vec!["previous_error".to_string()],
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "processed_with_cache_size_1");
}

#[tokio::test]
async fn test_async_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = AsyncBoundGeneric {
        input: "async_input".to_string(),
        processor_id: "async_processor_v2".to_string(),
        result_cache: Some("cached_result".to_string()),
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "async_processed_async_input");
}

#[tokio::test]
async fn test_nested_trait_bound_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let msg = NestedTraitBoundGeneric {
        original: "original_value".to_string(),
        intermediate: Some("intermediate_value".to_string()),
        final_result: Some("final_result".to_string()),
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "final_result");
}

#[tokio::test]
async fn test_error_handling_generic() {
    let (actor_ref, _join_handle) = spawn::<MessageHandlerTestActor>(());

    let mut error_recovery = HashMap::new();
    error_recovery.insert("error_type_1".to_string(), "recovery_value_1".to_string());

    let msg = ErrorHandlingGeneric {
        operations: vec![
            Ok("success1".to_string()),
            Err(CustomError {
                message: "test error".to_string(),
                code: 404,
            }),
            Ok("success2".to_string()),
            Err(CustomError {
                message: "another error".to_string(),
                code: 500,
            }),
        ],
        error_recovery,
    };

    let response: String = actor_ref.ask(msg).await.unwrap();
    assert_eq!(response, "Processed 2 successful operations out of 4");
}
