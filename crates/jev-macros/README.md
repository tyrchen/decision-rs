# jev-macros

Proc-macros for the [`jev`](../jev) SDK: `#[derive(Decision, ChoiceOptions, ScoreLevels)]`.

This is an implementation detail re-exported through `jev`'s `derive` feature
(default on). Add `jev-typed = "0.2"` to your dependencies and import `jev` in Rust;
the derives are exported from `jev::{Decision, ChoiceOptions, ScoreLevels}`.

See the [workspace README](../../README.md) for what the derives do; the short
version: `///` doc comments on struct fields and enum variants become the
`instructions` and criteria sent to the model, checked at compile time.

## License

MIT — see [LICENSE](../../LICENSE.md).
