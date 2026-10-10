//! `Decision` field of an unsupported type (not `Predicate`/`Choice<T>`/`Score<T>`).
use decision_rs::Decision;

#[derive(Decision)]
struct Bad {
    /// A plain string field, not a decision primitive.
    flag: String,
}

fn main() {}
