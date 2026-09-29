//! Runnable example: `decide_all` over a slice of states with bounded concurrency.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example batch --features "derive batch"
//! ```

use std::num::NonZeroUsize;

use jev::{Client, Decision, Noul, decide_all};

/// A minimal one-question decision: is this ticket urgent?
#[derive(Debug, Decision)]
struct QuickTriage {
    /// Does this ticket need attention within the hour?
    is_urgent: Noul,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let client = Client::from_env()?;
    let tickets: Vec<jev::State> = vec![
        "Site is down for all customers.".into(),
        "Typo in the footer copyright year.".into(),
        "Cannot log in, tried three times.".into(),
    ];

    let concurrency = NonZeroUsize::new(4).expect("4 is non-zero");
    let results = decide_all::<QuickTriage>(&client, tickets, concurrency).await;

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(triage) => println!(
                "ticket {i}: P(urgent) = {:.2}",
                triage.is_urgent.probability()
            ),
            Err(err) => eprintln!("ticket {i}: failed: {err}"),
        }
    }

    Ok(())
}
