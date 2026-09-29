//! Runnable example, level 5: reading `Thresholds` from configuration and using
//! `Verdict`/`decide_verdict`/`accept` to escalate ambiguous answers instead of
//! guessing. See `docs/thresholds.md` for the calibration-vs-accuracy background.
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 05_thresholds_and_escalation --features derive
//! ```

use jev::{Choice, ChoiceOptions, Client, Decision, Noul, Thresholds, Verdict};

/// Named, runtime-tunable thresholds — the same shape an application would load
/// from its own `Config` (see `jev::Config`); hard-coded here to keep the example
/// self-contained.
fn load_thresholds() -> Thresholds {
    let mut thresholds = Thresholds::new();
    thresholds
        .insert("isBugCertainty", 0.8)
        .expect("0.8 is a valid threshold");
    thresholds
        .insert("teamAutoRoute", 0.7)
        .expect("0.7 is a valid threshold");
    thresholds
}

/// A support ticket triage decision (see example 3 for the fully commented version).
#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a software defect?
    is_bug: Noul,
    /// Which team should own this ticket?
    team: Choice<Team>,
}

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let thresholds = load_thresholds();
    let client = Client::from_env()?;
    let ticket = "The checkout page has been showing a blank screen for an hour.";

    let triage: Triage = client.decide(ticket).await?;

    // One threshold, three-way branch: `Verdict::Yes` / `Unsure` / `No`, instead of
    // comparing `P(yes)` against a hand-picked cutoff at every call site.
    let is_bug_certainty = thresholds.require("isBugCertainty")?;
    let is_bug = &triage.is_bug;
    match is_bug.decide_verdict(is_bug_certainty) {
        Verdict::Yes => println!("confirmed bug: {is_bug}"),
        Verdict::No => println!("confirmed not-a-bug: {is_bug}"),
        Verdict::Unsure => println!("ambiguous ({is_bug}): escalate to a human reviewer"),
    }

    // `accept` returns `None` below the threshold, forcing the ambiguous case to be
    // handled explicitly rather than silently taking the model's best guess.
    let team_auto_route = thresholds.require("teamAutoRoute")?;
    let team = &triage.team;
    match team.accept(team_auto_route) {
        Some(picked) => println!("auto-routed to {picked:?}: {team}"),
        None => println!("team ambiguous ({team}): escalate to a human reviewer"),
    }

    Ok(())
}
