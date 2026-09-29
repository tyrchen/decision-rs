//! `Answer`, `Answers`, and the checked accessors that turn a wire map into either a
//! typed value or a named [`Error`].

use indexmap::IndexMap;
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer, MapAccess, Visitor},
};
use serde_json::Value;

// `Answer` intentionally has no `Serialize` impl: its `Other` variant carries
// arbitrary raw JSON under an unrecognized `type`, which a derived, internally
// tagged `Serialize` cannot round-trip. Callers who need to re-emit an answer as
// JSON should serialize the original response body instead.
use crate::error::Error;
use crate::guidance::Guidance;

/// One answered question.
///
/// Deserialization dispatches on the wire `type` field. A `type` this SDK version
/// does not recognize becomes [`Answer::Other`] instead of failing the whole
/// response, so a new server-side primitive never breaks existing callers using the
/// dynamic path (typed callers see [`Error::AnswerType`] if a field expects a known
/// primitive and gets `Other`).
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Answer to a `noul` question.
    Noul(NoulAnswer),
    /// Answer to a `choice` question.
    Choice(ChoiceAnswer),
    /// Answer to a `score` question.
    Score(ScoreAnswer),
    /// A primitive this SDK version does not know about.
    Other {
        /// The unrecognized wire `type` value.
        r#type: String,
        /// The raw JSON payload, preserved verbatim.
        raw: Value,
    },
}

impl Answer {
    /// The wire name of this answer's primitive.
    #[must_use]
    pub fn kind(&self) -> String {
        match self {
            Answer::Noul(_) => "noul".to_string(),
            Answer::Choice(_) => "choice".to_string(),
            Answer::Score(_) => "score".to_string(),
            Answer::Other { r#type, .. } => format!("other({type})"),
        }
    }
}

/// A `noul` answer: `P(yes)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NoulAnswer {
    /// `P(yes)`, always finite and within `0.0..=1.0`.
    pub noul: f64,
}

/// A `choice` answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChoiceAnswer {
    /// The option key the model selected.
    pub choice: String,
    /// Confidence in `choice`, when the server reports it.
    pub confidence: Option<f64>,
    /// Per-option probabilities. Empty, not absent, when the server omits them.
    pub probabilities: IndexMap<String, f64>,
}

/// A `score` answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoreAnswer {
    /// The continuous score, always finite and non-negative.
    pub score: f64,
    /// Confidence in `score`, when the server reports it.
    pub confidence: Option<f64>,
    /// Per-level probabilities. Empty, not absent, when the server omits them.
    pub probabilities: IndexMap<String, f64>,
    /// Per-level legend text, when the server echoes it back.
    pub legend: IndexMap<String, Guidance>,
}

/// Validates that `v` is finite and within `0.0..=1.0`.
fn validate_probability(field: &str, v: f64) -> Result<f64, Error> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return Err(Error::Parse(format!(
            "`{field}` must be finite and within 0.0..=1.0, got {v}"
        )));
    }
    Ok(v)
}

/// Validates that `v` is finite and non-negative.
fn validate_score(v: f64) -> Result<f64, Error> {
    if !v.is_finite() || v < 0.0 {
        return Err(Error::Parse(format!(
            "`score` must be finite and non-negative, got {v}"
        )));
    }
    Ok(v)
}

/// Parses an optional confidence field, validating it as a probability when present.
fn parse_confidence(obj: &serde_json::Map<String, Value>) -> Result<Option<f64>, Error> {
    match obj.get("confidence") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let f = v
                .as_f64()
                .ok_or_else(|| Error::Parse("`confidence` must be a number".to_string()))?;
            Ok(Some(validate_probability("confidence", f)?))
        }
    }
}

/// Parses a `probabilities` object into an ordered, range-checked map.
fn parse_probabilities(
    obj: &serde_json::Map<String, Value>,
) -> Result<IndexMap<String, f64>, Error> {
    match obj.get("probabilities") {
        None | Some(Value::Null) => Ok(IndexMap::new()),
        Some(Value::Object(map)) => {
            let mut out = IndexMap::with_capacity(map.len());
            for (k, v) in map {
                let f = v
                    .as_f64()
                    .ok_or_else(|| Error::Parse(format!("`probabilities.{k}` must be a number")))?;
                out.insert(
                    k.clone(),
                    validate_probability(&format!("probabilities.{k}"), f)?,
                );
            }
            Ok(out)
        }
        Some(_) => Err(Error::Parse(
            "`probabilities` must be an object".to_string(),
        )),
    }
}

/// Parses a `legend` object into an ordered map of [`Guidance`].
fn parse_legend(obj: &serde_json::Map<String, Value>) -> Result<IndexMap<String, Guidance>, Error> {
    match obj.get("legend") {
        None | Some(Value::Null) => Ok(IndexMap::new()),
        Some(Value::Object(map)) => Ok(map
            .iter()
            .map(|(k, v)| (k.clone(), Guidance::from(v.clone())))
            .collect()),
        Some(_) => Err(Error::Parse("`legend` must be an object".to_string())),
    }
}

/// Parses one JSON object into a typed [`Answer`], dispatching on `type`.
fn answer_from_value(value: Value) -> Result<Answer, Error> {
    let Value::Object(obj) = value else {
        return Err(Error::Parse("answer must be a JSON object".to_string()));
    };
    let r#type = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Parse("answer is missing a string `type` field".to_string()))?
        .to_string();

    match r#type.as_str() {
        "noul" => {
            let noul = obj.get("noul").and_then(Value::as_f64).ok_or_else(|| {
                Error::Parse("`noul` answer is missing a numeric `noul` field".to_string())
            })?;
            Ok(Answer::Noul(NoulAnswer {
                noul: validate_probability("noul", noul)?,
            }))
        }
        "choice" => {
            let choice = obj
                .get("choice")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::Parse("`choice` answer is missing a string `choice` field".to_string())
                })?
                .to_string();
            Ok(Answer::Choice(ChoiceAnswer {
                choice,
                confidence: parse_confidence(&obj)?,
                probabilities: parse_probabilities(&obj)?,
            }))
        }
        "score" => {
            let score = obj.get("score").and_then(Value::as_f64).ok_or_else(|| {
                Error::Parse("`score` answer is missing a numeric `score` field".to_string())
            })?;
            Ok(Answer::Score(ScoreAnswer {
                score: validate_score(score)?,
                confidence: parse_confidence(&obj)?,
                probabilities: parse_probabilities(&obj)?,
                legend: parse_legend(&obj)?,
            }))
        }
        other => Ok(Answer::Other {
            r#type: other.to_string(),
            raw: Value::Object(obj),
        }),
    }
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        answer_from_value(value).map_err(de::Error::custom)
    }
}

/// The full set of answers for one request: an ordered map of question key to
/// [`Answer`].
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), jev::Error> {
/// let body = r#"{"refund": {"type": "noul", "noul": 0.87}}"#;
/// let answers: jev::Answers = serde_json::from_str(body).unwrap();
/// assert!(answers.noul("refund")? > 0.5);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Answers(IndexMap<String, Answer>);

impl Answers {
    /// Looks up an answer by key, without checking its primitive type.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Answer> {
        self.0.get(key)
    }

    /// The number of answers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no answers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates answers in the order the server returned them.
    #[must_use]
    pub fn iter(&self) -> indexmap::map::Iter<'_, String, Answer> {
        self.0.iter()
    }

    /// Reads `key` as a `noul` answer's `P(yes)`.
    ///
    /// # Errors
    ///
    /// [`Error::MissingAnswer`] if `key` is absent; [`Error::AnswerType`] if present
    /// but not a `noul` answer.
    pub fn noul(&self, key: &str) -> Result<f64, Error> {
        match self.get(key) {
            None => Err(Error::MissingAnswer {
                key: key.to_string(),
            }),
            Some(Answer::Noul(a)) => Ok(a.noul),
            Some(other) => Err(Error::AnswerType {
                key: key.to_string(),
                expected: "noul",
                actual: other.kind(),
            }),
        }
    }

    /// Reads `key` as a `choice` answer.
    ///
    /// # Errors
    ///
    /// [`Error::MissingAnswer`] if `key` is absent; [`Error::AnswerType`] if present
    /// but not a `choice` answer.
    pub fn choice(&self, key: &str) -> Result<&ChoiceAnswer, Error> {
        match self.get(key) {
            None => Err(Error::MissingAnswer {
                key: key.to_string(),
            }),
            Some(Answer::Choice(a)) => Ok(a),
            Some(other) => Err(Error::AnswerType {
                key: key.to_string(),
                expected: "choice",
                actual: other.kind(),
            }),
        }
    }

    /// Reads `key` as a `score` answer.
    ///
    /// # Errors
    ///
    /// [`Error::MissingAnswer`] if `key` is absent; [`Error::AnswerType`] if present
    /// but not a `score` answer.
    pub fn score(&self, key: &str) -> Result<&ScoreAnswer, Error> {
        match self.get(key) {
            None => Err(Error::MissingAnswer {
                key: key.to_string(),
            }),
            Some(Answer::Score(a)) => Ok(a),
            Some(other) => Err(Error::AnswerType {
                key: key.to_string(),
                expected: "score",
                actual: other.kind(),
            }),
        }
    }
}

impl<'de> Deserialize<'de> for Answers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AnswersVisitor;

        impl<'de> Visitor<'de> for AnswersVisitor {
            type Value = Answers;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of question key to answer")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut out = IndexMap::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, answer)) = map.next_entry::<String, Answer>()? {
                    out.insert(key, answer);
                }
                Ok(Answers(out))
            }
        }

        deserializer.deserialize_map(AnswersVisitor)
    }
}

impl<'a> IntoIterator for &'a Answers {
    type Item = (&'a String, &'a Answer);
    type IntoIter = indexmap::map::Iter<'a, String, Answer>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_deserialize_noul_answer() {
        let answers: Answers = serde_json::from_value(json!({
            "refund": {"type": "noul", "noul": 0.87}
        }))
        .unwrap();
        assert_eq!(answers.noul("refund").unwrap(), 0.87);
    }

    #[test]
    fn test_should_deserialize_choice_answer_with_probabilities() {
        let answers: Answers = serde_json::from_value(json!({
            "team": {
                "type": "choice",
                "choice": "payments",
                "confidence": 0.9,
                "probabilities": {"payments": 0.9, "frontend": 0.1}
            }
        }))
        .unwrap();
        let choice = answers.choice("team").unwrap();
        assert_eq!(choice.choice, "payments");
        assert_eq!(choice.confidence, Some(0.9));
        assert_eq!(choice.probabilities.get("payments"), Some(&0.9));
    }

    #[test]
    fn test_should_deserialize_score_answer_with_legend() {
        let answers: Answers = serde_json::from_value(json!({
            "urgency": {
                "type": "score",
                "score": 1.7,
                "legend": {"0": "Low", "1": "High"}
            }
        }))
        .unwrap();
        let score = answers.score("urgency").unwrap();
        assert_eq!(score.score, 1.7);
        assert_eq!(
            score.legend.get("1").and_then(Guidance::as_str),
            Some("High")
        );
    }

    #[test]
    fn test_should_default_probabilities_and_legend_to_empty_map_not_option() {
        let answers: Answers =
            serde_json::from_value(json!({"team": {"type": "choice", "choice": "x"}})).unwrap();
        let choice = answers.choice("team").unwrap();
        assert!(choice.probabilities.is_empty());
    }

    #[test]
    fn test_should_map_unknown_type_to_other_instead_of_failing() {
        let answers: Answers = serde_json::from_value(json!({
            "future": {"type": "vibe", "vibe": "great"}
        }))
        .unwrap();
        match answers.get("future").unwrap() {
            Answer::Other { r#type, raw } => {
                assert_eq!(r#type, "vibe");
                assert_eq!(raw["vibe"], "great");
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn test_should_reject_nan_noul_probability() {
        let result: Result<Answers, _> =
            serde_json::from_value(json!({"refund": {"type": "noul", "noul": f64::NAN}}));
        assert!(result.is_err());
    }

    #[test]
    fn test_should_reject_out_of_range_probability() {
        let result: Result<Answers, _> =
            serde_json::from_value(json!({"refund": {"type": "noul", "noul": 1.5}}));
        assert!(result.is_err());

        let result: Result<Answers, _> =
            serde_json::from_value(json!({"refund": {"type": "noul", "noul": -0.1}}));
        assert!(result.is_err());
    }

    #[test]
    fn test_should_reject_negative_score() {
        let result: Result<Answers, _> =
            serde_json::from_value(json!({"urgency": {"type": "score", "score": -1.0}}));
        assert!(result.is_err());
    }

    #[test]
    fn test_should_reject_out_of_range_choice_probability() {
        let result: Result<Answers, _> = serde_json::from_value(json!({
            "team": {"type": "choice", "choice": "x", "probabilities": {"x": 2.0}}
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_should_return_missing_answer_error_for_absent_key() {
        let answers = Answers::default();
        assert!(matches!(
            answers.noul("missing"),
            Err(Error::MissingAnswer { .. })
        ));
    }

    #[test]
    fn test_should_return_answer_type_error_for_type_mismatch() {
        let answers: Answers =
            serde_json::from_value(json!({"team": {"type": "choice", "choice": "x"}})).unwrap();
        match answers.noul("team") {
            Err(Error::AnswerType {
                expected, actual, ..
            }) => {
                assert_eq!(expected, "noul");
                assert_eq!(actual, "choice");
            }
            other => panic!("expected AnswerType error, got {other:?}"),
        }
    }
}
