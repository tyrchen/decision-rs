//! Typed decisions and validated probability operations.

use std::{collections::HashSet, fmt};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    answer::{Answer, Answers, ChoiceAnswer, Distribution, ScoreAnswer},
    error::{DecodeError, Error},
    question::{LevelSpec, Questions},
};

/// A finite probability in the inclusive unit interval.
///
/// ```
/// let gate = jev::Probability::try_from(0.9)?;
/// assert_eq!(gate.get(), 0.9);
/// # Ok::<(), jev::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct Probability(f64);
impl Probability {
    /// Read the finite numeric value.
    ///
    /// ```no_run
    /// # fn example(value: jev::Probability) -> Result<(), jev::Error> {
    /// let probability = value.get();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for Probability {
    type Error = Error;
    fn try_from(value: f64) -> Result<Self, Error> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(Error::InvalidInput(
                "probability must be finite and in 0..=1",
            ));
        }
        Ok(Self(value))
    }
}
impl<'de> Deserialize<'de> for Probability {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::try_from(f64::deserialize(d)?)
            .map_err(|_| serde::de::Error::custom("invalid probability"))
    }
}
/// Schema and reconstruction contract, normally derived.
pub trait Decision: Sized {
    /// The immutable schema, cached by the derive.
    ///
    /// # Errors
    /// Returns invalid schema diagnostics without panicking.
    /// ```no_run
    /// # fn schema<D: jev::Decision>() -> Result<(), jev::Error> {
    /// let questions = D::questions()?;
    /// # Ok(())
    /// # }
    /// ```
    fn questions() -> Result<&'static Questions, Error>;
    /// Decode checked answers; Client attaches response metadata to refusals.
    ///
    /// # Errors
    /// Returns refused required fields or inconsistent handwritten mappings.
    /// ```no_run
    /// # fn decode<D: jev::Decision>(answers: &jev::Answers) -> Result<D, jev::DecodeError> {
    /// D::from_answers(answers)
    /// # }
    /// ```
    fn from_answers(answers: &Answers) -> Result<Self, DecodeError>;
}
/// A fieldless enum with stable choice keys and criteria.
pub trait ChoiceOptions: Sized + 'static {
    /// Ordered key and criteria pairs.
    /// ```no_run
    /// # fn inspect<T: jev::ChoiceOptions>() {
    /// let options = T::options();
    /// # }
    /// ```
    fn options() -> &'static [(&'static str, &'static str)];
    /// Decode an option key.
    /// ```no_run
    /// # fn inspect<T: jev::ChoiceOptions>() {
    /// let value = T::from_key("billing");
    /// # }
    /// ```
    fn from_key(key: &str) -> Option<Self>;
    /// Encode this option.
    /// ```no_run
    /// # fn inspect<T: jev::ChoiceOptions>(value: &T) {
    /// let key = value.key();
    /// # }
    /// ```
    fn key(&self) -> &'static str;
}
/// An ordered, fieldless score-level enum.
pub trait ScoreLevels: Sized + 'static {
    /// Stable level keys and criteria, lowest first.
    /// ```no_run
    /// # fn inspect<T: jev::ScoreLevels>() {
    /// let levels = T::levels();
    /// # }
    /// ```
    fn levels() -> &'static [LevelSpec];
    /// Decode a declaration-order position.
    /// ```no_run
    /// # fn inspect<T: jev::ScoreLevels>() {
    /// let first = T::from_index(0);
    /// # }
    /// ```
    fn from_index(index: usize) -> Option<Self>;
    /// This variant's position.
    /// ```no_run
    /// # fn inspect<T: jev::ScoreLevels>(value: &T) {
    /// let index = value.index();
    /// # }
    /// ```
    fn index(&self) -> usize;
}
/// Estimated probability that a condition holds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Predicate(pub(crate) Probability);
/// A threshold decision, including the ambiguous band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The positive threshold is satisfied.
    Yes,
    /// The negative threshold is satisfied.
    No,
    /// Neither or both thresholds are satisfied.
    Unsure,
}
impl Predicate {
    /// The probability of true.
    ///
    /// ```no_run
    /// # fn example(value: jev::Predicate) -> Result<(), jev::Error> {
    /// let probability = value.probability();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn probability(self) -> Probability {
        self.0
    }
    /// Whether true meets a threshold.
    ///
    /// ```no_run
    /// # fn example(value: jev::Predicate) -> Result<(), jev::Error> {
    /// let yes = value.is_yes(jev::Probability::try_from(0.9)?);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn is_yes(self, threshold: Probability) -> bool {
        self.0 >= threshold
    }
    /// Whether false meets a threshold.
    ///
    /// ```no_run
    /// # fn example(value: jev::Predicate) -> Result<(), jev::Error> {
    /// let no = value.is_no(jev::Probability::try_from(0.9)?);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn is_no(self, threshold: Probability) -> bool {
        1.0 - self.0.get() >= threshold.get()
    }
    /// Apply independent yes/no gates; overlapping gates return Unsure.
    ///
    /// ```no_run
    /// # fn example(value: jev::Predicate) -> Result<(), jev::Error> {
    /// let gate = jev::Probability::try_from(0.9)?;
    /// let verdict = value.decide(gate, gate);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn decide(self, yes: Probability, no: Probability) -> Verdict {
        match (self.is_yes(yes), self.is_no(no)) {
            (true, false) => Verdict::Yes,
            (false, true) => Verdict::No,
            _ => Verdict::Unsure,
        }
    }
}
/// One explicitly refused question.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    pub(crate) key: String,
}
impl Refusal {
    /// The refused schema question key.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Refusal) -> Result<(), jev::Error> {
    /// let question = value.key();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}
/// A field that deliberately retains refusal instead of failing the decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome<T> {
    /// A normal answer.
    Answered(T),
    /// An explicit model refusal, never a missing answer.
    Refused(Refusal),
}
/// A typed choice with distinct probabilities and native confidence.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Choice<T> {
    value: T,
    answer: ChoiceAnswer,
}
impl<T: ChoiceOptions> Choice<T> {
    /// Selected enum variant.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let selected = value.value();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }
    /// Consume the wrapper.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: jev::Choice<T>) -> Result<(), jev::Error> {
    /// let selected = value.into_value();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
    /// Native confidence, when reported.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let confidence = value.confidence();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn confidence(&self) -> Option<Probability> {
        self.answer.confidence
    }
    /// Probability of an option; None means the entire distribution is unavailable.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let probability = value.probability_of(value.value());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn probability_of(&self, value: &T) -> Option<Probability> {
        self.answer.probabilities.get(value.key())
    }
    /// Accept using the selected event's probability.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let selected = value.accept(jev::Probability::try_from(0.9)?);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn accept(&self, threshold: Probability) -> Option<&T> {
        self.probability_of(&self.value)
            .filter(|p| *p >= threshold)
            .map(|_| &self.value)
    }
    /// Accept using native confidence explicitly.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let selected = value.accept_confidence(jev::Probability::try_from(0.9)?);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn accept_confidence(&self, threshold: Probability) -> Option<&T> {
        self.confidence()
            .filter(|p| *p >= threshold)
            .map(|_| &self.value)
    }
    /// Weighted sampling with a caller-supplied reproducible roll.
    ///
    /// # Errors
    /// Returns `StatisticsUnavailable` or an invalid handwritten mapping.
    ///
    /// ```no_run
    /// # fn example<T: jev::ChoiceOptions>(value: &jev::Choice<T>) -> Result<(), jev::Error> {
    /// let sampled = value.sample(jev::Probability::try_from(0.42)?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn sample(&self, roll: Probability) -> Result<T, Error> {
        let Distribution::Reported(items) = &self.answer.probabilities else {
            return Err(Error::StatisticsUnavailable);
        };
        let mass: f64 = items.values().map(|p| p.get()).sum();
        let mut remaining = roll.get() * mass;
        let last = items
            .iter()
            .rev()
            .find(|(_, probability)| probability.get() > 0.0)
            .map(|(key, _)| key.as_str())
            .ok_or(Error::StatisticsUnavailable)?;
        let mut selected = last;
        for (key, probability) in items {
            remaining -= probability.get();
            if remaining < 0.0 {
                selected = key;
                break;
            }
        }
        let value = T::from_key(selected).ok_or_else(|| Error::protocol("InvalidMapping"))?;
        if value.key() != selected {
            return Err(Error::protocol("InvalidMapping"));
        }
        Ok(value)
    }
}
/// A continuous position on a typed ordered scale.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Score<T> {
    answer: ScoreAnswer,
    marker: std::marker::PhantomData<T>,
}
impl<T: ScoreLevels> Score<T> {
    /// The original continuous score.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let continuous = value.raw();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn raw(&self) -> f64 {
        self.answer.score
    }
    /// Position normalized to the unit interval.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let normalized = value.normalized();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // schema cardinality is at most 32
    pub fn normalized(&self) -> f64 {
        self.raw() / self.answer.levels.len().saturating_sub(1) as f64
    }
    /// The nearest level, with half positions rounded upwards; not the distribution mode.
    ///
    /// # Errors
    /// A handwritten mapping may return no variant or the wrong index.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let level = value.nearest()?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // validated score is in 0..31
    pub fn nearest(&self) -> Result<T, Error> {
        let index = self.raw().round() as usize;
        let value = T::from_index(index).ok_or_else(|| Error::protocol("InvalidMapping"))?;
        if value.index() != index {
            return Err(Error::protocol("InvalidMapping"));
        }
        Ok(value)
    }
    /// Whether the continuous score is at least this level.
    ///
    /// # Errors
    /// Rejects an inconsistent handwritten index.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let level = value.nearest()?;
    /// let meets_level = value.at_least(&level)?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::cast_precision_loss)] // validated level index is at most 31
    pub fn at_least(&self, level: &T) -> Result<bool, Error> {
        let index = level.index();
        if index >= self.answer.levels.len() {
            return Err(Error::protocol("InvalidMapping"));
        }
        Ok(self.raw() >= index as f64)
    }
    /// Native confidence, without deriving it from the distribution.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let confidence = value.confidence();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn confidence(&self) -> Option<Probability> {
        self.answer.confidence
    }
    /// Whether reported native confidence meets a gate.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let accepted = value.confident(jev::Probability::try_from(0.9)?);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn confident(&self, threshold: Probability) -> bool {
        self.confidence().is_some_and(|p| p >= threshold)
    }
    /// Probability of a level, or None if statistics are unavailable.
    ///
    /// ```no_run
    /// # fn example<T: jev::ScoreLevels>(value: &jev::Score<T>) -> Result<(), jev::Error> {
    /// let level = value.nearest()?;
    /// let probability = value.probability_of(&level);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn probability_of(&self, level: &T) -> Option<Probability> {
        self.answer.probabilities.get(&level.index().to_string())
    }
}
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Predicate {}
    impl<T: super::ChoiceOptions> Sealed for super::Choice<T> {}
    impl<T: super::ScoreLevels> Sealed for super::Score<T> {}
    impl<T: super::FromAnswer> Sealed for super::Outcome<T> {}
}
/// Checked answer conversion used by derive-generated and handwritten decisions.
pub trait FromAnswer: sealed::Sealed + Sized {
    /// Rebuild this field from a checked answer.
    ///
    /// # Errors
    /// Reports refusal or schema/mapping mismatch.
    /// ```no_run
    /// # fn decode<T: jev::FromAnswer>(answer: &jev::Answer) -> Result<T, jev::DecodeError> {
    /// T::from_answer("refund", answer)
    /// # }
    /// ```
    fn from_answer(key: &str, answer: &Answer) -> Result<Self, DecodeError>;
}
fn mapping(key: &str) -> DecodeError {
    DecodeError::InvalidMapping {
        key: key.to_owned(),
    }
}
fn refused(key: &str, answer: &Answer) -> Result<(), DecodeError> {
    if matches!(answer, Answer::Refusal(_)) {
        Err(DecodeError::Refused(vec![key.to_owned()]))
    } else {
        Ok(())
    }
}
impl FromAnswer for Predicate {
    fn from_answer(key: &str, answer: &Answer) -> Result<Self, DecodeError> {
        refused(key, answer)?;
        match answer {
            Answer::Predicate(value) => Ok(*value),
            _ => Err(mapping(key)),
        }
    }
}
impl<T: ChoiceOptions> FromAnswer for Choice<T> {
    fn from_answer(key: &str, answer: &Answer) -> Result<Self, DecodeError> {
        refused(key, answer)?;
        let Answer::Choice(answer) = answer else {
            return Err(mapping(key));
        };
        let options = T::options();
        if options.len() != answer.options.len() {
            return Err(mapping(key));
        }
        let keys: HashSet<&str> = options.iter().map(|(key, _)| *key).collect();
        if keys.len() != options.len()
            || answer
                .options
                .iter()
                .any(|key| !keys.contains(key.as_str()))
        {
            return Err(mapping(key));
        }
        let value = T::from_key(&answer.choice).ok_or_else(|| mapping(key))?;
        if value.key() != answer.choice {
            return Err(mapping(key));
        }
        Ok(Self {
            value,
            answer: answer.clone(),
        })
    }
}
impl<T: ScoreLevels> FromAnswer for Score<T> {
    fn from_answer(key: &str, answer: &Answer) -> Result<Self, DecodeError> {
        refused(key, answer)?;
        let Answer::Score(answer) = answer else {
            return Err(mapping(key));
        };
        if T::levels()
            .iter()
            .map(|l| l.key)
            .ne(answer.levels.iter().map(String::as_str))
        {
            return Err(mapping(key));
        }
        Ok(Self {
            answer: answer.clone(),
            marker: std::marker::PhantomData,
        })
    }
}
impl<T: FromAnswer> FromAnswer for Outcome<T> {
    fn from_answer(key: &str, answer: &Answer) -> Result<Self, DecodeError> {
        match answer {
            Answer::Refusal(value) => Ok(Self::Refused(value.clone())),
            _ => T::from_answer(key, answer).map(Self::Answered),
        }
    }
}
impl fmt::Display for Probability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
