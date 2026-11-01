// Declaration-only crate of the `member` fixture.
use rsactor::{message_handlers, Actor, ActorRef};

pub struct Start;

#[derive(Actor)]
pub struct Engine;

#[message_handlers]
impl Engine {
    #[handler]
    async fn start(&mut self, _m: Start, _: &ActorRef<Self>) -> bool {
        true
    }
}
