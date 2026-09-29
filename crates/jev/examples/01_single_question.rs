//! Runnable example, level 1 (easiest): a single yes/no question, no derive, no
//! struct — the smallest useful call this SDK offers.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 01_single_question
//! ```

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let client = jev::Client::from_env()?;

    // `Client::noul` sends one `noul` question in one request and hands back a
    // typed `Noul` directly — no `#[derive(Decision)]` struct needed for a single
    // one-off question.
    let is_refund = client
        .noul(
            "Customer: \"Please cancel my order and refund the charge to my card.\"",
            "Is the customer asking for money back?",
        )
        .await?;

    // `Noul` prints as a human-readable verdict via `Display`.
    println!("{is_refund}"); // e.g. "yes, 88% sure"
    println!("P(yes) = {:.2}", is_refund.probability());

    if is_refund.is_yes(0.8) {
        println!("-> route to billing");
    } else {
        println!("-> not confidently a refund; handle normally");
    }

    Ok(())
}
