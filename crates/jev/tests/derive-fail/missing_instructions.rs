//! Missing doc comment and missing `#[jev(instructions = ...)]` on a `Noul` field.
use jev::{Decision, Noul};

#[derive(Decision)]
struct Bad {
    flag: Noul,
}

fn main() {}
