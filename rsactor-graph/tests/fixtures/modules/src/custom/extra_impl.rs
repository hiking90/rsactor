// Loaded through `#[path = "custom/extra_impl.rs"] mod extra;`.
use crate::msgs::nested::Deep;
use rsactor::{Actor, ActorRef, Message};

pub struct Manual;

impl Actor for Manual {
    type Args = ();
    type Error = ();
    type IdleEvent = ();

    async fn on_start(_args: (), _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(Manual)
    }
}

impl Message<Deep> for Manual {
    type Reply = String;

    async fn handle(&mut self, _m: Deep, _r: &ActorRef<Self>) -> String {
        String::new()
    }
}
