# Choosing thresholds

## `confidence` is calibration, not accuracy

A `Choice<T>` with `confidence() == Some(0.95)` means: *across many similar
inputs, when this model reports 0.95, it is right about 95% of the time* —
assuming the model is well-calibrated on your distribution of states, which is an
assumption worth testing, not assuming. It does **not** mean "95% accurate on this
one ticket." Treating a single `confidence` value as a per-item accuracy guarantee
is the most common misuse of this SDK — and, more generally, of any calibrated
probabilistic model.

## The SDK never picks a threshold for you

Every gate takes an explicit threshold argument:

- [`Noul::is_yes(threshold)`](../crates/jev/src/typed.rs) / `is_no(threshold)` / `decide(yes_at, no_at)`
- [`Choice::confident(threshold)`](../crates/jev/src/typed.rs) / `accept(threshold)`
- [`Score::confident(threshold)`](../crates/jev/src/typed.rs)

There is no built-in default because the right threshold trades off automation
rate against error tolerance — a decision about acceptable risk, which belongs to
the application, not the SDK.

## Escalate the ambiguous band

`Noul::decide(yes_at, no_at)` returns `None` for the band between the two
thresholds — the model's own signal that this case is uncertain. Route that band to
a human reviewer, a stricter secondary check, or a conservative default, rather
than silently picking `yes` or `no`:

```rust
# use jev::Noul;
# let is_bug = Noul::try_new(0.6).unwrap();
match is_bug.decide(0.85, 0.85) {
    Some(true) => { /* auto-route as a bug */ }
    Some(false) => { /* auto-route as not-a-bug */ }
    None => { /* escalate: this ticket is genuinely ambiguous */ }
}
```

The same idea applies to `Choice::accept(threshold)`, which returns `None` instead
of the low-confidence value:

```rust
# use jev::{Choice, ChoiceOptions};
# #[derive(Debug, Clone, Copy, PartialEq, Eq)]
# enum Team { Payments }
# impl ChoiceOptions for Team {
#     fn options() -> &'static [(&'static str, &'static str)] { &[("payments", "Billing."), ("x", "y")] }
#     fn from_key(key: &str) -> Option<Self> { if key == "payments" { Some(Team::Payments) } else { None } }
#     fn key(&self) -> &'static str { "payments" }
# }
# let team = Choice::<Team>::try_new("team", "payments", Some(0.4), Default::default()).unwrap();
if let Some(team) = team.accept(0.7) {
    route_to(team);
} else {
    escalate();
}
# fn route_to(_: Team) {}
# fn escalate() {}
```

## Configure thresholds, don't hard-code them

[`Thresholds`](../crates/jev/src/config.rs) is a named, runtime-tunable map,
loadable from the same YAML as the rest of `Config`:

```yaml
jev:
  thresholds:
    autoRouteConfidence: 0.7
    isBugYes: 0.9
```

```rust
# fn load() -> Result<(), jev::Error> {
let yaml = "thresholds:\n  isBugYes: 0.9\n";
let config: jev::Config = serde_norway::from_str(yaml).unwrap();
let threshold = config.thresholds.require("isBugYes")?;
# Ok(())
# }
```

Every value is validated to be finite and within `0.0..=1.0` when the map is built
or deserialized — a typo that produces `1.9` or `NaN` fails at config-load time,
not silently at decision time. Hot-reloading `Config` (for example wrapped in
`ArcSwap`) is the application's responsibility; the SDK imposes no reload
mechanism of its own.

## Re-tuning across model versions

Thresholds are tuned against one model's calibration. [`Client`](../crates/jev/src/client.rs)
defaults to [`DEFAULT_MODEL`](../crates/jev/src/model.rs) — `Model::Jev113`, pinned,
**not** `Model::Latest` — precisely so that upgrading to a new model snapshot is a
deliberate, reviewable `with_model(Model::Latest)` (or a version bump of the
pinned default in a future SDK release) rather than something that happens
silently underneath a fleet of already-tuned thresholds.
