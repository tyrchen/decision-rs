//! `Decision` field of an unsupported type (not `Noul`/`Choice<T>`/`Score<T>`).
use jev::Decision;

#[derive(Decision)]
struct Bad {
    /// A plain string field, not a jev primitive.
    flag: String,
}

fn main() {}
