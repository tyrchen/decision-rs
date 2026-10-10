//! Option fields are rejected; explicit refusals use Outcome.
use jev::{Decision, Predicate};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    is_bug: Option<Predicate>,
}

fn main() {}
