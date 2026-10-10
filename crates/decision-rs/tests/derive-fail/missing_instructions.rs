//! Missing doc comment and missing `#[decision(instructions = ...)]` on a `Predicate` field.
use decision_rs::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    flag: Predicate,
}

fn main() {}
