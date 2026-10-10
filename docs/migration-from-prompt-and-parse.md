# From prompt-and-parse to typed decisions

Express independent classification questions as a `Decision` struct, categories
as a `ChoiceOptions` enum, and ordered scales as a `ScoreLevels` enum. Put precise
instructions on fields and criteria on variants. The derive turns that single
schema into each backend's request and a checked business result.

```rust,no_run
# async fn run(client: jev::Client) -> Result<(), jev::Error> {
use jev::{Decision, Outcome, Predicate};
#[derive(Debug, Decision)]
struct Review {
    /// Does the customer explicitly request money back?
    refund: Outcome<Predicate>,
}
let review: Review = client.decide("Please refund the duplicate charge.").await?;
match review.refund {
    Outcome::Answered(value) => { let probability = value.probability().get(); }
    Outcome::Refused(refusal) => { let question = refusal.key(); }
}
# Ok(())
# }
```

Pass text directly or construct `Input::json` for structured evidence. Separate
questions that depend on previous answers into multiple calls. Select the provider
in client construction; no response JSON parsing or wire-kind branching is needed.

Decisions return bounded categorical probabilities and continuous scores rather
than prose. Use explicit gates with an escalation band, evaluate per model, and
retain response metadata when monitoring outcomes. See [primitives](primitives.md),
[thresholds](thresholds.md) and [0.3 migration](migration-v0.3.md).
