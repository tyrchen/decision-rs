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

## Blocking issue for a real crates.io publish

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
