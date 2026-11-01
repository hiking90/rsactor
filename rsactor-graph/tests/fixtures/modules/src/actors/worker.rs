// The same message types are spelled three ways: through an in-scope glob,
// by full path, and through a renamed re-export. All must give one key.
use crate::msgs::Deep as Renamed;
use crate::msgs::*;
use rsactor::{message_handlers, Actor, ActorRef};

#[derive(Actor)]
pub struct Worker;

#[message_handlers]
impl Worker {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}

    #[handler]
    async fn pong(&mut self, _m: crate::msgs::Pong, _: &ActorRef<Self>) -> u32 {
        0
    }

    #[handler]
    async fn deep(&mut self, _m: Renamed, _: &ActorRef<Self>) {}
}
