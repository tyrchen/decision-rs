# Decision primitives

Every backend returns the same checked answer types. `Probability::try_from(f64)`
rejects NaN, infinities and values outside `0..=1`; public thresholds use this type.

`Predicate::probability` reports P(true). `is_yes(gate)` tests P(true), and
`is_no(gate)` tests P(false). `decide(yes_gate, no_gate)` returns `Yes`, `No` or
`Unsure`; overlapping gates also return Unsure. Two branch descriptions in
`#[jev(yes = "...", no = "...")]` remain intact on each backend. OpenAI internally
uses a boolean choice for that form and extracts P(true).

`Choice<T>` uses a documented fieldless `ChoiceOptions` enum. `value()` borrows the
selected variant and `into_value()` consumes it. `probability_of(&variant)` reads
the reported event probability. `accept(gate)` uses the selected event probability;
`accept_confidence(gate)` uses separately reported confidence. Neither substitutes
one statistic for the other. `sample(roll)` normalizes only sampling mass for
rounding and errors when the distribution is unavailable.

`Score<T>` uses a documented ordered `ScoreLevels` enum. Each `LevelSpec` retains a
stable key and description. `raw()` preserves the continuous score; `normalized()`
scales to `0..=1`. `nearest()` rounds halves upwards and returns a checked enum,
which can differ from the distribution's most probable level. `at_least(&level)`
compares the continuous score. `confident(gate)` tests native confidence only.

```rust
use jev::{Choice, ChoiceOptions, Decision, Outcome, Predicate, Score, ScoreLevels};
#[derive(Debug, ChoiceOptions)]
enum Team {
    /// Payments and refunds.
    Billing,
    /// Product failures.
    Technical,
}
#[derive(Debug, ScoreLevels)]
enum Urgency {
    /// Can wait for ordinary support.
    Low,
    /// Immediate attention is required.
    High,
}
#[derive(Debug, Decision)]
struct Triage {
    /// Does the customer report a defect?
    defect: Predicate,
    /// Which team should help?
    team: Outcome<Choice<Team>>,
    /// How urgent is the request?
    urgency: Score<Urgency>,
}
```

`Outcome<T>` preserves an explicit `Refused(Refusal)` beside normal answers.
Ordinary fields instead cause `Error::Refused` containing all required refused
question keys and response metadata. Missing/unknown/duplicate answers and malformed
statistics are protocol errors even for Outcome fields.

Dynamic schemas use `Questions::builder().predicate(...).choice(...).score(...).build()`.
`Client::ask(input, &questions)` returns `DecisionResponse`; `answers.decode::<T>(key)`
provides the same checked conversion. Answers iterate in schema declaration order.
Optional Jev distributions become `Distribution::Unavailable`; reported distributions
must be complete and sum to one within 0.001. Scores must agree with the normalized
weighted mean within `0.001 * max(1, levels - 1)`. Reported values are preserved.
