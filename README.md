![](https://github.com/tyrchen/jev-typed/workflows/build/badge.svg)

# jev-typed

A typed, third-party Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev)
decision model, available through OpenRouter and TypeSafe AI. Jev is a *System One*
model: it answers typed questions about a state with calibrated probabilities.
One Rust struct declares the questions, option sets, and answer shape, and the
compiler checks all three.

## Install

```toml
[dependencies]
jev-typed = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The package on crates.io is `jev-typed`; import it as `jev` in Rust.
The default features include derive macros, retry, and `rustls` TLS.
See the [0.2.0 release notes](CHANGELOG.md) for changes from 0.1.

## Choose a provider

Create a client with an API key loaded by your application:

```rust
use jev::{Client, Provider};

let openrouter = Client::openrouter(openrouter_key)?;
let typesafe = Client::typesafe(typesafe_key)?;
let selected = Client::for_provider(Provider::TypeSafe, key)?;
```

Keys can be `&str`, `String`, or `secrecy::SecretString`. `Client::new(key)`
defaults to OpenRouter. Use the key issued by the provider you select.

| Provider | Default endpoint | `Model::Jev113` | `Model::Latest` |
|---|---|---|---|
| OpenRouter | `https://openrouter.ai/api/alpha/decisions` | `typesafe/jev-1.13` | `~typesafe/jev-latest` |
| TypeSafe AI | `https://api.typesafe.ai/v1/systemone` | `jev-1.13.0` | `jev-latest` |

The client selects the endpoint and model ID automatically. TypeSafe always uses
System One; OpenRouter can also use `Surface::SystemOne`. Custom model IDs are
passed through unchanged.

Alternatively, configure the environment and call `Client::from_env()`:

```bash
# OpenRouter
export JEV_PROVIDER=openrouter
export OPENROUTER_API_KEY=your-openrouter-key

# Or TypeSafe AI
export JEV_PROVIDER=typesafe
export TYPESAFE_API_KEY=your-typesafe-key
```

With `JEV_PROVIDER` set, `JEV_API_KEY` takes precedence over that provider's key
variable. The other provider's key is never used as a fallback. Without
`JEV_PROVIDER`, the client tries `JEV_API_KEY` (OpenRouter), then
`OPENROUTER_API_KEY`, then `TYPESAFE_API_KEY` (TypeSafe). Set `JEV_PROVIDER`
explicitly when both platforms are configured.

For YAML configuration, use `provider: openrouter` or `provider: typesafe` and pass
the key separately to `Client::from_config(http, key, config)`. For a shared HTTP
pool, use `Client::with_http(http, key).with_provider(provider)`. See the
[quickstart](docs/quickstart.md#get-an-api-key) for configuration details.

## Make a typed decision

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
    let client = Client::from_env()?;
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
- [docs/cases.md](docs/cases.md) — load predefined cases from YAML/JSON and run them individually or concurrently.
- [docs/migration-from-prompt-and-parse.md](docs/migration-from-prompt-and-parse.md) — replacing a chat-model classification prompt.
- [specs/jev-sdk-design.md](specs/jev-sdk-design.md) — the full design doc this SDK implements.
- `crates/jev/examples/` — nine runnable programs, easiest to most complex: a
  single question, a single typed choice/score, the full multi-question derive
  path, dynamic (config-driven) questions, thresholds and escalation, weighted
  sampling, bounded-concurrency batching, a self-hosted backend, and cases loaded
  from a file. See
  [docs/quickstart.md](docs/quickstart.md#run-the-bundled-examples) for the full list.

## MSRV, feature flags, and the pinned default model

- **MSRV**: Rust 1.90 (`rust-version` in `Cargo.toml`); the pinned toolchain in
  `rust-toolchain.toml` tracks current stable.
- **Default model**: `Model::Jev113` (`typesafe/jev-1.13` on OpenRouter's default
  surface, `jev-1.13.0` on TypeSafe AI), pinned —
  *not* `Model::Latest`. Thresholds are tuned against one model's calibration, so
  silently following `jev-latest` would silently re-tune every gate in
  production. Select `Model::Latest` explicitly for exploratory work.
- **Feature flags** (`jev` crate):

  | Feature | Default | Effect |
  |---|---|---|
  | `derive` | yes | `#[derive(Decision, ChoiceOptions, ScoreLevels)]`. |
  | `retry` | yes | Bounded retry loop honouring `Retry-After`. |
  | `rustls-tls` | yes | `reqwest` over `rustls` with the `aws-lc-rs` backend. |
  | `native-tls` | no | `reqwest` over the platform TLS stack. |
  | `batch` | no | `decide_all` / `run_cases`, bounded-concurrency helpers. |
  | `yaml` | no | Load YAML cases with `CaseSet::from_yaml_str` / `from_file`. |

## Development

```bash
make build           # cargo build --workspace --all-features
make test            # unit + wiremock + trybuild + doctests (offline)
make test-live       # ignored live tests; requires a provider API key and makes billed requests
make lint            # cargo +nightly fmt --check && cargo clippy -D warnings
make feature-check   # cargo hack --feature-powerset check -p jev-typed
make audit           # cargo audit && cargo deny check
make publish-check   # verify both crate archives without uploading
make publish         # publish both crates to crates.io
```

## Agent support

This repository includes matching instructions for Codex and Claude:

- `AGENTS.md` for Codex project instructions.
- `CLAUDE.md` for Claude Code project instructions.

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2026 Tyr Chen
