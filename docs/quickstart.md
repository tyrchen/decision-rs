# Quickstart

Add `decision-rs = "0.4.0"` and Tokio with `macros` and
`rt-multi-thread`. Rust 1.99 and edition 2024 are required. Default features include
derives, bounded retry and rustls with aws-lc-rs.

```rust,no_run
use decision_rs::{Client, Decision, Predicate, RequestOptions};

#[derive(Debug, Decision)]
struct Review {
    /// Does the customer request a refund?
    refund: Predicate,
}

async fn review(key: String) -> Result<(), decision_rs::Error> {
    let client = Client::openai(key)?;
    let answer = client.decide_with::<Review>(RequestOptions::new("Please refund me.")?).await?;
    let probability = answer.value.refund.probability().get();
    let tokens = answer.meta.usage.input_tokens;
    Ok(())
}
```

Replace only the constructor with `Client::openrouter` or `Client::typesafe` to use
Jev. `Client::new(provider, key)` supports runtime selection for those providers;
Cloudflare additionally needs an account (see below). Known incompatible
model/provider combinations fail before sending; custom IDs are sent unchanged
except on Cloudflare, which accepts only the three known Clef models.
`Model::Default` resolves to Luna for OpenAI, Jev113 for either Jev provider, and
Clef for Cloudflare.

For environment construction, set `DECISION_PROVIDER` and the matching provider key or
`DECISION_API_KEY`. Without an explicit provider, exactly one provider-specific key can
be inferred. Generic keys require a provider; invalid present values are errors.

```sh
export DECISION_PROVIDER=openai
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
# fn setup() -> Result<(), decision_rs::Error> {
use decision_rs::{Auth, Client, Endpoint, HttpClient, Provider};
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
cargo test --test live --all-features test_should_accept_hosted_protocol::case_1 -- --ignored --exact # OpenAI
cargo test --test live --all-features test_should_accept_hosted_protocol::case_2 -- --ignored --exact # OpenRouter
cargo test --test live --all-features test_should_accept_hosted_protocol::case_3 -- --ignored --exact # TypeSafe
make test-live-cloudflare # All three Clef models; token and account required
```

See [primitives](primitives.md), [thresholds](thresholds.md), [cases](cases.md),
and [migration](migration-v0.4.md).

## Cloudflare-hosted Clef

Use a Workers AI API token and your 32-character hexadecimal account ID. The
account is separate from the secret token. Load both through your application or
secret manager; constructors never read the environment implicitly.

```rust,no_run
# fn setup(account_id: &str, token: String) -> Result<(), decision_rs::Error> {
use decision_rs::{Client, CloudflareAccountId, Model, Provider};
let account = CloudflareAccountId::try_from(account_id)?;
let client = Client::builder(Provider::Cloudflare)
    .cloudflare_account(account)
    .api_key(token)
    .model(Model::ClefFlash)
    .build()?;
# Ok(())
# }
```

`Client::cloudflare(account, token)?` selects Clef. `Client::new(Cloudflare, token)`
fails because it cannot supply an account. Available variants are `Clef`,
`ClefFlash`, and `ClefOmni`; their short names are `clef`, `clef-flash`, and
`clef-omni`. Parsing also accepts the corresponding full `@cf/cloudflare/...`
identifiers. The SDK sends the full ID in the route and the short name in the
body. `RequestOptions::model` overrides both for one call without changing clones.
Other Workers AI models and custom hosted IDs are rejected locally.

For `Client::from_env()`, select `DECISION_PROVIDER=cloudflare`, load
`CLOUDFLARE_API_TOKEN` (or `DECISION_API_KEY`) and `CLOUDFLARE_ACCOUNT_ID`, and optionally
set `DECISION_MODEL=clef-flash`. Without a provider, a sole Cloudflare token selects
Cloudflare but still requires the account; an account alone never selects a provider.
The SDK does not read the `CLOUDFLARE_AUTH_TOKEN` name used in some upstream examples.

```yaml
provider: cloudflare
cloudflareAccountId: "0123456789abcdef0123456789abcdef" # Replace with your account
model: clef-flash
timeoutMs: 10000
```

Load with `Config::from_yaml_str` and `Client::from_config(config, token)`, or use
`Config::cloudflare(account)` programmatically. Account IDs on other providers,
null/invalid account IDs, and secrets in YAML are rejected.

All three models accept text and canonical JSON through this SDK. Media input is
not exposed. Workers AI may truncate long state; byte limits are not token limits
and do not prove that every piece of evidence was processed. Model aliases may
change weights. `wire_model` records the body selector, `routing_model` the full
path model ID, and `returned_model` the server's actual model field. Cost remains
unavailable unless reported by the service. Native confidence is preserved and
must not be substituted for event probability.

For user-hosted Clef, use an actual joint-head-aware System One server:

```rust,no_run
# fn setup() -> Result<(), decision_rs::Error> {
use decision_rs::{Auth, Client, Endpoint};
let endpoint = Endpoint::clef_system_one("http://127.0.0.1:30000/v1/systemone")?;
let client = Client::for_endpoint(endpoint, Auth::None, "Cloudflare/clef".parse()?)?;
# Ok(())
# }
```

This endpoint expects the bare System One response and applies Clef rounding
bounds; it does not require a Cloudflare account. The checkpoint ID stays a custom
model. The short `clef`, `clef-flash`, and `clef-omni` names are now reserved hosted
aliases; custom deployments should use their checkpoint ID or another server alias. A normal chat-completions server is insufficient. The
[SGLang Clef cookbook](https://docs.sglang.io/cookbook/autoregressive/Cloudflare/clef)
covers Clef/Flash; verify serving version, image digest, effective context and
confidence semantics separately. Omni deployment is not established by that guide.
See the [research record](research/cloudflare-clef.md) for sources and evidence limits.
