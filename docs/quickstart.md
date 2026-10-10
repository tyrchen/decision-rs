# Quickstart

Add `jev = { package = "jev-typed", version = "0.3" }` and Tokio with `macros` and
`rt-multi-thread`. Rust 1.99 and edition 2024 are required. Default features include
derives, bounded retry and rustls with aws-lc-rs.

```rust,no_run
use jev::{Client, Decision, Predicate, RequestOptions};

#[derive(Debug, Decision)]
struct Review {
    /// Does the customer request a refund?
    refund: Predicate,
}

async fn review(key: String) -> Result<(), jev::Error> {
    let client = Client::openai(key)?;
    let answer = client.decide_with::<Review>(RequestOptions::new("Please refund me.")?).await?;
    let probability = answer.value.refund.probability().get();
    let tokens = answer.meta.usage.input_tokens;
    Ok(())
}
```

Replace only the constructor with `Client::openrouter` or `Client::typesafe` to use
Jev. `Client::new(provider, key)` supports runtime selection. Known incompatible
model/provider combinations fail before sending; custom IDs are sent unchanged.
`Model::Default` resolves to Luna for OpenAI and Jev113 for either Jev provider.

For environment construction, set `JEV_PROVIDER` and the matching provider key or
`JEV_API_KEY`. Without an explicit provider, exactly one provider-specific key can
be inferred. Generic keys require a provider; invalid present values are errors.

```sh
export JEV_PROVIDER=openai
# Load OPENAI_API_KEY through your environment or secret manager.
cargo run --example 03_multi_question_decision
cargo run --example 07_batch_concurrent --features batch
cargo run --example 09_cases_from_file --features batch,yaml -- cases.yaml
```

Secret-free YAML configuration requires the `yaml` feature:

```yaml
provider: openai
model: default
timeoutMs: 10000
maxInflight: 64
retry:
  maxAttempts: 3
  initialBackoffMs: 200
  maxBackoffMs: 2000
thresholds:
  refund: 0.9
```

Use `Config::from_yaml_str(text)?` followed by `Client::from_config(config, key)?`.
Unknown/duplicate keys, aliases, invalid thresholds and out-of-range settings are
rejected. `Settings` also works with `Client::builder(provider)`.

```rust,no_run
# fn setup() -> Result<(), jev::Error> {
use jev::{Auth, Client, Endpoint, HttpClient, Provider};
let http = HttpClient::new()?;
let hosted = Client::builder(Provider::OpenAi).api_key("application-loaded-key").http(http.clone()).build()?;
let endpoint = Endpoint::system_one("http://127.0.0.1:8080/v1/systemone")?;
let local = Client::for_endpoint(endpoint, Auth::None, "local-model".parse()?)?;
# Ok(())
# }
```

Custom URLs are complete endpoints: no path rewriting, credentials, query or
fragment. HTTP requires a literal loopback IP; other deployments use HTTPS.
`HttpClient` disables redirects and implicit retries, and clones share the pool.
Client clones additionally share concurrency permits.

Opt-in live smoke tests require the selected credential and may incur charges:

```sh
cargo test --test live --all-features -- --ignored case_1 # OpenAI
cargo test --test live --all-features -- --ignored case_2 # OpenRouter
cargo test --test live --all-features -- --ignored case_3 # TypeSafe
```

See [primitives](primitives.md), [thresholds](thresholds.md), [cases](cases.md),
and [migration](migration-v0.3.md).
