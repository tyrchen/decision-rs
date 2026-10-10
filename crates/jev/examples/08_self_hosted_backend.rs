//! Runnable example. Select JEV_PROVIDER and the corresponding API key.

use anyhow::Result;
use jev::{Auth, Client, Decision, Endpoint, Predicate};
#[derive(Debug, Decision)]
struct Review {
    /// Is the ticket a refund request?
    refund: Predicate,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let endpoint = Endpoint::system_one("http://127.0.0.1:8080/v1/systemone")?;
    let client = Client::for_endpoint(endpoint, Auth::None, "local-decision-model".parse()?)?;
    let result: Review = client.decide("Please refund the duplicate charge.").await?;
    tracing::info!(
        probability = result.refund.probability().get(),
        "self-hosted answer"
    );
    Ok(())
}
