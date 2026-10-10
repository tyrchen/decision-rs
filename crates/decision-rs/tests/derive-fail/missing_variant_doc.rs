//! `ChoiceOptions` variant missing its required doc comment.
use decision_rs::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    Payments,
    Frontend,
}

fn main() {}
