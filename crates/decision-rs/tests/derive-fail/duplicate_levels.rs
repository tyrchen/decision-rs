use decision_rs::ScoreLevels;
#[derive(Debug, ScoreLevels)]
enum Bad {
    /// First.
    #[decision(key = "same")]
    First,
    /// Second.
    #[decision(key = "same")]
    Second,
}
fn main() {}
