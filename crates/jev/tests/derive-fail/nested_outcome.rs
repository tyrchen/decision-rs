use jev::{Decision, Outcome, Predicate};
#[derive(Debug, Decision)]
struct Bad {
    /// Question.
    value: Outcome<Outcome<Predicate>>,
}
fn main() {}
