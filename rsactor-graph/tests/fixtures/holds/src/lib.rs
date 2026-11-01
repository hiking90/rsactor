// Fixture: holds edges and type-erased handler nodes. Not compiled.
use rsactor::{
    message_handlers, Actor, ActorControl, ActorRef, ActorWeak, AskHandler, TellHandler,
};
use std::collections::HashMap;

pub struct Ping;
pub struct Query;

#[derive(Actor)]
pub struct A {
    b: ActorRef<B>,
    // Same edge as `b`; both locations are kept.
    weak_b: Option<ActorWeak<B>>,
    workers: HashMap<u32, ActorRef<C>>,
    pinger: Box<dyn TellHandler<Ping> + Send>,
    asker: Box<dyn AskHandler<Query, u32>>,
    // Lifecycle control only: no edge.
    control: Box<dyn ActorControl>,
}

#[derive(Actor)]
pub struct B;

#[message_handlers]
impl B {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}
}

#[derive(Actor)]
pub struct C;

#[message_handlers]
impl C {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}

    #[handler]
    async fn query(&mut self, _m: Query, _: &ActorRef<Self>) -> u32 {
        0
    }
}

// Handles `Query` with a different reply: not an `AskHandler<Query, u32>`.
#[derive(Actor)]
pub struct D;

#[message_handlers]
impl D {
    #[handler]
    async fn query(&mut self, _m: Query, _: &ActorRef<Self>) -> String {
        String::new()
    }
}

// The reference arrives through `type Args`.
pub struct E;

impl Actor for E {
    type Args = (u32, ActorRef<B>);
    type Error = ();
    type IdleEvent = ();
    async fn on_start(_a: Self::Args, _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(E)
    }
}

// `ActorRef<T>` with a generic `T`: reported, no edge.
pub struct G<T: Actor> {
    child: ActorRef<T>,
}

impl<T: Actor> Actor for G<T> {
    type Args = ActorRef<T>;
    type Error = ();
    type IdleEvent = ();
    async fn on_start(child: ActorRef<T>, _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(G { child })
    }
}
