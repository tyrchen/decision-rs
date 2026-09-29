//! Predefined "cases" — a state plus one or more questions, loaded from YAML/JSON
//! configuration instead of assembled in code.
//!
//! A [`CaseSet`] is the config-file counterpart of [`Questions::builder`]: where the
//! dynamic path builds one [`Questions`] value per call in Rust, a case set is a whole
//! batch of named `(state, questions)` pairs — for example a fixture file of support
//! tickets with the questions to ask about each — deserialized once and replayed
//! through [`crate::Client::run_case`] (one call) or [`crate::run_cases`] (many, bounded
//! concurrency).
//!
//! # File format
//!
//! ```yaml
//! version: 1
//! cases:
//!   - id: ticket-001
//!     state: "Customer wants a refund for a duplicate charge."
//!     questions:
//!       refund:
//!         type: noul
//!         instructions: "Is the customer asking for money back?"
//!       team:
//!         type: choice
//!         instructions: "Which team should own this ticket?"
//!         criteria:
//!           payments: "Checkout, billing, or payment processing issues."
//!           frontend: "Rendering, layout, or browser compatibility issues."
//!   - id: ticket-002
//!     name: "Login outage report"
//!     state:
//!       subject: "Login broken"
//!       body: "Cannot log in after the last release."
//!     questions:
//!       urgency:
//!         type: score
//!         instructions: "How urgent is this?"
//!         criteria: ["Can wait", "This week", "Blocking revenue"]
//! ```
//!
//! `state` accepts the same `string | object | array` shapes as [`crate::State`];
//! each question accepts the same `type`-tagged shapes as [`crate::Question`] (`noul`
//! with optional `criteria.true`/`criteria.false`, `choice` with a `criteria` map,
//! `score` with a `criteria` list), keyed by question key, in file order. Loading a
//! `CaseSet` (via [`CaseSet::from_json_str`], [`CaseSet::from_yaml_str`], or
//! [`CaseSet::from_file`]) validates every case's questions eagerly through
//! [`Questions::builder`]'s checks — key charset, guidance length, option/level counts
//! — so a malformed fixture fails at load time, not mid-batch.
//!
//! # Examples
//!
//! ```
//! use jev::CaseSet;
//!
//! let json = r#"{
//!     "cases": [{
//!         "id": "ticket-001",
//!         "state": "Customer wants a refund for a duplicate charge.",
//!         "questions": {
//!             "refund": {
//!                 "type": "noul",
//!                 "instructions": "Is the customer asking for money back?"
//!             }
//!         }
//!     }]
//! }"#;
//! let cases = CaseSet::from_json_str(json).unwrap();
//! assert_eq!(cases.len(), 1);
//! assert_eq!(cases.iter().next().unwrap().id, "ticket-001");
//! ```

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Deserialize;

use crate::{
    error::Error,
    guidance::Guidance,
    question::{Questions, QuestionsBuilder},
    state::State,
    wire::Response,
};

/// Minimum number of cases in one [`CaseSet`].
pub const MIN_CASES: usize = 1;
/// Maximum number of cases in one [`CaseSet`].
pub const MAX_CASES: usize = 10_000;
/// Maximum length, in bytes, of a case's `id` or `name`.
pub const MAX_CASE_ID_BYTES: usize = 256;

/// One configured question inside a [`Case`]: the same `noul`/`choice`/`score` shape
/// [`crate::Question`] sends on the wire, deserialized from a case file instead of
/// built through [`QuestionsBuilder`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaseQuestion {
    /// A yes/no question. See [`crate::Question::Noul`].
    Noul {
        /// What to ask the model.
        instructions: Guidance,
        /// Optional per-branch criteria clarifying what counts as "true" or "false".
        #[serde(default)]
        criteria: Option<CaseNoulCriteria>,
    },
    /// A single-select question over a fixed, ordered option set. See
    /// [`crate::Question::Choice`].
    Choice {
        /// What to ask the model.
        instructions: Guidance,
        /// Wire key -> criteria, in declaration order.
        criteria: IndexMap<String, Guidance>,
    },
    /// A question that places the state on an ordered scale. See
    /// [`crate::Question::Score`].
    Score {
        /// What to ask the model.
        instructions: Guidance,
        /// One criterion per level, lowest first.
        criteria: Vec<Guidance>,
    },
}

/// The `"true"`/`"false"` criteria pair for a [`CaseQuestion::Noul`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CaseNoulCriteria {
    /// Criterion describing what counts as "yes".
    #[serde(rename = "true")]
    pub r#true: Guidance,
    /// Criterion describing what counts as "no".
    #[serde(rename = "false")]
    pub r#false: Guidance,
}

/// One predefined case: one state, one or more questions, one API call.
///
/// Construct these by deserializing a [`CaseSet`], which validates `questions` eagerly
/// via [`Case::build_questions`]; there is no public unchecked constructor.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Case {
    /// Caller-defined identifier for this case, echoed back on [`CaseResult`] for log
    /// correlation and report keying.
    pub id: String,
    /// Optional human-readable label, distinct from `id` (dashboards, reports).
    #[serde(default)]
    pub name: Option<String>,
    /// The state this case's questions are evaluated against.
    pub state: State,
    /// Question key -> question, in file order.
    pub questions: IndexMap<String, CaseQuestion>,
}

impl Case {
    /// Converts this case's `questions` into a validated [`Questions`], running the
    /// same checks [`QuestionsBuilder::build`] applies to the hand-written path.
    ///
    /// This does not itself validate `id`/`name`; [`CaseSet::from_json_str`] and
    /// friends do so for every case as part of loading the whole file.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] on the first invariant violated: question count,
    /// duplicate/invalid keys, guidance length, or option/level counts.
    pub fn build_questions(&self) -> Result<Questions, Error> {
        let mut builder = Questions::builder();
        for (key, question) in &self.questions {
            builder = push_question(builder, key, question);
        }
        builder.build()
    }
}

/// Appends one [`CaseQuestion`] to a [`QuestionsBuilder`] under `key`.
fn push_question(
    builder: QuestionsBuilder,
    key: &str,
    question: &CaseQuestion,
) -> QuestionsBuilder {
    match question {
        CaseQuestion::Noul {
            instructions,
            criteria: None,
        } => builder.noul(key, instructions.clone()),
        CaseQuestion::Noul {
            instructions,
            criteria: Some(criteria),
        } => builder.noul_with(
            key,
            instructions.clone(),
            criteria.r#true.clone(),
            criteria.r#false.clone(),
        ),
        CaseQuestion::Choice {
            instructions,
            criteria,
        } => builder.choice(
            key,
            instructions.clone(),
            criteria.iter().map(|(k, v)| (k.clone(), v.clone())),
        ),
        CaseQuestion::Score {
            instructions,
            criteria,
        } => builder.score(key, instructions.clone(), criteria.clone()),
    }
}

/// A batch of [`Case`]s loaded from one YAML/JSON document: the config-file
/// counterpart of hand-assembling many [`Questions`] values in code.
///
/// See the module-level docs for the file format.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaseSet {
    /// Format version of this file, for callers that need to evolve the schema. Not
    /// otherwise interpreted by this SDK.
    #[serde(default)]
    pub version: Option<u32>,
    /// The cases, in file order.
    pub cases: Vec<Case>,
}

impl CaseSet {
    /// Parses a `CaseSet` from a JSON document.
    ///
    /// # Errors
    ///
    /// [`Error::Parse`] if `json` is not valid JSON; [`Error::Invalid`] if it parses
    /// but fails case-set validation (see the module-level docs).
    pub fn from_json_str(json: &str) -> Result<Self, Error> {
        let case_set: CaseSet =
            serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;
        validate(case_set)
    }

    /// Parses a `CaseSet` from an already-parsed JSON value.
    ///
    /// # Errors
    ///
    /// [`Error::Parse`] if `value` does not match the case-set schema; [`Error::Invalid`]
    /// if it parses but fails case-set validation.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, Error> {
        let case_set: CaseSet =
            serde_json::from_value(value).map_err(|e| Error::Parse(e.to_string()))?;
        validate(case_set)
    }

    /// Parses a `CaseSet` from a YAML document.
    ///
    /// # Errors
    ///
    /// [`Error::Parse`] if `yaml` is not valid YAML; [`Error::Invalid`] if it parses
    /// but fails case-set validation.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(yaml: &str) -> Result<Self, Error> {
        let case_set: CaseSet =
            serde_norway::from_str(yaml).map_err(|e| Error::Parse(e.to_string()))?;
        validate(case_set)
    }

    /// Loads a `CaseSet` from a file, choosing the JSON or YAML parser by extension
    /// (`.json` vs. `.yaml`/`.yml`).
    ///
    /// Reads the file with `tokio::fs`, not `std::fs`, so it never blocks the async
    /// runtime's worker thread.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be read; [`Error::Invalid`] if the extension is
    /// missing, unrecognized, or names a YAML file while the `yaml` feature is
    /// disabled, or if the parsed content fails case-set validation; [`Error::Parse`]
    /// if the content is not well-formed JSON/YAML.
    pub async fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let extension = path.extension().and_then(std::ffi::OsStr::to_str);
        // Resolve which parser to use, or the `Invalid` rejection, before touching the
        // filesystem: an unrecognized extension is a caller programming error, not an
        // I/O condition, and should not depend on whether the (wrong) file happens to
        // exist.
        let parser = match extension {
            Some("json") => Self::from_json_str,
            #[cfg(feature = "yaml")]
            Some("yaml" | "yml") => Self::from_yaml_str,
            #[cfg(not(feature = "yaml"))]
            Some("yaml" | "yml") => {
                return Err(Error::Invalid {
                    field: "path".to_string(),
                    reason: format!(
                        "{} is a YAML file but the `yaml` feature is disabled",
                        path.display()
                    ),
                });
            }
            _ => {
                return Err(Error::Invalid {
                    field: "path".to_string(),
                    reason: format!(
                        "unrecognized case file extension {extension:?}; expected json, yaml, or \
                         yml"
                    ),
                });
            }
        };

        let contents = tokio::fs::read_to_string(path).await?;
        parser(&contents)
    }

    /// The number of cases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// Whether there are no cases.
    ///
    /// Never `true` for a successfully loaded `CaseSet`: loading rejects an empty
    /// `cases` list (see [`MIN_CASES`]). Provided for API symmetry with
    /// [`Vec::is_empty`] and to satisfy `clippy::len_without_is_empty`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// Iterates cases in file order.
    pub fn iter(&self) -> std::slice::Iter<'_, Case> {
        self.cases.iter()
    }

    /// Borrows the cases as a slice, for [`crate::run_cases`].
    #[must_use]
    pub fn as_slice(&self) -> &[Case] {
        &self.cases
    }
}

impl<'a> IntoIterator for &'a CaseSet {
    type Item = &'a Case;
    type IntoIter = std::slice::Iter<'a, Case>;

    fn into_iter(self) -> Self::IntoIter {
        self.cases.iter()
    }
}

/// Validates a freshly deserialized `CaseSet`: case count, `id`/`name` bounds, `id`
/// uniqueness, and (via [`Case::build_questions`]) every case's question schema.
fn validate(case_set: CaseSet) -> Result<CaseSet, Error> {
    let count = case_set.cases.len();
    if !(MIN_CASES..=MAX_CASES).contains(&count) {
        return Err(Error::Invalid {
            field: "cases".to_string(),
            reason: format!("expected {MIN_CASES}..={MAX_CASES} cases, got {count}"),
        });
    }

    let mut seen_ids: HashSet<&str> = HashSet::with_capacity(count);
    for case in &case_set.cases {
        validate_id_field("case.id", &case.id)?;
        if let Some(name) = &case.name {
            validate_id_field("case.name", name)?;
        }
        if !seen_ids.insert(case.id.as_str()) {
            return Err(Error::Invalid {
                field: "cases".to_string(),
                reason: format!("duplicate case id `{}`", case.id),
            });
        }
        case.build_questions()?;
    }

    Ok(case_set)
}

/// Validates that a case's `id` or `name` is non-empty and within
/// [`MAX_CASE_ID_BYTES`].
fn validate_id_field(field: &str, value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > MAX_CASE_ID_BYTES {
        return Err(Error::Invalid {
            field: field.to_string(),
            reason: format!(
                "length must be 1..={MAX_CASE_ID_BYTES} bytes, got {}",
                value.len()
            ),
        });
    }
    Ok(())
}

/// The result of running one [`Case`] through [`crate::Client::run_case`]: the case's
/// `id` alongside the raw [`Response`].
#[derive(Debug)]
pub struct CaseResult {
    /// The case's `id`, for correlating this result back to its source case.
    pub id: String,
    /// The server's response: answers, usage, and identifiers.
    pub response: Response,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_parse_minimal_case_set_from_json() {
        let cases = CaseSet::from_json_value(json!({
            "cases": [{
                "id": "t1",
                "state": "hello",
                "questions": {
                    "flag": {"type": "noul", "instructions": "Is this flagged?"}
                }
            }]
        }))
        .unwrap();
        assert_eq!(cases.len(), 1);
        let case = cases.iter().next().unwrap();
        assert_eq!(case.id, "t1");
        assert_eq!(case.state, State::Text("hello".to_string()));
        assert_eq!(case.build_questions().unwrap().len(), 1);
    }

    #[test]
    fn test_should_parse_case_with_structured_state_and_all_primitives() {
        let cases = CaseSet::from_json_value(json!({
            "cases": [{
                "id": "t1",
                "state": {"subject": "billing"},
                "questions": {
                    "refund": {"type": "noul", "instructions": "Refund?",
                               "criteria": {"true": "Yes text.", "false": "No text."}},
                    "team": {"type": "choice", "instructions": "Which team?",
                              "criteria": {"a": "A.", "b": "B."}},
                    "urgency": {"type": "score", "instructions": "How urgent?",
                                "criteria": ["Low", "High"]}
                }
            }]
        }))
        .unwrap();
        let case = &cases.iter().next().unwrap();
        let questions = case.build_questions().unwrap();
        assert_eq!(questions.len(), 3);
    }

    #[test]
    fn test_should_reject_empty_case_list() {
        let err = CaseSet::from_json_value(json!({"cases": []})).unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_duplicate_case_ids() {
        let err = CaseSet::from_json_value(json!({
            "cases": [
                {"id": "dup", "state": "a", "questions": {"q": {"type": "noul", "instructions": "Q?"}}},
                {"id": "dup", "state": "b", "questions": {"q": {"type": "noul", "instructions": "Q?"}}}
            ]
        }))
        .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_empty_case_id() {
        let err = CaseSet::from_json_value(json!({
            "cases": [{"id": "", "state": "a", "questions": {"q": {"type": "noul", "instructions": "Q?"}}}]
        }))
        .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_case_with_invalid_question_schema() {
        // Question key uses an uppercase letter, invalid per `Questions::builder`'s charset.
        let err = CaseSet::from_json_value(json!({
            "cases": [{"id": "t1", "state": "a", "questions": {"Q": {"type": "noul", "instructions": "Q?"}}}]
        }))
        .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_unknown_top_level_field() {
        let err = serde_json::from_value::<CaseSet>(json!({
            "cases": [],
            "bogus": true
        }))
        .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn test_should_reject_json_naming_unsupported_extension() {
        let err = CaseSet::from_file("cases.toml").await.unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_should_return_io_error_for_missing_file() {
        let err = CaseSet::from_file("/no/such/path/cases.json")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Io(_)));
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn test_should_parse_case_set_from_yaml() {
        let yaml = r"
cases:
  - id: t1
    state: hello
    questions:
      flag:
        type: noul
        instructions: Is this flagged?
";
        let cases = CaseSet::from_yaml_str(yaml).unwrap();
        assert_eq!(cases.len(), 1);
    }
}
