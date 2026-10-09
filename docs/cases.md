# Predefined cases from YAML/JSON

## What a case is

A [`Case`](../crates/jev/src/cases.rs) is one `(state, questions)` pair: one API call.
A [`CaseSet`](../crates/jev/src/cases.rs) is a batch of them, deserialized from a single
YAML or JSON document — a fixture file of support tickets, a regression suite of prior
decisions, a config-driven batch job. It is the file-backed counterpart of
[`Questions::builder`](../crates/jev/src/question.rs): the same `noul`/`choice`/`score`
shapes, described in a file instead of assembled in code.

```yaml
version: 1
cases:
  - id: ticket-001
    state: "Customer wants a refund for a duplicate charge."
    questions:
      refund:
        type: noul
        instructions: "Is the customer asking for money back?"
      team:
        type: choice
        instructions: "Which team should own this ticket?"
        criteria:
          payments: "Checkout, billing, or payment processing issues."
          frontend: "Rendering, layout, or browser compatibility issues."
  - id: ticket-002
    name: "Login outage report"
    state:
      subject: "Login broken"
      body: "Cannot log in after the last release."
    questions:
      urgency:
        type: score
        instructions: "How urgent is this?"
        criteria: ["Can wait", "This week", "Blocking revenue"]
```

- `id` is required, unique within the file, and echoed back on
  [`CaseResult`](../crates/jev/src/cases.rs) so a result can be correlated back to its
  source case (log lines, a report row, a test assertion).
- `name` is an optional human-readable label, distinct from `id`.
- `state` accepts the same `string | object | array` shapes as
  [`State`](../crates/jev/src/state.rs).
- `questions` accepts the same `type`-tagged `noul`/`choice`/`score` shapes as
  [`Question`](../crates/jev/src/question.rs), keyed by question key, in file order.

## Loading

```rust
use jev::CaseSet;

let json = r#"{"cases": [{
    "id": "ticket-001",
    "state": "Customer wants a refund for a duplicate charge.",
    "questions": {
        "refund": {"type": "noul", "instructions": "Is the customer asking for money back?"}
    }
}]}"#;
let cases = CaseSet::from_json_str(json)?;
# Ok::<(), jev::Error>(())
```

JSON needs no extra feature (`serde_json` is always a dependency). YAML needs the
`yaml` feature:

```toml
[dependencies]
jev-typed = { version = "0.2", features = ["yaml"] }
```

```rust,no_run
# #[cfg(feature = "yaml")]
# async fn run() -> Result<(), jev::Error> {
use jev::CaseSet;

let cases = CaseSet::from_yaml_str(&std::fs::read_to_string("cases.yaml")?)?;
// or, to read the file asynchronously and pick the parser by extension:
let cases = CaseSet::from_file("cases.yaml").await?;
# Ok(())
# }
```

Every loader validates eagerly: case count (`1..=10_000`), `id`/`name` length and
uniqueness, and — via the same checks
[`QuestionsBuilder::build`](../crates/jev/src/question.rs) applies to the hand-written
path — every case's question key charset, guidance length, and option/level counts. A
malformed fixture fails at load time, before any request is sent, not mid-batch.

## Running

One case, one call:

```rust,no_run
# async fn run(client: &jev::Client, case: &jev::Case) -> Result<(), jev::Error> {
let result = client.run_case(case).await?;
println!("{}: {:?}", result.id, result.response.answers);
# Ok(())
# }
```

Many cases, bounded concurrency (the `batch` feature), preserving input order and
reporting one `Result` per case rather than discarding the batch on the first failure:

```rust,no_run
# async fn run() -> Result<(), jev::Error> {
use std::num::NonZeroUsize;

use jev::{CaseSet, Client, run_cases};

let cases = CaseSet::from_json_str(r#"{"cases": [{
    "id": "ticket-001",
    "state": "a refund ticket",
    "questions": {"refund": {"type": "noul", "instructions": "Refund?"}}
}]}"#)?;
let client = Client::from_env()?;
let results = run_cases(&client, cases.as_slice(), NonZeroUsize::new(4).unwrap()).await;
for result in results {
    match result {
        Ok(case) => println!("{}: {:?}", case.id, case.response.answers),
        Err(err) => eprintln!("failed: {err}"),
    }
}
# Ok(())
# }
```

See `examples/09_cases_from_file.rs` for the full runnable version, and
[thresholds.md](./thresholds.md) for how to turn `result.response.answers` back into a
decision once you have it.
