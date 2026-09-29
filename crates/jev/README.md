# jev-typed

Typed Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev)
decision model — a *System One* model that answers typed `noul`/`choice`/`score`
questions with calibrated probabilities instead of prose.

Published as the `jev-typed` package (the library itself is still named `jev`,
so every `use jev::...` below is exactly what you write).

See the [workspace README](../../README.md) and [crate-level rustdoc](https://docs.rs/jev-typed)
for the full picture; the short version:

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
| `batch`      | no      | `decide_all`, a bounded-concurrency helper.                                                        |

## License

MIT — see [LICENSE](../../LICENSE.md).
