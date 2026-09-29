//! Live integration tests against the real Jev API. Ignored by default — run with
//! `cargo test -p jev -- --ignored` and `JEV_API_KEY` (or `OPENROUTER_API_KEY`) set
//! in the environment. Also exercised by `make test-live`.

use jev::{Choice, ChoiceOptions, Client, Decision, Model, Noul, Score, ScoreLevels, Surface};

/// A support ticket triage decision, matching the crate-level docs example.
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

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
    /// Login, permissions, or profile issues.
    Account,
}

/// Ordered urgency scale, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Should be fixed this week.
    ThisWeek,
    /// Blocking revenue right now.
    BlockingRevenue,
}

const CHECKOUT_OUTAGE_TICKET: &str =
    "Customer says checkout throws a 500 error on every card, losing sales right now. This is \
     clearly a defect, it started an hour ago and is blocking all revenue on the payments page.";

#[tokio::test]
#[ignore = "requires a live JEV_API_KEY / OPENROUTER_API_KEY and makes a billed request"]
async fn test_should_triage_a_clear_cut_bug_report_correctly() {
    let client = Client::from_env().expect("JEV_API_KEY or OPENROUTER_API_KEY must be set");

    let triage: Triage = client
        .decide(CHECKOUT_OUTAGE_TICKET)
        .await
        .expect("live decide request failed");

    assert!(
        triage.is_bug.probability() > 0.5,
        "expected P(is_bug) > 0.5, got {}",
        triage.is_bug.probability()
    );
    assert_eq!(*triage.team.value(), Team::Payments);
    assert_eq!(triage.urgency.nearest(), Urgency::BlockingRevenue);
}

#[tokio::test]
#[ignore = "requires a live JEV_API_KEY / OPENROUTER_API_KEY and makes a billed request"]
async fn test_should_report_usage_on_live_response() {
    let client = Client::from_env().expect("JEV_API_KEY or OPENROUTER_API_KEY must be set");

    let decided = client
        .decide_with::<Triage>(jev::RequestOptions::new(CHECKOUT_OUTAGE_TICKET))
        .await
        .expect("live decide_with request failed");

    assert!(decided.meta.usage.input_tokens > 0);
}

#[tokio::test]
#[ignore = "requires a live JEV_API_KEY / OPENROUTER_API_KEY and makes a billed request"]
async fn test_should_return_api_error_for_invalid_model() {
    let client = Client::from_env()
        .expect("JEV_API_KEY or OPENROUTER_API_KEY must be set")
        .with_model(Model::Custom("this/model-does-not-exist-v99".to_string()));

    let result: Result<Triage, _> = client.decide(CHECKOUT_OUTAGE_TICKET).await;
    let err = result.expect_err("an invalid model should fail");
    let status = err.status().expect("expected an HTTP-status-bearing error");
    assert!(
        (400..500).contains(&status),
        "expected a 4xx error, got {status}"
    );
}

#[tokio::test]
#[ignore = "requires a live JEV_API_KEY / OPENROUTER_API_KEY and makes a billed request"]
async fn test_should_accept_custom_model_on_system_one_surface() {
    let client = Client::from_env()
        .expect("JEV_API_KEY or OPENROUTER_API_KEY must be set")
        .with_surface(Surface::SystemOne)
        .with_model(Model::Custom("jev-1.13".to_string()));

    let triage: Triage = client
        .decide(CHECKOUT_OUTAGE_TICKET)
        .await
        .expect("live decide request failed");
    assert!(triage.is_bug.probability() > 0.5);
}
