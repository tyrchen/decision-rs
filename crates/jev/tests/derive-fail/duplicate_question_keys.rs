//! Two `Decision` fields resolving to the same explicit question key.
use jev::{Decision, Noul};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    #[jev(key = "same")]
    is_bug: Noul,
    /// Is this urgent?
    #[jev(key = "same")]
    is_urgent: Noul,
}

fn main() {}
