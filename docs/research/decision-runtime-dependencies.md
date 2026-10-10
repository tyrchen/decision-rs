# Decision runtime dependency review

Reviewed on 2026-10-09 for the 0.3 implementation. Dependencies are workspace-managed;
features enable only the capabilities used by the SDK.

`reqwest` 0.13.5 remains the HTTP client, with default features disabled and optional
rustls/aws-lc-rs transport. Its [builder API](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html)
supports an explicit no-redirect policy and `retry(reqwest::retry::never())`, which
lets the SDK own every attempt. No OpenAI SDK is needed for the small Decisions protocol.

Replace `serde_norway` with `serde-saphyr` 1.3.0, enabling only `deserialize` for
optional YAML loading. Its maintained pure Rust parser exposes [Options](https://docs.rs/serde-saphyr/1.3.0/serde_saphyr/struct.Options.html)
and parser-level [Budget](https://docs.rs/serde-saphyr/1.3.0/serde_saphyr/struct.Budget.html)
controls. Reject aliases, anchors, merge keys, includes and duplicate keys; enforce
byte, depth, node, event and scalar limits before constructing the case domain.
JSON case loading separately uses a bounded duplicate-aware parser. Per-evidence
limits are stricter than the case envelope limits.

Use `httpdate` 1.0.3 for standards-compatible HTTP date parsing of Retry-After,
alongside numeric seconds. Its [parser](https://docs.rs/httpdate/1.0.3/httpdate/fn.parse_http_date.html)
supports current and legacy HTTP date forms. It was already a transitive HTTP-stack
dependency; making it direct adds no new package to the graph.

Both new direct dependencies use MIT/Apache-2.0 licensing. Release verification
runs cargo audit and cargo deny on the final lockfile; the accompanying verification
spec records the actual results. Parser budgets, no-secret diagnostics, date-based
retry and feature-disabled deadlines have regression coverage.

CI uses [cargo-hack 0.6.45](https://docs.rs/crate/cargo-hack/0.6.45) with `--feature-powerset --no-dev-deps`, so dev dependencies cannot conceal missing production features. [install-action](https://github.com/taiki-e/install-action) supplies the pinned tool version. The matrix contains 37 configurations, including no-default-features.

Pre-commit verification uses [Gitleaks 8.30.1](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1), the current upstream release checked for this change. `make secret-check` scans the staged patch with `git --pre-commit --staged --redact`; it does not change the Rust dependency graph. The local release archive was verified against its upstream SHA-256 checksum before execution.
