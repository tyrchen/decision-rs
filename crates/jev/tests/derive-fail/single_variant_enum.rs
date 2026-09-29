//! `ChoiceOptions` on a single-variant enum: cannot express a choice.
use jev::ChoiceOptions;

#[derive(ChoiceOptions)]
enum OnlyOne {
    /// The only option.
    Solo,
}

fn main() {}
