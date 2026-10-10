//! `Predicate` field with `yes` but no `no`.
use decision_rs::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[decision(yes = "Describes broken behavior.")]
    is_bug: Predicate,
}

fn main() {}
