//! Runnable example. Select JEV_PROVIDER and the corresponding API key.

use anyhow::Result;
use jev::{Client, Decision, Input, Predicate, decide_all};
#[derive(Debug, Decision)]
struct Review {
    /// Does the customer request a refund?
    refund: Predicate,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let inputs = [
        Input::try_from("Please refund me.")?,
        Input::try_from("How do I change my password?")?,
    ];
    for result in decide_all::<Review>(&Client::from_env()?, &inputs, 2).await? {
        match result {
            Ok(value) => tracing::info!(
                probability = value.refund.probability().get(),
                "batch answer"
            ),
            Err(error) => tracing::error!(%error, "batch item failed"),
        }
    }
    Ok(())
}
