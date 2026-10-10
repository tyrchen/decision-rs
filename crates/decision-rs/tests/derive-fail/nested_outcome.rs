use decision_rs::{Decision, Outcome, Predicate};
#[derive(Debug, Decision)]
struct Bad {
    /// Question.
    value: Outcome<Outcome<Predicate>>,
}
fn main() {}
