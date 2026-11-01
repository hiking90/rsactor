// The bin declares its own `Shared` (same crate name `libbin`, same path)
// and refers to the lib's `Note` as `libbin::Note`.
use rsactor::{message_handlers, Actor, ActorRef};

#[derive(Actor)]
struct Shared;

#[message_handlers]
impl Shared {
    #[handler]
    async fn note(&mut self, _m: libbin::Note, _: &ActorRef<Self>) {}
}

fn main() {}
