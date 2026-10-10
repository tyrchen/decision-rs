//! Provider-neutral question schemas, validated once at construction.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::error::SchemaError;

/// Validated natural-language instructions.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GuidanceText(String);
impl std::fmt::Debug for GuidanceText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuidanceText")
            .field("bytes", &self.0.len())
            .finish()
    }
}
impl GuidanceText {
    /// Borrow instructions without exposing them through Debug.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::GuidanceText) -> Result<(), decision_rs::Error> {
    /// let instructions = value.as_str();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for GuidanceText {
    type Error = SchemaError;
    fn try_from(value: &str) -> Result<Self, SchemaError> {
        if value.trim().is_empty()
            || value.len() > 4096
            || value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(SchemaError(
                "guidance must be nonempty, at most 4096 bytes, without abnormal controls",
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

/// One stable ordered score level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LevelSpec {
    /// Stable label key; declaration order determines the numeric position.
    pub key: &'static str,
    /// Human-readable criteria.
    pub description: &'static str,
}
impl LevelSpec {
    /// Declare a level; Questions validates its contents before use.
    ///
    /// ```
    /// let level = decision_rs::LevelSpec::new("low", "Can wait.");
    /// assert_eq!(level.key, "low");
    /// ```
    #[must_use]
    pub const fn new(key: &'static str, description: &'static str) -> Self {
        Self { key, description }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Predicate {
        criteria: Option<(GuidanceText, GuidanceText)>,
    },
    Choice(Vec<(String, GuidanceText)>),
    Score(Vec<(String, GuidanceText)>),
}
/// Read-only validated question.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Question {
    pub(crate) instructions: GuidanceText,
    pub(crate) kind: Kind,
}
impl Question {
    /// The model instructions.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Question) -> Result<(), decision_rs::Error> {
    /// let instructions = value.instructions().as_str();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn instructions(&self) -> &GuidanceText {
        &self.instructions
    }
    /// The provider-neutral question kind.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Question) -> Result<(), decision_rs::Error> {
    /// let kind = value.kind();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self.kind {
            Kind::Predicate { .. } => "predicate",
            Kind::Choice(_) => "choice",
            Kind::Score(_) => "score",
        }
    }
}
/// Immutable declaration-ordered schema.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Questions(pub(crate) IndexMap<String, Question>);
impl Questions {
    /// Start a schema builder.
    ///
    /// ```
    /// let questions = decision_rs::Questions::builder().predicate("bug", "Is this a bug?").build()?;
    /// assert_eq!(questions.len(), 1);
    /// # Ok::<(), decision_rs::SchemaError>(())
    /// ```
    #[must_use]
    pub fn builder() -> QuestionsBuilder {
        QuestionsBuilder::default()
    }
    /// Number of questions.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Questions) -> Result<(), decision_rs::Error> {
    /// let count = value.len();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether this schema contains no questions.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Questions) -> Result<(), decision_rs::Error> {
    /// let empty = value.is_empty();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Read a question by key.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Questions) -> Result<(), decision_rs::Error> {
    /// let question = value.get("refund");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Question> {
        self.0.get(key)
    }
    /// Iterate in declaration order.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Questions) -> Result<(), decision_rs::Error> {
    /// for (key, question) in value { let instructions = question.instructions(); }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn iter(&self) -> indexmap::map::Iter<'_, String, Question> {
        self.0.iter()
    }
}
/// A collecting builder that retains the first error and bounds allocations.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct QuestionsBuilder {
    entries: IndexMap<String, Question>,
    error: Option<SchemaError>,
}
impl QuestionsBuilder {
    fn add(mut self, key: &str, instructions: &str, kind: Result<Kind, SchemaError>) -> Self {
        if self.error.is_some() {
            return self;
        }
        let result = (|| {
            validate_key(key)?;
            if self.entries.len() >= 64 || self.entries.contains_key(key) {
                return Err(SchemaError("too many or duplicate questions"));
            }
            let instructions = GuidanceText::try_from(instructions)?;
            Ok(Question {
                instructions,
                kind: kind?,
            })
        })();
        match result {
            Ok(q) => {
                self.entries.insert(key.to_owned(), q);
            }
            Err(e) => self.error = Some(e),
        }
        self
    }
    /// Add a condition without separate yes/no criteria.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let questions = decision_rs::Questions::builder().predicate("refund", "Refund requested?").build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn predicate(self, key: &str, instructions: &str) -> Self {
        self.add(key, instructions, Ok(Kind::Predicate { criteria: None }))
    }
    /// Add a condition with both branch descriptions.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let questions = decision_rs::Questions::builder().predicate_with("refund", "Refund requested?", "Money back requested.", "No money back requested.").build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn predicate_with(self, key: &str, instructions: &str, yes: &str, no: &str) -> Self {
        let kind = GuidanceText::try_from(yes).and_then(|yes| {
            GuidanceText::try_from(no).map(|no| Kind::Predicate {
                criteria: Some((yes, no)),
            })
        });
        self.add(key, instructions, kind)
    }
    /// Add an unordered fixed set of choices.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let questions = decision_rs::Questions::builder().choice("team", "Which team?", [("billing", "Payments."), ("technical", "Product failures.")]).build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn choice<K: AsRef<str>, G: AsRef<str>, I: IntoIterator<Item = (K, G)>>(
        self,
        key: &str,
        instructions: &str,
        options: I,
    ) -> Self {
        self.add(
            key,
            instructions,
            collect_options(options, 64).map(Kind::Choice),
        )
    }
    /// Add ordered levels as stable key and description pairs.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let questions = decision_rs::Questions::builder().score("urgency", "How urgent?", [("low", "Can wait."), ("high", "Immediate help.")]).build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn score<K: AsRef<str>, G: AsRef<str>, I: IntoIterator<Item = (K, G)>>(
        self,
        key: &str,
        instructions: &str,
        levels: I,
    ) -> Self {
        self.add(
            key,
            instructions,
            collect_options(levels, 32).map(Kind::Score),
        )
    }
    /// Freeze a validated schema.
    ///
    /// # Errors
    /// Rejects duplicate keys, invalid descriptions, empty schemas, and excess counts.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let builder = decision_rs::Questions::builder().predicate("refund", "Refund requested?");
    /// let questions = builder.build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Result<Questions, SchemaError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.entries.is_empty() {
            return Err(SchemaError("at least one question required"));
        }
        Ok(Questions(self.entries))
    }
}
pub(crate) fn validate_key(key: &str) -> Result<(), SchemaError> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(SchemaError(
            "key must be 1..64 ASCII letters, digits, underscores or hyphens",
        ));
    }
    Ok(())
}
fn collect_options<K: AsRef<str>, G: AsRef<str>, I: IntoIterator<Item = (K, G)>>(
    items: I,
    max: usize,
) -> Result<Vec<(String, GuidanceText)>, SchemaError> {
    let mut result = Vec::new();
    let mut keys = HashSet::new();
    for (key, text) in items {
        if result.len() >= max {
            return Err(SchemaError("too many choices or levels"));
        }
        let key = key.as_ref();
        validate_key(key)?;
        if !keys.insert(key.to_owned()) {
            return Err(SchemaError("duplicate choice or level key"));
        }
        result.push((key.to_owned(), GuidanceText::try_from(text.as_ref())?));
    }
    if result.len() < 2 {
        return Err(SchemaError("at least two choices or levels required"));
    }
    Ok(result)
}

impl<'a> IntoIterator for &'a Questions {
    type Item = (&'a String, &'a Question);
    type IntoIter = indexmap::map::Iter<'a, String, Question>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
