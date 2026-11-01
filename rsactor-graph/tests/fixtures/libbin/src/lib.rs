// Fixture: a lib and a bin with the same crate name (node keys include the
// target).
// Not compiled.
use rsactor::Actor;

pub struct Note;

#[derive(Actor)]
pub struct Shared;
