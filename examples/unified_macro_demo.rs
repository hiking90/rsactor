use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};

// Demo of non-generic actor (original syntax)
#[derive(Debug)]
struct SimpleActor {
    counter: i32,
}

impl Actor for SimpleActor {
    type Error = String;
    type Args = i32;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        println!("SimpleActor started with counter: {}", args);
        Ok(SimpleActor { counter: args })
    }
}

struct Increment;
struct GetCount;

impl Message<Increment> for SimpleActor {
    type Reply = ();

    async fn handle(&mut self, _: Increment, _: &ActorRef<Self>) -> Self::Reply {
        self.counter += 1;
        println!("SimpleActor: incremented to {}", self.counter);
    }
}

impl Message<GetCount> for SimpleActor {
    type Reply = i32;

    async fn handle(&mut self, _: GetCount, _: &ActorRef<Self>) -> Self::Reply {
        self.counter
    }
}

// Using unified macro with non-generic syntax
impl_message_handler!(SimpleActor, [Increment, GetCount]);

// Demo of generic actor (new syntax)
#[derive(Debug)]
struct GenericActor<T: Send + std::fmt::Debug + Clone + 'static> {
    value: Option<T>,
}

impl<T: Send + std::fmt::Debug + Clone + 'static> Actor for GenericActor<T> {
    type Error = String;
    type Args = ();

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        println!("GenericActor<{}> started", std::any::type_name::<T>());
        Ok(GenericActor { value: None })
    }
}

struct SetValue<T: Send + std::fmt::Debug + 'static>(T);
struct GetValue;

impl<T: Send + std::fmt::Debug + Clone + 'static> Message<SetValue<T>> for GenericActor<T> {
    type Reply = ();

    async fn handle(&mut self, msg: SetValue<T>, _: &ActorRef<Self>) -> Self::Reply {
        self.value = Some(msg.0);
        println!("GenericActor: set value to {:?}", self.value);
    }
}

impl<T: Send + std::fmt::Debug + Clone + 'static> Message<GetValue> for GenericActor<T> {
    type Reply = Option<T>;

    async fn handle(&mut self, _: GetValue, _: &ActorRef<Self>) -> Self::Reply {
        self.value.clone()
    }
}

// Using unified macro with generic syntax
impl_message_handler!([T: Send + std::fmt::Debug + Clone + 'static] for GenericActor<T>, [SetValue<T>, GetValue]);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Unified Macro Demo");
    println!("=====================");

    // Test non-generic actor
    println!("\n--- Testing Non-Generic Actor ---");
    let (simple_ref, _) = spawn::<SimpleActor>(0);

    simple_ref.tell(Increment).await?;
    simple_ref.tell(Increment).await?;
    let count: i32 = simple_ref.ask(GetCount).await?;
    println!("Final count: {}", count);

    simple_ref.stop().await?;

    // Test generic actor with String
    println!("\n--- Testing Generic Actor<String> ---");
    let (string_ref, _) = spawn::<GenericActor<String>>(());

    string_ref.tell(SetValue("Hello World".to_string())).await?;
    let string_value: Option<String> = string_ref.ask(GetValue).await?;
    println!("String value: {:?}", string_value);

    string_ref.stop().await?;

    // Test generic actor with i32
    println!("\n--- Testing Generic Actor<i32> ---");
    let (int_ref, _) = spawn::<GenericActor<i32>>(());

    int_ref.tell(SetValue(42)).await?;
    let int_value: Option<i32> = int_ref.ask(GetValue).await?;
    println!("Integer value: {:?}", int_value);

    int_ref.stop().await?;

    println!("\n✅ Unified macro works for both generic and non-generic actors!");

    Ok(())
}
