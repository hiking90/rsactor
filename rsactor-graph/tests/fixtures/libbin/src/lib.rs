// Fixture: a lib and a bin with the same crate name (§6.1 node key).
// Not compiled.
use rsactor::Actor;

pub struct Note;

#[derive(Actor)]
pub struct Shared;
