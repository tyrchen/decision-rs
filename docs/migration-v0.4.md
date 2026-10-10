# Migrating to decision-rs 0.4

The SDK is now `decision-rs`, reflecting its support for OpenAI Decisions,
OpenRouter Jev and TypeSafe System One. Both workspace packages are versioned at
0.4.0. Rust 1.99 and edition 2024 remain required.

Replace the old dependency with:

```toml
[dependencies]
decision-rs = "0.4.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Cargo exposes the library as `decision_rs`. The default `derive` feature re-exports
the macros from `decision-macros`; applications only need the SDK dependency.

| Previous name | New name |
|---|---|
| Package `jev-typed` | `decision-rs` |
| Rust import `jev` | `decision_rs` |
| Macro package `jev-macros` | `decision-macros` |
| Derive helper `#[jev(...)]` | `#[decision(...)]` |
| `JEV_PROVIDER` | `DECISION_PROVIDER` |
| `JEV_API_KEY` | `DECISION_API_KEY` |
| `JEV_MODEL` | `DECISION_MODEL` |
| `crates/jev` | `crates/decision-rs` |
| `crates/jev-macros` | `crates/decision-macros` |

Update imports, qualified paths, derive helpers and deployment environment
settings together. The old SDK names and environment variables have no
compatibility aliases.

```rust,no_run
use decision_rs::{Client, Decision, Predicate};

#[derive(Debug, Decision)]
struct Review {
    /// Does the customer request a refund?
    #[decision(key = "refund", yes = "Requests money back.", no = "No refund requested.")]
    refund: Predicate,
}

async fn review() -> Result<Review, decision_rs::Error> {
    Client::from_env()?.decide("Please refund me.").await
}
```

For example, set `DECISION_PROVIDER=openai` with `OPENAI_API_KEY` or
`DECISION_API_KEY`. `OPENAI_API_KEY`, `OPENROUTER_API_KEY` and `TYPESAFE_API_KEY`
keep their names and selection rules. `DECISION_MODEL` overrides the selected
provider's default.

Public decision types, client methods, feature flags, YAML configuration and
version 2 case files retain their behavior. Jev is still a backend protocol and
model family: keep `Model::Jev113`, `Model::JevLatest`, `Endpoint::jev_decisions`
and wire model IDs such as `typesafe/jev-1.13` unchanged. HTTP requests identify
the SDK as `decision-rs/0.4.0`.

Projects migrating from 0.2 also need the API and behavior changes described in
the [0.3 migration guide](migration-v0.3.md), followed by the naming changes above.
See the [quickstart](quickstart.md) for current examples and configuration.
