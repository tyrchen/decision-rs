//! Runnable example. Select JEV_PROVIDER and the corresponding API key.

use anyhow::Result;
use jev::{Choice, ChoiceOptions, Client, Decision, Score, ScoreLevels};
/// Routing categories.
#[derive(Debug, ChoiceOptions)]
enum Team {
    /// Payments, invoices and refunds.
    Billing,
    /// Product failures and technical assistance.
    Technical,
}
/// Ordered urgency.
#[derive(Debug, ScoreLevels)]
enum Urgency {
    /// Routine support.
    Low,
    /// Immediate help.
    High,
}
#[derive(Debug, Decision)]
struct Routing {
    /// Which team should own this ticket?
    team: Choice<Team>,
    /// How urgent is this ticket?
    urgency: Score<Urgency>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let result: Routing = Client::from_env()?
        .decide("Checkout is broken for every customer.")
        .await?;
    tracing::info!(team = ?result.team.value(), urgency = ?result.urgency.nearest()?, score = result.urgency.raw(), "routing decision");
    Ok(())
}
