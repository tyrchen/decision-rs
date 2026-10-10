//! `ChoiceOptions` on an enum with a data-carrying variant.
use decision_rs::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    Payments,
    /// UI issues, with a ticket number.
    Frontend(u32),
}

fn main() {}
