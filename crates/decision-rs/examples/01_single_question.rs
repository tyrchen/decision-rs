//! Runnable example. Select DECISION_PROVIDER and the corresponding API key.

use anyhow::Result;
use decision_rs::{Client, Predicate, Questions};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let questions = Questions::builder()
        .predicate("refund", "Does the customer request money back?")
        .build()?;
    let response = Client::from_env()?
        .ask("Please refund the duplicate payment.", &questions)
        .await?;
    let refund: Predicate = response.answers.decode("refund")?;
    tracing::info!(
        probability = refund.probability().get(),
        tokens = response.meta.usage.input_tokens,
        "refund decision"
    );
    Ok(())
}
