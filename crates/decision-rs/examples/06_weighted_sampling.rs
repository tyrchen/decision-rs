//! Runnable example. Select DECISION_PROVIDER and the corresponding API key.

use anyhow::Result;
use decision_rs::{Choice, ChoiceOptions, Client, Decision, Probability};
#[derive(Debug, ChoiceOptions)]
enum Route {
    /// Billing support.
    Billing,
    /// Technical support.
    Technical,
}
#[derive(Debug, Decision)]
struct Routing {
    /// Which team should own this ticket?
    team: Choice<Route>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let result: Routing = Client::from_env()?
        .decide("Duplicate payment after a checkout error.")
        .await?;
    // The caller supplies a reproducible roll; unavailable distributions return an error.
    let sampled = result.team.sample(Probability::try_from(0.42)?)?;
    tracing::info!(?sampled, "sampled route");
    Ok(())
}
