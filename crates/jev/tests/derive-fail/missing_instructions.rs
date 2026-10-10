//! Missing doc comment and missing `#[jev(instructions = ...)]` on a `Predicate` field.
use jev::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    flag: Predicate,
}

fn main() {}
