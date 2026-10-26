//! Opt-in hosted protocol smoke tests. Credentials select a provider explicitly.

use decision_rs::{
    Answer, Choice, ChoiceOptions, Client, Decision, Distribution, Input, Model, Outcome,
    Predicate, Provider, RequestOptions, Score, ScoreLevels,
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
    #[decision(
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
    check_hosted_model(provider, key_name, Model::Default).await;
}

#[rstest]
#[case(Provider::OpenRouter, "OPENROUTER_API_KEY", Model::Jev114)]
#[case(Provider::OpenRouter, "OPENROUTER_API_KEY", Model::JevLatest)]
#[case(Provider::TypeSafe, "TYPESAFE_API_KEY", Model::Jev114)]
#[case(Provider::TypeSafe, "TYPESAFE_API_KEY", Model::JevLatest)]
#[tokio::test]
#[ignore = "requires a selected Jev provider credential; may incur API charges"]
async fn test_should_accept_jev_114_protocol(
    #[case] provider: Provider,
    #[case] key_name: &str,
    #[case] model: Model,
) {
    check_hosted_model(provider, key_name, model).await;
}

async fn check_hosted_model(provider: Provider, key_name: &str, model: Model) {
    let key = std::env::var(key_name).expect("selected provider credential is required");
    let client = Client::builder(provider)
        .api_key(key)
        .model(model)
        .build()
        .unwrap();
    let result = client
        .decide_with::<Review>(
            RequestOptions::new("The screen arrived broken. Please refund me.").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.meta.provider, Some(provider));
    assert_eq!(&result.meta.resolved_model, client.model());
    if client.model() == &Model::Jev114 {
        assert!(result.meta.returned_model.contains("jev-1.14"));
    }
    assert!(!result.meta.returned_model.is_empty());
    assert!(matches!(result.value.defect, Outcome::Answered(_)));
    assert!(matches!(result.value.refund, Outcome::Answered(_)));
    assert!(matches!(result.value.team, Outcome::Answered(_)));
    assert!(matches!(result.value.urgency, Outcome::Answered(_)));
}

/// Explicitly opt-in: synthetic inputs only; three Workers AI models incur charges.
#[rstest]
#[case(Model::Clef)]
#[case(Model::ClefFlash)]
#[case(Model::ClefOmni)]
#[tokio::test]
#[ignore = "requires CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID; incurs API charges"]
async fn test_should_accept_cloudflare_clef_protocol(#[case] model: Model) {
    let token = std::env::var("CLOUDFLARE_API_TOKEN").expect("Cloudflare token required");
    let account = std::env::var("CLOUDFLARE_ACCOUNT_ID")
        .expect("Cloudflare account required")
        .parse()
        .unwrap();
    let client = Client::builder(Provider::Cloudflare)
        .cloudflare_account(account)
        .api_key(token)
        .model(model.clone())
        .build()
        .unwrap();
    let inputs = [
        Input::try_from("The screen arrived broken. Please refund me.").unwrap(),
        Input::try_from("屏幕收到时已经损坏，请退款。").unwrap(),
        Input::json(&serde_json::json!({"ticket":"Refund for broken screen", "urgent":true}))
            .unwrap(),
    ];
    for input in inputs {
        let result = client
            .ask(input, Review::questions().unwrap())
            .await
            .unwrap();
        assert_eq!(result.meta.provider, Some(Provider::Cloudflare));
        assert_eq!(result.meta.resolved_model, model);
        assert_eq!(result.meta.wire_model, model.to_string());
        assert_eq!(
            result.meta.routing_model,
            Some(format!("@cf/cloudflare/{model}"))
        );
        assert!(!result.meta.returned_model.is_empty());
        assert!(result.meta.usage.input_tokens > 0);
        for (_, answer) in result.answers.iter() {
            match answer {
                Answer::Predicate(p) => assert!((0.0..=1.0).contains(&p.probability().get())),
                Answer::Choice(c) => {
                    assert!(matches!(c.probabilities(), Distribution::Reported(_)))
                }
                Answer::Score(s) => assert!(matches!(s.probabilities(), Distribution::Reported(_))),
                _ => panic!("synthetic smoke requires answered questions"),
            }
        }
        Review::from_answers(&result.answers).unwrap();
    }
    let override_model = if model == Model::ClefFlash {
        Model::ClefOmni
    } else {
        Model::ClefFlash
    };
    let result = client
        .decide_with::<Review>(
            RequestOptions::new("Refund the broken screen.")
                .unwrap()
                .model(override_model.clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.meta.resolved_model, override_model);
    assert_eq!(
        result.meta.routing_model,
        Some(format!("@cf/cloudflare/{override_model}"))
    );
    let restored = client
        .decide_with::<Review>(RequestOptions::new("Refund the broken screen.").unwrap())
        .await
        .unwrap();
    assert_eq!(restored.meta.resolved_model, model);
    assert_eq!(client.model(), &model);
}
