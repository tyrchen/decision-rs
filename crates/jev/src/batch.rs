//! Bounded futures; no spawned or detached work. Results preserve input order.

use futures_util::{StreamExt, stream};

use crate::{CaseResult, CaseSet, Client, Decision, Error, Input};

fn validate(len: usize, concurrency: usize) -> Result<(), Error> {
    if len > 10_000 || !(1..=128).contains(&concurrency) {
        return Err(Error::InvalidInput("batch count or concurrency"));
    }
    Ok(())
}
/// Execute prepared inputs with bounded concurrency and per-item errors.
/// # Errors
/// Returns an outer `InvalidInput` error for excessive count or invalid concurrency.
/// Cancellation drops all unfinished request futures.
/// ```no_run
/// # async fn example<D: jev::Decision>(client: &jev::Client, inputs: &[jev::Input]) -> Result<(), jev::Error> {
/// let results = jev::decide_all::<D>(client, inputs, 4).await?;
/// # Ok(())
/// # }
/// ```
pub async fn decide_all<D: Decision>(
    client: &Client,
    inputs: &[Input],
    concurrency: usize,
) -> Result<Vec<Result<D, Error>>, Error> {
    validate(inputs.len(), concurrency)?;
    Ok(
        stream::iter(inputs.iter().map(|input| client.decide::<D>(input)))
            .buffered(concurrency)
            .collect()
            .await,
    )
}
/// Execute a validated case set, preserving order and individual failures.
/// # Errors
/// Returns an outer `InvalidInput` error for invalid concurrency or excessive count.
/// ```no_run
/// # async fn example(client: &jev::Client, cases: &jev::CaseSet) -> Result<(), jev::Error> {
/// let results = jev::run_cases(client, cases, 4).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_cases(
    client: &Client,
    cases: &CaseSet,
    concurrency: usize,
) -> Result<Vec<Result<CaseResult, Error>>, Error> {
    validate(cases.len(), concurrency)?;
    Ok(stream::iter(cases.iter().map(|case| client.run_case(case)))
        .buffered(concurrency)
        .collect()
        .await)
}
