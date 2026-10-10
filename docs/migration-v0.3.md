# Migrating to 0.3

> Historical 0.3 migration guide. For the current package, imports, attributes and
> environment variables, apply the [0.4 migration guide](migration-v0.4.md) as well.

This release directly replaces the Jev-specific public API with provider-neutral
decisions. Package name remains `jev-typed`, library import remains `jev`. No deprecated
aliases are retained. Rust 1.99 is required; the workspace version becomes 0.3.0.

| Previous API | Replacement |
|---|---|
| `Noul`, `.noul`, `.noul_with` | `Predicate`, `.predicate`, `.predicate_with` |
| `State`, JSON `Guidance` | Validated `Input`; structured evidence uses `Input::json`; guidance is text |
| `Client::new(key)`, `for_provider`, `with_provider` | `new(provider, key)`, `openai/openrouter/typesafe`, or builder |
| `with_http(reqwest_client)` | SDK-controlled `HttpClient`, shared through builder |
| `Surface`, `with_surface`, `with_api_base` | Hosted endpoint selection, or complete explicit `Endpoint` with declared protocol |
| `Model::Latest`, `DEFAULT_MODEL` | `JevLatest`, provider-aware `Model::Default` |
| `Model::Custom(String)` | `Model::Custom(ModelId::try_from(id)?)`, or `id.parse::<Model>()?` |
| `raw`, public wire `Response` | `ask(input, &questions)` and checked `DecisionResponse` |
| `Option` fields, `#[jev(optional)]` | `Outcome` fields to retain refusals; missing answers always fail |
| `Choice::accept` confidence gate | Now selected-event probability; use `accept_confidence` for native confidence |
| Raw f64 thresholds/sample rolls | Validated `Probability` |
| Infallible nearest/sample | Checked `Result`; missing distribution makes sampling fail |
| Description-only `ScoreLevels::levels` | Ordered `LevelSpec { key, description }` via `LevelSpec::new` |
| Infallible `Decision::questions` | Cached `Result<&'static Questions, Error>` |
| `Decision::from_answers` returning Error | `DecodeError`; Client adds refusal metadata |
| `session_id`, `user`, `trace` passthrough | Removed; keep application-local tracing context |
| Version 1 case files | Explicit `version: 2`, `input`, `predicate`, options/levels pairs |
| Optional native TLS feature | Removed; default rustls uses aws-lc-rs |

Environment precedence changed deliberately. Specify `JEV_PROVIDER` when multiple
keys exist or when using `JEV_API_KEY`; only the selected provider's credentials
are eligible. Blank or invalid present values are errors. Default models resolve
for the provider, and built-in mismatches fail before HTTP. Custom endpoints require
an explicit custom model and never infer a hosted default.

JSON evidence now becomes the same sorted compact text on every backend. Supplied
choice confidence never replaces P(selected), absent distributions never become
one-hot results, and Score nearest follows rounding rather than distribution mode.
Re-evaluate thresholds on labelled application cases before changing model policies.

A network deadline covers queueing, sends, reads, retries and backoff even without
the retry feature. Only establishment failures and definite transient statuses can
retry. Quota exhaustion and failures after a request may have been sent do not replay.
Retry-After is honored without shortening to the computed backoff cap.

See [quickstart](quickstart.md) and the [design](../specs/unified-decision-backends-design.md)
for complete construction and migration contracts.
