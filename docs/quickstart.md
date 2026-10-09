# Quickstart

## Install

```toml
[dependencies]
jev-typed = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The package is `jev-typed` on crates.io (the library itself is still named `jev`,
so every `use jev::...` in your code stays the same — only the `[dependencies]`
line names the package).

The default features (`derive`, `retry`, `rustls-tls`) cover the common case: derive
macros, bounded retry, and a pure-Rust TLS stack. See the [feature flags table](../crates/jev/src/lib.rs)
in the crate-level docs if you need a different combination (for example
`native-tls` for a corporate proxy's trust store, or `default-features = false` to
supply your own `reqwest::Client`).

## Get an API key

Jev is served through [OpenRouter](https://openrouter.ai) and the
[TypeSafe AI direct API](https://docs.typesafe.ai/introduction/quickstart).
Use the API key issued by the provider you select.

### Create a client with a key

Pass an API key loaded by your application directly into the constructor:

```rust,no_run
use jev::{Client, Provider};

let openrouter = Client::openrouter(openrouter_key)?;
let typesafe = Client::typesafe(typesafe_key)?;
let selected = Client::for_provider(Provider::TypeSafe, key)?;
```

`Client::new(key)` remains shorthand for `Client::openrouter(key)`. Constructors
accept `&str`, `String`, or `secrecy::SecretString`. Keys are redacted in `Debug`
output; empty keys, whitespace, non-ASCII bytes, and keys over 4096 bytes are
rejected without including the key in the error.

| Provider | Default endpoint | Pinned model | `Model::Latest` |
|---|---|---|---|
| OpenRouter | `https://openrouter.ai/api/alpha/decisions` | `typesafe/jev-1.13` | `~typesafe/jev-latest` |
| TypeSafe AI | `https://api.typesafe.ai/v1/systemone` | `jev-1.13.0` | `jev-latest` |

The endpoint and model conventions follow the
[OpenRouter guide](https://openrouter.ai/blog/insights/what-is-jev/) and
[TypeSafe model reference](https://docs.typesafe.ai/models).
TypeSafe always uses System One and omits `session_id`, `user`, and `trace`.
Custom model IDs are passed through unchanged. A custom base URL overrides the
host while retaining the selected provider's endpoint path.

For a shared HTTP pool, use `Client::with_http(http, key).with_provider(provider)`.
That constructor validates the key when a request is sent. For YAML configuration,
set `provider: typesafe` (default: `openrouter`) and pass the key separately to
`Client::from_config(http, key, config)`; keys are never stored in `Config`.

### Create a client from environment variables

For OpenRouter, set one of:

```bash
export JEV_API_KEY=sk-...
# or, if you already have an OpenRouter key configured for other models:
export OPENROUTER_API_KEY=sk-...
```

For TypeSafe AI:

```bash
export JEV_PROVIDER=typesafe
export TYPESAFE_API_KEY=your-key
```

`Client::from_env()` supports `JEV_PROVIDER=openrouter|typesafe`. With an explicit
provider it reads `JEV_API_KEY` first, then only that provider's key variable;
it never falls back to the other provider's key. Without `JEV_PROVIDER`, it tries
`JEV_API_KEY` (OpenRouter), `OPENROUTER_API_KEY`, then `TYPESAFE_API_KEY` (TypeSafe).
This preserves existing OpenRouter configurations and selects TypeSafe when only
`TYPESAFE_API_KEY` is set. Set `JEV_PROVIDER` when both providers are configured.
An optional `JEV_BASE_URL` overrides the API base (mainly useful for local mocks).

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
`Choice<T>` and `Score<T>`, and `examples/03_multi_question_decision.rs` for all
three primitives answered in a single request.

## Run the bundled examples

Nine runnable programs under `crates/jev/examples/`, easiest to most complex:

```bash
# 1. The smallest useful call: one yes/no question, no derive, no struct.
JEV_API_KEY=sk-... cargo run --example 01_single_question

# 2. A single typed `choice` and a single typed `score` question, still without a
#    Decision struct — one derived enum per question is enough.
JEV_API_KEY=sk-... cargo run --example 02_single_typed_choice_and_score

# 3. The flagship derive path: three questions, three primitives, one API call.
JEV_API_KEY=sk-... cargo run --example 03_multi_question_decision

# 4. Questions built from configuration at runtime (the dynamic path).
JEV_API_KEY=sk-... cargo run --example 04_dynamic_questions

# 5. Thresholds, Verdict, and escalating an ambiguous answer instead of guessing.
JEV_API_KEY=sk-... cargo run --example 05_thresholds_and_escalation

# 6. Reproducible weighted sampling over a choice distribution.
JEV_API_KEY=sk-... cargo run --example 06_weighted_sampling

# 7. `decide_all` over many states with bounded concurrency.
JEV_API_KEY=sk-... cargo run --example 07_batch_concurrent --features batch

# 8. A self-hosted backend: no API key, a custom model, the systemOne surface.
SELF_HOSTED_URL=http://127.0.0.1:8099 cargo run --example 08_self_hosted_backend

# 9. Predefined cases loaded from a YAML fixture, run with bounded concurrency.
JEV_API_KEY=sk-... cargo run --example 09_cases_from_file --features "batch yaml"
```

All need the `derive` feature except #1, #4, and #9 (on by default); #7 and #9 also
need `batch`; #9 also needs `yaml`. See each file's header comment for its exact
invocation.

## Next steps

- [primitives.md](./primitives.md) — `Noul`, `Choice<T>`, `Score<T>` in depth.
- [thresholds.md](./thresholds.md) — how to pick and configure the numbers you pass to
  `is_yes`, `confident`, and friends.
- [cases.md](./cases.md) — loading predefined `(state, questions)` cases from YAML/JSON.
- [migration-from-prompt-and-parse.md](./migration-from-prompt-and-parse.md) — replacing
  a chat-model classification prompt.
