//! Canonical answers; only schema-validated backend results can construct them.

use indexmap::IndexMap;

use crate::{
    error::DecodeError,
    typed::{FromAnswer, Predicate, Probability, Refusal},
};

/// Complete reported probabilities, or explicit absence of statistics.
#[derive(Debug, Clone, PartialEq)]
pub enum Distribution {
    /// Validated complete distribution in schema order.
    Reported(IndexMap<String, Probability>),
    /// The backend omitted optional statistics.
    Unavailable,
}
impl Distribution {
    /// Read a probability; never invent zero for unavailable statistics.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Distribution) -> Result<(), jev::Error> {
    /// let probability = value.get("billing");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Probability> {
        match self {
            Self::Reported(values) => values.get(key).copied(),
            Self::Unavailable => None,
        }
    }
}
/// Checked choice information.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ChoiceAnswer {
    pub(crate) choice: String,
    pub(crate) confidence: Option<Probability>,
    pub(crate) probabilities: Distribution,
    pub(crate) options: Vec<String>,
}
impl ChoiceAnswer {
    /// The selected stable key.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ChoiceAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.choice();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn choice(&self) -> &str {
        &self.choice
    }
    /// Backend-reported native confidence.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ChoiceAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.confidence();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn confidence(&self) -> Option<Probability> {
        self.confidence
    }
    /// Distribution or explicit unavailability.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ChoiceAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.probabilities();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn probabilities(&self) -> &Distribution {
        &self.probabilities
    }
}
/// Checked continuous score information.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ScoreAnswer {
    pub(crate) score: f64,
    pub(crate) confidence: Option<Probability>,
    pub(crate) probabilities: Distribution,
    pub(crate) levels: Vec<String>,
}
impl ScoreAnswer {
    /// Continuous score, not a selected index.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ScoreAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.score();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn score(&self) -> f64 {
        self.score
    }
    /// Backend-reported native confidence.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ScoreAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.confidence();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn confidence(&self) -> Option<Probability> {
        self.confidence
    }
    /// Distribution indexed by decimal level position.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ScoreAnswer) -> Result<(), jev::Error> {
    /// let statistic = value.probabilities();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn probabilities(&self) -> &Distribution {
        &self.probabilities
    }
}
/// Provider-neutral answer to one question.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Answer {
    /// Condition probability.
    Predicate(Predicate),
    /// Fixed-category choice.
    Choice(ChoiceAnswer),
    /// Continuous ordered score.
    Score(ScoreAnswer),
    /// Explicit question refusal.
    Refusal(Refusal),
}
/// Schema-ordered checked answers.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Answers(pub(crate) IndexMap<String, Answer>);
impl Answers {
    /// Read an answer by question key.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::Error> {
    /// let answer = value.get("refund");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Answer> {
        self.0.get(key)
    }
    /// Number of answers, including refusals.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::Error> {
    /// let count = value.len();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether no answers are present.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::Error> {
    /// let empty = value.is_empty();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iterate in request schema order.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::Error> {
    /// for (key, answer) in value { let kind = answer; }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn iter(&self) -> indexmap::map::Iter<'_, String, Answer> {
        self.0.iter()
    }
    /// Decode a field using checked schema mappings.
    ///
    /// # Errors
    /// Returns refusal or invalid typed mapping.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::DecodeError> {
    /// let refund: jev::Predicate = value.decode("refund")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn decode<T: FromAnswer>(&self, key: &str) -> Result<T, DecodeError> {
        let answer = self.get(key).ok_or_else(|| DecodeError::InvalidMapping {
            key: key.to_owned(),
        })?;
        T::from_answer(key, answer)
    }
    /// Collect all refusals among required fields before decoding.
    ///
    /// # Errors
    /// Returns all refused required keys, or a missing mapping.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Answers) -> Result<(), jev::DecodeError> {
    /// value.require(&["refund"])?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn require(&self, keys: &[&str]) -> Result<(), DecodeError> {
        let mut refused = Vec::new();
        for key in keys {
            match self.get(key) {
                Some(Answer::Refusal(_)) => refused.push((*key).to_owned()),
                Some(_) => {}
                None => {
                    return Err(DecodeError::InvalidMapping {
                        key: (*key).to_owned(),
                    });
                }
            }
        }
        if refused.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::Refused(refused))
        }
    }
}

impl<'a> IntoIterator for &'a Answers {
    type Item = (&'a String, &'a Answer);
    type IntoIter = indexmap::map::Iter<'a, String, Answer>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
