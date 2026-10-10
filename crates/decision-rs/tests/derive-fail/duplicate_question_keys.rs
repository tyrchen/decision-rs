//! Two `Decision` fields resolving to the same explicit question key.
use decision_rs::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[decision(key = "same")]
    is_bug: Predicate,
    /// Is this urgent?
    #[decision(key = "same")]
    is_urgent: Predicate,
}

fn main() {}
