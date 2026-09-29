# Quickstart

## Install

```toml
[dependencies]
jev = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The default features (`derive`, `retry`, `rustls-tls`) cover the common case: derive
macros, bounded retry, and a pure-Rust TLS stack. See the [feature flags table](../crates/jev/src/lib.rs)
in the crate-level docs if you need a different combination (for example
`native-tls` for a corporate proxy's trust store, or `default-features = false` to
supply your own `reqwest::Client`).

## Get an API key

Jev is served through [OpenRouter](https://openrouter.ai). Set one of:

```bash
export JEV_API_KEY=sk-...
# or, if you already have an OpenRouter key configured for other models:
export OPENROUTER_API_KEY=sk-...
```

`Client::from_env()` reads `JEV_API_KEY` first, then falls back to
`OPENROUTER_API_KEY`. An optional `JEV_BASE_URL` overrides the API base (mainly
useful for pointing at a mock server in tests).

## Your first decision

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
    let result: RefundCheck = client.decide("Please cancel my order and refund the charge.").await?;

    if result.is_refund.is_yes(0.8) {
        println!("route to billing");
    }
    Ok(())
}
```

One field, one question, one HTTP round trip. The doc comment on `is_refund` *is*
the `instructions` sent to the model — see [primitives.md](./primitives.md) for
`Choice<T>` and `Score<T>`, and the runnable `examples/triage.rs` for all three
primitives together.

## Run the bundled examples

```bash
JEV_API_KEY=sk-... cargo run --example triage
JEV_API_KEY=sk-... cargo run --example dynamic
JEV_API_KEY=sk-... cargo run --example batch --features batch
```

## Next steps

- [primitives.md](./primitives.md) — `Noul`, `Choice<T>`, `Score<T>` in depth.
- [thresholds.md](./thresholds.md) — how to pick and configure the numbers you pass to
  `is_yes`, `confident`, and friends.
- [migration-from-prompt-and-parse.md](./migration-from-prompt-and-parse.md) — replacing
  a chat-model classification prompt.
