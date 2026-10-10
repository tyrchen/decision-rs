use jev::ScoreLevels;
#[derive(Debug, ScoreLevels)]
enum Bad {
    /// First.
    #[jev(key = "same")]
    First,
    /// Second.
    #[jev(key = "same")]
    Second,
}
fn main() {}
