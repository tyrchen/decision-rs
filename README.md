# decision-rs

Typed decisions for OpenAI Decisions, OpenRouter Jev and TypeSafe System One.
Declare questions once and select a provider when constructing the client. The
same business types, acceptance methods and error categories work on every backend.

```toml
[dependencies]
decision-rs = "0.4.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Import the library as `decision_rs`; derive helper attributes use `#[decision(...)]`.
Requires Rust 1.99 or later, edition 2024.

```rust,no_run
use decision_rs::{Choice, ChoiceOptions, Client, Decision, Outcome, Predicate, Probability};

#[derive(Debug, Decision)]
struct Triage {
    /// Does the ticket describe a product defect?
    #[decision(yes = "Describes broken product behavior.",
          no = "Requests information or a new feature.")]
    defect: Predicate,
    /// Which team should help?
    team: Outcome<Choice<Team>>,
}

#[derive(Debug, ChoiceOptions)]
enum Team {
    /// Payments, invoices and refunds.
    Billing,
    /// Product failures and technical support.
    Technical,
}

async fn triage(key: String) -> Result<(), decision_rs::Error> {
    let client = Client::openai(key)?;
    // Client::openrouter(key)? and Client::typesafe(key)? use the same schema.
    let answer: Triage = client.decide("The screen arrived broken.").await?;
    let gate = Probability::try_from(0.9)?;
    if answer.defect.is_yes(gate) {
        match answer.team {
            Outcome::Answered(team) => { let accepted = team.accept(gate); }
            Outcome::Refused(refusal) => { let question = refusal.key(); }
        }
    }
    Ok(())
}
```

- `Predicate`: probability that a condition is true.
- `Choice<T>`: selected enum variant, a complete distribution when available, and
  separately reported native confidence.
- `Score<T>`: continuous position on ordered levels; nearest rounds half upwards.
- `Outcome<T>`: explicitly answered or refused. Required fields instead return
  `Error::Refused`, retaining response accounting.

`Choice::accept` uses the selected option's probability. Use
`accept_confidence` to gate on native confidence. Statistics omitted by a backend
remain unavailable; weighted sampling returns `StatisticsUnavailable` when needed.
Model calibration can differ, so evaluate thresholds for each model.

`Input::json(&object)` serializes canonical compact JSON text identically for every
backend. Runtime schemas use `Questions::builder`; `Client::ask` returns checked,
declaration-ordered answers and metadata. No provider wire types enter business code.

Set `DECISION_PROVIDER=openai|openrouter|typesafe` and either `DECISION_API_KEY` or the matching
`OPENAI_API_KEY`, `OPENROUTER_API_KEY`, or `TYPESAFE_API_KEY`. With no provider, exactly
one provider-specific key is required. Multiple keys or an unassigned generic key
produce an ambiguity error. `DECISION_MODEL` optionally overrides the provider-aware default.

| Provider | Default model |
|---|---|
| OpenAI | `gpt-6-luna` |
| OpenRouter | `typesafe/jev-1.13` |
| TypeSafe | `jev-1.13.0` |

Clients freeze provider, model and credentials together. `Client::builder` accepts
validated runtime settings and an SDK-controlled `HttpClient` pool that can be
shared across clients. Custom deployments declare a full `Endpoint` URL, protocol,
`Auth` and explicit custom model. Redirects and hidden HTTP retries are disabled.

Network deadlines include queueing, body reads and backoff. Retries cover connection
establishment failures, transient 429 and 500/502/503/504, never exhausted quota or
ambiguous failures after sending. Evidence, schemas, bodies and concurrency are bounded.

| Feature | Default | Purpose |
|---|---|---|
| `derive` | yes | Decision and enum derives |
| `retry` | yes | Explicit bounded retry; deadlines remain enabled without it |
| `rustls-tls` | yes | rustls with aws-lc-rs |
| `batch` | no | Bounded, ordered batches with individual errors |
| `yaml` | no | Strict budgeted YAML configuration and version 2 case files |

[Quickstart](docs/quickstart.md) · [Primitives](docs/primitives.md) ·
[Thresholds](docs/thresholds.md) · [Cases](docs/cases.md) ·
[0.4 migration](docs/migration-v0.4.md) · [Design](specs/unified-decision-backends-design.md)

Run `make build`, `make test`, `make lint`, `make feature-check`, `make audit`, and
`make publish-check` for the release gates. `make test-live` requires all three
provider credentials and can incur API charges; individual provider filters are
shown in the quickstart. The nine examples cover typed/dynamic questions, thresholds,
sampling, batches, custom endpoints and case files.
