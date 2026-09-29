//! `ChoiceOptions` on an enum with a data-carrying variant.
use jev::ChoiceOptions;

#[derive(ChoiceOptions)]
enum Team {
    /// Billing issues.
    Payments,
    /// UI issues, with a ticket number.
    Frontend(u32),
}

fn main() {}
