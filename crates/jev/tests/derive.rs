//! Derive behavior: generated `Questions` equal a hand-built one, doc-comment
//! extraction, `rename_all`, `#[jev(optional)]`, and the `trybuild` compile-fail
//! suite for every documented invalid shape.

use jev::{Choice, ChoiceOptions, Decision, Noul, Questions, Score, ScoreLevels};

#[test]
fn test_should_fail_to_compile_every_documented_invalid_shape() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/derive-fail/missing_instructions.rs");
    t.compile_fail("tests/derive-fail/missing_variant_doc.rs");
    t.compile_fail("tests/derive-fail/data_carrying_variant.rs");
    t.compile_fail("tests/derive-fail/single_variant_enum.rs");
    t.compile_fail("tests/derive-fail/yes_without_no.rs");
    t.compile_fail("tests/derive-fail/duplicate_keys.rs");
    t.compile_fail("tests/derive-fail/unsupported_field_type.rs");
    t.compile_fail("tests/derive-fail/duplicate_question_keys.rs");
    t.compile_fail("tests/derive-fail/option_without_optional_attr.rs");
}

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
}

/// Ordered urgency scale, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Blocking revenue right now.
    BlockingRevenue,
}

#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a software defect?
    #[jev(
        yes = "The customer describes broken or unexpected product behavior.",
        no = "The customer is asking a question or requesting a feature."
    )]
    is_bug: Noul,
    /// Which team should own this ticket?
    team: Choice<Team>,
    /// How urgent is this ticket?
    urgency: Score<Urgency>,
}

#[test]
fn test_should_generate_questions_equal_to_hand_built_ones() {
    let expected = Questions::builder()
        .noul_with(
            "is_bug",
            "Is the customer reporting a software defect?",
            "The customer describes broken or unexpected product behavior.",
            "The customer is asking a question or requesting a feature.",
        )
        .choice(
            "team",
            "Which team should own this ticket?",
            [
                (
                    "payments",
                    "Checkout, billing, or payment processing issues.",
                ),
                (
                    "frontend",
                    "Rendering, layout, or browser compatibility issues.",
                ),
            ],
        )
        .score(
            "urgency",
            "How urgent is this ticket?",
            [
                "Can wait for the next release.",
                "Blocking revenue right now.",
            ],
        )
        .build()
        .unwrap();

    assert_eq!(Triage::questions(), &expected);
}

#[test]
fn test_should_extract_multi_line_and_indented_doc_comments() {
    /// Line one.
    ///     Indented continuation.
    #[derive(Debug, Decision)]
    #[allow(dead_code)] // fixture: only `questions()` metadata is checked here
    struct MultiLine {
        /// First sentence.
        ///     Second, indented, sentence.
        flag: Noul,
    }

    let questions = MultiLine::questions();
    let instructions = questions.get("flag").unwrap().instructions();
    assert_eq!(
        instructions.as_str(),
        Some("First sentence. Second, indented, sentence.")
    );
}

#[test]
fn test_should_apply_rename_all_kebab_case_to_choice_keys() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
    #[jev(rename_all = "kebab-case")]
    enum FrontEndOrBackEnd {
        /// The user-facing side.
        FrontEnd,
        /// The server-facing side.
        BackEnd,
    }

    assert_eq!(FrontEndOrBackEnd::FrontEnd.key(), "front-end");
    assert_eq!(FrontEndOrBackEnd::BackEnd.key(), "back-end");
}

#[test]
fn test_should_honor_explicit_key_override_on_choice_variant() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
    enum Team {
        /// Billing.
        #[jev(key = "pay")]
        Payments,
        /// UI.
        Frontend,
    }

    assert_eq!(Team::Payments.key(), "pay");
    assert_eq!(Team::from_key("pay"), Some(Team::Payments));
    assert_eq!(Team::from_key("payments"), None);
}

#[test]
fn test_should_tolerate_missing_answer_for_optional_field() {
    #[derive(Debug, Decision)]
    struct WithOptional {
        /// Is this a bug?
        is_bug: Noul,
        /// Ahead-of-rollout question that may not be answered yet.
        #[jev(optional)]
        is_regression: Option<Noul>,
    }

    let answers: jev::Answers =
        serde_json::from_value(serde_json::json!({"is_bug": {"type": "noul", "noul": 0.9}}))
            .unwrap();
    let decision = WithOptional::from_answers(&answers).unwrap();
    assert!(decision.is_bug.is_yes(0.5));
    assert!(decision.is_regression.is_none());
}

#[test]
fn test_should_populate_optional_field_when_answer_present() {
    #[derive(Debug, Decision)]
    struct WithOptional {
        /// Is this a bug?
        is_bug: Noul,
        /// Ahead-of-rollout question that may not be answered yet.
        #[jev(optional)]
        is_regression: Option<Noul>,
    }

    let answers: jev::Answers = serde_json::from_value(serde_json::json!({
        "is_bug": {"type": "noul", "noul": 0.9},
        "is_regression": {"type": "noul", "noul": 0.2}
    }))
    .unwrap();
    let decision = WithOptional::from_answers(&answers).unwrap();
    assert!(decision.is_bug.is_yes(0.5));
    assert!(decision.is_regression.is_some());
    assert!(decision.is_regression.unwrap().is_no(0.5));
}

#[test]
fn test_should_use_explicit_key_and_instructions_attributes() {
    #[derive(Debug, Decision)]
    #[allow(dead_code)] // fixture: only `questions()` metadata is checked here
    struct Explicit {
        #[jev(key = "custom_key", instructions = "Custom instructions here.")]
        flag: Noul,
    }

    let questions = Explicit::questions();
    assert!(questions.get("custom_key").is_some());
    assert_eq!(
        questions.get("custom_key").unwrap().instructions().as_str(),
        Some("Custom instructions here.")
    );
}

#[test]
fn test_should_build_end_to_end_from_answers() {
    let answers: jev::Answers = serde_json::from_value(serde_json::json!({
        "is_bug": {"type": "noul", "noul": 0.95},
        "team": {"type": "choice", "choice": "payments", "confidence": 0.9},
        "urgency": {"type": "score", "score": 1.0, "confidence": 0.8}
    }))
    .unwrap();

    let triage = Triage::from_answers(&answers).unwrap();
    assert!(triage.is_bug.is_yes(0.9));
    assert_eq!(*triage.team.value(), Team::Payments);
    assert_eq!(triage.urgency.nearest(), Urgency::BlockingRevenue);
}

#[test]
fn test_should_return_unknown_choice_error_for_server_option_outside_enum() {
    let answers: jev::Answers = serde_json::from_value(serde_json::json!({
        "is_bug": {"type": "noul", "noul": 0.5},
        "team": {"type": "choice", "choice": "legal"},
        "urgency": {"type": "score", "score": 0.0}
    }))
    .unwrap();

    let err = Triage::from_answers(&answers).unwrap_err();
    assert!(matches!(err, jev::Error::UnknownChoice { .. }));
}

#[test]
fn test_should_return_missing_answer_error_for_absent_required_question() {
    let answers = jev::Answers::default();
    let err = Triage::from_answers(&answers).unwrap_err();
    assert!(matches!(err, jev::Error::MissingAnswer { .. }));
}
