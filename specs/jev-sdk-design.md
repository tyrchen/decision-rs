# Jev Rust SDK — Typed Decision Client

> Historical v0.1 design. The public contract is replaced by [the unified backend design](unified-decision-backends-design.md). Current names follow the [0.4 migration guide](../docs/migration-v0.4.md).


**Status**: Implemented (v0.1.0)
**Date**: 2026-05-13
**Category**: SDK / Library
**Type**: design

## Executive Summary

Build `jev`, a standalone, third-party Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev) decision model, plus a companion proc-macro crate `jev-macros`. The SDK's thesis is that Rust can express a decision *as a type*: one struct declares the questions, the option sets, and the answer shape, and the compiler checks all three.

Jev is a *System One* model. Instead of prose it answers typed questions about a state you send it, and returns probabilities. There are three primitives:

| Primitive | Question | Answer shape |
|-----------|----------|--------------|
| `noul` | Does this condition hold? | `{ type, noul: f64 }` — P(yes) |
| `choice` | Which one of these options? | `{ type, choice: String, confidence?, probabilities? }` |
| `score` | Where on an ordered scale? | `{ type, score: f64, confidence?, probabilities?, legend? }` |

The wire protocol is an untyped `map<String, Question>` in and `map<String, Answer>` out. Existing clients in other languages surface that map directly, so every question key, every option string, and every answer field access is a runtime gamble. This SDK collapses the map into a struct:

```rust
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

let client = Client::from_env()?;                      // model defaults to a pinned Jev
let triage: Triage = client.decide(&ticket).await?;    // one HTTP round trip

if triage.is_bug.is_yes(0.9) && triage.team.confident(0.7) {
    route_to(triage.team.value());                     // Team, not String
}
let urgency: Urgency = triage.urgency.nearest();       // Urgency, not f64
```

The doc comments *are* the prompt: the `///` that documents the Rust item is the exact text the model reads. There is no second, drifting copy of the instructions, and `cargo doc` shows a reviewer precisely what the model is asked.

## Problem

### 1. There is no Rust client

Jev is reachable over plain HTTP and has official clients for TypeScript, Python, and Go. Rust callers hand-roll `reqwest` plus `serde_json::Value` walking, and each one re-invents the same five things: retry classification, `Retry-After` handling, error-envelope parsing, API-key redaction, and answer extraction. None of that is domain logic, and all of it is easy to get subtly wrong (retrying a non-idempotent, billed request on `400`, for instance).

### 2. Prompt-and-parse with a chat model is the wrong tool

The pattern a Rust service reaches for today is: build a prompt that says "answer with exactly one of BUG, QUESTION, FEATURE", call a chat model, lowercase the reply, and string-match. That has four standing problems:

- **Parse failures are silent or late.** The model answers "This looks like a bug." and the match falls through to a default.
- **No calibrated uncertainty.** A chat completion carries no usable probability, so there is no principled way to escalate ambiguous cases to a human.
- **Cost and latency.** A reasoning-capable chat model is expensive for a yes/no question; Jev bills input tokens only.
- **Prompt drift.** The option list lives in a format string, the enum lives in Rust, and nothing keeps them in sync.

### 3. Stringly-typed clients push failures to runtime, and that is the interesting gap

Every failure below is a runtime bug in an untyped client and a compile error — or an explicit, named error — here:

| Failure mode with an untyped client | Prevented by |
|---|---|
| Asked for question `refund`, read `answers.refunds` | Answers are struct fields |
| Read `answers.team.noul` on a `choice` question | `Choice<T>` has no `noul()` |
| `choice` returned an option missing from your `match` | `Choice<T>` parses into your enum; unknown options are a named error |
| Score index `2` means "urgent"… or was it `0`? | `Score<T>` maps index → variant, ordered by declaration |
| Criteria list in code drifts from the enum | Criteria are generated *from* the enum |
| Forgot `criteria` on a `choice` question (HTTP 400) | `Choice<T>` cannot be built without `ChoiceOptions` |
| Instruction-less question (HTTP 400) | Missing doc comment is a compile error |

### 4. Two surfaces, one model

Jev is reachable two ways, with identical semantics and near-identical payloads:

| Surface | Endpoint | Notes |
|---|---|---|
| Decisions | `POST https://openrouter.ai/api/alpha/decisions` | Alpha. Extra fields: `provider`, `session_id`, `trace`, `user`. Namespaced model IDs (`typesafe/jev-1.13`). |
| System One | `POST https://openrouter.ai/api/v1/systemone` | Stable `v1` path. Accepts bare IDs (`jev-1.13`) and namespaces them itself. |

Callers should not have to care. The SDK targets one surface by default, lets the other be selected with a single builder call, and normalizes model IDs per surface.

## Goals / Non-goals

**Goals**

1. Typed, derive-driven decisions: one Rust type == one request == one answer struct.
2. A dynamic escape hatch for question sets built at runtime from configuration.
3. Both surfaces behind one client type.
4. Production hygiene by default: timeouts, bounded retry honouring `Retry-After`, redacted secrets, no state content in logs, no `unwrap`/`expect`/panic on server input.
5. Thresholds as first-class configuration data, never hard-coded literals in the SDK.
6. Zero-friction embedding: cheap `Clone`, no runtime assumptions beyond Tokio, no framework coupling, minimal dependency tree.
7. Publishable on crates.io with complete rustdoc, runnable examples, and a documented MSRV.

**Non-goals (v1)**

- Streaming — the API is request/response only.
- A batch endpoint — none exists upstream; a bounded-concurrency helper covers the need.
- Response caching or memoization.
- Chat-model fallback, cascades, or agent orchestration — that is application policy built on top.
- A CLI binary (see Rollout).
- Blocking (non-async) client.

## Design Principles

1. **The doc comment is the prompt.** `///` on a field becomes `instructions`; `///` on an enum variant becomes that option's criteria. One source of truth.
2. **Make illegal states unrepresentable.** `Noul`, `Choice<T>`, and `Score<T>` are distinct types with disjoint accessors. A `Choice<T>` cannot exist without a generated option list.
3. **Typed first, dynamic second.** The derive path is the headline API; `Questions`/`Answers` maps stay public for config-driven callers, and the derive path is implemented *on top of* them so there is exactly one copy of the wire logic.
4. **Probabilities are data, thresholds are configuration.** The SDK never picks a threshold. It provides `is_yes(t)`, `confident(t)`, and a `Thresholds` config type loadable from YAML so gates are tunable without a redeploy.
5. **State may contain PII.** `state` and criteria text are never logged at any level. Spans carry model, question keys, counts, latency, and usage only.
6. **Forward compatible.** Unknown answer `type` values deserialize into `Answer::Other { r#type, raw }` instead of failing the whole response; unknown JSON fields are ignored.
7. **Leaf crate.** Dependencies are `reqwest` (no default features; TLS backend selected by feature), `serde`, `serde_json`, `indexmap`, `secrecy`, `thiserror`, `tracing`, `url`, and `tokio` (time only, for retry). Nothing else is required, and every extra is feature-gated — the default build pulls no C dependencies.
8. **Hostile server, hostile input.** Every value from the wire is range-checked before it reaches a typed accessor; every caller-supplied key, option, and instruction is validated before it reaches the network.

## Architecture

### Crate layout

```
crates/jev/
├── Cargo.toml                 # features: derive, retry, rustls-tls (default); native-tls, batch
├── src/
│   ├── lib.rs                 # crate docs, re-exports, derive re-export
│   ├── error.rs               # Error + body truncation
│   ├── model.rs               # Model, surface-aware ID normalization, context limits
│   ├── config.rs              # Config, Surface, RetryConfig, Thresholds
│   ├── client.rs              # Client: decide / decide_with / ask / raw, retry loop, tracing
│   ├── guidance.rs            # Guidance: String | Object | Array (instructions & criteria)
│   ├── state.rs               # State: Text | Json, conversions, size estimate
│   ├── question.rs            # Question, Questions, QuestionsBuilder, validation
│   ├── answer.rs              # Answer, Answers, checked accessors
│   ├── typed.rs               # Decision / ChoiceOptions / ScoreLevels traits,
│   │                          # Noul, Choice<T>, Score<T> wrappers
│   ├── batch.rs               # decide_all bounded-concurrency helper
│   ├── wire.rs                # Request / Response serde, Usage, ResponseMeta
│   └── util.rs                # truncate_on_char_boundary, token estimate
├── examples/
│   ├── triage.rs              # the derive path end to end
│   ├── dynamic.rs             # questions built from YAML at runtime
│   └── batch.rs               # decide_all over a slice of states
└── tests/
    ├── wire.rs                # wiremock: request shapes, error mapping, retry
    ├── derive.rs              # derive behaviour + trybuild compile-fail cases
    └── live.rs                # #[ignore] live tests, needs JEV_API_KEY

crates/jev-macros/
├── Cargo.toml                 # proc-macro = true; syn, quote, proc-macro2
└── src/
    ├── lib.rs                 # #[derive(Decision)] / ChoiceOptions / ScoreLevels
    ├── decision.rs
    ├── options.rs
    └── docs.rs                # doc-comment extraction + normalization
```

Names are chosen so that idiomatic use reads well without a prelude: `jev::Client`, `jev::Error`, `jev::Decision`, `jev::Noul`. `jev-macros` is an implementation detail re-exported through the `derive` feature; callers never depend on it directly.

### Layer 1 — wire types

Faithful mappings of the published schemas: `#[serde(rename_all = "snake_case")]` (the API is snake_case — `input_tokens`, `session_id`) and `skip_serializing_if` on every optional.

```rust
#[derive(Debug, Serialize)]
pub(crate) struct DecisionRequest<'a> {
    pub model: &'a str,
    pub state: &'a State,
    pub questions: &'a Questions,
    #[serde(skip_serializing_if = "Option::is_none")] pub session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")] pub user: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")] pub trace: Option<&'a TraceConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub model: String,
    pub answers: Answers,
    pub usage: Usage,
    #[serde(default)] pub id: Option<String>,
    #[serde(default)] pub provider: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Billed cost, when the gateway reports it.
    #[serde(default)] pub cost: Option<f64>,
}
```

`State` is `enum State { Text(String), Json(Value) }` with `#[serde(untagged)]`, matching the schema's `anyOf: [string, object, array]`. It implements `From<&str>`, `From<String>`, `From<Value>`, and offers `State::json(&T) -> Result<State, Error>` for any `Serialize` type — the common case is "serialize my domain struct as the state".

`Guidance` covers the repeated `string | object | array` shape used by `instructions` and by each criterion: `From<&str>`, `From<String>`, `From<Value>`, and `Display` for the string case. Callers who only ever pass strings never name the type, because every constructor takes `impl Into<Guidance>`.

`ResponseMeta { id, model, provider, usage, latency }` is the envelope handed to callers that need cost or the dated model snapshot.

### Layer 2 — dynamic questions and answers

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Noul   { instructions: Guidance,
             #[serde(skip_serializing_if = "Option::is_none")] criteria: Option<NoulCriteria> },
    Choice { instructions: Guidance, criteria: IndexMap<String, Guidance> },
    Score  { instructions: Guidance, criteria: Vec<Guidance> },
}

/// Ordered map of question key -> question. Ordering is preserved so request
/// payloads are stable and byte-comparable in tests.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Questions(IndexMap<String, Question>);
```

`NoulCriteria { r#true: Guidance, r#false: Guidance }` serializes to the `"true"`/`"false"` keys the API expects. It is optional because the schema requires only `type` and `instructions` for `noul`.

The builder serves the config-driven path:

```rust
let questions = Questions::builder()
    .noul("refund", "Is the customer asking for money back?")
    .noul_with("is_bug", "Is the customer reporting a defect?",
               "Describes broken behavior.", "Asks a question or requests a feature.")
    .choice("team", "Which team should own this?",
            [("payments", "Checkout, billing, or payments."),
             ("frontend", "Rendering or browser issues.")])
    .score("urgency", "How urgent is this?",
           ["Can wait", "This week", "Blocking revenue"])
    .build()?;
```

`build()` enforces every documented and practical invariant, each as a named `Error::Invalid { field, reason }`:

| Invariant | Limit |
|---|---|
| Question count | `1..=64` |
| Question key charset | `^[a-z0-9_]{1,64}$` |
| Instructions length | non-empty, ≤ 4 KiB per guidance string |
| `choice` options | `2..=64`, keys match `^[a-z0-9_-]{1,64}$`, criteria non-empty |
| `score` levels | `2..=32`, criteria non-empty |
| Duplicate question keys | structurally impossible (`IndexMap`) |

These are not cosmetic: keys and criteria are frequently assembled from configuration or user-adjacent data, so bounding length in **bytes** and allowlisting the charset happens at the boundary, before anything reaches the network or a log line.

Answers use a hand-written `Deserialize` (buffer to `Value`, dispatch on `type`) so unknown primitives degrade instead of failing the response:

```rust
#[derive(Debug, Clone)]
pub enum Answer {
    Noul(NoulAnswer),
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    /// A primitive this SDK version does not know about.
    Other { r#type: String, raw: Value },
}

pub struct NoulAnswer   { pub noul: f64 }
pub struct ChoiceAnswer { pub choice: String, pub confidence: Option<f64>,
                          pub probabilities: IndexMap<String, f64> }
pub struct ScoreAnswer  { pub score: f64, pub confidence: Option<f64>,
                          pub probabilities: IndexMap<String, f64>,
                          pub legend: IndexMap<String, Guidance> }
```

`probabilities` and `legend` default to empty maps rather than `Option`, because `T` has a meaningful empty value. `confidence` stays `Option` because absence is meaningful — the schema marks it optional.

Deserialization validates ranges rather than trusting the server: `noul` and every probability must be finite and within `0.0..=1.0`, `score` must be finite and non-negative, or the response is rejected with `Error::Parse`. A NaN probability that reaches a comparison silently disables a gate, so it is stopped at the boundary.

`Answers` exposes checked accessors so dynamic callers still get errors, not silent `None`:

```rust
impl Answers {
    pub fn noul(&self, key: &str) -> Result<f64, Error>;
    pub fn choice(&self, key: &str) -> Result<&ChoiceAnswer, Error>;
    pub fn score(&self, key: &str) -> Result<&ScoreAnswer, Error>;
    pub fn get(&self, key: &str) -> Option<&Answer>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

A type mismatch yields `Error::AnswerType { key, expected, actual }`; a missing key yields `Error::MissingAnswer { key }`.

### Layer 3 — typed decisions

Three traits and three wrapper types.

```rust
/// A set of questions about one state, answered as one struct.
pub trait Decision: Sized {
    /// The questions to send. Built once and cached in a `LazyLock` by the derive.
    fn questions() -> &'static Questions;
    /// Rebuild the struct from the model's answers.
    fn from_answers(answers: &Answers) -> Result<Self, Error>;
}

/// A fieldless enum usable as `choice` criteria.
pub trait ChoiceOptions: Sized {
    /// Wire keys in declaration order, with their criteria text.
    fn options() -> &'static [(&'static str, &'static str)];
    fn from_key(key: &str) -> Option<Self>;
    fn key(&self) -> &'static str;
}

/// A fieldless enum usable as an ordered `score` scale, lowest level first.
pub trait ScoreLevels: Sized {
    fn levels() -> &'static [&'static str];
    fn from_index(index: usize) -> Option<Self>;
    fn index(&self) -> usize;
}
```

Wrappers carry raw probabilities alongside the typed value, so thresholding stays possible:

```rust
pub struct Noul { probability: f64 }
impl Noul {
    pub fn probability(&self) -> f64;
    /// P(yes) >= threshold.
    pub fn is_yes(&self, threshold: f64) -> bool;
    /// P(yes) <= 1 - threshold.
    pub fn is_no(&self, threshold: f64) -> bool;
    /// `Some(true/false)` outside the ambiguous band, `None` inside it.
    pub fn decide(&self, yes_at: f64, no_at: f64) -> Option<bool>;
}

pub struct Choice<T: ChoiceOptions> { /* value, confidence, probabilities */ }
impl<T: ChoiceOptions> Choice<T> {
    pub fn value(&self) -> &T;
    pub fn into_value(self) -> T;
    pub fn confidence(&self) -> Option<f64>;
    pub fn probability_of(&self, option: &T) -> f64;
    /// confidence is present and >= threshold.
    pub fn confident(&self, threshold: f64) -> bool;
    /// `Some(value)` when `confident(threshold)`, else `None` — escalate.
    pub fn accept(&self, threshold: f64) -> Option<&T>;
}

pub struct Score<T: ScoreLevels> { /* score, confidence, probabilities */ }
impl<T: ScoreLevels> Score<T> {
    /// Probability-weighted position on the scale, e.g. 1.99.
    pub fn raw(&self) -> f64;
    /// Nearest discrete level.
    pub fn nearest(&self) -> T;
    /// `raw()` normalized to `0.0..=1.0` — handy for sorting.
    pub fn normalized(&self) -> f64;
    /// `raw() >= level.index()`.
    pub fn at_least(&self, level: &T) -> bool;
    pub fn confidence(&self) -> Option<f64>;
    pub fn confident(&self, threshold: f64) -> bool;
}
```

`nearest()` returns `T`, not `Option<T>`: a `Score<T>` can only be constructed from a response already validated against `T::levels()`, so a level always exists. An out-of-range score from the server is rejected at construction with `Error::ScoreRange { key, score, levels }`, and an unknown option with `Error::UnknownChoice { key, choice, expected }`. All three wrappers implement `Debug`, `Clone`, and `PartialEq` where `T` allows.

### Derive macros

`#[derive(Decision)]` applies to a struct whose fields are `Noul`, `Choice<T>`, `Score<T>`, or `Option<_>` of those:

- **Question key** = field name, or `#[jev(key = "…")]`.
- **Instructions** = the field's doc comment (lines joined and trimmed), or `#[jev(instructions = "…")]`. Missing both is a **compile error** pointing at the field, because an instruction-less question is a guaranteed HTTP 400.
- **Noul criteria** = optional `#[jev(yes = "…", no = "…")]`; one without the other is a compile error.
- **Choice / Score criteria** come from `T::options()` / `T::levels()` — nothing to repeat, nothing to drift.
- Generates `impl Decision`, with `questions()` returning `&'static Questions` from a private `LazyLock<Questions>`: built once per process, zero per-call allocation, and infallible because the macro already validated the shape.
- `from_answers` reads each field through the checked accessor, so errors name the offending question.
- `#[jev(optional)]` on an `Option<_>` field tolerates a missing answer instead of erroring — for questions added ahead of a model rollout.

`#[derive(ChoiceOptions)]` applies to a fieldless enum:

- Wire key = `snake_case` of the variant, overridable per variant with `#[jev(key = "…")]` and for the whole enum with `#[jev(rename_all = "snake_case" | "kebab-case" | "lowercase")]`.
- Criteria = the variant doc comment; missing is a compile error, since `choice` requires criteria.
- Rejected at compile time: data-carrying variants, fewer than two variants, duplicate keys.

`#[derive(ScoreLevels)]` is the same, except the level text comes from the doc comment and the index is the declaration position. Documented loudly in rustdoc: **declaration order is the scale, lowest first; reordering variants silently changes the meaning of every tuned threshold.**

Each rule has a `trybuild` compile-fail case in `tests/derive.rs`. Macro diagnostics use `syn::Error::to_compile_error` with spans on the offending field or variant, never a whole-item error.

### Models and surfaces

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    /// `typesafe/jev-1.13` — pinned; the default.
    Jev113,
    /// `~typesafe/jev-latest` — tracks the newest release.
    Latest,
    /// Any other System One model ID, passed through unchanged.
    Custom(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Surface {
    /// `POST /api/alpha/decisions` — richer request (session_id, trace, user).
    #[default] Decisions,
    /// `POST /api/v1/systemone` — stable v1 path.
    SystemOne,
}
```

`Model::wire_id(Surface) -> Cow<'_, str>` applies the documented mapping: the Decisions surface needs the author prefix (`typesafe/jev-1.13`, `~typesafe/jev-latest`), while the System One surface accepts bare IDs and passes already-prefixed IDs through. `Custom` is never rewritten — the caller owns it. `FromStr` and `Display` are implemented so model IDs round-trip through configuration.

**The default is `Jev113`, pinned — not `Latest`.** Thresholds are tuned against one model's calibration, so silently following `jev-latest` would silently re-tune every gate in production. `Latest` is one builder call away and documented for exploratory work. The default is also exposed as `pub const DEFAULT_MODEL: Model` so a bump is one reviewable line.

`Model::context_tokens()` returns `Some(32_000)` for known models and `None` for `Custom`. The client uses it for a cheap pre-flight estimate (serialized-payload bytes / 4) and returns `Error::StateTooLarge { estimated, limit }` before spending a billed round trip. This is a guard, not a tokenizer: the server stays authoritative and the check is disabled with `with_context_check(false)`.

Fields the System One surface does not define are dropped rather than errored when that surface is selected, and a `debug!` records the drop.

### Client, configuration, and reliability

```rust
#[derive(Clone)]
pub struct Client { /* http: reqwest::Client, api_key: SecretString, config: Arc<Config> */ }

impl Client {
    /// Typed path: `D::questions()` out, `D` back.
    pub async fn decide<D: Decision>(&self, state: impl Into<State>) -> Result<D, Error>;
    /// Typed path with per-call knobs (session, user, trace, model override).
    pub async fn decide_with<D: Decision>(&self, req: RequestOptions<'_>) -> Result<Decided<D>, Error>;
    /// Dynamic path.
    pub async fn ask(&self, state: impl Into<State>, questions: &Questions) -> Result<Response, Error>;
    /// Escape hatch: full request in, full response out.
    pub async fn raw(&self, req: RequestOptions<'_>) -> Result<Response, Error>;
}

/// A typed decision plus the response envelope (id, model snapshot, usage, cost).
pub struct Decided<D> { pub value: D, pub meta: ResponseMeta }
```

`decide` is the 90 % API and deliberately drops the envelope; `decide_with` returns `Decided<D>` for callers that record cost or the dated model snapshot.

Construction:

- `Client::new(impl Into<SecretString>)` builds its own `reqwest::Client` with sane defaults, using whichever TLS backend feature is enabled (rustls by default).
- `Client::with_http(reqwest::Client, impl Into<SecretString>)` shares a caller-owned pool — the right choice inside a service, and the way to supply a custom TLS configuration such as a pinned internal CA.
- Chainable knobs: `with_model`, `with_surface`, `with_api_base`, `with_timeout`, `with_retry`, `with_context_check`, `with_session_id`, `with_user`, `with_thresholds`.
- `Client::from_config(http, key, Config)` for the config-file path.
- `Client::from_env()` reads `JEV_API_KEY`, falling back to `OPENROUTER_API_KEY`, plus optional `JEV_BASE_URL`; returns `Error::MissingApiKey` when unset.
- `Clone` is cheap (`reqwest::Client` plus two `Arc`s), so the client drops straight into shared application state.
- `Debug` prints `api_base`, `model`, `surface`, and `api_key: "[REDACTED]"`. A unit test asserts the raw key never appears in the output.

`Config` derives `Deserialize` with `#[serde(rename_all = "camelCase", default, deny_unknown_fields)]` so it nests inside an application's YAML and typos are rejected instead of ignored:

```yaml
jev:
  model: typesafe/jev-1.13
  surface: decisions
  timeoutMs: 10000
  retry:
    maxAttempts: 3
    initialBackoffMs: 200
    maxBackoffMs: 2000
  thresholds:
    autoRouteConfidence: 0.7
    isBugYes: 0.9
```

`Thresholds` is a newtype over `HashMap<String, f64>` with `get(name) -> Option<f64>` and `require(name) -> Result<f64, Error>`, so gates are named and runtime-tunable instead of hard-coded. Values are validated to be finite and within `0.0..=1.0` at load. Hot reload is the application's business (wrap `Config` in `ArcSwap`); the SDK imposes no reload mechanism.

**Retry** (feature `retry`, default on, pulls `tokio` `time`) applies to `429`, `500`, `502`, `503`, `504`, and transport-level timeout/connect errors. It never retries `400`, `401`, `402`, `403`, `404`, or `413` — those are deterministic. Backoff is exponential with full jitter and a cap; `Retry-After` (seconds or HTTP-date) takes precedence when present, clamped to `max_backoff`. Because the endpoint has no idempotency key, a retry can be double-billed; that is documented on `RetryConfig`, and `max_attempts: 1` disables retry entirely. A per-request overall deadline bounds the whole attempt chain, so retry cannot extend latency without limit.

**Errors** are one `thiserror` enum, non-exhaustive, with variants chosen so callers can act without string matching:

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("request failed: {0}")]                     Http(#[from] reqwest::Error),
    #[error("API error (HTTP {status}): {message}")]     Api { status: u16, message: String, body: String },
    #[error("rate limited (HTTP 429), retry after {retry_after_secs:?}s")]
                                                        RateLimited { retry_after_secs: Option<u64> },
    #[error("insufficient credits (HTTP 402): {message}")] InsufficientCredits { message: String },
    #[error("failed to parse response: {0}")]           Parse(String),
    #[error("missing answer for question `{key}`")]     MissingAnswer { key: String },
    #[error("question `{key}`: expected {expected} answer, got {actual}")]
                                                        AnswerType { key: String, expected: &'static str, actual: String },
    #[error("question `{key}`: unknown choice `{choice}` (expected one of {expected})")]
                                                        UnknownChoice { key: String, choice: String, expected: String },
    #[error("question `{key}`: score {score} out of range for {levels} levels")]
                                                        ScoreRange { key: String, score: f64, levels: usize },
    #[error("invalid {field}: {reason}")]               Invalid { field: String, reason: String },
    #[error("state too large: ~{estimated} tokens exceeds {limit}")]
                                                        StateTooLarge { estimated: usize, limit: usize },
    #[error("no API key configured (set JEV_API_KEY)")] MissingApiKey,
    #[error("request timed out after {elapsed_ms}ms")]  Timeout { elapsed_ms: u64 },
}

impl Error {
    /// Whether retrying this request could plausibly succeed.
    pub fn is_retryable(&self) -> bool;
    /// HTTP status, when the error came from the API.
    pub fn status(&self) -> Option<u16>;
}
```

The `Api` variant parses the `{"error":{"code","message"}}` envelope into `message` while keeping `body` — truncated to 4 KiB on a char boundary — for diagnostics. `402` and `429` get their own variants because callers respond differently (stop versus back off). No error message ever embeds state or criteria text.

**Observability** uses `tracing` only, never `println!`: `#[instrument(skip(state), fields(model, surface, questions = questions.len(), attempt, request_id, input_tokens, cost, latency_ms))]`. State and criteria are never recorded at any level. A `warn!` fires when a returned `confidence` falls below `0.5`, so miscalibrated gates surface in dashboards without extra application code. The SDK installs no subscriber.

### Concurrency helper

Batching here means "many states, same questions", which the API does not fuse, so the SDK ships one helper instead of a fake batch surface:

```rust
/// Run one decision per state with bounded concurrency, preserving input order.
pub async fn decide_all<D: Decision>(
    client: &Client,
    states: Vec<State>,
    concurrency: NonZeroUsize,
) -> Vec<Result<D, Error>>;
```

Implemented with `futures_util::stream::iter(..).map(..).buffered(n)` behind a `batch` feature so the `futures_util` dependency is opt-in. Results are per item, so one failure does not discard the batch.

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `derive` | yes | Re-exports `Decision`, `ChoiceOptions`, `ScoreLevels` derives from `jev-macros`. |
| `retry` | yes | Bounded retry loop; pulls `tokio` with `time`. |
| `rustls-tls` | yes | `reqwest` over `rustls` with the `aws-lc-rs` backend. |
| `native-tls` | no | Platform TLS, for callers that need the system trust store. |
| `batch` | no | `decide_all`; pulls `futures_util`. |

**TLS defaults to `rustls-tls` and is caller-configurable.** `reqwest` is pulled with `default-features = false`, so the SDK never drags in a TLS stack the caller did not ask for. The default is rustls with the `aws-lc-rs` backend because it is pure Rust, behaves identically on every platform, and keeps OpenSSL out of the dependency tree. Callers who need the operating-system trust store — typically for a corporate MITM proxy or an internal CA distributed by device management — select `native-tls` instead:

```toml
jev = { version = "0.1", default-features = false, features = ["native-tls", "derive", "retry"] }
```

The two TLS features are additive rather than mutually exclusive, because Cargo feature unification means a dependency graph can legitimately request both. When both are enabled the client prefers rustls and emits a one-time `debug!` naming the backend in use, so the effective choice is observable rather than inferred from lock-file archaeology. Enabling neither is also valid and is the right choice when the caller passes an already-configured client to `Client::with_http`; in that configuration the SDK builds no TLS stack of its own, and `Client::new`, which must construct a client, returns `Error::Invalid` explaining that a TLS feature or `with_http` is required.

`default-features = false` with no other features yields a macro-free, retry-free, TLS-stack-free client driven entirely through `with_http`. Every feature combination that CI can reach is checked with `cargo hack --feature-powerset check`, including both TLS features together and neither.

## Security posture

- `#![forbid(unsafe_code)]` at the crate root; no `unsafe` anywhere, including tests.
- No `unwrap`, `expect`, `panic!`, `todo!`, or slice indexing on any path reachable from server or caller input. Enforced with `cargo clippy -W clippy::unwrap_used -W clippy::expect_used -W clippy::panic -W clippy::indexing_slicing`.
- API keys live in `secrecy::SecretString`; `Debug` redacts, and a test asserts redaction.
- Response bodies are read with a hard byte cap before parsing, so a hostile or broken gateway cannot exhaust memory; the error body is additionally truncated to 4 KiB on a char boundary.
- TLS defaults to `rustls` with the `aws-lc-rs` backend — pure Rust, uniform across platforms, no OpenSSL exposure. `native-tls` is available for callers who must use the system trust store; see "Feature flags" for the trade-off and for what happens when both or neither are enabled.
- The API base URL is parsed with `url::Url` and restricted to `https` unless the caller explicitly opts into `http` for a local mock — wiremock tests use that opt-in.
- Probability and score arithmetic rejects non-finite values at the boundary rather than propagating NaN into comparisons.
- Caller-supplied keys, option keys, and guidance strings are length-bounded in bytes and charset-allowlisted at `Questions::build()`.

## Testing

- **Unit (in-file `#[cfg(test)]`)**: model-ID normalization per surface; `Guidance`/`State` serde round-trips; `Questions::builder` validation for every invariant in the table; `Answer` deserialization including unknown type → `Other`, NaN rejection, and out-of-range probabilities; `Noul`/`Choice`/`Score` threshold arithmetic at boundaries (`raw = 0.0`, `raw = levels - 1`, `confidence = None`); `Score::normalized` on a two-level scale; retry classification (`rstest` parameterized over status → retryable); `Retry-After` parsing for both seconds and HTTP-date; error-body truncation across a multi-byte boundary; `Debug` redaction; `Thresholds` validation; `Client::new` returning `Error::Invalid` when no TLS feature is enabled.
- **Property (`proptest`)**: `Model` `Display`/`FromStr` round-trip; `Score::normalized` stays within `0.0..=1.0` for any valid score and level count; `Questions::build` never accepts a key outside the allowlist.
- **Wire (`tests/wire.rs`, `wiremock`)**: golden request bodies for the triage example on both surfaces, asserting exact JSON — which is why question order is preserved; `400`/`402`/`429` mapping; retry succeeding on the second attempt; retry *not* firing on `400`; `Retry-After` respected; oversized response body rejected.
- **Derive (`tests/derive.rs`)**: generated questions equal a hand-built `Questions`; doc-comment extraction for multi-line and indented comments; `rename_all`; `#[jev(optional)]`; `trybuild` compile-fail for missing instructions, missing variant docs, data-carrying variant, single-variant enum, `yes` without `no`, duplicate keys, and an unsupported field type.
- **Docs**: every public item carries a rustdoc example, so `cargo test --doc` is a real integration surface. `#![warn(missing_docs, missing_debug_implementations)]` keeps it that way.
- **Live (`tests/live.rs`, `#[ignore]`)**: the triage decision against the real API asserting `is_bug.probability() > 0.5`, `team.value() == Team::Payments`, `urgency.nearest() == Urgency::BlockingRevenue`, and that usage is reported; an invalid model returning 4xx; a `Custom` model on the System One surface. Run with `cargo test -p jev -- --ignored` and a key from the environment.

`make test` runs the offline suite via `cargo nextest run --all-features`; a `make test-live` target runs the ignored set. Neither the unit nor the wire suite touches the network.

## Documentation and examples

- Crate-level rustdoc opens with the triage example, then the dynamic path, then a "choosing thresholds" section that explains calibration and escalation explicitly — the most common misuse is treating `confidence` as accuracy.
- `docs/` carries a quickstart, a page on each primitive with worked examples, a threshold-tuning guide, and a migration recipe from prompt-and-parse classification.
- `examples/` holds three runnable programs (`triage`, `dynamic`, `batch`), each executable with a key in the environment and referenced from the README.
- The README states MSRV, feature flags, and the pinned default model, and links the primitive pages.

## Alternatives Considered

**No proc macros — builders only.** Rejected as the *primary* API, kept as the secondary one. Builders cannot produce a typed `Choice<Team>`, cannot keep criteria in sync with an enum, and cannot reject an instruction-less question at compile time. Those three are the entire reason a Rust SDK beats an untyped one. The macro crate is small — three derives and one attribute parser — and sits behind a default-on feature so macro-free builds stay possible.

**Generate criteria from variant names only.** Rejected. `Payments` alone is a far weaker criterion than "Checkout, billing, or payment processing issues.", and answer quality is dominated by criteria quality. Requiring the doc comment also makes it impossible for the docs and the prompt to disagree.

**`#[serde(tag = "type")]` for `Answer`.** Rejected. An internally tagged enum hard-fails the entire response on an unknown `type`, breaking every caller the day a fourth primitive ships. A hand-written `Deserialize` with an `Other` fallback costs about forty lines and makes new primitives a non-event.

**`HashMap` for questions and probabilities.** Rejected in favour of `IndexMap`. Stable ordering makes request bodies byte-comparable in tests and keeps score legends and probabilities in their natural order when logged or rendered.

**Expose only the System One surface.** Rejected. It lacks `session_id` and `trace`, which cross-service observability wants, and the Decisions surface is the one documented for non-official-SDK clients. Supporting both is one enum and one `match` on the path.

**Default to `~typesafe/jev-latest`.** Rejected — see "Models and surfaces". Auto-following releases silently invalidates tuned thresholds.

**Generic `Decision` over a caller-supplied HTTP client trait.** Rejected for v1. `reqwest` covers the ecosystem, and a transport trait would leak into every signature for the benefit of a hypothetical caller. The `raw` escape hatch plus public wire types let anyone drive their own transport if needed.

**A built-in response cache.** Deferred. Correct keying needs a hash over model, state, and questions plus a per-gate TTL policy, and caching a decision about mutable state is an easy footgun. Callers can wrap `decide` today; a cache deserves its own spec.

**A blocking client.** Deferred. `reqwest::blocking` inside a library forces a nested runtime decision on the caller. Synchronous callers can wrap `decide` in their own runtime handle, which is documented.

## Rollout

1. **Phase 1** — Wire and dynamic layers (`state`, `guidance`, `question`, `answer`, `wire`, `error`, `util`) with full unit and wiremock coverage. Usable, untyped, correct.
2. **Phase 2** — Client, config, retry, tracing, and the `Thresholds` type. The dynamic path is production-ready.
3. **Phase 3** — `typed.rs` plus `jev-macros`: the three traits, three wrappers, three derives, and the `trybuild` suite. The headline API lands.
4. **Phase 4** — `decide_all`, the three examples, `docs/` pages, README, and a `0.1.0` crates.io release of `jev` and `jev-macros`.
5. **Phase 5** — Threshold-tuning tooling: a small binary that replays labeled samples and reports precision/recall per threshold. Specced separately once Phase 4 shows tuning is the real cost.

## Open Questions

- **Default model cadence.** `Jev113` is pinned and exposed as `DEFAULT_MODEL`. Should a minor release ever bump it, or is that reserved for major versions, given thresholds are calibration-sensitive? Leaning: minor releases may add variants, only a major release changes the default.
- **`provider` routing preferences.** The Decisions schema accepts provider preferences. With a single provider serving Jev today, v1 omits the field rather than modelling a large schema it cannot exercise. Revisit when a second System One provider appears.
- **Trace configuration shape.** `trace` is passed through as an opaque `TraceConfig` in v1. If the field stabilizes into something richer, it deserves typed sub-structs rather than a pass-through.
- **MSRV policy.** The design targets latest stable Rust 2024. Confirm whether the SDK commits to an N-2 stable floor and tests it in CI, which would rule out newer standard-library conveniences in hot paths.
