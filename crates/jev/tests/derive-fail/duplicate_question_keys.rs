//! Two `Decision` fields resolving to the same explicit question key.
use jev::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[jev(key = "same")]
    is_bug: Predicate,
    /// Is this urgent?
    #[jev(key = "same")]
    is_urgent: Predicate,
}

fn main() {}
