# Specs Index

| Spec | Type | Status | Summary |
|---|---|---|---|
| [jev-sdk-design.md](./jev-sdk-design.md) | design | Implemented (v0.1.0) | Standalone Rust SDK for the Jev decision model: typed `noul`/`choice`/`score` primitives, derive macros that turn doc comments into prompts, dynamic question builder, dual surfaces, retry, and threshold configuration. |
| [jev-sdk-review.md](./jev-sdk-review.md) | review | Final | Implementation review: deviations from the design doc and test results at completion. |
| [unified-decision-backends-design.md](./unified-decision-backends-design.md) | design | Implemented (v0.4) | Provider-neutral decision types and client API, OpenAI/Jev adapters, model resolution, refusal handling, validation, migration, and release criteria. |
| [unified-decision-backends-verification.md](./unified-decision-backends-verification.md) | verification | Historical (v0.3) | Implementation review, deterministic gates, package verification, live OpenAI/OpenRouter smoke, and explicit TypeSafe/calibration limitations. |
| [cloudflare-clef-design.md](./cloudflare-clef-design.md) | design | Awaiting hosted acceptance | Cloudflare-hosted Clef/Flash/Omni integration: account routing, shared System One codec, configuration, numerical precision, errors and self-hosted boundary. |
| [cloudflare-clef-verification-plan.md](./cloudflare-clef-verification-plan.md) | verification-plan | Offline checks passed | Deterministic protocol/configuration matrix, precision regressions, three-model live smoke and release criteria. |
| [cloudflare-clef-review.md](./cloudflare-clef-review.md) | review | Offline verified; live pending | Implemented API and protocol changes, Rust 1.99.1 gate results, numerical checks and outstanding hosted acceptance. |
