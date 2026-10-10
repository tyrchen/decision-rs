//! Opt-in hosted protocol smoke tests. Credentials select a provider explicitly.

use jev::{
    Choice, ChoiceOptions, Client, Decision, Outcome, Predicate, Provider, RequestOptions, Score,
    ScoreLevels,
};
use rstest::rstest;

#[derive(Debug, ChoiceOptions)]
enum Team {
    /// Payments and refunds.
    Billing,
    /// Product failures.
    Technical,
}
#[derive(Debug, ScoreLevels)]
enum Urgency {
    /// Can wait for regular support.
    Low,
    /// Immediate support is required.
    High,
}
#[derive(Debug, Decision)]
struct Review {
    /// Does the customer describe a defective product?
    defect: Outcome<Predicate>,
    /// Does the customer explicitly request a refund?
    #[jev(
        yes = "Explicitly requests money back.",
        no = "Does not request money back."
    )]
    refund: Outcome<Predicate>,
    /// Which team should help?
    team: Outcome<Choice<Team>>,
    /// How urgent is this request?
    urgency: Outcome<Score<Urgency>>,
}
#[rstest]
#[case(Provider::OpenAi, "OPENAI_API_KEY")]
#[case(Provider::OpenRouter, "OPENROUTER_API_KEY")]
#[case(Provider::TypeSafe, "TYPESAFE_API_KEY")]
#[tokio::test]
#[ignore = "requires a selected hosted credential; may incur API charges"]
async fn test_should_accept_hosted_protocol(#[case] provider: Provider, #[case] key_name: &str) {
    let key = std::env::var(key_name).expect("selected provider credential is required");
    let client = Client::new(provider, key).unwrap();
    let result = client
        .decide_with::<Review>(
            RequestOptions::new("The screen arrived broken. Please refund me.").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.meta.provider, Some(provider));
    assert!(!result.meta.returned_model.is_empty());
    assert!(matches!(result.value.defect, Outcome::Answered(_)));
    assert!(matches!(result.value.refund, Outcome::Answered(_)));
    assert!(matches!(result.value.team, Outcome::Answered(_)));
    assert!(matches!(result.value.urgency, Outcome::Answered(_)));
}
