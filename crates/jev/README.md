# jev-typed

Typed Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev)
decision model — a *System One* model that answers typed `noul`/`choice`/`score`
questions with calibrated probabilities instead of prose. Supports OpenRouter
and TypeSafe AI's direct API.

Published as the `jev-typed` package (the library itself is still named `jev`,
so every `use jev::...` below is exactly what you write).

See the [workspace README](../../README.md) and [crate-level rustdoc](https://docs.rs/jev-typed)
for the full picture.

## Install

```toml
[dependencies]
jev-typed = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Configure a provider

Pass an API key loaded by your application directly:

```rust
use jev::{Client, Provider};

let openrouter = Client::openrouter(openrouter_key)?;
let typesafe = Client::typesafe(typesafe_key)?;
let selected = Client::for_provider(Provider::TypeSafe, key)?;
```

Constructors accept `&str`, `String`, or `secrecy::SecretString`.
`Client::new(key)` defaults to OpenRouter. Keys are redacted in `Debug` output.

For `Client::from_env()`, configure `JEV_PROVIDER=openrouter` with
`OPENROUTER_API_KEY`, or `JEV_PROVIDER=typesafe` with `TYPESAFE_API_KEY`.
`JEV_API_KEY` overrides the selected provider's key variable. Without an explicit
provider, precedence is `JEV_API_KEY` (OpenRouter), `OPENROUTER_API_KEY`, then
`TYPESAFE_API_KEY` (TypeSafe). Set `JEV_PROVIDER` when both platforms are configured.

The default model is pinned: `typesafe/jev-1.13` on OpenRouter's Decisions API,
`jev-1.13.0` on TypeSafe AI. Choose `Model::Latest` explicitly to follow new
releases. See the [quickstart](../../docs/quickstart.md#get-an-api-key) for endpoint
details, shared HTTP clients, and YAML configuration.

## Make a decision

```rust,no_run
use jev::{Client, Decision, Noul};

/// Is this ticket about a refund?
#[derive(Debug, Decision)]
struct RefundCheck {
    /// Is the customer asking for money back?
    is_refund: Noul,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    let client = Client::from_env()?;
    let result: RefundCheck = client.decide("Please refund my order.").await?;
    println!("P(refund) = {:.2}", result.is_refund.probability());
    Ok(())
}
```

## Feature flags

| Feature      | Default | Effect                                                                                             |
| ------------ | ------- | -------------------------------------------------------------------------------------------------- |
| `derive`     | yes     | `#[derive(Decision, ChoiceOptions, ScoreLevels)]`, re-exported from [`jev-macros`](../jev-macros). |
| `retry`      | yes     | Bounded retry loop honouring `Retry-After`.                                                        |
| `rustls-tls` | yes     | `reqwest` over `rustls` with the `aws-lc-rs` backend.                                              |
| `native-tls` | no      | `reqwest` over the platform TLS stack.                                                             |
| `batch`      | no      | `decide_all` / `run_cases`, bounded-concurrency helpers.                                            |
| `yaml`       | no      | `CaseSet::from_yaml_str` / `CaseSet::from_file` for YAML case files.                                |

## License

MIT — see [LICENSE](../../LICENSE.md).
