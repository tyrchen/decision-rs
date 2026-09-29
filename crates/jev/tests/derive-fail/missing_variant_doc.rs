//! `ChoiceOptions` variant missing its required doc comment.
use jev::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    Payments,
    Frontend,
}

fn main() {}
