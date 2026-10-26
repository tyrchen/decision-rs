//! Self-hosted Clef through a joint-head-aware System One server (for example SGLang).
//! The server must already be running locally; no Cloudflare account is needed.

use anyhow::Result;
use decision_rs::{Auth, Client, Decision, Endpoint, Predicate};
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
    let endpoint = Endpoint::clef_system_one("http://127.0.0.1:30000/v1/systemone")?;
    let client = Client::for_endpoint(endpoint, Auth::None, "Cloudflare/clef".parse()?)?;
    let result: Review = client.decide("Please refund the duplicate charge.").await?;
    tracing::info!(
        probability = result.refund.probability().get(),
        "self-hosted answer"
    );
    Ok(())
}
