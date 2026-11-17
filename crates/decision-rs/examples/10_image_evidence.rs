//! Runnable example. Select DECISION_PROVIDER=openai|gemini and the corresponding API key.

use anyhow::Result;
use decision_rs::{
    Choice, ChoiceOptions, Client, Decision, Image, ImageDetail, Input, Predicate, RequestOptions,
};

#[derive(Debug, ChoiceOptions)]
enum Composition {
    /// Balanced, readable layout.
    Full,
    /// Mostly empty space.
    Sparse,
    /// Too many competing elements.
    Crowded,
}

#[derive(Debug, Decision)]
struct FrameReview {
    /// Does the frame visibly show the narrated line?
    shows_line: Predicate,
    /// How full is the frame?
    composition: Choice<Composition>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .try_init()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let path = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: cargo run --example 10_image_evidence -- frame.png")
    })?;
    let frame = Image::try_from(tokio::fs::read(path).await?)?.with_detail(ImageDetail::Low);
    let input = Input::try_from(
        "Narration: \"Revenue doubled in Q3.\"\nIntended picture: a rising bar chart.",
    )?
    .with_image(frame)?;
    let review = Client::from_env()?
        .decide_with::<FrameReview>(RequestOptions::new(&input)?)
        .await?;
    tracing::info!(
        digest = %input.digest(),
        shows_line = review.value.shows_line.probability().get(),
        composition = ?review.value.composition.value(),
        input_tokens = review.meta.usage.input_tokens,
        output_tokens = review.meta.usage.output_tokens,
        cached_tokens = ?review.meta.usage.cached_tokens,
        model = review.meta.returned_model.as_deref().unwrap_or(&review.meta.wire_model),
        "frame reviewed"
    );
    Ok(())
}
