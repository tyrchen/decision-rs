//! Runnable example. Select JEV_PROVIDER and the corresponding API key.

use anyhow::Result;
use jev::{Client, Questions};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let questions = Questions::builder()
        .predicate_with(
            "refund",
            "Is money back requested?",
            "Explicit refund request.",
            "No refund request.",
        )
        .choice(
            "team",
            "Which team?",
            [("billing", "Payments."), ("technical", "Product failures.")],
        )
        .score(
            "urgency",
            "How urgent?",
            [("low", "Can wait."), ("high", "Immediate attention.")],
        )
        .build()?;
    let response = Client::from_env()?
        .ask("Payment failed and I was charged twice.", &questions)
        .await?;
    tracing::info!(answers = ?response.answers, model = %response.meta.returned_model, "dynamic decision");
    Ok(())
}
