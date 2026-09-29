//! Runnable example: predefined cases loaded from a YAML fixture file and run with
//! bounded concurrency.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 09_cases_from_file --features "batch yaml"
//! ```

use std::num::NonZeroUsize;

use jev::{CaseSet, Client, run_cases};

/// A batch of support tickets and the questions to ask about each, checked in next
/// to this example. A real application would ship the same shape as a fixture file
/// (regression suite) or a config-driven question set (production batch job).
const CASES_YAML: &str = r#"
version: 1
cases:
  - id: ticket-001
    state: "Customer wants a refund for a duplicate charge."
    questions:
      refund:
        type: noul
        instructions: "Is the customer asking for money back?"
      team:
        type: choice
        instructions: "Which team should own this ticket?"
        criteria:
          payments: "Checkout, billing, or payment processing issues."
          frontend: "Rendering, layout, or browser compatibility issues."
  - id: ticket-002
    name: "Login outage report"
    state:
      subject: "Login broken"
      body: "Cannot log in after the last release."
    questions:
      urgency:
        type: score
        instructions: "How urgent is this?"
        criteria: ["Can wait", "This week", "Blocking revenue"]
"#;

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    // `CaseSet::from_yaml_str` validates every case's questions eagerly (key charset,
    // guidance length, option/level counts), so a malformed fixture fails here, before
    // any request is sent. Loading from a checked-in file instead is
    // `CaseSet::from_file("cases.yaml").await?`.
    let cases = CaseSet::from_yaml_str(CASES_YAML)?;
    let client = Client::from_env()?;

    let concurrency = NonZeroUsize::new(4).expect("4 is non-zero");
    let results = run_cases(&client, cases.as_slice(), concurrency).await;

    for result in results {
        match result {
            Ok(case) => {
                println!("case {}: {} answers", case.id, case.response.answers.len());
                for (key, answer) in &case.response.answers {
                    println!("  {key}: {answer:?}");
                }
            }
            Err(err) => eprintln!("case failed: {err}"),
        }
    }

    Ok(())
}
