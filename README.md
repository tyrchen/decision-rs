![](https://github.com/tyrchen/jev-typed/workflows/build/badge.svg)

# jev-typed

A typed, third-party Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev)
decision model. Jev is a *System One* model: instead of prose it answers typed
questions about a state you send it, with calibrated probabilities. This SDK's
thesis is that Rust can express a decision *as a type* — one struct declares the
questions, the option sets, and the answer shape, and the compiler checks all
three.

```rust,no_run
use jev::{Choice, ChoiceOptions, Client, Decision, Noul, Score, ScoreLevels};

/// A support ticket triage decision.
#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a software defect?
    #[jev(yes = "The customer describes broken or unexpected product behavior.",
          no  = "The customer is asking a question or requesting a feature.")]
    is_bug: Noul,
    /// Which team should own this ticket?
    team: Choice<Team>,
    /// How urgent is this ticket?
    urgency: Score<Urgency>,
}

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
    /// Login, permissions, or profile issues.
    Account,
}

/// Ordered urgency scale, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Should be fixed this week.
    ThisWeek,
    /// Blocking revenue right now.
    BlockingRevenue,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    let client = Client::from_env()?;                          // reads JEV_API_KEY
    let triage: Triage = client.decide("a customer ticket...").await?;

    if triage.is_bug.is_yes(0.9) {
        if let Some(team) = triage.team.accept(0.7) {
            println!("route to {team:?}");
        }
    }
    println!("urgency: {:?}", triage.urgency.nearest());
    Ok(())
}
```

The doc comments *are* the prompt: the `///` that documents the Rust item is the
exact text the model reads. There is no second, drifting copy of the
instructions, and `cargo doc` shows a reviewer precisely what the model is asked.

## Crates

| Crate (package name on crates.io)          | Description                                                                                                                                           |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`jev-typed`](crates/jev) (library: `jev`) | The SDK: wire types, dynamic question builder, typed `Decision`/`ChoiceOptions`/`ScoreLevels` traits, async `Client` with retry, `Thresholds` config. |
| [`jev-macros`](crates/jev-macros)          | `#[derive(Decision, ChoiceOptions, ScoreLevels)]`, re-exported from `jev` behind the `derive` feature (default on).                                   |

The package is named `jev-typed` because `jev` was already taken on crates.io by
an unrelated crate; the library itself keeps the name `jev` via `[lib] name`,
so `use jev::...` is exactly what application code writes regardless of the
package name in `Cargo.toml`.

## Documentation

- [docs/quickstart.md](docs/quickstart.md) — install, get an API key, run the triage example.
- [docs/primitives.md](docs/primitives.md) — `Noul`, `Choice<T>`, `Score<T>` in depth.
- [docs/thresholds.md](docs/thresholds.md) — calibration vs. accuracy, escalation, `Thresholds` config.
- [docs/migration-from-prompt-and-parse.md](docs/migration-from-prompt-and-parse.md) — replacing a chat-model classification prompt.
- [specs/jev-sdk-design.md](specs/jev-sdk-design.md) — the full design doc this SDK implements.
- `crates/jev/examples/` — eight runnable programs, easiest to most complex: a
  single question, a single typed choice/score, the full multi-question derive
  path, dynamic (config-driven) questions, thresholds and escalation, weighted
  sampling, bounded-concurrency batching, and a self-hosted backend. See
  [docs/quickstart.md](docs/quickstart.md#run-the-bundled-examples) for the full list.

## MSRV, feature flags, and the pinned default model

- **MSRV**: Rust 1.90 (`rust-version` in `Cargo.toml`); the pinned toolchain in
  `rust-toolchain.toml` tracks current stable.
- **Default model**: `Model::Jev113` (`typesafe/jev-1.13` / `jev-1.13`), pinned —
  *not* `Model::Latest`. Thresholds are tuned against one model's calibration, so
  silently following `jev-latest` would silently re-tune every gate in
  production. Select `Model::Latest` explicitly for exploratory work.
- **Feature flags** (`jev` crate):

  | Feature      | Default | Effect                                                |
  | ------------ | ------- | ----------------------------------------------------- |
  | `derive`     | yes     | `#[derive(Decision, ChoiceOptions, ScoreLevels)]`.    |
  | `retry`      | yes     | Bounded retry loop honouring `Retry-After`.           |
  | `rustls-tls` | yes     | `reqwest` over `rustls` with the `aws-lc-rs` backend. |
  | `native-tls` | no      | `reqwest` over the platform TLS stack.                |
  | `batch`      | no      | `decide_all`, a bounded-concurrency helper.           |

## Development

```bash
make build          # cargo build --workspace --all-features
make test            # unit + wiremock + trybuild + doctests (offline)
make test-live       # ignored live tests; needs JEV_API_KEY
make lint            # cargo +nightly fmt --check && cargo clippy -D warnings
make feature-check   # cargo hack --feature-powerset check -p jev
make audit           # cargo audit && cargo deny check
```

## Agent support

Generated projects include agent-facing guidance for both Codex and Claude:

- `AGENTS.md` for Codex project instructions.
- `.agents/skills/{spec,research,impl}` for Codex skills.
- `CLAUDE.md` and `.claude/skills/{spec,research,impl}` for Claude Code compatibility.

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2026 Tyr Chen
