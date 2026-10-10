//! Runnable example. Select DECISION_PROVIDER and the corresponding API key.

use anyhow::Result;
use decision_rs::{Client, Decision, Input, Outcome, Predicate, RequestOptions};
#[derive(Debug, Decision)]
struct Triage {
    /// Does the ticket describe a product defect?
    #[decision(
        yes = "Describes broken product behavior.",
        no = "Requests information or a new feature."
    )]
    defect: Predicate,
    /// Does the ticket explicitly request money back?
    refund: Outcome<Predicate>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let input =
        Input::json(&serde_json::json!({"subject":"Broken screen", "body":"Please refund me."}))?;
    let result = Client::from_env()?
        .decide_with::<Triage>(RequestOptions::new(input)?)
        .await?;
    tracing::info!(defect = result.value.defect.probability().get(), refund = ?result.value.refund, request_id = ?result.meta.request_id, "triage completed");
    Ok(())
}
