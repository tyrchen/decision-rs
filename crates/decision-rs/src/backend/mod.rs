//! Private protocol compilation and validation against the original schema.

mod cloudflare;
mod gemini;
mod jev;
mod openai;

use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    Answer, Answers, ChoiceAnswer, Distribution, Error, Image, Input, Predicate, Probability,
    Questions, ScoreAnswer, Usage, json, typed::Refusal,
};

/// Serialized bytes allowed per inline image beyond its base64 payload
/// (data URL prefix, media type, detail and JSON framing).
const INLINE_IMAGE_OVERHEAD: usize = 128;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Backend {
    Jev,
    SystemOne,
    OpenAi,
    Cloudflare,
    ClefSystemOne,
    Gemini,
}
/// The immutable schema is the response plan; boolean predicates are interpreted
/// only according to the backend and the presence of both criteria.
pub(crate) struct ResponsePlan<'a> {
    pub questions: &'a Questions,
}
pub(crate) struct Decoded {
    pub answers: Answers,
    pub model: Option<String>,
    pub id: Option<String>,
    pub usage: Usage,
}
impl Backend {
    /// Whether the protocol carries image evidence natively.
    pub const fn accepts_images(self) -> bool {
        matches!(self, Self::OpenAi | Self::Gemini)
    }
    /// Encode a request. `limit` bounds everything except inline image payloads,
    /// which `Limits::image_bytes` bounds before encoding.
    pub fn prepare(
        self,
        input: &Input,
        questions: &Questions,
        model: &str,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        if !input.images().is_empty() && !self.accepts_images() {
            return Err(Error::ImagesUnsupported { provider: None });
        }
        let limit = input
            .images()
            .iter()
            .try_fold(limit, |cap, image| {
                image
                    .byte_len()
                    .div_ceil(3)
                    .checked_mul(4)?
                    .checked_add(INLINE_IMAGE_OVERHEAD)
                    .and_then(|encoded| cap.checked_add(encoded))
            })
            .ok_or(Error::InvalidInput("request body limit"))?;
        match self {
            Self::OpenAi => openai::prepare(input, questions, model, limit),
            Self::Gemini => gemini::prepare(input, questions, model, limit),
            Self::Jev | Self::SystemOne | Self::Cloudflare | Self::ClefSystemOne => {
                jev::prepare(input, questions, model, limit)
            }
        }
    }
    pub fn decode(self, body: &[u8], plan: &ResponsePlan<'_>) -> Result<Decoded, Error> {
        let value = json::parse(body, 4096, 256)?;
        match self {
            Self::OpenAi => openai::decode(value, plan.questions),
            Self::Jev | Self::SystemOne => {
                jev::decode(value, plan.questions, NumericPolicy::Standard)
            }
            Self::ClefSystemOne => jev::decode(value, plan.questions, NumericPolicy::Clef),
            Self::Cloudflare => cloudflare::decode(value, plan.questions),
            Self::Gemini => gemini::decode(value, plan.questions),
        }
    }
    pub fn http_error(
        self,
        status: u16,
        bytes: &[u8],
        retry_after_secs: Option<u64>,
        request_id: Option<String>,
    ) -> Error {
        let summary = match self {
            Self::Cloudflare => cloudflare::error_summary(bytes),
            Self::Gemini => gemini::error_summary(bytes),
            Self::OpenAi | Self::Jev | Self::SystemOne | Self::ClefSystemOne => {
                standard_error_summary(self, bytes)
            }
        };
        match status {
            401 => Error::Authentication,
            400 if summary.invalid_key => Error::Authentication,
            403 => Error::PermissionDenied,
            402 => Error::QuotaExceeded,
            429 if summary.quota => Error::QuotaExceeded,
            429 => Error::RateLimited {
                retry_after_secs: retry_after_secs.or(summary.retry_after_secs),
            },
            400 if summary.blocked => Error::InvalidInput("upstream content filter blocked input"),
            400 | 413 | 422 => Error::InvalidInput("upstream rejected request"),
            _ => Error::Api {
                status,
                code: summary.code,
                request_id,
            },
        }
    }
}
/// Standard padded base64, streamed into the JSON writer without an intermediate string.
pub(super) struct Base64<'a>(pub &'a Image);
impl Serialize for Base64<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&Base64Display::new(self.0.as_bytes(), &STANDARD))
    }
}
/// `data:<sniffed media type>;base64,<payload>` URL, streamed like `Base64`.
pub(super) struct DataUrl<'a>(pub &'a Image);
impl Serialize for DataUrl<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!(
            "data:{};base64,{}",
            self.0.media_type(),
            Base64Display::new(self.0.as_bytes(), &STANDARD)
        ))
    }
}
/// Protocol-specific facts extracted from a bounded error body; never remote prose.
#[derive(Debug, Default)]
pub(super) struct ErrorSummary {
    pub code: Option<String>,
    pub quota: bool,
    pub invalid_key: bool,
    pub blocked: bool,
    pub retry_after_secs: Option<u64>,
}
fn standard_error_summary(backend: Backend, bytes: &[u8]) -> ErrorSummary {
    let dto = json::parse(bytes, 4096, 256)
        .ok()
        .and_then(|v| serde_json::from_value::<ErrorEnvelope>(v).ok());
    let code = dto
        .as_ref()
        .and_then(|d| d.error.code.as_deref())
        .and_then(|s| bounded_identifier(s, 128).ok());
    let quota = matches!(backend, Backend::OpenAi)
        && dto.as_ref().is_some_and(|d| {
            d.error.kind.as_deref() == Some("insufficient_quota")
                || matches!(
                    d.error.code.as_deref(),
                    Some(
                        "insufficient_quota"
                            | "credit_balance_exhausted"
                            | "organization_spend_limit_exceeded"
                            | "project_spend_limit_exceeded"
                            | "organization_usage_limit_exceeded"
                    )
                )
        });
    ErrorSummary {
        code,
        quota,
        ..ErrorSummary::default()
    }
}
#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}
#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    code: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}
pub(crate) fn bounded_identifier(text: &str, max: usize) -> Result<String, Error> {
    if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
        return Err(Error::protocol("identifier bounds"));
    }
    Ok(text.to_owned())
}
#[derive(Deserialize)]
pub(super) struct WireUsage {
    #[serde(rename = "input_tokens")]
    input: u64,
    #[serde(rename = "output_tokens")]
    output: u64,
    #[serde(default, rename = "total_tokens")]
    total: Option<u64>,
    #[serde(default, rename = "input_tokens_details")]
    input_details: Option<InputDetails>,
    #[serde(default, rename = "output_tokens_details")]
    output_details: Option<OutputDetails>,
    #[serde(default)]
    cost: Option<f64>,
}
#[derive(Deserialize)]
struct InputDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
    #[serde(default)]
    cache_write_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct OutputDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}
impl WireUsage {
    fn validate(self) -> Result<Usage, Error> {
        let computed = self
            .input
            .checked_add(self.output)
            .ok_or_else(|| Error::protocol("token count overflow"))?;
        if self
            .cost
            .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
        {
            return Err(Error::protocol("cost bounds"));
        }
        Ok(Usage {
            input_tokens: self.input,
            output_tokens: self.output,
            total_tokens: self.total.unwrap_or(computed),
            cached_tokens: self.input_details.as_ref().and_then(|d| d.cached_tokens),
            cache_write_tokens: self.input_details.and_then(|d| d.cache_write_tokens),
            reasoning_tokens: self.output_details.and_then(|d| d.reasoning_tokens),
            reported_cost_usd: self.cost,
        })
    }
}
/// Numeric tolerance is a codec property, never a global relaxation.
#[derive(Debug, Clone, Copy)]
pub(super) enum NumericPolicy {
    Standard,
    /// Reference implementation rounds every value to four decimals.
    Clef,
    /// Documented responses round values to as few as two decimals.
    Gemini,
}
impl NumericPolicy {
    /// Half of the last reported decimal place when each value is rounded independently.
    fn half_unit(self) -> Option<f64> {
        match self {
            Self::Standard => None,
            Self::Clef => Some(0.00005),
            Self::Gemini => Some(0.005),
        }
    }
    fn mass_tolerance(self, count: usize) -> Result<f64, Error> {
        let count = u32::try_from(count).map_err(|_| Error::protocol("option count"))?;
        Ok(self
            .half_unit()
            .map_or(0.001, |half| f64::from(count) * half + 1e-9))
    }
}
pub(super) fn distribution(
    key: &str,
    keys: &[String],
    entries: Option<Vec<(String, Probability)>>,
    policy: NumericPolicy,
) -> Result<Distribution, Error> {
    let Some(entries) = entries else {
        return Ok(Distribution::Unavailable);
    };
    let mut reported = IndexMap::new();
    for (name, probability) in entries {
        if !keys.contains(&name) || reported.insert(name, probability).is_some() {
            return Err(Error::for_question(
                key,
                "unknown or duplicate distribution entry",
            ));
        }
    }
    if reported.len() != keys.len()
        || (reported.values().map(|p| p.get()).sum::<f64>() - 1.0).abs()
            > policy.mass_tolerance(keys.len())?
    {
        return Err(Error::for_question(
            key,
            "incomplete or unnormalized distribution",
        ));
    }
    // Reorder for predictable iteration and handwritten mapping checks.
    let mut ordered = IndexMap::new();
    for name in keys {
        let probability = reported
            .shift_remove(name)
            .ok_or_else(|| Error::for_question(key, "distribution entry missing"))?;
        ordered.insert(name.clone(), probability);
    }
    Ok(Distribution::Reported(ordered))
}
pub(super) fn choice(
    key: &str,
    selected: String,
    keys: Vec<String>,
    confidence: Option<Probability>,
    entries: Option<Vec<(String, Probability)>>,
    policy: NumericPolicy,
) -> Result<Answer, Error> {
    if !keys.contains(&selected) {
        return Err(Error::for_question(key, "unknown choice"));
    }
    Ok(Answer::Choice(ChoiceAnswer {
        choice: selected,
        probabilities: distribution(key, &keys, entries, policy)?,
        options: keys,
        confidence,
    }))
}
#[allow(clippy::cast_precision_loss)] // Score schemas are bounded to 32 levels.
pub(super) fn score(
    key: &str,
    raw: f64,
    keys: Vec<String>,
    confidence: Option<Probability>,
    entries: Option<Vec<(String, Probability)>>,
    policy: NumericPolicy,
) -> Result<Answer, Error> {
    let max = keys.len().saturating_sub(1) as f64;
    if !raw.is_finite() || !(0.0..=max).contains(&raw) {
        return Err(Error::for_question(key, "score bounds"));
    }
    let indices: Vec<String> = (0..keys.len()).map(|i| i.to_string()).collect();
    let distribution = distribution(key, &indices, entries, policy)?;
    if let Distribution::Reported(probs) = &distribution {
        let mean = probs
            .values()
            .enumerate()
            .map(|(i, p)| i as f64 * p.get())
            .sum::<f64>();
        let (mean, tolerance) = match policy.half_unit() {
            None => (
                mean / probs.values().map(|p| p.get()).sum::<f64>(),
                0.001 * max.max(1.0),
            ),
            Some(half) => (mean, half * (1.0 + max * (max + 1.0) / 2.0) + 1e-9),
        };
        if (mean - raw).abs() > tolerance {
            return Err(Error::for_question(
                key,
                "score disagrees with distribution",
            ));
        }
    }
    Ok(Answer::Score(ScoreAnswer {
        score: raw,
        confidence,
        probabilities: distribution,
        levels: keys,
    }))
}
pub(super) fn predicate(probability: Probability) -> Answer {
    Answer::Predicate(Predicate(probability))
}
pub(super) fn refusal(key: &str) -> Answer {
    Answer::Refusal(Refusal {
        key: key.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::Image;

    fn framed(image_len: usize) -> Input {
        let mut bytes = vec![b'x'; image_len];
        bytes[..3].copy_from_slice(b"\xff\xd8\xff");
        Input::try_from("frame")
            .unwrap()
            .with_image(Image::try_from(bytes).unwrap())
            .unwrap()
    }
    fn one_question() -> Questions {
        Questions::builder()
            .predicate("q", "Condition?")
            .build()
            .unwrap()
    }

    #[test]
    fn test_should_exclude_inline_images_from_request_byte_limit() {
        let input = framed(64 * 1024);
        for backend in [Backend::OpenAi, Backend::Gemini] {
            let body = backend.prepare(&input, &one_question(), "m", 1024).unwrap();
            assert!(body.len() > 64 * 1024);
            assert!(matches!(
                backend.prepare(&input, &one_question(), "m", 16),
                Err(Error::InvalidInput("request body limit"))
            ));
        }
    }

    #[test]
    fn test_should_refuse_images_on_text_only_protocols() {
        let input = framed(16);
        for backend in [
            Backend::Jev,
            Backend::SystemOne,
            Backend::Cloudflare,
            Backend::ClefSystemOne,
        ] {
            assert!(!backend.accepts_images());
            assert!(matches!(
                backend.prepare(&input, &one_question(), "m", 1 << 20),
                Err(Error::ImagesUnsupported { provider: None })
            ));
        }
    }
    proptest! {
        #[test]
        fn test_should_associate_reordered_answers_by_name(count in 1_usize..32, reverse in any::<bool>(), probability in 0.0_f64..1.0) {
            let mut builder = Questions::builder();
            for i in 0..count { builder = builder.predicate(&format!("q{i}"), "Condition?"); }
            let questions = builder.build().unwrap();
            let mut answers: Vec<_> = (0..count).map(|i| json!({"name":format!("q{i}"),"type":"predicate","probability":probability})).collect();
            if reverse { answers.reverse(); }
            let bytes = serde_json::to_vec(&json!({"model":"fixture","answers":answers,"usage":{"input_tokens":0,"output_tokens":0}})).unwrap();
            let decoded = Backend::OpenAi.decode(&bytes, &ResponsePlan { questions: &questions }).unwrap();
            prop_assert_eq!(decoded.answers.iter().map(|(key,_)| key.clone()).collect::<Vec<_>>(), questions.iter().map(|(key,_)| key.clone()).collect::<Vec<_>>());
            for (_, answer) in decoded.answers.iter() {
                let Answer::Predicate(answer) = answer else { prop_assert!(false); continue; };
                prop_assert_eq!(answer.probability().get(), probability);
            }
        }
        #[test]
        fn test_should_preserve_reported_probability_mass(p in 0.0_f64..1.0) {
            let keys = vec!["a".to_owned(), "b".to_owned()];
            let entries = vec![("b".to_owned(), Probability::try_from(1.0-p).unwrap()), ("a".to_owned(), Probability::try_from(p).unwrap())];
            let distribution = distribution("q", &keys, Some(entries), NumericPolicy::Standard).unwrap();
            prop_assert_eq!(distribution.get("a").unwrap().get(), p);
            prop_assert_eq!(distribution.get("b").unwrap().get(), 1.0-p);
        }
    }
}
