# jev-macros

Proc-macros for the [`jev`](../jev) SDK: `#[derive(Decision, ChoiceOptions, ScoreLevels)]`.

This is an implementation detail re-exported through `jev`'s `derive` feature
(default on). Depend on `jev`, not on this crate directly — the derives are
exported from `jev::{Decision, ChoiceOptions, ScoreLevels}`.

See the [workspace README](../../README.md) for what the derives do; the short
version: `///` doc comments on struct fields and enum variants become the
`instructions` and criteria sent to the model, checked at compile time.

## License

MIT — see [LICENSE](../../LICENSE.md).
