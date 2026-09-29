//! Faithful wire-format mappings: request/response envelopes, usage, and metadata.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{answer::Answers, question::Questions, state::State};

/// Opaque, pass-through trace configuration.
///
/// The Decisions surface accepts a `trace` object whose shape is not yet stable
/// upstream; this SDK passes it through verbatim rather than modelling a schema
/// that might change. See the design doc's "Open Questions" for the plan to grow
/// this into typed sub-structs once the field stabilizes.
///
/// # Examples
///
/// ```
/// use jev::TraceConfig;
///
/// let trace = TraceConfig::new(serde_json::json!({"sampled": true}));
/// assert_eq!(trace.value()["sampled"], true);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceConfig(Value);

impl TraceConfig {
    /// Wraps an arbitrary JSON value as trace configuration.
    #[must_use]
    pub fn new(value: Value) -> Self {
        TraceConfig(value)
    }

    /// The wrapped JSON value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.0
    }
}

/// The request body sent to either surface.
///
/// `session_id`, `user`, and `trace` are Decisions-surface-only; the client clears
/// them (and logs a `debug!`) before serializing when [`crate::config::Surface::SystemOne`]
/// is selected, so this one struct is the single copy of the wire-serialization logic
/// for both surfaces.
#[derive(Debug, Serialize)]
pub(crate) struct DecisionRequest<'a> {
    pub model: &'a str,
    pub state: &'a State,
    pub questions: &'a Questions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<&'a TraceConfig>,
}

/// Token and cost accounting for one request, as reported by the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Usage {
    /// Input tokens billed for the request (state + questions).
    pub input_tokens: u32,
    /// Output tokens billed for the response.
    pub output_tokens: u32,
    /// Billed cost, when the gateway reports it.
    #[serde(default)]
    pub cost: Option<f64>,
}

/// The full response body from either surface.
#[derive(Debug, Deserialize)]
pub struct Response {
    /// The model snapshot that actually served the request.
    pub model: String,
    /// The answered questions.
    pub answers: Answers,
    /// Token and cost accounting.
    pub usage: Usage,
    /// Request identifier, when the gateway reports one.
    #[serde(default)]
    pub id: Option<String>,
    /// The serving provider, when the gateway reports one.
    #[serde(default)]
    pub provider: Option<String>,
}

/// The response envelope without the answers: identifiers, model snapshot, usage,
/// cost, and latency.
///
/// Returned alongside a typed decision by [`crate::Client::decide_with`], for callers
/// who need to record cost or the dated model snapshot without giving up the typed
/// answer struct.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseMeta {
    /// Request identifier, when the gateway reported one.
    pub id: Option<String>,
    /// The model snapshot that actually served the request.
    pub model: String,
    /// The serving provider, when the gateway reported one.
    pub provider: Option<String>,
    /// Token and cost accounting.
    pub usage: Usage,
    /// Wall-clock latency of the request, including any retries.
    pub latency: std::time::Duration,
}

impl ResponseMeta {
    /// Builds a `ResponseMeta` from a [`Response`] and the measured latency.
    pub(crate) fn from_response(response: &Response, latency: std::time::Duration) -> Self {
        ResponseMeta {
            id: response.id.clone(),
            model: response.model.clone(),
            provider: response.provider.clone(),
            usage: response.usage,
            latency,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_deserialize_full_response() {
        let body = r#"{
            "model": "typesafe/jev-1.13",
            "id": "req_123",
            "provider": "openrouter",
            "usage": {"input_tokens": 100, "output_tokens": 5, "cost": 0.001},
            "answers": {"refund": {"type": "noul", "noul": 0.9}}
        }"#;
        let response: Response = serde_json::from_str(body).unwrap();
        assert_eq!(response.model, "typesafe/jev-1.13");
        assert_eq!(response.id, Some("req_123".to_string()));
        assert_eq!(response.usage.input_tokens, 100);
        assert_eq!(response.answers.noul("refund").unwrap(), 0.9);
    }

    #[test]
    fn test_should_default_optional_response_fields_when_absent() {
        let body = r#"{
            "model": "jev-1.13",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "answers": {}
        }"#;
        let response: Response = serde_json::from_str(body).unwrap();
        assert_eq!(response.id, None);
        assert_eq!(response.provider, None);
        assert_eq!(response.usage.cost, None);
    }

    #[test]
    fn test_should_wrap_trace_config_transparently() {
        let trace = TraceConfig::new(serde_json::json!({"sampled": true}));
        let encoded = serde_json::to_value(&trace).unwrap();
        assert_eq!(encoded, serde_json::json!({"sampled": true}));
    }

    #[test]
    fn test_should_skip_none_optional_request_fields() {
        let questions = Questions::builder()
            .noul("refund", "Is this a refund?")
            .build()
            .unwrap();
        let state: State = "hi".into();
        let req = DecisionRequest {
            model: "jev-1.13",
            state: &state,
            questions: &questions,
            session_id: None,
            user: None,
            trace: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("session_id").is_none());
        assert!(json.get("user").is_none());
        assert!(json.get("trace").is_none());
    }

    #[test]
    fn test_should_build_response_meta_from_response() {
        let body = r#"{
            "model": "jev-1.13",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "answers": {}
        }"#;
        let response: Response = serde_json::from_str(body).unwrap();
        let meta = ResponseMeta::from_response(&response, std::time::Duration::from_millis(42));
        assert_eq!(meta.model, "jev-1.13");
        assert_eq!(meta.latency, std::time::Duration::from_millis(42));
    }
}
