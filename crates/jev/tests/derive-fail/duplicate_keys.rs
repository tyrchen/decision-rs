//! Two `ChoiceOptions` variants resolving to the same explicit key.
use jev::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    #[jev(key = "same")]
    Payments,
    /// UI issues.
    #[jev(key = "same")]
    Frontend,
}

fn main() {}
