# decision-macros

Derives for `decision-rs` 0.5.1, re-exported through its default `derive` feature.
Depend on `decision-rs` with library name `decision_rs`; generated code uses `::decision_rs`.

`Decision` accepts concrete named structs with 1–64 `Predicate`, `Choice<T>`,
`Score<T>` or `Outcome<T>` fields. Field documentation supplies instructions;
`#[decision(instructions = "...")]` overrides them. `#[decision(key = "...")]` overrides the
question key. Predicate criteria must specify both `yes` and `no`.

`ChoiceOptions` and `ScoreLevels` accept fieldless enums with documented variants.
Stable keys default to snake_case and support per-variant `#[decision(key = "...")]`
and enum-level `rename_all = "snake_case" | "kebab-case" | "lowercase"`.
Score declaration order defines the scale; each level preserves its key and description.

The derives reject duplicate keys, invalid guidance, excessive counts, unsupported
fields, nested Outcome and Option fields. Generated schemas use the same checked
builder as runtime schemas and cache `Result`, so malformed handwritten option
traits produce an error without panicking. Typed decoding collects required refusals
and retains explicit `Outcome::Refused` values.
