//! The typed-decision layer: three traits, three wrapper types, built on top of
//! [`crate::question::Questions`] / [`crate::answer::Answers`] so there is exactly
//! one copy of the wire logic.

use indexmap::IndexMap;

use crate::{answer::Answers, error::Error, question::Questions};

/// A set of questions about one state, answered as one struct.
///
/// Implemented by `#[derive(Decision)]`; see the crate-level docs for the derive's
/// rules. `questions()` is built once per process (the derive caches it in a private
/// `LazyLock`) and is infallible, because the derive validates the question shape at
/// compile time.
///
/// # Examples
///
/// ```
/// use jev::{Answers, Decision, Error, Noul, Questions};
///
/// struct Manual {
///     is_bug: Noul,
/// }
///
/// impl Decision for Manual {
///     fn questions() -> &'static Questions {
///         use std::sync::LazyLock;
///         static QUESTIONS: LazyLock<Questions> = LazyLock::new(|| {
///             Questions::builder().noul("is_bug", "Is this a defect?").build().unwrap()
///         });
///         &QUESTIONS
///     }
///
///     fn from_answers(answers: &Answers) -> Result<Self, Error> {
///         Ok(Manual { is_bug: Noul::try_new(answers.noul("is_bug")?)? })
///     }
/// }
/// ```
pub trait Decision: Sized {
    /// The questions to send, built once and cached by the derive.
    fn questions() -> &'static Questions;

    /// Rebuilds the struct from the model's answers.
    ///
    /// # Errors
    ///
    /// Any [`Error`] naming the offending question: [`Error::MissingAnswer`],
    /// [`Error::AnswerType`], [`Error::UnknownChoice`], or [`Error::ScoreRange`].
    fn from_answers(answers: &Answers) -> Result<Self, Error>;
}

/// A fieldless enum usable as `choice` criteria.
///
/// Implemented by `#[derive(ChoiceOptions)]`. Wire keys and criteria come from the
/// enum's variants and their doc comments; see the crate-level docs for the derive's
/// rules.
pub trait ChoiceOptions: Sized + 'static {
    /// Wire keys in declaration order, paired with their criteria text.
    fn options() -> &'static [(&'static str, &'static str)];

    /// Looks up the variant for a wire key, or `None` if unrecognized.
    fn from_key(key: &str) -> Option<Self>;

    /// This variant's wire key.
    fn key(&self) -> &'static str;
}

/// A fieldless enum usable as an ordered `score` scale, lowest level first.
///
/// Implemented by `#[derive(ScoreLevels)]`. **Declaration order is the scale**:
/// reordering variants silently changes the meaning of every tuned threshold.
pub trait ScoreLevels: Sized + 'static {
    /// Level criteria text, in declaration order (index 0 is the lowest level).
    fn levels() -> &'static [&'static str];

    /// Looks up the variant at `index`, or `None` if out of range.
    fn from_index(index: usize) -> Option<Self>;

    /// This variant's declaration-order index.
    fn index(&self) -> usize;
}

/// A `noul` answer: `P(yes)`, plus threshold arithmetic.
///
/// # Examples
///
/// ```
/// use jev::Noul;
///
/// let noul = Noul::try_new(0.92).unwrap();
/// assert!(noul.is_yes(0.9));
/// assert_eq!(noul.decide(0.9, 0.1), Some(true));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noul {
    probability: f64,
}

impl Noul {
    /// Builds a `Noul` from a raw probability.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] if `probability` is not finite or not within
    /// `0.0..=1.0`. In practice this cannot happen for a value that came from
    /// [`Answers::noul`], which already validates the range; the check here guards
    /// hand-built values from a dynamic caller.
    pub fn try_new(probability: f64) -> Result<Self, Error> {
        if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
            return Err(Error::Parse(format!(
                "noul probability must be finite and within 0.0..=1.0, got {probability}"
            )));
        }
        Ok(Noul { probability })
    }

    /// `P(yes)`.
    #[must_use]
    pub fn probability(&self) -> f64 {
        self.probability
    }

    /// `P(yes) >= threshold`.
    #[must_use]
    pub fn is_yes(&self, threshold: f64) -> bool {
        self.probability >= threshold
    }

    /// `P(yes) <= 1 - threshold`, computed as `1 - P(yes) >= threshold` so common
    /// thresholds like `0.9` do not fail from floating-point subtraction order
    /// (`1.0 - 0.9` is not bit-exact, but `1.0 - 0.1 >= 0.9` is).
    #[must_use]
    pub fn is_no(&self, threshold: f64) -> bool {
        1.0 - self.probability >= threshold
    }

    /// `Some(true)` when `P(yes) >= yes_at`, `Some(false)` when `P(yes) <= 1 - no_at`,
    /// `None` inside the ambiguous band between the two — the caller's cue to escalate.
    #[must_use]
    pub fn decide(&self, yes_at: f64, no_at: f64) -> Option<bool> {
        if self.is_yes(yes_at) {
            Some(true)
        } else if self.is_no(no_at) {
            Some(false)
        } else {
            None
        }
    }
}

/// A `choice` answer, carrying the typed value alongside its raw probabilities.
///
/// # Examples
///
/// ```
/// use jev::{Choice, ChoiceOptions};
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Team {
///     Payments,
///     Frontend,
/// }
///
/// impl ChoiceOptions for Team {
///     fn options() -> &'static [(&'static str, &'static str)] {
///         &[("payments", "Billing issues."), ("frontend", "UI issues.")]
///     }
///     fn from_key(key: &str) -> Option<Self> {
///         match key {
///             "payments" => Some(Team::Payments),
///             "frontend" => Some(Team::Frontend),
///             _ => None,
///         }
///     }
///     fn key(&self) -> &'static str {
///         match self {
///             Team::Payments => "payments",
///             Team::Frontend => "frontend",
///         }
///     }
/// }
///
/// let choice = Choice::<Team>::try_new("team", "payments", Some(0.8), Default::default()).unwrap();
/// assert_eq!(*choice.value(), Team::Payments);
/// assert!(choice.confident(0.7));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Choice<T: ChoiceOptions> {
    value: T,
    confidence: Option<f64>,
    probabilities: IndexMap<String, f64>,
}

impl<T: ChoiceOptions> Choice<T> {
    /// Builds a `Choice` from a wire choice key, confidence, and probability map.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownChoice`] when `choice` names an option outside `T`.
    /// `key` names the question this answer came from, used only in the
    /// [`Error::UnknownChoice`] message if `choice` is not one of `T::options()`.
    pub fn try_new(
        key: &str,
        choice: &str,
        confidence: Option<f64>,
        probabilities: IndexMap<String, f64>,
    ) -> Result<Self, Error> {
        let value = T::from_key(choice).ok_or_else(|| Error::UnknownChoice {
            key: key.to_string(),
            choice: choice.to_string(),
            expected: T::options()
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>()
                .join(", "),
        })?;
        Ok(Choice {
            value,
            confidence,
            probabilities,
        })
    }

    /// The selected option.
    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consumes the wrapper, returning the selected option.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// The server's confidence in `value`, when reported.
    #[must_use]
    pub fn confidence(&self) -> Option<f64> {
        self.confidence
    }

    /// The raw probability the server assigned to `option`, or `0.0` if absent.
    #[must_use]
    pub fn probability_of(&self, option: &T) -> f64 {
        self.probabilities.get(option.key()).copied().unwrap_or(0.0)
    }

    /// Confidence is present and `>= threshold`.
    #[must_use]
    pub fn confident(&self, threshold: f64) -> bool {
        self.confidence.is_some_and(|c| c >= threshold)
    }

    /// `Some(value)` when [`Self::confident`], else `None` — the caller's cue to
    /// escalate to a human or a fallback path.
    #[must_use]
    pub fn accept(&self, threshold: f64) -> Option<&T> {
        self.confident(threshold).then(|| self.value())
    }
}

/// A `score` answer, carrying the raw continuous score alongside the typed nearest
/// level.
///
/// # Examples
///
/// ```
/// use jev::{Score, ScoreLevels};
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Urgency {
///     NextRelease,
///     ThisWeek,
///     BlockingRevenue,
/// }
///
/// impl ScoreLevels for Urgency {
///     fn levels() -> &'static [&'static str] {
///         &["Can wait.", "This week.", "Blocking revenue."]
///     }
///     fn from_index(index: usize) -> Option<Self> {
///         match index {
///             0 => Some(Urgency::NextRelease),
///             1 => Some(Urgency::ThisWeek),
///             2 => Some(Urgency::BlockingRevenue),
///             _ => None,
///         }
///     }
///     fn index(&self) -> usize {
///         match self {
///             Urgency::NextRelease => 0,
///             Urgency::ThisWeek => 1,
///             Urgency::BlockingRevenue => 2,
///         }
///     }
/// }
///
/// let score = Score::<Urgency>::try_new("urgency", 1.9, None, Default::default()).unwrap();
/// assert_eq!(score.nearest(), Urgency::BlockingRevenue);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Score<T: ScoreLevels> {
    raw: f64,
    confidence: Option<f64>,
    probabilities: IndexMap<String, f64>,
    _levels: std::marker::PhantomData<T>,
}

impl<T: ScoreLevels> Score<T> {
    /// Builds a `Score` from a raw wire score, confidence, and probability map.
    ///
    /// `key` names the question this answer came from, used only in the
    /// [`Error::ScoreRange`] message if `raw` is out of range for `T`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ScoreRange`] when `raw` falls outside `0.0..=(levels - 1)`
    /// for `T::levels()`.
    #[allow(clippy::cast_precision_loss)] // levels is bounded by MAX_SCORE_LEVELS (32)
    pub fn try_new(
        key: &str,
        raw: f64,
        confidence: Option<f64>,
        probabilities: IndexMap<String, f64>,
    ) -> Result<Self, Error> {
        let levels = T::levels().len();
        let max_index = (levels.saturating_sub(1)) as f64;
        if !raw.is_finite() || !(0.0..=max_index).contains(&raw) {
            return Err(Error::ScoreRange {
                key: key.to_string(),
                score: raw,
                levels,
            });
        }
        Ok(Score {
            raw,
            confidence,
            probabilities,
            _levels: std::marker::PhantomData,
        })
    }

    /// The probability-weighted position on the scale, e.g. `1.99`.
    #[must_use]
    pub fn raw(&self) -> f64 {
        self.raw
    }

    /// The nearest discrete level.
    ///
    /// Always returns a value: a `Score<T>` can only be constructed from a raw score
    /// already validated against `T::levels()` by [`Self::try_new`], so rounding
    /// `raw()` to the nearest integer always lands on a valid index.
    ///
    /// # Panics
    ///
    /// Never panics for a `T` whose `ScoreLevels` impl keeps `index()`/`from_index()`
    /// consistent with `levels()` (guaranteed by `#[derive(ScoreLevels)]`). A
    /// hand-written `ScoreLevels` impl that violates that contract — for example,
    /// `from_index(0)` returning `None` — is a caller-side bug this method cannot
    /// recover from.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // levels is bounded by MAX_SCORE_LEVELS (32)
    pub fn nearest(&self) -> T {
        let levels = T::levels().len();
        let max_index = levels.saturating_sub(1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = self.raw.round().clamp(0.0, max_index as f64) as usize;
        T::from_index(rounded).unwrap_or_else(|| {
            T::from_index(0).expect(
                "ScoreLevels::from_index(0) must succeed: derive(ScoreLevels) guarantees at least \
                 2 variants, so index 0 always exists",
            )
        })
    }

    /// `raw()` normalized to `0.0..=1.0`, handy for sorting across different scales.
    ///
    /// A single-level scale (`levels() == 1`) normalizes to `0.0`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // levels is bounded by MAX_SCORE_LEVELS (32)
    pub fn normalized(&self) -> f64 {
        let max_index = T::levels().len().saturating_sub(1);
        if max_index == 0 {
            0.0
        } else {
            self.raw / max_index as f64
        }
    }

    /// `raw() >= level.index()`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // index() is bounded by MAX_SCORE_LEVELS (32)
    pub fn at_least(&self, level: &T) -> bool {
        self.raw >= level.index() as f64
    }

    /// The server's confidence in `score`, when reported.
    #[must_use]
    pub fn confidence(&self) -> Option<f64> {
        self.confidence
    }

    /// Confidence is present and `>= threshold`.
    #[must_use]
    pub fn confident(&self, threshold: f64) -> bool {
        self.confidence.is_some_and(|c| c >= threshold)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Team {
        Payments,
        Frontend,
    }

    impl ChoiceOptions for Team {
        fn options() -> &'static [(&'static str, &'static str)] {
            &[("payments", "Billing."), ("frontend", "UI.")]
        }
        fn from_key(key: &str) -> Option<Self> {
            match key {
                "payments" => Some(Team::Payments),
                "frontend" => Some(Team::Frontend),
                _ => None,
            }
        }
        fn key(&self) -> &'static str {
            match self {
                Team::Payments => "payments",
                Team::Frontend => "frontend",
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Urgency {
        Low,
        Mid,
        High,
    }

    impl ScoreLevels for Urgency {
        fn levels() -> &'static [&'static str] {
            &["Low.", "Mid.", "High."]
        }
        fn from_index(index: usize) -> Option<Self> {
            match index {
                0 => Some(Urgency::Low),
                1 => Some(Urgency::Mid),
                2 => Some(Urgency::High),
                _ => None,
            }
        }
        fn index(&self) -> usize {
            match self {
                Urgency::Low => 0,
                Urgency::Mid => 1,
                Urgency::High => 2,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TwoLevel {
        Off,
        On,
    }

    impl ScoreLevels for TwoLevel {
        fn levels() -> &'static [&'static str] {
            &["Off.", "On."]
        }
        fn from_index(index: usize) -> Option<Self> {
            match index {
                0 => Some(TwoLevel::Off),
                1 => Some(TwoLevel::On),
                _ => None,
            }
        }
        fn index(&self) -> usize {
            match self {
                TwoLevel::Off => 0,
                TwoLevel::On => 1,
            }
        }
    }

    #[test]
    fn test_should_reject_out_of_range_noul_probability() {
        assert!(Noul::try_new(1.5).is_err());
        assert!(Noul::try_new(-0.1).is_err());
        assert!(Noul::try_new(f64::NAN).is_err());
    }

    #[test]
    fn test_should_evaluate_is_yes_and_is_no_at_boundaries() {
        let noul = Noul::try_new(0.9).unwrap();
        assert!(noul.is_yes(0.9));
        assert!(!noul.is_yes(0.91));
        let noul = Noul::try_new(0.1).unwrap();
        assert!(noul.is_no(0.9));
        assert!(!noul.is_no(0.91));
    }

    #[test]
    fn test_should_return_none_from_decide_in_ambiguous_band() {
        let noul = Noul::try_new(0.5).unwrap();
        assert_eq!(noul.decide(0.9, 0.9), None);
    }

    #[test]
    fn test_should_return_some_true_or_false_outside_ambiguous_band() {
        assert_eq!(Noul::try_new(0.95).unwrap().decide(0.9, 0.9), Some(true));
        assert_eq!(Noul::try_new(0.05).unwrap().decide(0.9, 0.9), Some(false));
    }

    #[test]
    fn test_should_build_choice_and_reject_unknown_option() {
        let choice =
            Choice::<Team>::try_new("team", "payments", Some(0.9), IndexMap::new()).unwrap();
        assert_eq!(*choice.value(), Team::Payments);

        let err = Choice::<Team>::try_new("team", "legal", None, IndexMap::new()).unwrap_err();
        assert!(matches!(err, Error::UnknownChoice { .. }));
    }

    #[test]
    fn test_should_report_choice_confidence_and_accept_threshold() {
        let choice =
            Choice::<Team>::try_new("team", "payments", Some(0.9), IndexMap::new()).unwrap();
        assert!(choice.confident(0.8));
        assert_eq!(choice.accept(0.8), Some(&Team::Payments));
        assert_eq!(choice.accept(0.95), None);
    }

    #[test]
    fn test_should_treat_missing_confidence_as_not_confident() {
        let choice = Choice::<Team>::try_new("team", "payments", None, IndexMap::new()).unwrap();
        assert!(!choice.confident(0.0));
        assert_eq!(choice.accept(0.0), None);
    }

    #[test]
    fn test_should_return_zero_probability_for_option_without_entry() {
        let choice = Choice::<Team>::try_new("team", "payments", None, IndexMap::new()).unwrap();
        assert_eq!(choice.probability_of(&Team::Frontend), 0.0);
    }

    #[test]
    fn test_should_build_score_and_report_nearest_level() {
        let score = Score::<Urgency>::try_new("urgency", 1.9, Some(0.7), IndexMap::new()).unwrap();
        assert_eq!(score.nearest(), Urgency::High);
        assert_eq!(score.raw(), 1.9);
        assert!(score.confident(0.5));
    }

    #[test]
    fn test_should_reject_score_out_of_range() {
        assert!(Score::<Urgency>::try_new("urgency", -0.1, None, IndexMap::new()).is_err());
        assert!(Score::<Urgency>::try_new("urgency", 2.1, None, IndexMap::new()).is_err());
        assert!(Score::<Urgency>::try_new("urgency", f64::NAN, None, IndexMap::new()).is_err());
    }

    #[test]
    fn test_should_accept_score_at_boundaries() {
        assert!(Score::<Urgency>::try_new("urgency", 0.0, None, IndexMap::new()).is_ok());
        assert!(Score::<Urgency>::try_new("urgency", 2.0, None, IndexMap::new()).is_ok());
    }

    #[test]
    fn test_should_evaluate_at_least() {
        let score = Score::<Urgency>::try_new("urgency", 1.0, None, IndexMap::new()).unwrap();
        assert!(score.at_least(&Urgency::Low));
        assert!(score.at_least(&Urgency::Mid));
        assert!(!score.at_least(&Urgency::High));
    }

    #[test]
    fn test_should_normalize_two_level_scale_to_zero_or_one() {
        let low = Score::<TwoLevel>::try_new("flag", 0.0, None, IndexMap::new()).unwrap();
        let high = Score::<TwoLevel>::try_new("flag", 1.0, None, IndexMap::new()).unwrap();
        assert_eq!(low.normalized(), 0.0);
        assert_eq!(high.normalized(), 1.0);
    }

    #[test]
    fn test_should_normalize_three_level_scale() {
        let mid = Score::<Urgency>::try_new("urgency", 1.0, None, IndexMap::new()).unwrap();
        assert_eq!(mid.normalized(), 0.5);
    }

    proptest! {
        #[test]
        fn test_should_keep_normalized_within_unit_interval(raw in 0.0f64..=2.0) {
            let score = Score::<Urgency>::try_new("urgency", raw, None, IndexMap::new()).unwrap();
            let normalized = score.normalized();
            prop_assert!((0.0..=1.0).contains(&normalized));
        }
    }
}
