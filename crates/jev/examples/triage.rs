//! Runnable example: the derive path end to end.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example triage --features derive
//! ```

use jev::{Choice, ChoiceOptions, Client, Decision, Noul, Score, ScoreLevels};

/// A support ticket triage decision.
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

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let client = Client::from_env()?;
    let ticket = "Customer says checkout throws a 500 error on every card, losing sales right now.";

    let triage: Triage = client.decide(ticket).await?;

    println!("is_bug: P(yes) = {:.2}", triage.is_bug.probability());
    if triage.is_bug.is_yes(0.9) {
        println!("  -> confirmed bug");
    }

    if let Some(team) = triage.team.accept(0.7) {
        println!("team: {team:?} (confidence {:?})", triage.team.confidence());
    } else {
        println!("team: ambiguous, escalating to a human");
    }

    let urgency: Urgency = triage.urgency.nearest();
    println!(
        "urgency: {urgency:?} (raw score {:.2})",
        triage.urgency.raw()
    );

    Ok(())
}
