// Fixture: handler recognition, `open` actors and message keys (§4.2).
// Not compiled.
use rsactor::{message_handlers, Actor, ActorRef, Message};
use std::marker::PhantomData;
use tokio::task::JoinHandle;

pub trait Calculator {}

pub mod calc {
    pub use super::Calculator;
}

pub struct Ping;
pub struct Job;
pub struct Unhandled;

#[derive(Actor)]
pub struct Basic;

#[message_handlers]
impl Basic {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}

    // The derive macro matches `is_ident("handler")`: this is not a handler.
    #[rsactor::handler]
    async fn not_a_handler(&mut self, _m: Unhandled, _: &ActorRef<Self>) {}

    #[handler]
    #[cfg(feature = "extra")]
    async fn gated(&mut self, _m: Job, _: &ActorRef<Self>) -> JoinHandle<String> {
        todo!()
    }

    #[handler]
    async fn calc_a(&mut self, _m: Box<dyn Calculator>, _: &ActorRef<Self>) {}

    #[handler]
    async fn wrong_arity(&mut self, _m: Ping) {}
}

#[derive(Actor)]
pub struct Other;

#[message_handlers]
impl Other {
    // Same trait object as `Basic::calc_a`, spelled through a re-export.
    #[handler]
    async fn calc_b(&mut self, _m: Box<dyn crate::calc::Calculator>, _: &ActorRef<Self>) {}

    // Must not share a key with the `Box<dyn Calculator>` messages.
    #[handler]
    async fn func(&mut self, _m: Box<dyn FnOnce() + Send>, _: &ActorRef<Self>) {}
}

// Blanket impl over the impl's own parameter: handles any message type.
pub struct Logger;

impl Actor for Logger {
    type Args = ();
    type Error = ();
    type IdleEvent = ();
    async fn on_start(_a: (), _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(Logger)
    }
}

impl<M: Send + 'static> Message<M> for Logger {
    type Reply = ();
    async fn handle(&mut self, _m: M, _r: &ActorRef<Self>) {}
}

// The handler's message is the actor's own type parameter.
pub struct Worker<T>(PhantomData<T>);

impl<T: Send + 'static> Actor for Worker<T> {
    type Args = ();
    type Error = ();
    type IdleEvent = ();
    async fn on_start(_a: (), _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(Worker(PhantomData))
    }
}

#[message_handlers]
impl<T: Send + 'static> Worker<T> {
    #[handler]
    async fn take(&mut self, _m: T, _: &ActorRef<Self>) {}
}

// A local trait that happens to be called `Message` does not make an actor.
mod foreign {
    pub trait Message<T> {}
    pub struct NotAnActor;
    impl Message<super::Ping> for NotAnActor {}
}
