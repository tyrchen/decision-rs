//! Runnable example, level 6: reproducible weighted sampling with `Choice::sample`
//! — for a game or simulation that wants an outcome to vary across runs without
//! ever picking an option the model ruled out, while keeping the draw
//! reproducible (the caller supplies the roll; the crate has no `rand` dependency).
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 06_weighted_sampling --features derive
//! ```

use jev::{ChoiceOptions, Client};

/// What the stranger blocking the path does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Reaction {
    /// The stranger attacks without warning.
    Attack,
    /// The stranger demands a toll before allowing passage.
    Demand,
    /// The stranger steps aside and lets the traveler pass.
    Yield,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let client = Client::from_env()?;
    let scene = "A stranger blocks the narrow bridge, hand resting on the hilt of a sword.";

    let reaction = client
        .choice::<Reaction>(scene, "What does the stranger do next?")
        .await?;
    println!("distribution: {reaction}");

    // `value()` is the model's single likeliest pick — deterministic, always the
    // same option for the same distribution.
    println!("mode (always the same): {:?}", reaction.value());

    // `sample(roll)` instead draws from the full distribution: a roll in the top
    // `probability_of(Attack)` fraction of `0.0..=1.0` returns `Attack`, the next
    // band returns `Demand`, and so on — so a low-probability option still comes up
    // sometimes, in proportion to how likely the model thought it was, without ever
    // returning an option the model gave zero probability.
    //
    // Fixed rolls here stand in for the caller's own RNG (a game's own seeded
    // generator, a UUID-derived value, anything reproducible); a real caller would
    // roll once per playthrough, not hard-code a list like this.
    println!("sampled outcomes across five reproducible rolls:");
    for roll in [0.05, 0.25, 0.5, 0.75, 0.95] {
        println!("  roll {roll:.2} -> {:?}", reaction.sample(roll));
    }

    Ok(())
}
