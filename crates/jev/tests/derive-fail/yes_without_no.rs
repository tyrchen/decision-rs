//! `Noul` field with `yes` but no `no`.
use jev::{Decision, Noul};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[jev(yes = "Describes broken behavior.")]
    is_bug: Noul,
}

fn main() {}
