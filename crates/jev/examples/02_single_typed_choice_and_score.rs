//! Runnable example, level 2: a single `choice` question and a single `score`
//! question, each still without a `#[derive(Decision)]` struct — one derived enum
//! per question is enough for `Client::choice`/`Client::score` to return a typed
//! answer.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 02_single_typed_choice_and_score --features derive
//! ```

use jev::{ChoiceOptions, Client, ScoreLevels};

/// Owning team for a ticket. Derives `ChoiceOptions` on its own — no `Decision`
/// struct wraps it, since `Client::choice::<Team>` only needs one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
    /// Login, permissions, or profile issues.
    Account,
}

/// Ordered urgency scale, lowest first. Same idea as `Team`: derives `ScoreLevels`
/// standalone for a single `Client::score::<Urgency>` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Should be fixed this week.
    ThisWeek,
    /// Blocking revenue right now.
    BlockingRevenue,
}

const TICKET: &str = "Checkout is returning a 500 on every card, losing sales right now.";

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let client = Client::from_env()?;

    let team = client
        .choice::<Team>(TICKET, "Which team should own this ticket?")
        .await?;
    // `Choice<T>` prints its full distribution and confidence via `Display`.
    println!("team: {team}");
    if let Some(team) = team.accept(0.7) {
        println!("-> route to {team:?}");
    } else {
        println!("-> ambiguous, escalate to a human");
    }

    let urgency = client
        .score::<Urgency>(TICKET, "How urgent is this ticket?")
        .await?;
    println!("urgency: {urgency}");
    println!("-> nearest level: {:?}", urgency.nearest());

    Ok(())
}
