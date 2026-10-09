# Changelog

All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

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
