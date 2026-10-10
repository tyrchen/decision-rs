//! Proc-macros for the `decision_rs` SDK.
//!
//! This crate is an implementation detail re-exported through `decision_rs`'s `derive`
//! feature; callers should depend on `decision-rs`, not on `decision-macros` directly. See
//! `decision_rs`'s crate-level docs for the full derive rules and examples.

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

mod decision;
mod docs;
mod options;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Derives `impl decision_rs::Decision` for a struct whose fields are `Predicate`, `Choice<T>`,
/// `Score<T>`, or `Outcome<_>` of those.
///
/// - **Question key**: the field name (`snake_case`), or `#[decision(key = "...")]`.
/// - **Instructions**: the field's doc comment, or `#[decision(instructions = "...")]`. Missing
///   both is a compile error.
/// - **Predicate criteria**: optional `#[decision(yes = "...", no = "...")]`; one without the other
///   is a compile error.
/// - **Choice / Score criteria**: come from `T::options()` / `T::levels()`.
/// - `Outcome<_>` retains explicit refusals; missing answers are always errors.
///
/// See `decision_rs`'s crate-level documentation for a full worked example.
#[proc_macro_derive(Decision, attributes(decision))]
pub fn derive_decision(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    decision::expand_decision(&input).into()
}

/// Derives `impl decision_rs::ChoiceOptions` for a fieldless enum.
///
/// - **Wire key**: `snake_case` of the variant, overridable per variant with `#[decision(key =
///   "...")]` and for the whole enum with `#[decision(rename_all = "snake_case" | "kebab-case" |
///   "lowercase")]`.
/// - **Criteria**: the variant's doc comment; missing is a compile error.
/// - Rejected at compile time: data-carrying variants, fewer than two variants, more than 64
///   variants, and duplicate keys.
#[proc_macro_derive(ChoiceOptions, attributes(decision))]
pub fn derive_choice_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    options::expand_choice_options(&input).into()
}

/// Derives `impl decision_rs::ScoreLevels` for a fieldless enum.
///
/// The level text comes from each variant's doc comment; the index is the
/// declaration position. **Declaration order is the scale, lowest first**:
/// reordering variants silently changes the meaning of every tuned threshold.
/// Rejected at compile time: data-carrying variants, fewer than two variants, and
/// more than 32 variants or duplicate stable keys. Keys follow the same rules as choices.
#[proc_macro_derive(ScoreLevels, attributes(decision))]
pub fn derive_score_levels(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    options::expand_score_levels(&input).into()
}
