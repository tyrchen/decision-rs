# Migrating from prompt-and-parse classification

## The pattern this replaces

A common way to get a yes/no or multiple-choice answer out of a chat model:

```rust,ignore
// Before: a hand-built prompt, a chat completion, and string matching.
let prompt = format!(
    "Answer with exactly one of BUG, QUESTION, or FEATURE.\n\nTicket:\n{ticket}"
);
let completion = chat_client.complete(&prompt).await?;
let answer = completion.trim().to_uppercase();
let category = match answer.as_str() {
    "BUG" => Category::Bug,
    "QUESTION" => Category::Question,
    "FEATURE" => Category::Feature,
    _ => Category::Bug, // <-- silent, wrong default on any parse miss
};
```

Four standing problems with this, in order of how often they bite in production:

1. **Parse failures are silent or late.** The model answers "This looks like a
   bug, though it could be a feature request." and the `match` falls through to a
   default that happens to be wrong half the time.
2. **No calibrated uncertainty.** A chat completion carries no probability, so
   there's no principled way to say "escalate this one, it's ambiguous."
3. **Cost and latency.** A reasoning-capable chat model is expensive for a
   yes/no question; Jev bills input tokens only.
4. **Prompt drift.** The option list lives in a format string, the enum lives in
   Rust code, and nothing keeps the two in sync — add a `Category::Duplicate`
   variant and the prompt silently stops mentioning it.

## The recipe

### 1. Turn the option list into an enum with `#[derive(ChoiceOptions)]`

```rust
use jev::ChoiceOptions;

/// Ticket category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Category {
    /// The customer describes broken or unexpected product behavior.
    Bug,
    /// The customer is asking how to do something that already works.
    Question,
    /// The customer wants capability the product does not have yet.
    Feature,
}
```

The doc comment on each variant *is* the criteria sent to the model — the same
text a careful prompt author would have written in the format string, now living
next to the enum it describes, checked by `cargo doc`, and impossible to drift
out of sync with the variant list (add `Duplicate`, the criteria list grows with
it; delete a variant, the criteria list shrinks — there is only one list).

### 2. Turn the prompt into a struct field with `#[derive(Decision)]`

```rust
use jev::{Choice, Decision};
# use jev::ChoiceOptions;
# #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
# enum Category {
#     /// The customer describes broken or unexpected product behavior.
#     Bug,
#     /// The customer is asking how to do something that already works.
#     Question,
#     /// The customer wants capability the product does not have yet.
#     Feature,
# }

/// A support ticket classification.
#[derive(Debug, Decision)]
struct Classification {
    /// What kind of request is this ticket?
    category: Choice<Category>,
}
```

### 3. Replace the completion call with `decide`, and the `match` with `accept`

```rust,no_run
# use jev::{Choice, ChoiceOptions, Client, Decision};
# #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
# enum Category {
#     /// bug
#     Bug,
#     /// question
#     Question,
#     /// feature
#     Feature,
# }
# #[derive(Debug, Decision)]
# struct Classification { category: Choice<Category> }
# async fn run(ticket: &str) -> Result<(), jev::Error> {
let client = Client::from_env()?;
let classification: Classification = client.decide(ticket).await?;

match classification.category.accept(0.7) {
    Some(Category::Bug) => route_to_engineering(),
    Some(Category::Question) => route_to_support(),
    Some(Category::Feature) => route_to_product(),
    None => escalate_to_human(), // <-- explicit, not a silent wrong default
}
# Ok(())
# }
# fn route_to_engineering() {}
# fn route_to_support() {}
# fn route_to_product() {}
# fn escalate_to_human() {}
```

## What you get

| Before | After |
|---|---|
| String-match on a completion | `Choice<Category>` — an enum, checked by the compiler |
| Silent wrong default on parse miss | `accept(threshold)` returns `None`, forcing a decision |
| No uncertainty signal | `confidence()` / `probability_of()` on every answer |
| Prompt and enum can drift | Criteria generated *from* the enum's doc comments |
| A whole chat completion, billed | One `noul`/`choice`/`score` question, input tokens only |

See [primitives.md](./primitives.md) for `Noul` and `Score<T>` — the same recipe
applies to yes/no gates and ordered scales, not just multiple-choice.
