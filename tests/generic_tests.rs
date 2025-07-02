// filepath: /Volumes/Workspace/rust/rsactor/tests/generic_tests.rs
// filepath: /Volumes/Workspace/rust/rsactor/tests/generic_tests.rs

use anyhow::Result;
use rsactor::{spawn, Actor, ActorRef, ActorResult, Message};
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
        Self {
            id,
            name: name.to_string(),
        }
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
        let (actor_ref, join_handle) =
            spawn::<GenericActor<MyTestData>>(Some(initial_data.clone()));

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

    #[tokio::test]
    async fn test_bi_generic_actor() {
        // Test BiGenericActor with String and i32
        let (actor_ref, join_handle) = spawn::<BiGenericActor<String, i32>>((None, None));

        // Set key-value pair
        actor_ref
            .tell(SetKeyValue("hello".to_string(), 42))
            .await
            .unwrap();

        // Get key
        let key: Option<String> = actor_ref.ask(GetKey).await.unwrap();
        assert_eq!(key, Some("hello".to_string()));

        // Get value
        let value: Option<i32> = actor_ref.ask(GetValueFromBi).await.unwrap();
        assert_eq!(value, Some(42));

        // Clear both
        actor_ref.tell(ClearBoth).await.unwrap();
        let key: Option<String> = actor_ref.ask(GetKey).await.unwrap();
        let value: Option<i32> = actor_ref.ask(GetValueFromBi).await.unwrap();
        assert_eq!(key, None);
        assert_eq!(value, None);

        // Stop actor
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
    }

    #[tokio::test]
    async fn test_tri_generic_actor() {
        // Test TriGenericActor with Vec<u8>, bool, and String
        let (actor_ref, join_handle) =
            spawn::<TriGenericActor<Vec<u8>, bool, String>>((true, "initial".to_string()));

        // Test GetAll to get initial state
        let (t_data, u_data, w_data): (Vec<u8>, bool, String) =
            actor_ref.ask(GetAll).await.unwrap();
        assert_eq!(t_data, Vec::<u8>::default()); // T::default()
        assert!(u_data);
        assert_eq!(w_data, "initial".to_string());

        // Set T data
        actor_ref.tell(SetT(vec![1, 2, 3])).await.unwrap();

        // Set U data
        actor_ref.tell(SetU(false)).await.unwrap();

        // Set W data
        actor_ref.tell(SetW("updated".to_string())).await.unwrap();

        // Get all updated data
        let (t_data, u_data, w_data): (Vec<u8>, bool, String) =
            actor_ref.ask(GetAll).await.unwrap();
        assert_eq!(t_data, vec![1, 2, 3]);
        assert!(!u_data);
        assert_eq!(w_data, "updated".to_string());

        // Test CompareU
        let is_equal: bool = actor_ref.ask(CompareU(false)).await.unwrap();
        assert!(is_equal);

        let is_equal: bool = actor_ref.ask(CompareU(true)).await.unwrap();
        assert!(!is_equal);

        // Test GetWAsString
        let w_as_string: String = actor_ref.ask(GetWAsString).await.unwrap();
        assert_eq!(w_as_string, "updated".to_string());

        // Stop actor
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
    }

    #[tokio::test]
    async fn test_serializable_actor_json() {
        // Test SerializableActor with JsonData
        let (actor_ref, join_handle) = spawn::<SerializableActor<JsonData>>(());

        // Set JsonData
        let json_data = JsonData {
            value: "test".to_string(),
        };
        actor_ref
            .tell(SetSerializable(json_data.clone()))
            .await
            .unwrap();

        // Get original data
        let original: Option<JsonData> = actor_ref.ask(GetOriginal).await.unwrap();
        assert_eq!(original.unwrap().value, "test".to_string());

        // Get serialized data
        let serialized: Option<String> = actor_ref.ask(GetSerialized).await.unwrap();
        assert_eq!(serialized.unwrap(), "{\"value\":\"test\"}".to_string());

        // Stop actor
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
    }

    #[tokio::test]
    async fn test_serializable_actor_xml() {
        // Test SerializableActor with XmlData
        let (actor_ref, join_handle) = spawn::<SerializableActor<XmlData>>(());

        // Set XmlData
        let xml_data = XmlData {
            content: "example".to_string(),
        };
        actor_ref
            .tell(SetSerializable(xml_data.clone()))
            .await
            .unwrap();

        // Get original data
        let original: Option<XmlData> = actor_ref.ask(GetOriginal).await.unwrap();
        assert_eq!(original.unwrap().content, "example".to_string());

        // Get serialized data
        let serialized: Option<String> = actor_ref.ask(GetSerialized).await.unwrap();
        assert_eq!(serialized.unwrap(), "<data>example</data>".to_string());

        // Stop actor
        actor_ref.stop().await.unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_completed());
    }
}

// ---- Multi-Generic Type Examples ----

// Example 1: Actor with two generic types
#[derive(Debug)]
struct BiGenericActor<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static> {
    key: Option<K>,
    value: Option<V>,
}

impl<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static> Actor
    for BiGenericActor<K, V>
{
    type Args = (Option<K>, Option<V>);
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(BiGenericActor {
            key: args.0,
            value: args.1,
        })
    }
}

// Messages for BiGenericActor
#[derive(Debug)]
pub struct SetKeyValue<K: Send + Debug + 'static, V: Send + Debug + 'static>(pub K, pub V);

#[derive(Debug)]
pub struct GetKey;

#[derive(Debug)]
pub struct GetValueFromBi;

#[derive(Debug)]
pub struct ClearBoth;

// Message implementations for BiGenericActor
impl<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static>
    Message<SetKeyValue<K, V>> for BiGenericActor<K, V>
{
    type Reply = ();

    async fn handle(&mut self, msg: SetKeyValue<K, V>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.key = Some(msg.0);
        self.value = Some(msg.1);
    }
}

impl<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static> Message<GetKey>
    for BiGenericActor<K, V>
{
    type Reply = Option<K>;

    async fn handle(&mut self, _msg: GetKey, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.key.clone()
    }
}

impl<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static> Message<GetValueFromBi>
    for BiGenericActor<K, V>
{
    type Reply = Option<V>;

    async fn handle(&mut self, _msg: GetValueFromBi, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.value.clone()
    }
}

impl<K: Send + Debug + Clone + 'static, V: Send + Debug + Clone + 'static> Message<ClearBoth>
    for BiGenericActor<K, V>
{
    type Reply = ();

    async fn handle(&mut self, _msg: ClearBoth, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.key = None;
        self.value = None;
    }
}

// Example 2: Actor with three generic types and complex constraints
#[derive(Debug)]
struct TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    data_t: T,
    data_u: U,
    data_w: W,
}

impl<T, U, W> Actor for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Args = (U, W);
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(TriGenericActor {
            data_t: T::default(),
            data_u: args.0,
            data_w: args.1,
        })
    }
}

// Messages for TriGenericActor
#[derive(Debug)]
pub struct SetT<T: Send + Debug + 'static>(pub T);

#[derive(Debug)]
pub struct SetU<U: Send + Debug + 'static>(pub U);

#[derive(Debug)]
pub struct SetW<W: Send + Debug + 'static>(pub W);

#[derive(Debug)]
pub struct GetAll;

#[derive(Debug)]
pub struct CompareU<U: Send + Debug + 'static>(pub U);

#[derive(Debug)]
pub struct GetWAsString;

// Message implementations for TriGenericActor
impl<T, U, W> Message<SetT<T>> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = ();

    async fn handle(&mut self, msg: SetT<T>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data_t = msg.0;
    }
}

impl<T, U, W> Message<SetU<U>> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = ();

    async fn handle(&mut self, msg: SetU<U>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data_u = msg.0;
    }
}

impl<T, U, W> Message<SetW<W>> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = ();

    async fn handle(&mut self, msg: SetW<W>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data_w = msg.0;
    }
}

impl<T, U, W> Message<GetAll> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = (T, U, W);

    async fn handle(&mut self, _msg: GetAll, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        (
            self.data_t.clone(),
            self.data_u.clone(),
            self.data_w.clone(),
        )
    }
}

impl<T, U, W> Message<CompareU<U>> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = bool;

    async fn handle(&mut self, msg: CompareU<U>, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data_u == msg.0
    }
}

impl<T, U, W> Message<GetWAsString> for TriGenericActor<T, U, W>
where
    T: Send + Debug + Clone + Default + 'static,
    U: Send + Debug + Clone + PartialEq + 'static,
    W: Send + Debug + Clone + ToString + 'static,
{
    type Reply = String;

    async fn handle(&mut self, _msg: GetWAsString, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data_w.to_string()
    }
}

// Example 3: Generic actor with associated types constraints
pub trait Serializable: Send + Debug + Clone + 'static {
    type Output: Send + Debug + Clone + 'static;
    fn serialize(&self) -> Self::Output;
}

#[derive(Debug, Clone)]
pub struct JsonData {
    pub value: String,
}

impl Serializable for JsonData {
    type Output = String;

    fn serialize(&self) -> Self::Output {
        format!("{{\"value\":\"{}\"}}", self.value)
    }
}

#[derive(Debug, Clone)]
pub struct XmlData {
    pub content: String,
}

impl Serializable for XmlData {
    type Output = String;

    fn serialize(&self) -> Self::Output {
        format!("<data>{}</data>", self.content)
    }
}

#[derive(Debug)]
struct SerializableActor<T: Serializable> {
    data: Option<T>,
}

impl<T: Serializable> Actor for SerializableActor<T> {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(SerializableActor { data: None })
    }
}

// Messages for SerializableActor
#[derive(Debug)]
pub struct SetSerializable<T: Serializable>(pub T);

#[derive(Debug)]
pub struct GetSerialized;

#[derive(Debug)]
pub struct GetOriginal;

// Message implementations for SerializableActor
impl<T: Serializable> Message<SetSerializable<T>> for SerializableActor<T> {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: SetSerializable<T>,
        _actor_ref: &ActorRef<Self>,
    ) -> Self::Reply {
        self.data = Some(msg.0);
    }
}

impl<T: Serializable> Message<GetSerialized> for SerializableActor<T> {
    type Reply = Option<T::Output>;

    async fn handle(&mut self, _msg: GetSerialized, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data.as_ref().map(|d| d.serialize())
    }
}

impl<T: Serializable> Message<GetOriginal> for SerializableActor<T> {
    type Reply = Option<T>;

    async fn handle(&mut self, _msg: GetOriginal, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.data.clone()
    }
}
