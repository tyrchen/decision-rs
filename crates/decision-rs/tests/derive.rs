//! Compile-time schemas and safe handwritten traits.

use decision_rs::{
    Choice, ChoiceOptions, Decision, Error, LevelSpec, Outcome, Predicate, Questions, Score,
    ScoreLevels,
};

#[derive(Debug, ChoiceOptions)]
#[decision(rename_all = "kebab-case")]
/// Choice schema fixture.
pub enum Route {
    /// Billing assistance.
    BillingTeam,
    /// Technical support.
    #[decision(key = "TECH")]
    TechnicalTeam,
}
#[derive(Debug, ScoreLevels)]
/// Ordered score fixture.
pub enum Priority {
    /// Routine.
    #[decision(key = "routine")]
    Low,
    /// Urgent.
    High,
}
#[derive(Debug, Decision)]
/// Valid typed schema fixture.
pub struct Review {
    /// First question.
    #[decision(key = "first-question", yes = "Yes case.", no = "No case.")]
    pub first: Predicate,
    /// Routing question.
    pub route: Outcome<Choice<Route>>,
    /// Priority question.
    pub priority: Score<Priority>,
}
#[test]
fn test_should_cache_validated_schema_and_stable_enum_keys() {
    let schema = Review::questions().unwrap();
    assert!(std::ptr::eq(schema, Review::questions().unwrap()));
    assert_eq!(
        schema.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["first-question", "route", "priority"]
    );
    assert_eq!(
        Route::options(),
        &[
            ("billing-team", "Billing assistance."),
            ("TECH", "Technical support.")
        ]
    );
    assert_eq!(Route::TechnicalTeam.key(), "TECH");
    assert_eq!(
        Priority::levels(),
        &[
            LevelSpec::new("routine", "Routine."),
            LevelSpec::new("high", "Urgent.")
        ]
    );
}
#[derive(Debug)]
/// Intentionally invalid handwritten schema.
pub enum InvalidLevels {
    /// First entry.
    First,
    /// Second entry.
    Second,
}
impl ScoreLevels for InvalidLevels {
    fn levels() -> &'static [LevelSpec] {
        const LEVELS: &[LevelSpec] = &[
            LevelSpec::new("duplicate", "First."),
            LevelSpec::new("duplicate", "Second."),
        ];
        LEVELS
    }
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::First),
            1 => Some(Self::Second),
            _ => None,
        }
    }
    fn index(&self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}
#[derive(Debug, Decision)]
/// Runtime validation fixture.
pub struct InvalidSchema {
    /// Question whose handwritten levels are invalid.
    pub value: Score<InvalidLevels>,
}
#[test]
fn test_should_cache_schema_errors_without_panicking() {
    assert!(matches!(
        InvalidSchema::questions(),
        Err(Error::InvalidSchema(_))
    ));
    assert!(matches!(
        InvalidSchema::questions(),
        Err(Error::InvalidSchema(_))
    ));
}
#[test]
fn test_should_enforce_dynamic_cardinality_limits() {
    assert!(
        Questions::builder()
            .choice(
                "q",
                "Question",
                (0..65).map(|i| (i.to_string(), "Description"))
            )
            .build()
            .is_err()
    );
    assert!(
        Questions::builder()
            .score(
                "q",
                "Question",
                (0..33).map(|i| (i.to_string(), "Description"))
            )
            .build()
            .is_err()
    );
    let mut builder = Questions::builder();
    for i in 0..65 {
        builder = builder.predicate(&i.to_string(), "Question");
    }
    assert!(builder.build().is_err());
}
#[test]
fn test_should_report_invalid_derive_usage() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/derive-fail/*.rs");
}
