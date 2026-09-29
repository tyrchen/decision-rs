# Jev Rust SDK — Implementation Review

**Status**: Final
**Date**: 2026-05-13
**Category**: SDK / Library
**Type**: review

## Summary

`crates/jev` and `crates/jev-macros` implement [jev-sdk-design.md](./jev-sdk-design.md) in
full: all three layers (wire, dynamic, typed), both surfaces, retry with jitter and
`Retry-After`, the three derive macros, `decide_all`, three runnable examples, and the
full test matrix (unit, wiremock, trybuild, ignored live tests). All Rollout phases
1–4 are complete; phase 5 (threshold-tuning tooling) is out of scope for this pass, per
the design doc's own phasing.

## Deviations from the design doc

Each deviation below preserves the design's intent and public API; none change a
documented type signature, error variant, or wire behavior.

1. **`jev-macros` uses `darling` for attribute parsing**, not hand-rolled `syn::Attribute`
   parsing as the design doc's crate layout implies. `darling` is a small, mature,
   zero-`unsafe` dependency used only by the proc-macro crate (never in `jev` itself,
   so it never reaches a consumer's dependency tree at runtime). It reduces attribute-parsing
   boilerplate materially without weakening any compile-time check the design specifies —
   every documented compile-fail case (missing instructions, missing variant doc,
   data-carrying variant, single-variant enum, `yes` without `no`, duplicate keys,
   unsupported field type, duplicate question keys, `Option` without `#[jev(optional)]`)
   still fails to compile with a span-accurate error, verified by `tests/derive.rs`'s
   `trybuild` suite.
2. **`jev-macros` also uses `heck`** for `rename_all` casing (`snake_case`, `kebab-case`,
   `lowercase`) instead of hand-rolled case conversion. Same rationale: proc-macro-only,
   zero-`unsafe`, well-established.
3. **`serde_norway` replaces `serde_yaml`** for the `Config` YAML example and doc-test.
   `serde_yaml` is archived upstream; `serde_norway` is a maintained fork with the same
   `serde` integration. This is a `dev-dependency` only — `Config`'s `Deserialize` impl
   itself is YAML-library-agnostic (any `serde` `Deserializer` works), so callers may use
   whichever YAML crate they prefer.
4. **`reqwest`'s TLS feature is named `rustls`, not `rustls-tls`**, as of `reqwest` 0.13
   (the design doc was written against an older `reqwest` feature name). `jev`'s own
   feature is still called `rustls-tls` for the caller-facing name documented in the
   design; internally it maps to `reqwest/rustls`.
5. **`Choice::try_new` / `Score::try_new` always take a `key: &str` parameter**, rather
   than the design's sketch of a keyless `try_new` plus a `pub(crate) try_new_for_key`.
   The derive-generated `from_answers` needs the question key in the
   `UnknownChoice`/`ScoreRange` error message, and it can only call `pub` API (generated
   code lives in the consuming crate, not inside `jev`), so the two-tier split had no
   living caller and was collapsed into one public constructor.
6. **Retry backoff jitter is a small dependency-free PRNG substitute** (XOR of a
   monotonic clock reading and the process ID), not the `rand` crate. Backoff timing has
   no security property — only a fairness one (avoiding a thundering herd) — so a
   non-cryptographic, allocation-free source was preferred over adding a dependency for
   a jitter fraction. Documented inline at `full_jitter`.
7. **`Retry-After` HTTP-date parsing is a hand-rolled IMF-fixdate parser**
   (`parse_imf_fixdate` in `client.rs`), not the `httpdate` crate. Cross-checked against
   the RFC 7231 worked example (`Sun, 06 Nov 1994 08:49:37 GMT` → Unix `784111777`) in
   `test_should_parse_retry_after_http_date`. Only the IMF-fixdate form is accepted;
   the obsolete RFC 850 / ANSI C `Retry-After` date forms are not, which matches every
   modern HTTP server's output and keeps the parser small.
8. **`trybuild` compile-fail tests commit `.stderr` snapshots** under
   `crates/jev/tests/derive-fail/*.stderr`. The design doc calls for "a `trybuild`
   compile-fail case" per rule without specifying whether exact-stderr matching is used;
   this implementation follows the same pattern as `syn`/`serde`'s own macro test suites
   (exact match, snapshots checked in, regenerate on intentional message changes).

## Test results at time of review

- `cargo build --workspace --all-features --all-targets`: clean.
- `cargo test --workspace --all-features`: 192 passed (119 `jev` unit + 11 derive + 14
  wire + 20 `jev-macros` unit + 24 `jev` doctests + 4 ignored live), 0 failed.
- `cargo clippy --workspace --all-targets --all-features --tests --benches -- -D warnings`:
  clean.
- `cargo clippy -p jev --lib --all-features -- -D warnings -W clippy::pedantic` (allowing
  the one stylistic `single_match_else` finding in `jev-macros`, which reads more clearly
  as a `match` than an `if let … else`): clean.
- `cargo +nightly fmt --all -- --check`: clean.
- `cargo hack --feature-powerset check -p jev`: all 37 combinations pass, including both
  TLS features together, neither TLS feature, and every default/derive/retry/batch
  combination.
- `cargo audit`: 0 advisories against 250 crate dependencies.
- `cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok` (exit 0); the only
  output is informational warnings for `syn` v2/v3 coexisting — expected, since several
  transitive dependencies (`rstest`, `jni`, `serde_derive`, `tokio-macros`, `tracing-attributes`)
  have not yet migrated to `syn` 3, and unmatched license allowances in `deny.toml` that
  predate this crate and are not exercised by this dependency tree.

## Open items carried forward

Matches the design doc's own "Open Questions" verbatim — no new open items were
introduced by implementation:

- Default model cadence policy (minor vs. major release bumps).
- `provider` routing preferences, deferred until a second System One provider exists.
- `TraceConfig`'s eventual typed shape, once the upstream field stabilizes.
- MSRV floor policy beyond "latest stable"; this implementation pins `rust-version = "1.90"`
  in `Cargo.toml` (the crate uses `let`-chains, stable since 1.88) and the toolchain to
  `1.97.1` in `rust-toolchain.toml`, but has not committed to an N-2 stable floor.

## Package renamed to `jev-typed` (superseding the section below)

The package was renamed from `jev` to **`jev-typed`** at the requester's direction, to
match this repository's own name. The rename is package-name-only:
`crates/jev/Cargo.toml` sets `[package] name = "jev-typed"` and `[lib] name = "jev"`,
so every `use jev::...` statement in this workspace and in any downstream consumer is
unchanged — only the `[dependencies]` line in a consumer's `Cargo.toml` now reads
`jev-typed = "0.1"` instead of `jev = "0.1"`. `jev-macros` is unaffected (its own name
was never taken) and its `dev-dependency` on the SDK crate was updated to the new
workspace-dependency key (`jev-typed`).

**Checking crates.io while making this change surfaced that `jev-typed` is *also*
already taken** (v0.1.0, unrelated: a third independent async client for the same
TypeSafe API, with a builder-style `EvaluateRequest::new(state).question(id, Noul::new(...))`
shape). So neither `jev` nor `jev-typed` is actually publishable to crates.io today —
the rename satisfies "match the repository name" but does not, by itself, resolve a
crates.io publish. The three options in the superseded section below still apply,
now against two taken names instead of one; a crates.io release under either identity
still needs a decision from whoever owns that call (a different name entirely, e.g.
`jev-typesafe` or `typesafe-jev`, is the most likely to be free, but was not checked
exhaustively since renaming was not the ask this pass).

---

## Superseded: original blocking issue for a real crates.io publish

*(Written when the package was still named `jev`; kept for history. See the section
above for the current state after the rename to `jev-typed`.)*

**The name `jev` is already taken on crates.io** (v0.1.2, unrelated: a blocking/async
client for "the TypeSafe System One HTTP API" with a materially different API shape—
`TypeSafeClient`, `Questions::new()` + `.insert(...)`, no typed derive path). This has no
effect on this workspace's local build/test (all inter-crate references are path
dependencies), but `cargo publish -p jev` will fail, and `cargo package -p jev-macros`
already fails today because it depends on `jev = { path = ..., version = "0.1.0" }` and
cargo's packaging step verifies that constraint against the real registry, where
versions `0.1.0`–`0.1.2` exist and do not have a `derive` feature.

This was not called out in the design doc and is not this implementation's call to
resolve unilaterally — the design doc explicitly names the crate `jev` throughout, and
a rename touches every public path in both crates. Options for whoever owns the
crates.io release decision:

1. Publish under a different crate name (`jev-sdk`, `jev-client`, `openrouter-jev`, ...).
   Note that the crates.io name and the Rust `use` path are the same identifier, so
   this also changes every `use jev::...` in downstream code.
2. Contact the existing `jev` crate's owner about the name.
3. Ship as a git/path dependency indefinitely, skipping crates.io.

All code, tests, and docs in this workspace assume the name stays `jev`; renaming is a
find-and-replace across `Cargo.toml` package names, `use jev::...`, and doc prose, with
no structural changes required.

## Public-interface improvements adopted from prior art

At the same time as the rename, the public interface of an independent, mature Rust
client for the same API — [`porky11/jev`](https://gitlab.com/porky11/jev) (published as
crates.io `jev` v0.1.2, the crate that occupies the name above) — was studied for
interface ideas. Three ergonomic patterns from that crate (and, for the sampling idea,
echoed by a third independent client, crates.io `jev-typed` v0.1.0) were adopted, each
as an *addition* that composes with the existing typed/dynamic layering rather than a
replacement:

1. **`Verdict` and `Noul::verdict`/`decide`.** `porky11/jev`'s `Verdict::{Yes, Unsure, No}`
   plus `Verdict::from_probability(yes, threshold)` gives a caller a three-way branch
   for a yes/no gate instead of manually comparing two thresholds. Adopted as
   [`Verdict`] and [`Noul::decide_verdict`] in `crates/jev/src/typed.rs`, alongside the
   existing two-threshold [`Noul::decide`] (kept — the two are complementary: one
   threshold with a symmetric "distance from the coin flip" semantic, or two independent
   thresholds for an asymmetric yes/no risk tolerance).
2. **Human-readable `Display`.** `porky11/jev` renders a `Noul` answer as
   `"yes, 88% sure"` and a `Score` answer as `"1.88 (harmless 0%, wary 12%, lethal 88%;
   confidence 82%)"`. Adopted as `Display` impls on `Noul`, `Choice<T>`, and `Score<T>`
   (using this SDK's own percent/ranking conventions, not copied verbatim), so
   `println!("{answer}")` is useful without every caller writing its own formatter.
3. **Reproducible weighted sampling.** `porky11/jev`'s `ChoiceAnswer::sample(roll: f64)`
   walks the probability distribution and returns the option a caller-supplied `roll`
   lands on, keeping the draw reproducible (and the crate free of a `rand` dependency)
   for a caller that wants a scene to play out differently across runs without ever
   picking an option the model ruled out entirely. Adopted as `Choice::sample(roll)`,
   returning the typed `T` rather than a string key.

Also adopted, addressing a real gap the prior-art review surfaced (both `porky11/jev`
and the third `jev-typed` client support an API key-less, self-hosted backend; this SDK
did not):

4. **`Client::local` / an unauthenticated constructor.** For a System One-compatible
   backend a caller hosts themselves (no API key, and possibly no `Authorization`
   header support at all), `Client::with_http` already covered "bring your own
   `reqwest::Client`" but every constructor still required an API key. Added
   `Client::without_api_key(base_url)` for the no-`Authorization`-header case,
   keeping `Client::new`/`from_env`/`from_config` (which do send a key) unchanged.
5. **Single-question convenience methods on `Client`.** For a one-off dynamic question
   that does not warrant a `#[derive(Decision)]` struct, added `Client::noul`,
   `Client::choice::<T>`, and `Client::score::<T>` — each builds a single-question
   request and returns the typed [`Noul`]/[`Choice<T>`]/[`Score<T>`] answer directly,
   named to avoid colliding with the derive path's own `Decision`/`ChoiceOptions`/
   `ScoreLevels` trait methods.

Deliberately **not** adopted, with rationale:

- **`BTreeMap`-keyed `Questions`/answers.** This SDK already committed to `IndexMap` for
  declaration-order preservation (see the design doc's "Alternatives Considered"); a
  `BTreeMap`'s alphabetic reordering is a regression for byte-comparable request bodies
  and natural-order score legends, not an improvement.
- **A blocking client.** `porky11/jev`'s `TypeSafeClient` blocks by default with an
  `async` feature adding a second client type. This SDK is async-only by design
  (see "Alternatives Considered" — "A blocking client" — in the design doc); mirroring
  the dual-client split would double the client surface for a use case (game engines,
  synchronous CLIs) this SDK's design doc explicitly defers.
- **Optional/lenient answer-shape parsing** (`type` tag optional, `usage` defaulting to
  zero, unknown top-level fields silently ignored beyond `serde`'s own default). This
  SDK's hostile-input posture treats a response that does not match the documented
  shape as an error (or, for an unrecognized `type`, as `Answer::Other`) rather than
  silently filling in a zero; loosening this would weaken the "hostile server" boundary
  validation the design doc calls for.

## Examples restructured into an easy-to-complex progression

`crates/jev/examples/` grew from three programs to eight, numbered `01`–`08` so
`ls`/tab-completion sorts them in teaching order. The original three were renamed
rather than duplicated (`triage.rs` → `03_multi_question_decision.rs`,
`dynamic.rs` → `04_dynamic_questions.rs`, `batch.rs` → `07_batch_concurrent.rs`);
their content is materially unchanged beyond the header comment's invocation line.
The five new programs each demonstrate one concept in isolation rather than
re-explaining the whole SDK:

| #   | File                                  | Concept                                                                                           | Features          |
| --- | ------------------------------------- | ------------------------------------------------------------------------------------------------- | ----------------- |
| 1   | `01_single_question.rs`               | `Client::noul`: one question, no derive, no struct.                                               | none              |
| 2   | `02_single_typed_choice_and_score.rs` | `Client::choice::<T>`/`Client::score::<T>`: one derived enum, still no `Decision`.                | `derive`          |
| 3   | `03_multi_question_decision.rs`       | `#[derive(Decision)]`: **multiple questions from one API call** (the design doc's headline case). | `derive`          |
| 4   | `04_dynamic_questions.rs`             | `Questions::builder()`: config-driven questions built at runtime.                                 | none              |
| 5   | `05_thresholds_and_escalation.rs`     | `Thresholds`, `Verdict`/`decide_verdict`, `accept`: escalating ambiguity instead of guessing.     | `derive`          |
| 6   | `06_weighted_sampling.rs`             | `Choice::sample`: reproducible weighted sampling for games/simulations.                           | `derive`          |
| 7   | `07_batch_concurrent.rs`              | `decide_all`: bounded-concurrency batching over many states.                                      | `derive`, `batch` |
| 8   | `08_self_hosted_backend.rs`           | `Client::without_api_key`, `Model::Custom`, `Surface::SystemOne`: a self-hosted backend.          | `derive`          |

All eight compile under `cargo build --all-features --examples` and are covered by
`cargo clippy --all-targets --all-features -- -D warnings` (and a clean pedantic
pass). None make a live network call as part of the automated test suite; #1–#7
need `JEV_API_KEY`/`OPENROUTER_API_KEY` to actually run (per `Client::from_env`),
and #8 needs a real System One-compatible backend listening at `SELF_HOSTED_URL`.
`[[example]]` entries with `required-features` were added to `crates/jev/Cargo.toml`
for every example that needs `derive` and/or `batch`, matching the existing pattern.
