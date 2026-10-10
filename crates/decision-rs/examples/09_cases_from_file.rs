//! Runnable example. Select DECISION_PROVIDER and the corresponding API key.

use anyhow::Result;
use decision_rs::{CaseSet, Client, run_cases};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let path = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!(
            "usage: cargo run --example 09_cases_from_file --features batch,yaml -- cases.yaml"
        )
    })?;
    let cases = CaseSet::from_file(path).await?;
    for result in run_cases(&Client::from_env()?, &cases, 4).await? {
        match result {
            Ok(value) => {
                tracing::info!(case = %value.id, answers = ?value.response.answers, "case completed")
            }
            Err(error) => tracing::error!(%error, "case failed"),
        }
    }
    Ok(())
}
