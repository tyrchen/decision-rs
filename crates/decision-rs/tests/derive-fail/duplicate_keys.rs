//! Two `ChoiceOptions` variants resolving to the same explicit key.
use decision_rs::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    #[decision(key = "same")]
    Payments,
    /// UI issues.
    #[decision(key = "same")]
    Frontend,
}

fn main() {}
