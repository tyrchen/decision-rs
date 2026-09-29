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

    /// A one-threshold alternative to [`Noul::decide`]: reads `P(yes)` as a
    /// [`Verdict`] against a single `certainty` threshold, measured as distance from
    /// the coin flip rather than as a raw probability.
    ///
    /// Where `decide(yes_at, no_at)` takes two independent thresholds — useful when
    /// the cost of a false yes and a false no differ — `decide_verdict` takes one
    /// symmetric threshold: `certainty = 0.8` accepts [`Verdict::Yes`] from `P(yes) >=
    /// 0.9` and [`Verdict::No`] from `P(yes) <= 0.1`, leaving everything between them
    /// [`Verdict::Unsure`]. `certainty = 0.0` never returns `Unsure` and splits at
    /// `0.5`, matching [`Verdict::from_probability`]'s own default.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::{Noul, Verdict};
    ///
    /// let noul = Noul::try_new(0.94).unwrap();
    /// assert_eq!(noul.decide_verdict(0.8), Verdict::Yes);
    ///
    /// let ambiguous = Noul::try_new(0.55).unwrap();
    /// assert_eq!(ambiguous.decide_verdict(0.8), Verdict::Unsure);
    /// ```
    #[must_use]
    pub fn decide_verdict(&self, certainty: f64) -> Verdict {
        Verdict::from_probability(self.probability, certainty)
    }
}

impl std::fmt::Display for Noul {
    /// Renders as a verdict with its certainty, e.g. `"yes, 88% sure"` or
    /// `"no, 70% sure"`. See [`Verdict::from_probability`] for what "sure" measures.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let certainty = certainty_from_probability(self.probability);
        let word = if self.probability >= 0.5 { "yes" } else { "no" };
        write!(f, "{word}, {} sure", format_percent(certainty))
    }
}

/// A yes/no judgment that may also come out undecided.
///
/// Where [`Noul::is_yes`]/[`Noul::is_no`] each take their own threshold, and
/// [`Noul::decide`] takes two independent thresholds for an asymmetric risk
/// tolerance, `Verdict` is the result of [`Noul::decide_verdict`]'s single symmetric
/// threshold: a caller that falls back to non-AI behavior on an uncertain answer
/// branches on this instead of comparing probabilities by hand.
///
/// # Examples
///
/// ```
/// use jev::Verdict;
///
/// assert_eq!(Verdict::from_probability(0.94, 0.8), Verdict::Yes);
/// assert_eq!(Verdict::from_probability(0.55, 0.8), Verdict::Unsure);
/// assert_eq!(Verdict::from_probability(0.05, 0.8), Verdict::No);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Yes, at least as certain as the threshold asked for.
    Yes,
    /// Too close to the coin flip to call either way.
    Unsure,
    /// No, at least as certain as the threshold asked for.
    No,
}

impl Verdict {
    /// Reads a `P(yes)` probability as a `Verdict`, [`Verdict::Unsure`] below
    /// `certainty`.
    ///
    /// `certainty` measures distance from the coin flip (`0.0..=1.0`), not a raw
    /// probability: `0.8` accepts a yes from `P(yes) >= 0.9` and a no from `P(yes) <=
    /// 0.1`. Both arguments are clamped into `0.0..=1.0` first, so a `certainty` above
    /// `1.0` is treated as exactly `1.0`: `Unsure` for every probability except the
    /// exact extremes `0.0` and `1.0`, rather than for every probability.
    #[must_use]
    pub fn from_probability(yes: f64, certainty: f64) -> Self {
        let yes = yes.clamp(0.0, 1.0);
        let certainty = certainty.clamp(0.0, 1.0);
        let actual_certainty = certainty_from_probability(yes);
        if actual_certainty < certainty {
            Verdict::Unsure
        } else if yes >= 0.5 {
            Verdict::Yes
        } else {
            Verdict::No
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Verdict::Yes => "yes",
            Verdict::Unsure => "unsure",
            Verdict::No => "no",
        })
    }
}

/// The distance of `yes` from the coin flip (`0.5`), stretched to `0.0..=1.0`: `0.5`
/// and `1.0` (or `0.0`) map to `0.0` and `1.0` respectively.
fn certainty_from_probability(yes: f64) -> f64 {
    2.0f64.mul_add(yes, -1.0).abs()
}

/// Renders a probability in `0.0..=1.0` as a whole-percent string, e.g. `"88%"`.
fn format_percent(probability: f64) -> String {
    format!("{:.0}%", probability * 100.0)
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

    /// `Some(value)` when [`Self::accept`] confident, else `None` — the caller's cue to
    /// escalate to a human or a fallback path.
    #[must_use]
    pub fn accept(&self, threshold: f64) -> Option<&T> {
        self.confident(threshold).then(|| self.value())
    }

    /// Picks an option by its probability instead of by [`Self::value`]'s mode.
    ///
    /// Walks `T::options()` in declaration order, subtracting each option's
    /// probability (`0.0` for an option [`Self::probability_of`] has no entry for)
    /// from `roll`, and returns the option the roll lands in: an option the model
    /// gave a fifth of the mass to comes up for a fifth of the rolls. This lets a
    /// scene play out differently from one run to the next without ever picking an
    /// option the model ruled out entirely.
    ///
    /// `roll` is a value in `0.0..=1.0` supplied by the caller, so the draw stays
    /// reproducible (seeded from the caller's own RNG, a fixed test seed, or
    /// anything else) and this crate stays free of a `rand` dependency. When `roll`
    /// is outside `0.0..=1.0`, or the probabilities do not sum to at least `roll`
    /// (for example because the server reported none), [`Self::value`] comes back.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::{Choice, ChoiceOptions};
    /// use indexmap::IndexMap;
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// enum Team { Payments, Frontend }
    /// impl ChoiceOptions for Team {
    ///     fn options() -> &'static [(&'static str, &'static str)] {
    ///         &[("payments", "Billing."), ("frontend", "UI.")]
    ///     }
    ///     fn from_key(key: &str) -> Option<Self> {
    ///         match key { "payments" => Some(Team::Payments), "frontend" => Some(Team::Frontend), _ => None }
    ///     }
    ///     fn key(&self) -> &'static str {
    ///         match self { Team::Payments => "payments", Team::Frontend => "frontend" }
    ///     }
    /// }
    ///
    /// let mut probabilities = IndexMap::new();
    /// probabilities.insert("payments".to_string(), 0.7);
    /// probabilities.insert("frontend".to_string(), 0.3);
    /// let choice = Choice::<Team>::try_new("team", "payments", Some(0.7), probabilities).unwrap();
    ///
    /// assert_eq!(choice.sample(0.5), Team::Payments); // within the first 0.7
    /// assert_eq!(choice.sample(0.9), Team::Frontend); // within the last 0.3
    /// ```
    /// # Panics
    ///
    /// Never panics for a `T` whose `ChoiceOptions` impl keeps `key()`/`from_key()`
    /// consistent (guaranteed by `#[derive(ChoiceOptions)]`): `self.value.key()` is
    /// always one of `T::options()`'s keys, so the fallback `T::from_key` always
    /// succeeds. A hand-written `ChoiceOptions` impl that violates that contract is a
    /// caller-side bug this method cannot recover from.
    #[must_use]
    pub fn sample(&self, roll: f64) -> T {
        let mut remaining = roll;
        for (key, _) in T::options() {
            let probability = self.probabilities.get(*key).copied().unwrap_or(0.0);
            remaining -= probability;
            if remaining < 0.0 {
                // `key` came from `T::options()`, so `from_key` always succeeds here.
                if let Some(value) = T::from_key(key) {
                    return value;
                }
            }
        }
        // Roll exceeded the total probability mass (or there was none reported):
        // fall back to a value equivalent to the answer's own selection.
        T::from_key(self.value.key()).unwrap_or_else(|| unreachable_choice_key(self.value.key()))
    }
}

/// Panics: only reachable from a hand-written `ChoiceOptions` impl whose `key()` and
/// `from_key()` disagree, which `#[derive(ChoiceOptions)]` never produces. See
/// [`Choice::sample`]'s `# Panics` section.
fn unreachable_choice_key<T>(key: &str) -> T {
    panic!(
        "ChoiceOptions::from_key({key:?}) must succeed: key() must return one of T::options()'s \
         own keys"
    )
}

impl<T: ChoiceOptions + std::fmt::Debug> std::fmt::Display for Choice<T> {
    /// Renders as the selected option followed by the full distribution and
    /// confidence, e.g. `"Payments (payments 88%, frontend 12%; confidence 82%)"`.
    /// Probabilities are listed in `T::options()` declaration order.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} (", self.value)?;
        for (index, (key, _)) in T::options().iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            let probability = self.probabilities.get(*key).copied().unwrap_or(0.0);
            write!(f, "{key} {}", format_percent(probability))?;
        }
        let confidence = self
            .confidence
            .map_or_else(|| "n/a".to_string(), format_percent);
        write!(f, "; confidence {confidence})")
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

impl<T: ScoreLevels> std::fmt::Display for Score<T> {
    /// Renders as the raw score followed by each level's probability and the
    /// confidence, e.g. `"1.88 (harmless 0%, wary 12%, lethal 88%; confidence
    /// 82%)"`. Levels are listed in `T::levels()` declaration order; a level with no
    /// reported probability shows `0%`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2} (", self.raw)?;
        for (index, level) in T::levels().iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            let probability = self
                .probabilities
                .get(&index.to_string())
                .copied()
                .unwrap_or(0.0);
            write!(f, "{level} {}", format_percent(probability))?;
        }
        let confidence = self
            .confidence
            .map_or_else(|| "n/a".to_string(), format_percent);
        write!(f, "; confidence {confidence})")
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

    #[test]
    fn test_should_compute_verdict_from_probability() {
        assert_eq!(Verdict::from_probability(0.94, 0.8), Verdict::Yes);
        assert_eq!(Verdict::from_probability(0.55, 0.8), Verdict::Unsure);
        assert_eq!(Verdict::from_probability(0.05, 0.8), Verdict::No);
    }

    #[test]
    fn test_should_never_return_unsure_for_zero_certainty_threshold() {
        assert_eq!(Verdict::from_probability(0.51, 0.0), Verdict::Yes);
        assert_eq!(Verdict::from_probability(0.49, 0.0), Verdict::No);
    }

    #[test]
    fn test_should_return_unsure_for_non_extreme_probability_at_certainty_above_one() {
        // Clamped threshold of 1.0 requires certainty == 1.0, achievable only at the
        // probability extremes (exactly 0.0 or 1.0); anything else is Unsure.
        assert_eq!(Verdict::from_probability(0.99, 1.5), Verdict::Unsure);
        assert_eq!(Verdict::from_probability(0.01, 1.5), Verdict::Unsure);
    }

    #[test]
    fn test_should_clamp_out_of_range_probability_for_verdict() {
        assert_eq!(Verdict::from_probability(1.5, 0.0), Verdict::Yes);
        assert_eq!(Verdict::from_probability(-0.5, 0.0), Verdict::No);
    }

    #[test]
    fn test_should_derive_verdict_from_noul() {
        let noul = Noul::try_new(0.94).unwrap();
        assert_eq!(noul.decide_verdict(0.8), Verdict::Yes);
        let ambiguous = Noul::try_new(0.55).unwrap();
        assert_eq!(ambiguous.decide_verdict(0.8), Verdict::Unsure);
    }

    #[test]
    fn test_should_display_verdict_as_lowercase_word() {
        assert_eq!(Verdict::Yes.to_string(), "yes");
        assert_eq!(Verdict::Unsure.to_string(), "unsure");
        assert_eq!(Verdict::No.to_string(), "no");
    }

    #[test]
    fn test_should_display_noul_as_verdict_with_certainty() {
        let noul = Noul::try_new(0.94).unwrap();
        assert_eq!(noul.to_string(), "yes, 88% sure");
        let noul = Noul::try_new(0.15).unwrap();
        assert_eq!(noul.to_string(), "no, 70% sure");
    }

    #[test]
    fn test_should_sample_choice_within_first_probability_band() {
        let mut probabilities = IndexMap::new();
        probabilities.insert("payments".to_string(), 0.7);
        probabilities.insert("frontend".to_string(), 0.3);
        let choice = Choice::<Team>::try_new("team", "payments", Some(0.7), probabilities).unwrap();

        assert_eq!(choice.sample(0.0), Team::Payments);
        assert_eq!(choice.sample(0.69), Team::Payments);
        assert_eq!(choice.sample(0.71), Team::Frontend);
        assert_eq!(choice.sample(0.99), Team::Frontend);
    }

    #[test]
    fn test_should_sample_choice_deterministically_for_same_roll() {
        let mut probabilities = IndexMap::new();
        probabilities.insert("payments".to_string(), 0.5);
        probabilities.insert("frontend".to_string(), 0.5);
        let choice = Choice::<Team>::try_new("team", "payments", None, probabilities).unwrap();

        assert_eq!(choice.sample(0.3), choice.sample(0.3));
    }

    #[test]
    fn test_should_fall_back_to_value_when_sample_roll_exceeds_probability_mass() {
        // No probabilities reported at all: any roll falls back to `value()`.
        let choice = Choice::<Team>::try_new("team", "payments", None, IndexMap::new()).unwrap();
        assert_eq!(choice.sample(0.5), Team::Payments);
        assert_eq!(choice.sample(1.5), Team::Payments);
    }

    #[test]
    fn test_should_display_choice_with_ranked_probabilities_and_confidence() {
        let mut probabilities = IndexMap::new();
        probabilities.insert("payments".to_string(), 0.88);
        probabilities.insert("frontend".to_string(), 0.12);
        let choice =
            Choice::<Team>::try_new("team", "payments", Some(0.82), probabilities).unwrap();
        assert_eq!(
            choice.to_string(),
            "Payments (payments 88%, frontend 12%; confidence 82%)"
        );
    }

    #[test]
    fn test_should_display_choice_without_confidence_as_not_applicable() {
        let choice = Choice::<Team>::try_new("team", "payments", None, IndexMap::new()).unwrap();
        assert_eq!(
            choice.to_string(),
            "Payments (payments 0%, frontend 0%; confidence n/a)"
        );
    }

    #[test]
    fn test_should_display_score_with_level_probabilities_and_confidence() {
        let mut probabilities = IndexMap::new();
        probabilities.insert("0".to_string(), 0.0);
        probabilities.insert("1".to_string(), 0.12);
        probabilities.insert("2".to_string(), 0.88);
        let score = Score::<Urgency>::try_new("urgency", 1.88, Some(0.82), probabilities).unwrap();
        assert_eq!(
            score.to_string(),
            "1.88 (Low. 0%, Mid. 12%, High. 88%; confidence 82%)"
        );
    }
}
