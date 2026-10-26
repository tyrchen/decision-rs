# Thresholds and escalation

Use event probabilities for automation gates and evaluate each provider/model on
application-labelled examples. A uniform SDK interface cannot make different
models equally calibrated. Native confidence and selected-event probability are
separate statistics, even when some responses happen to return equal numbers.

```rust,no_run
# async fn run(client: decision_rs::Client) -> Result<(), decision_rs::Error> {
use decision_rs::{Decision, Predicate, Probability, Verdict};
#[derive(Debug, Decision)]
struct Review {
    /// Does the customer explicitly request a refund?
    refund: Predicate,
}
let review: Review = client.decide("Please refund the duplicate charge.").await?;
let gate = Probability::try_from(0.9)?;
match review.refund.decide(gate, gate) {
    Verdict::Yes => tracing::info!("refund workflow"),
    Verdict::No => tracing::info!("ordinary support"),
    Verdict::Unsure => tracing::info!("human review"),
}
# Ok(())
# }
```

`Choice::accept` gates on the selected option's probability. To preserve the old
confidence-based policy, explicitly use `accept_confidence`. Absent statistics do
not satisfy a gate; weighted sampling requiring absent statistics returns an error.
`Score::confident` remains a native-confidence gate, while `at_least` uses continuous
position. Overlapping Predicate yes/no gates produce Unsure.

`Thresholds` stores bounded names and validated `Probability` values. Load them via
YAML `thresholds: {refund: 0.9}` or `Thresholds::insert`, then use `require(name)`.
Missing names and invalid values are errors. Settings are immutable inside a client;
create a new client with updated settings and reuse the pool for configuration reloads.

Compare false positives, false negatives, refusal rate, automation coverage, latency
and usage on the same labelled set. Record metadata's requested/resolved/wire/returned
models and SDK version. The Luna alias does not guarantee fixed weights. Costs are
reported only when supplied by the provider; missing cost never means free usage.

For Cloudflare Clef, Clef Flash and Clef Omni, calibrate each model independently.
The self-hosted reference implementation and SGLang can report different native
confidence statistics even for the same weights. Clef's rounded probability
vectors are preserved as reported, within a count-dependent rounding bound;
they are not silently normalized. A high confidence does not prove that long
state was preserved: Workers AI can truncate it to fit the context window.
