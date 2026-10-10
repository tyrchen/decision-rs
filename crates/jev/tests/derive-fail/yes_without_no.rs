//! `Predicate` field with `yes` but no `no`.
use jev::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[jev(yes = "Describes broken behavior.")]
    is_bug: Predicate,
}

fn main() {}
