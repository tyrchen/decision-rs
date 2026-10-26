//! Provider-neutral accounting and response metadata.

use std::time::Duration;

use crate::{answer::Answers, model::Model, provider::Provider};

/// Server-reported accounting; absence never implies zero billing.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Usage {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Total tokens, checked when synthesized.
    pub total_tokens: u64,
    /// Cache-read detail, if reported.
    pub cached_tokens: Option<u64>,
    /// Cache-write detail, if reported.
    pub cache_write_tokens: Option<u64>,
    /// Reasoning detail, if reported.
    pub reasoning_tokens: Option<u64>,
    /// Provider-reported USD cost, never locally estimated.
    pub reported_cost_usd: Option<f64>,
}
/// Metadata retained even when required typed fields are refused.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResponseMeta {
    /// Hosted provider; None for custom endpoints.
    pub provider: Option<Provider>,
    /// Logical model requested, including Default.
    pub requested_model: Model,
    /// Model resolved for this call.
    pub resolved_model: Model,
    /// Exact model ID sent.
    pub wire_model: String,
    /// Separate URL model identifier for model-routed providers; never an account URL.
    pub routing_model: Option<String>,
    /// Model string returned by the server.
    pub returned_model: String,
    /// Server request header identifier.
    pub request_id: Option<String>,
    /// Separate response body identifier.
    pub response_id: Option<String>,
    /// SDK version for reproducibility.
    pub sdk_version: &'static str,
    /// Accounting from this response.
    pub usage: Usage,
    /// Network lifecycle duration, including queueing and backoff.
    pub latency: Duration,
    /// Actual HTTP attempts.
    pub attempts: u32,
}
/// Checked dynamic response with all answers in schema order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DecisionResponse {
    /// Answers, including explicit refusals.
    pub answers: Answers,
    /// Provider-neutral metadata.
    pub meta: ResponseMeta,
}
