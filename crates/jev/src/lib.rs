//! # jev
//!
//! A typed Rust SDK for the [Jev](https://openrouter.ai/docs/guides/community/jev)
//! decision model — a *System One* model that answers typed questions about a state
//! you send it, with calibrated probabilities, instead of prose.
//!
//! Three primitives, three questions:
//!
//! | Primitive | Question | Answer shape |
//! |-----------|----------|--------------|
//! | [`Noul`] | Does this condition hold? | `P(yes)` |
//! | [`Choice<T>`] | Which one of these options? | an enum variant, plus confidence |
//! | [`Score<T>`] | Where on an ordered scale? | a continuous position, plus the nearest level |
//!
//! ## The typed path
//!
//! One Rust type is one request and one answer struct. `///` doc comments on struct
//! fields become `instructions`; `///` on enum variants become that option's
//! criteria — there is exactly one copy of the prompt, and `cargo doc` shows a
//! reviewer precisely what the model is asked.
//!
//! ```no_run
//! # #[cfg(feature = "derive")]
//! # async fn run() -> Result<(), jev::Error> {
//! use jev::{Choice, ChoiceOptions, Client, Decision, Noul, Score, ScoreLevels};
//!
//! /// A support ticket triage decision.
//! #[derive(Debug, Decision)]
//! struct Triage {
//!     /// Is the customer reporting a software defect?
//!     #[jev(yes = "The customer describes broken or unexpected product behavior.",
//!           no  = "The customer is asking a question or requesting a feature.")]
//!     is_bug: Noul,
//!
//!     /// Which team should own this ticket?
//!     team: Choice<Team>,
//!
//!     /// How urgent is this ticket?
//!     urgency: Score<Urgency>,
//! }
//!
//! /// Owning team for a ticket.
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
//! enum Team {
//!     /// Checkout, billing, or payment processing issues.
//!     Payments,
//!     /// Rendering, layout, or browser compatibility issues.
//!     Frontend,
//!     /// Login, permissions, or profile issues.
//!     Account,
//! }
//!
//! /// Ordered urgency scale, lowest first.
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
//! enum Urgency {
//!     /// Can wait for the next release.
//!     NextRelease,
//!     /// Should be fixed this week.
//!     ThisWeek,
//!     /// Blocking revenue right now.
//!     BlockingRevenue,
//! }
//!
//! let client = Client::from_env()?;
//! let triage: Triage = client.decide("a customer ticket about a checkout error").await?;
//!
//! if triage.is_bug.is_yes(0.9) {
//!     if let Some(team) = triage.team.accept(0.7) {
//!         println!("route to {team:?}");
//!     }
//! }
//! let urgency: Urgency = triage.urgency.nearest();
//! println!("{urgency:?}");
//! # Ok(())
//! # }
//! ```
//!
//! ## Predefined cases from YAML/JSON
//!
//! For a fixed batch of `(state, questions)` pairs — a fixture file of support
//! tickets, a regression suite of prior decisions — [`CaseSet`] loads them from one
//! YAML/JSON document instead of assembling each [`Questions`] value in code:
//!
//! ```
//! use jev::CaseSet;
//!
//! let json = r#"{"cases": [{
//!     "id": "ticket-001",
//!     "state": "Customer wants a refund for a duplicate charge.",
//!     "questions": {
//!         "refund": {"type": "noul", "instructions": "Is the customer asking for money back?"}
//!     }
//! }]}"#;
//! let cases = CaseSet::from_json_str(json).unwrap();
//! # let _ = cases;
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "yaml")]
//! # async fn run() -> Result<(), jev::Error> {
//! use jev::{CaseSet, Client};
//!
//! let cases = CaseSet::from_file("cases.yaml").await?;
//! let client = Client::from_env()?;
//! for case in &cases {
//!     let result = client.run_case(case).await?;
//!     println!("{}: {:?}", result.id, result.response.answers);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! See [`CaseSet`]'s docs for the file format, and [`run_cases`] (behind the `batch`
//! feature) for running the whole set with bounded concurrency.
//!
//! ## The dynamic path
//!
//! For question sets assembled at runtime from configuration, [`Questions::builder`]
//! is the same wire logic the derive uses, minus the compile-time checks:
//!
//! ```
//! use jev::Questions;
//!
//! let questions = Questions::builder()
//!     .noul("refund", "Is the customer asking for money back?")
//!     .choice("team", "Which team should own this?", [("payments", "Billing."), ("frontend", "UI.")])
//!     .build()
//!     .unwrap();
//! ```
//!
//! ## Choosing thresholds
//!
//! `confidence` is a *calibration* signal, not accuracy: a `Choice<T>` with
//! `confidence = 0.95` means the model is well-calibrated to be right about 95% of
//! the time on inputs like this one, assuming the model's calibration holds for your
//! distribution of states. The SDK never picks a threshold for you — see [`Noul::decide`],
//! [`Choice::accept`], and [`crate::Thresholds`] — because the right threshold trades
//! off automation rate against error tolerance, which is a product decision, not an
//! SDK one. Escalate the ambiguous band (between `is_yes`/`is_no`, or below
//! `accept`'s threshold) to a human reviewer rather than picking a default that
//! silently mispredicts on the cases the model itself is uncertain about.
//!
//! ## Feature flags
//!
//! | Feature | Default | Effect |
//! |---|---|---|
//! | `derive` | yes | Re-exports the `Decision`/`ChoiceOptions`/`ScoreLevels` derives. |
//! | `retry` | yes | Bounded retry loop honouring `Retry-After`. |
//! | `rustls-tls` | yes | `reqwest` over `rustls` with the `aws-lc-rs` backend. |
//! | `native-tls` | no | `reqwest` over the platform TLS stack. |
//! | `batch` | no | [`decide_all`] and [`run_cases`], bounded-concurrency helpers. |
//! | `yaml` | no | [`CaseSet::from_yaml_str`] / [`CaseSet::from_file`] for `.yaml`/`.yml`. |

#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]
#![warn(rust_2024_compatibility)]

mod answer;
#[cfg(feature = "batch")]
mod batch;
mod cases;
mod client;
mod config;
mod error;
mod guidance;
mod model;
mod question;
mod state;
mod typed;
mod util;
mod wire;

pub use answer::{Answer, Answers, ChoiceAnswer, NoulAnswer, ScoreAnswer};
#[cfg(feature = "batch")]
pub use batch::{decide_all, run_cases};
pub use cases::{Case, CaseNoulCriteria, CaseQuestion, CaseResult, CaseSet};
pub use client::{Client, Decided, RequestOptions};
pub use config::{Config, RetryConfig, Surface, Thresholds};
pub use error::Error;
pub use guidance::Guidance;
#[cfg(feature = "derive")]
pub use jev_macros::{ChoiceOptions, Decision, ScoreLevels};
pub use model::{DEFAULT_MODEL, Model};
pub use question::{NoulCriteria, Question, Questions, QuestionsBuilder};
pub use state::State;
pub use typed::{Choice, ChoiceOptions, Decision, Noul, Score, ScoreLevels, Verdict};
pub use wire::{Response, ResponseMeta, TraceConfig, Usage};
