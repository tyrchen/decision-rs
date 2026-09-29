//! An `Option<Noul>` field missing the required `#[jev(optional)]` attribute.
use jev::{Decision, Noul};

#[derive(Decision)]
struct Bad {
    /// Is this a bug?
    is_bug: Option<Noul>,
}

fn main() {}
