//! Typed decisions with a uniform API across `OpenAI`, `OpenRouter` and `TypeSafe`.
//!
//! Declare a schema once, then select the backend when constructing the client.
//! Public answers are validated against the same schema on every backend.
//!
//! ```no_run
//! # #[cfg(feature = "derive")]
//! # async fn run(key: String) -> Result<(), decision_rs::Error> {
//! use decision_rs::{Client, Decision, Outcome, Predicate, Probability};
//! #[derive(Debug, Decision)]
//! struct Review {
//!     /// Does this ticket describe a product defect?
//!     is_bug: Predicate,
//!     /// Does the customer explicitly request a refund?
//!     refund: Outcome<Predicate>,
//! }
//! let client = Client::openai(key)?;
//! let review: Review = client.decide("The screen is broken. Please refund me.").await?;
//! let gate = Probability::try_from(0.9)?;
//! if review.is_bug.is_yes(gate) { tracing::info!("route to defect review"); }
//! # Ok(())
//! # }
//! ```
//!
//! `Input::json` turns structured evidence into canonical compact JSON text for
//! every backend. `Outcome` preserves explicit refusals; ordinary required fields
//! return `Error::Refused` with accounting. Missing answers are protocol errors.
//!
//! Probability and native confidence are distinct. `Choice::accept` uses the
//! selected option's probability; `Choice::accept_confidence` uses confidence.
//! Absent statistics stay unavailable; thresholds should be evaluated per model.
//!
//! Default features: `derive`, `retry`, `rustls-tls` (aws-lc-rs). Optional `batch`
//! enables bounded execution and `yaml` enables bounded configuration/case loading.
//! Deadlines and concurrency limits remain active with retry disabled.

#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations, rust_2024_compatibility)]

mod answer;
mod backend;
#[cfg(feature = "batch")]
mod batch;
mod cases;
mod client;
mod config;
mod error;
mod input;
mod json;
mod metadata;
mod model;
mod provider;
mod question;
mod transport;
mod typed;
#[cfg(feature = "yaml")]
mod yaml;

pub use answer::{Answer, Answers, ChoiceAnswer, Distribution, ScoreAnswer};
#[cfg(feature = "batch")]
pub use batch::{decide_all, run_cases};
pub use cases::{Case, CaseResult, CaseSet};
pub use client::{Client, ClientBuilder, Decided, RequestOptions};
pub use config::{Config, Limits, RetryConfig, Settings, Thresholds};
#[cfg(feature = "derive")]
pub use decision_macros::{ChoiceOptions, Decision, ScoreLevels};
pub use error::{DecodeError, Error, SchemaError};
pub use input::{Input, IntoInput};
pub use metadata::{DecisionResponse, ResponseMeta, Usage};
pub use model::{Model, ModelId};
pub use provider::Provider;
pub use question::{GuidanceText, LevelSpec, Question, Questions, QuestionsBuilder};
pub use transport::{Auth, Endpoint, HttpClient};
pub use typed::{
    Choice, ChoiceOptions, Decision, FromAnswer, Outcome, Predicate, Probability, Refusal, Score,
    ScoreLevels, Verdict,
};
