# The three primitives

Jev answers exactly three kinds of question. Each has a dedicated Rust type with
disjoint accessors, so a `noul` question can never be read as a `choice`, and vice
versa — the compiler enforces the primitive, not a runtime `match`.

## `Noul` — does this condition hold?

```rust
use jev::{Decision, Noul};

/// A support ticket triage decision.
#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a software defect?
    #[jev(yes = "The customer describes broken or unexpected product behavior.",
          no  = "The customer is asking a question or requesting a feature.")]
    is_bug: Noul,
}
```

The field's doc comment becomes `instructions`; `#[jev(yes = ..., no = ...)]` becomes
the optional per-branch criteria (both or neither — one without the other is a
compile error). At runtime:

```rust
# use jev::Noul;
# let is_bug = Noul::try_new(0.92).unwrap();
assert!(is_bug.probability() > 0.5);   // the raw P(yes)
assert!(is_bug.is_yes(0.9));            // P(yes) >= 0.9
assert!(!is_bug.is_no(0.9));            // P(yes) <= 0.1

// Escalate the ambiguous band instead of guessing:
match is_bug.decide(0.9, 0.9) {
    Some(true) => { /* confirmed bug */ }
    Some(false) => { /* confirmed not-a-bug */ }
    None => { /* ambiguous: route to a human */ }
}
```

## `Choice<T>` — which one of these options?

```rust
use jev::{Choice, ChoiceOptions, Decision};

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
    /// Login, permissions, or profile issues.
    Account,
}

#[derive(Debug, Decision)]
struct Triage {
    /// Which team should own this ticket?
    team: Choice<Team>,
}
```

`ChoiceOptions`'s wire key defaults to `snake_case` of the variant name
(`Payments` → `"payments"`), overridable with `#[jev(key = "...")]` per variant or
`#[jev(rename_all = "kebab-case")]` for the whole enum. Criteria come from each
variant's doc comment — required, since `choice` cannot be built without criteria.

```rust
# use jev::{Choice, ChoiceOptions};
# #[derive(Debug, Clone, Copy, PartialEq, Eq)]
# enum Team { Payments, Frontend }
# impl ChoiceOptions for Team {
#     fn options() -> &'static [(&'static str, &'static str)] { &[("payments", "Billing."), ("frontend", "UI.")] }
#     fn from_key(key: &str) -> Option<Self> { match key { "payments" => Some(Team::Payments), "frontend" => Some(Team::Frontend), _ => None } }
#     fn key(&self) -> &'static str { match self { Team::Payments => "payments", Team::Frontend => "frontend" } }
# }
# let team = Choice::<Team>::try_new("team", "payments", Some(0.88), Default::default()).unwrap();
if let Some(team) = team.accept(0.7) {
    println!("route to {team:?}");
} else {
    println!("ambiguous, escalate");
}
```

An option the server returns that isn't in your enum is
[`Error::UnknownChoice`](../crates/jev/src/error.rs) — a named error, not a silent
`match` fallthrough.

## `Score<T>` — where on an ordered scale?

```rust
use jev::{Decision, Score, ScoreLevels};

/// Ordered urgency scale, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Should be fixed this week.
    ThisWeek,
    /// Blocking revenue right now.
    BlockingRevenue,
}

#[derive(Debug, Decision)]
struct Triage {
    /// How urgent is this ticket?
    urgency: Score<Urgency>,
}
```

**Declaration order is the scale, lowest first.** Reordering variants silently
changes the meaning of every threshold tuned against the old order — this is
documented loudly on the derive and worth repeating here.

```rust
# use jev::{Score, ScoreLevels};
# #[derive(Debug, Clone, Copy, PartialEq, Eq)]
# enum Urgency { NextRelease, ThisWeek, BlockingRevenue }
# impl ScoreLevels for Urgency {
#     fn levels() -> &'static [&'static str] { &["Can wait.", "This week.", "Blocking."] }
#     fn from_index(i: usize) -> Option<Self> { match i { 0 => Some(Urgency::NextRelease), 1 => Some(Urgency::ThisWeek), 2 => Some(Urgency::BlockingRevenue), _ => None } }
#     fn index(&self) -> usize { match self { Urgency::NextRelease => 0, Urgency::ThisWeek => 1, Urgency::BlockingRevenue => 2 } }
# }
# let urgency = Score::<Urgency>::try_new("urgency", 1.9, Some(0.8), Default::default()).unwrap();
let level: Urgency = urgency.nearest();       // the nearest discrete level
let raw = urgency.raw();                       // e.g. 1.9 — probability-weighted position
let normalized = urgency.normalized();         // raw() / (levels - 1), in 0.0..=1.0
assert!(urgency.at_least(&Urgency::ThisWeek)); // raw() >= ThisWeek.index()
```
