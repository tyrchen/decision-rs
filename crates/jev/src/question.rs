//! `Question`, `Questions`, and the validating builder.

use indexmap::IndexMap;
use serde::Serialize;

use crate::{error::Error, guidance::Guidance};

/// Minimum number of questions in a request.
pub const MIN_QUESTIONS: usize = 1;
/// Maximum number of questions in a request.
pub const MAX_QUESTIONS: usize = 64;
/// Maximum length, in bytes, of a question or option key.
pub const MAX_KEY_BYTES: usize = 64;
/// Maximum length, in bytes, of a single guidance string (`instructions` or one
/// criterion).
pub const MAX_GUIDANCE_BYTES: usize = 4096;
/// Minimum number of options in a `choice` question.
pub const MIN_CHOICE_OPTIONS: usize = 2;
/// Maximum number of options in a `choice` question.
pub const MAX_CHOICE_OPTIONS: usize = 64;
/// Minimum number of levels in a `score` question.
pub const MIN_SCORE_LEVELS: usize = 2;
/// Maximum number of levels in a `score` question.
pub const MAX_SCORE_LEVELS: usize = 32;

/// One question sent to the model: `noul`, `choice`, or `score`.
///
/// See the crate-level documentation for the three primitives. Construct these
/// through [`Questions::builder`] (validated) or `#[derive(Decision)]` (validated at
/// compile time); there is no public unchecked constructor.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// A yes/no question. `criteria` is optional: the schema requires only `type` and
    /// `instructions`.
    Noul {
        /// What to ask the model.
        instructions: Guidance,
        /// Optional per-branch criteria clarifying what counts as "true" or "false".
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// A single-select question over a fixed, ordered option set.
    Choice {
        /// What to ask the model.
        instructions: Guidance,
        /// Wire key -> criteria, in declaration order.
        criteria: IndexMap<String, Guidance>,
    },
    /// A question that places the state on an ordered scale.
    Score {
        /// What to ask the model.
        instructions: Guidance,
        /// One criterion per level, lowest first.
        criteria: Vec<Guidance>,
    },
}

impl Question {
    /// The question's `instructions` guidance, common to all three primitives.
    #[must_use]
    pub fn instructions(&self) -> &Guidance {
        match self {
            Question::Noul { instructions, .. }
            | Question::Choice { instructions, .. }
            | Question::Score { instructions, .. } => instructions,
        }
    }

    /// The wire name of this question's primitive: `"noul"`, `"choice"`, or `"score"`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Question::Noul { .. } => "noul",
            Question::Choice { .. } => "choice",
            Question::Score { .. } => "score",
        }
    }
}

/// The `"true"`/`"false"` criteria pair for a [`Question::Noul`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoulCriteria {
    /// Criterion describing what counts as "yes".
    #[serde(rename = "true")]
    pub r#true: Guidance,
    /// Criterion describing what counts as "no".
    #[serde(rename = "false")]
    pub r#false: Guidance,
}

/// An ordered map of question key to [`Question`].
///
/// Ordering is preserved (backed by [`indexmap::IndexMap`]) so request payloads are
/// stable and byte-comparable in tests, and so score legends and probabilities read
/// back in their natural order.
///
/// # Examples
///
/// ```
/// use jev::Questions;
///
/// let questions = Questions::builder()
///     .noul("refund", "Is the customer asking for money back?")
///     .build()
///     .unwrap();
/// assert_eq!(questions.len(), 1);
/// ```
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Questions(IndexMap<String, Question>);

impl Questions {
    /// Starts a validating builder.
    #[must_use]
    pub fn builder() -> QuestionsBuilder {
        QuestionsBuilder::default()
    }

    /// The number of questions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no questions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Looks up a question by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Question> {
        self.0.get(key)
    }

    /// Iterates questions in declaration order.
    #[must_use]
    pub fn iter(&self) -> indexmap::map::Iter<'_, String, Question> {
        self.0.iter()
    }

    /// Builds a `Questions` from already-validated entries without re-running the
    /// [`QuestionsBuilder::build`] checks.
    ///
    /// This exists solely for `#[derive(Decision)]`, which performs the equivalent
    /// validation at compile time (key charset, guidance non-emptiness, duplicate
    /// keys) using `syn`/`quote` over literal strings, so re-validating at runtime
    /// on every process start would be pure overhead. It is not part of the public
    /// API surface for hand-written callers, who should use [`Questions::builder`].
    #[doc(hidden)]
    #[must_use]
    pub fn from_validated_entries(entries: Vec<(String, Question)>) -> Self {
        Questions(entries.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a Questions {
    type Item = (&'a String, &'a Question);
    type IntoIter = indexmap::map::Iter<'a, String, Question>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Validating builder for [`Questions`], the config-driven / dynamic-question path.
///
/// Every terminal invariant documented on [`Questions`] is enforced in [`Self::build`],
/// not on each call, so calls can be chained freely before the single fallible step.
///
/// # Examples
///
/// ```
/// use jev::Questions;
///
/// let questions = Questions::builder()
///     .noul("refund", "Is the customer asking for money back?")
///     .noul_with(
///         "is_bug",
///         "Is the customer reporting a defect?",
///         "Describes broken behavior.",
///         "Asks a question or requests a feature.",
///     )
///     .choice(
///         "team",
///         "Which team should own this?",
///         [
///             ("payments", "Checkout, billing, or payments."),
///             ("frontend", "Rendering or browser issues."),
///         ],
///     )
///     .score("urgency", "How urgent is this?", ["Can wait", "This week", "Blocking revenue"])
///     .build()
///     .unwrap();
/// assert_eq!(questions.len(), 4);
/// ```
#[derive(Debug, Default)]
pub struct QuestionsBuilder {
    entries: Vec<(String, Question)>,
}

impl QuestionsBuilder {
    /// Adds a `noul` question with no explicit branch criteria.
    #[must_use]
    pub fn noul(mut self, key: impl Into<String>, instructions: impl Into<Guidance>) -> Self {
        self.entries.push((
            key.into(),
            Question::Noul {
                instructions: instructions.into(),
                criteria: None,
            },
        ));
        self
    }

    /// Adds a `noul` question with explicit "yes" and "no" criteria.
    #[must_use]
    pub fn noul_with(
        mut self,
        key: impl Into<String>,
        instructions: impl Into<Guidance>,
        yes: impl Into<Guidance>,
        no: impl Into<Guidance>,
    ) -> Self {
        self.entries.push((
            key.into(),
            Question::Noul {
                instructions: instructions.into(),
                criteria: Some(NoulCriteria {
                    r#true: yes.into(),
                    r#false: no.into(),
                }),
            },
        ));
        self
    }

    /// Adds a `choice` question over the given `(option_key, criteria)` pairs, in order.
    #[must_use]
    pub fn choice<K, G>(
        mut self,
        key: impl Into<String>,
        instructions: impl Into<Guidance>,
        options: impl IntoIterator<Item = (K, G)>,
    ) -> Self
    where
        K: Into<String>,
        G: Into<Guidance>,
    {
        let criteria: IndexMap<String, Guidance> = options
            .into_iter()
            .map(|(k, g)| (k.into(), g.into()))
            .collect();
        self.entries.push((
            key.into(),
            Question::Choice {
                instructions: instructions.into(),
                criteria,
            },
        ));
        self
    }

    /// Adds a `score` question over the given levels, lowest first.
    #[must_use]
    pub fn score<G: Into<Guidance>>(
        mut self,
        key: impl Into<String>,
        instructions: impl Into<Guidance>,
        levels: impl IntoIterator<Item = G>,
    ) -> Self {
        let criteria: Vec<Guidance> = levels.into_iter().map(Into::into).collect();
        self.entries.push((
            key.into(),
            Question::Score {
                instructions: instructions.into(),
                criteria,
            },
        ));
        self
    }

    /// Validates every accumulated question and produces a [`Questions`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] on the first invariant violated, in declaration
    /// order: question count, duplicate keys, key charset, guidance length/emptiness,
    /// `choice` option count/charset, and `score` level count.
    pub fn build(self) -> Result<Questions, Error> {
        let count = self.entries.len();
        if !(MIN_QUESTIONS..=MAX_QUESTIONS).contains(&count) {
            return Err(Error::Invalid {
                field: "questions".to_string(),
                reason: format!(
                    "expected {MIN_QUESTIONS}..={MAX_QUESTIONS} questions, got {count}"
                ),
            });
        }

        let mut map: IndexMap<String, Question> = IndexMap::with_capacity(count);
        for (key, question) in self.entries {
            validate_key("question key", &key, false)?;
            validate_question(&key, &question)?;
            if map.insert(key.clone(), question).is_some() {
                return Err(Error::Invalid {
                    field: "questions".to_string(),
                    reason: format!("duplicate question key `{key}`"),
                });
            }
        }

        Ok(Questions(map))
    }
}

/// Validates one question's instructions and, for `choice`/`score`, its criteria set.
fn validate_question(key: &str, question: &Question) -> Result<(), Error> {
    validate_guidance(
        &format!("questions.{key}.instructions"),
        question.instructions(),
    )?;

    match question {
        Question::Noul { criteria, .. } => {
            if let Some(NoulCriteria { r#true, r#false }) = criteria {
                validate_guidance(&format!("questions.{key}.criteria.true"), r#true)?;
                validate_guidance(&format!("questions.{key}.criteria.false"), r#false)?;
            }
            Ok(())
        }
        Question::Choice { criteria, .. } => {
            let count = criteria.len();
            if !(MIN_CHOICE_OPTIONS..=MAX_CHOICE_OPTIONS).contains(&count) {
                return Err(Error::Invalid {
                    field: format!("questions.{key}.criteria"),
                    reason: format!(
                        "expected {MIN_CHOICE_OPTIONS}..={MAX_CHOICE_OPTIONS} choice options, got \
                         {count}"
                    ),
                });
            }
            for (option_key, guidance) in criteria {
                validate_key(
                    &format!("questions.{key}.criteria option key"),
                    option_key,
                    true,
                )?;
                validate_guidance(&format!("questions.{key}.criteria.{option_key}"), guidance)?;
            }
            Ok(())
        }
        Question::Score { criteria, .. } => {
            let count = criteria.len();
            if !(MIN_SCORE_LEVELS..=MAX_SCORE_LEVELS).contains(&count) {
                return Err(Error::Invalid {
                    field: format!("questions.{key}.criteria"),
                    reason: format!(
                        "expected {MIN_SCORE_LEVELS}..={MAX_SCORE_LEVELS} score levels, got \
                         {count}"
                    ),
                });
            }
            for (i, guidance) in criteria.iter().enumerate() {
                validate_guidance(&format!("questions.{key}.criteria[{i}]"), guidance)?;
            }
            Ok(())
        }
    }
}

/// Validates a key against the allowlisted charset and length bound.
///
/// Question keys allow `[a-z0-9_]`; `choice` option keys additionally allow `-`.
fn validate_key(field: &str, key: &str, allow_hyphen: bool) -> Result<(), Error> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(Error::Invalid {
            field: field.to_string(),
            reason: format!(
                "length must be 1..={MAX_KEY_BYTES} bytes, got {}",
                key.len()
            ),
        });
    }
    let valid = key.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || (allow_hyphen && b == b'-')
    });
    if !valid {
        let charset = if allow_hyphen {
            "[a-z0-9_-]"
        } else {
            "[a-z0-9_]"
        };
        return Err(Error::Invalid {
            field: field.to_string(),
            reason: format!("`{key}` must match `^{charset}{{1,{MAX_KEY_BYTES}}}$`"),
        });
    }
    Ok(())
}

/// Validates that a guidance string is non-empty and within the byte-length bound.
fn validate_guidance(field: &str, guidance: &Guidance) -> Result<(), Error> {
    if guidance.is_empty() {
        return Err(Error::Invalid {
            field: field.to_string(),
            reason: "must not be empty".to_string(),
        });
    }
    let len = guidance.byte_len();
    if len > MAX_GUIDANCE_BYTES {
        return Err(Error::Invalid {
            field: field.to_string(),
            reason: format!("length must be <= {MAX_GUIDANCE_BYTES} bytes, got {len}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_build_minimal_valid_questions() {
        let questions = Questions::builder()
            .noul("refund", "Is this a refund request?")
            .build()
            .unwrap();
        assert_eq!(questions.len(), 1);
        assert!(questions.get("refund").is_some());
    }

    #[test]
    fn test_should_build_noul_with_explicit_criteria() {
        let questions = Questions::builder()
            .noul_with(
                "is_bug",
                "Is this a bug?",
                "Describes broken behavior.",
                "Asks a question.",
            )
            .build()
            .unwrap();
        match questions.get("is_bug").unwrap() {
            Question::Noul {
                criteria: Some(c), ..
            } => {
                assert_eq!(c.r#true.as_str(), Some("Describes broken behavior."));
            }
            other => panic!("expected Noul with criteria, got {other:?}"),
        }
    }

    #[test]
    fn test_should_build_choice_and_score_questions() {
        let questions = Questions::builder()
            .choice(
                "team",
                "Which team?",
                [("payments", "Billing."), ("frontend", "UI.")],
            )
            .score("urgency", "How urgent?", ["Low", "High"])
            .build()
            .unwrap();
        assert_eq!(questions.len(), 2);
        assert_eq!(questions.get("team").unwrap().kind(), "choice");
        assert_eq!(questions.get("urgency").unwrap().kind(), "score");
    }

    #[test]
    fn test_should_reject_zero_questions() {
        let err = Questions::builder().build().unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_too_many_questions() {
        let mut builder = Questions::builder();
        for i in 0..=MAX_QUESTIONS {
            builder = builder.noul(format!("q{i}"), "Some instructions here.");
        }
        assert!(builder.build().is_err());
    }

    #[test]
    fn test_should_accept_max_questions() {
        let mut builder = Questions::builder();
        for i in 0..MAX_QUESTIONS {
            builder = builder.noul(format!("q{i}"), "Some instructions here.");
        }
        assert_eq!(builder.build().unwrap().len(), MAX_QUESTIONS);
    }

    #[test]
    fn test_should_reject_duplicate_question_keys() {
        let err = Questions::builder()
            .noul("dup", "First.")
            .noul("dup", "Second.")
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_uppercase_or_symbol_in_question_key() {
        assert!(
            Questions::builder()
                .noul("Refund", "Is this a refund?")
                .build()
                .is_err()
        );
        assert!(
            Questions::builder()
                .noul("refund!", "Is this a refund?")
                .build()
                .is_err()
        );
        assert!(
            Questions::builder()
                .noul("refund-request", "Is this a refund?")
                .build()
                .is_err()
        );
    }

    #[test]
    fn test_should_reject_empty_instructions() {
        assert!(Questions::builder().noul("refund", "").build().is_err());
        assert!(Questions::builder().noul("refund", "   ").build().is_err());
    }

    #[test]
    fn test_should_reject_oversized_instructions() {
        let huge = "x".repeat(MAX_GUIDANCE_BYTES + 1);
        assert!(Questions::builder().noul("refund", huge).build().is_err());
    }

    #[test]
    fn test_should_accept_max_length_instructions() {
        let max = "x".repeat(MAX_GUIDANCE_BYTES);
        assert!(Questions::builder().noul("refund", max).build().is_ok());
    }

    #[test]
    fn test_should_reject_choice_with_one_option() {
        let err = Questions::builder()
            .choice("team", "Which?", [("only", "One.")])
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_choice_option_key_with_uppercase() {
        let err = Questions::builder()
            .choice(
                "team",
                "Which?",
                [("Payments", "Billing."), ("frontend", "UI.")],
            )
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_allow_hyphen_in_choice_option_key_only() {
        assert!(
            Questions::builder()
                .choice(
                    "team",
                    "Which?",
                    [("front-end", "UI."), ("back-end", "Server.")]
                )
                .build()
                .is_ok()
        );
    }

    #[test]
    fn test_should_reject_score_with_one_level() {
        let err = Questions::builder()
            .score("urgency", "How urgent?", ["Only"])
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }));
    }

    #[test]
    fn test_should_reject_score_with_too_many_levels() {
        let levels: Vec<String> = (0..=MAX_SCORE_LEVELS)
            .map(|i| format!("Level {i}"))
            .collect();
        assert!(
            Questions::builder()
                .score("urgency", "How urgent?", levels)
                .build()
                .is_err()
        );
    }

    #[test]
    fn test_should_accept_structured_guidance_via_json_value() {
        let g = json!({"note": "structured"});
        assert!(Questions::builder().noul("refund", g).build().is_ok());
    }

    #[test]
    fn test_should_serialize_choice_question_with_type_tag() {
        let questions = Questions::builder()
            .choice("team", "Which?", [("a", "A."), ("b", "B.")])
            .build()
            .unwrap();
        let json = serde_json::to_value(&questions).unwrap();
        assert_eq!(json["team"]["type"], "choice");
        assert_eq!(json["team"]["instructions"], "Which?");
        assert_eq!(json["team"]["criteria"]["a"], "A.");
    }

    #[test]
    fn test_should_serialize_noul_without_criteria_key_when_absent() {
        let questions = Questions::builder()
            .noul("refund", "Is this a refund?")
            .build()
            .unwrap();
        let json = serde_json::to_value(&questions).unwrap();
        assert!(json["refund"].get("criteria").is_none());
    }

    #[test]
    fn test_should_serialize_noul_criteria_as_true_false_keys() {
        let questions = Questions::builder()
            .noul_with("is_bug", "Is this a bug?", "Yes text.", "No text.")
            .build()
            .unwrap();
        let json = serde_json::to_value(&questions).unwrap();
        assert_eq!(json["is_bug"]["criteria"]["true"], "Yes text.");
        assert_eq!(json["is_bug"]["criteria"]["false"], "No text.");
    }

    proptest! {
        #[test]
        fn test_should_never_accept_key_outside_allowlist(key in "[A-Za-z !@#$%^&*()]{1,20}") {
            // Any key containing an uppercase letter or a disallowed symbol must be rejected.
            let has_invalid_char = key.bytes().any(|b| !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
            if has_invalid_char {
                let result = Questions::builder().noul(key, "Some instructions.").build();
                prop_assert!(result.is_err());
            }
        }
    }
}
