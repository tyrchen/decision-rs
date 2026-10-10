//! Runnable example. Select DECISION_PROVIDER and the corresponding API key.

use anyhow::Result;
use decision_rs::{Client, Decision, Predicate, Probability, Verdict};
#[derive(Debug, Decision)]
struct Review {
    /// Does this ticket request a refund?
    refund: Predicate,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let result: Review = Client::from_env()?
        .decide("I would like my payment back.")
        .await?;
    let gate = Probability::try_from(0.9)?;
    match result.refund.decide(gate, gate) {
        Verdict::Yes => tracing::info!("route to refund workflow"),
        Verdict::No => tracing::info!("route to general support"),
        Verdict::Unsure => tracing::info!("route to human review"),
    }
    Ok(())
}
