# Changelog

All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

## 0.3.0 (unreleased)

### Added

- OpenAI Decisions with provider-aware Luna defaults, alongside OpenRouter Jev,
  TypeSafe System One and explicit custom endpoints.
- Provider-neutral `Predicate`, `Choice`, `Score`, `Outcome`, `Probability`,
  validated input/schema/answers and response provenance/accounting.
- Canonical JSON evidence, strict version 2 case files, budgeted YAML loading,
  shared safe connection pools and bounded ordered batches.
- Explicit refusal handling, complete distribution/score validation, quota-aware
  error categories, total network deadlines and SDK-owned retry attempts.

### Breaking changes

- Replace Jev-specific Noul/State/Guidance/wire/Surface APIs; remove provider mutation,
  arbitrary reqwest client injection, optional fields and remote trace passthrough.
- Choice acceptance now gates on P(selected); native confidence remains separately
  available through `accept_confidence`.
- Require validated probabilities, checked typed decoding and explicit custom models.
- Credentials require deterministic provider selection; generic and multiple keys
  no longer silently select OpenRouter.
- Remove native TLS; rustls uses aws-lc-rs. Require Rust 1.99 and version workspace
  packages at 0.3.0. See [the migration guide](docs/migration-v0.3.md).

## [0.2.0](https://github.com/tyrchen/jev-typed/releases/tag/v0.2.0) - 2026-10-09

### Added

- OpenRouter and TypeSafe AI constructors accepting API keys directly:
  `Client::openrouter`, `Client::typesafe`, and `Client::for_provider`.
- Provider selection through `Config::provider`, `Client::with_provider`, and
  `JEV_PROVIDER`, with `TYPESAFE_API_KEY` support and provider-specific key fallback.
- TypeSafe endpoint and model mapping, including the pinned `jev-1.13.0` model.
- API key validation and sensitive authorization headers.
- `make publish-check` and `make publish` for workspace packaging and publication.

### Changed

- Both `jev-typed` and `jev-macros` are versioned at 0.2.0.
- `Config` gains a `provider` field. Full struct literals must specify it or use
  `..Config::default()`. Existing YAML defaults to OpenRouter.
- The development toolchain is pinned to Rust 1.99.0.
- The lockfile uses non-yanked `yoke-derive` 0.8.4.

### Fixed

- CI uses the same nightly formatting and Makefile lint/test targets as local development.
- Macro generation satisfies the pedantic single-pattern match lint.
- Public case documentation no longer links to a private constant.
- Generated changelog links point to this repository.
