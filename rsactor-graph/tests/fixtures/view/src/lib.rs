// Fixture: DOT readability options and `--focus` (plan §8, stage 6).
// Not compiled.
use rsactor::{message_handlers, Actor, ActorRef};

pub struct Ping;
pub struct Query;

#[derive(Actor)]
pub struct Hub;

#[message_handlers]
impl Hub {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}
    #[handler]
    async fn query(&mut self, _m: Query, _: &ActorRef<Self>) -> u32 {
        0
    }
}

/// Holds Hub and both tells and asks it: one send arrow, holds hidden.
#[derive(Actor)]
pub struct Caller {
    hub: ActorRef<Hub>,
}

impl Caller {
    async fn work(&self) {
        self.hub.tell(Ping).await.ok();
        self.hub.ask(Query).await.ok();
    }
}

/// Holds Hub but never sends to it: the holds edge stays visible. Also holds
/// Caller, an edge between two neighbours of Hub that `--focus Hub
/// --depth 1` must leave out.
#[derive(Actor)]
pub struct Keeper {
    hub: ActorRef<Hub>,
    caller: ActorRef<Caller>,
}

/// Two hops from Hub: Leaf <- Middle <- (nothing) ; Middle -> Leaf.
#[derive(Actor)]
pub struct Middle {
    leaf: ActorRef<Leaf>,
}

impl Middle {
    async fn forward(&self) {
        self.leaf.tell(Ping).await.ok();
    }
}

#[derive(Actor)]
pub struct Leaf;

#[message_handlers]
impl Leaf {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}
}

/// Three methods of one non-actor type: one group node.
pub struct Service {
    hub: ActorRef<Hub>,
}

impl Service {
    pub async fn a(&self) {
        self.hub.tell(Ping).await.ok();
    }
    pub async fn b(&self) {
        self.hub.tell(Ping).await.ok();
    }
    pub async fn c(&self) {
        self.hub.ask(Query).await.ok();
    }
}

/// Free functions of one module: one group node.
pub mod tools {
    use super::*;

    pub async fn one(hub: ActorRef<super::Hub>) {
        hub.tell(Ping).await.ok();
    }
    pub async fn two(hub: ActorRef<super::Hub>) {
        hub.tell(Ping).await.ok();
    }
}

/// A lone function stays a plain function node.
pub async fn alone(hub: ActorRef<Hub>, middle: ActorRef<Middle>) {
    hub.tell(Ping).await.ok();
    let _ = middle;
}

/// Two actors whose paths end in `Twin` make `--focus Twin` ambiguous.
pub mod a {
    #[derive(rsactor::Actor)]
    pub struct Twin;
}
pub mod b {
    #[derive(rsactor::Actor)]
    pub struct Twin;
}
