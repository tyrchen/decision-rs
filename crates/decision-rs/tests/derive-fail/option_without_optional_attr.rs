//! Option fields are rejected; explicit refusals use Outcome.
use decision_rs::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    is_bug: Option<Predicate>,
}

fn main() {}
