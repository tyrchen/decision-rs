//! Runnable example: questions built from configuration at runtime (the dynamic path).
//!
//! ```text
//! JEV_API_KEY=sk-... cargo run --example 04_dynamic_questions
//! ```

use jev::{Client, Questions};

/// A minimal stand-in for a question set loaded from YAML/JSON at runtime. A real
/// application would deserialize this shape from its own config file instead of
/// hard-coding it.
struct QuestionConfig {
    key: &'static str,
    kind: &'static str,
    instructions: &'static str,
    options: &'static [(&'static str, &'static str)],
}

const CONFIGURED_QUESTIONS: &[QuestionConfig] = &[
    QuestionConfig {
        key: "refund",
        kind: "noul",
        instructions: "Is the customer asking for money back?",
        options: &[],
    },
    QuestionConfig {
        key: "team",
        kind: "choice",
        instructions: "Which team should own this ticket?",
        options: &[
            (
                "payments",
                "Checkout, billing, or payment processing issues.",
            ),
            (
                "frontend",
                "Rendering, layout, or browser compatibility issues.",
            ),
        ],
    },
];

/// Builds a [`Questions`] from the runtime configuration, the same way an
/// application would build one from a deserialized YAML/JSON document.
fn build_questions(config: &[QuestionConfig]) -> Result<Questions, jev::Error> {
    let mut builder = Questions::builder();
    for question in config {
        builder = match question.kind {
            "noul" => builder.noul(question.key, question.instructions),
            "choice" => builder.choice(
                question.key,
                question.instructions,
                question.options.to_vec(),
            ),
            other => {
                return Err(jev::Error::Invalid {
                    field: "kind".to_string(),
                    reason: format!("unknown question kind `{other}`"),
                });
            }
        };
    }
    builder.build()
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let questions = build_questions(CONFIGURED_QUESTIONS)?;
    let client = Client::from_env()?;

    let response = client
        .ask(
            "Customer wants a refund for a duplicate charge.",
            &questions,
        )
        .await?;

    println!("refund: P(yes) = {:.2}", response.answers.noul("refund")?);
    let team = response.answers.choice("team")?;
    println!("team: {} (confidence {:?})", team.choice, team.confidence);

    Ok(())
}
