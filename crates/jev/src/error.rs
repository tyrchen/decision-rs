//! The crate's single error type.

use crate::util::truncate_on_char_boundary;

/// Maximum number of bytes of an API error body kept for diagnostics.
const MAX_ERROR_BODY_BYTES: usize = 4096;

/// Everything that can go wrong calling Jev, in one non-exhaustive enum.
///
/// Variants are chosen so callers can act on the *kind* of failure without parsing
/// error strings: [`Error::is_retryable`] and [`Error::status`] cover the two most
/// common branches (back off and try again, or surface the HTTP status).
///
/// No variant embeds request `state` or question criteria text: those may contain
/// PII and are never placed in an error message or a `tracing` field.
///
/// # Examples
///
/// ```
/// use jev::Error;
///
/// let err = Error::RateLimited { retry_after_secs: Some(2) };
/// assert!(err.is_retryable());
/// assert_eq!(err.status(), Some(429));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The underlying HTTP transport failed (connect, timeout, TLS, decoding, ...).
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Reading a case file (see [`crate::CaseSet::from_file`]) failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The API responded with a non-2xx status carrying a `{"error": {...}}` envelope.
    #[error("API error (HTTP {status}): {message}")]
    Api {
        /// HTTP status code returned by the API.
        status: u16,
        /// Human-readable message extracted from the error envelope, if present.
        message: String,
        /// Raw response body, truncated to 4 KiB on a UTF-8 character boundary.
        body: String,
    },

    /// HTTP 429. Split out because callers back off rather than fail.
    #[error("rate limited (HTTP 429), retry after {retry_after_secs:?}s")]
    RateLimited {
        /// Seconds to wait before retrying, parsed from `Retry-After`, when present.
        retry_after_secs: Option<u64>,
    },

    /// HTTP 402. Split out because callers stop rather than back off.
    #[error("insufficient credits (HTTP 402): {message}")]
    InsufficientCredits {
        /// Human-readable message extracted from the error envelope, if present.
        message: String,
    },

    /// The response body did not match the expected shape.
    #[error("failed to parse response: {0}")]
    Parse(String),

    /// A [`crate::typed::Decision`] field had no corresponding entry in the answer map.
    #[error("missing answer for question `{key}`")]
    MissingAnswer {
        /// The question key that was expected but absent.
        key: String,
    },

    /// A question answer had the wrong primitive type for the accessor used.
    #[error("question `{key}`: expected {expected} answer, got {actual}")]
    AnswerType {
        /// The question key being read.
        key: String,
        /// The primitive the caller expected (`"noul"`, `"choice"`, or `"score"`).
        expected: &'static str,
        /// The primitive actually present, or `"other(<type>)"` for an unknown one.
        actual: String,
    },

    /// A `choice` answer named an option outside the caller's [`crate::typed::ChoiceOptions`].
    #[error("question `{key}`: unknown choice `{choice}` (expected one of {expected})")]
    UnknownChoice {
        /// The question key being read.
        key: String,
        /// The option key the server returned.
        choice: String,
        /// A comma-separated list of the option keys the caller declared.
        expected: String,
    },

    /// A `score` answer fell outside `0.0..=(levels - 1)` for the caller's
    /// [`crate::typed::ScoreLevels`].
    #[error("question `{key}`: score {score} out of range for {levels} levels")]
    ScoreRange {
        /// The question key being read.
        key: String,
        /// The out-of-range score returned by the server.
        score: f64,
        /// The number of levels the caller's enum declares.
        levels: usize,
    },

    /// A caller-supplied value failed validation before it reached the network.
    #[error("invalid {field}: {reason}")]
    Invalid {
        /// The name of the field or configuration key that failed validation.
        field: String,
        /// A human-readable reason, safe to log (never echoes the offending value
        /// when the value itself might be sensitive).
        reason: String,
    },

    /// The serialized state exceeded the model's estimated context window.
    #[error("state too large: ~{estimated} tokens exceeds {limit}")]
    StateTooLarge {
        /// The estimated token count of the serialized request.
        estimated: usize,
        /// The model's context window, in tokens.
        limit: usize,
    },

    /// Neither `JEV_API_KEY` nor `OPENROUTER_API_KEY` was set.
    #[error("no API key configured (set JEV_API_KEY)")]
    MissingApiKey,

    /// The overall per-request deadline elapsed, including any retries.
    #[error("request timed out after {elapsed_ms}ms")]
    Timeout {
        /// Milliseconds elapsed before the deadline fired.
        elapsed_ms: u64,
    },
}

impl Error {
    /// Builds an [`Error::Api`] (or the more specific [`Error::RateLimited`] /
    /// [`Error::InsufficientCredits`]) from a status code and raw response body.
    ///
    /// The body is parsed as `{"error": {"message": ..., ...}}` on a best-effort basis;
    /// a body that does not match the envelope still produces a usable error with the
    /// raw text kept (truncated) for diagnostics.
    pub(crate) fn from_api_response(
        status: u16,
        body: &str,
        retry_after_secs: Option<u64>,
    ) -> Self {
        let truncated = truncate_on_char_boundary(body, MAX_ERROR_BODY_BYTES).to_string();
        let message = parse_error_message(body).unwrap_or_else(|| truncated.clone());

        match status {
            402 => Error::InsufficientCredits { message },
            429 => Error::RateLimited { retry_after_secs },
            _ => Error::Api {
                status,
                message,
                body: truncated,
            },
        }
    }

    /// Whether retrying the request that produced this error could plausibly succeed.
    ///
    /// Deterministic client errors (`400`, `401`, `402`, `403`, `404`, `413`, and any
    /// caller-side validation failure) are never retryable; transport-level timeouts,
    /// `429`, and `5xx` are.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::Error;
    ///
    /// assert!(Error::RateLimited { retry_after_secs: None }.is_retryable());
    /// assert!(!Error::InsufficientCredits { message: "no credits".into() }.is_retryable());
    /// ```
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api { status, .. } => is_retryable_status(*status),
            Error::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
            Error::RateLimited { .. } | Error::Timeout { .. } => true,
            Error::InsufficientCredits { .. }
            | Error::Parse(_)
            | Error::MissingAnswer { .. }
            | Error::AnswerType { .. }
            | Error::UnknownChoice { .. }
            | Error::ScoreRange { .. }
            | Error::Invalid { .. }
            | Error::StateTooLarge { .. }
            | Error::MissingApiKey
            | Error::Io(_) => false,
        }
    }

    /// The HTTP status code this error carries, when it originated from the API.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::Error;
    ///
    /// assert_eq!(Error::RateLimited { retry_after_secs: None }.status(), Some(429));
    /// assert_eq!(Error::MissingApiKey.status(), None);
    /// ```
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            Error::RateLimited { .. } => Some(429),
            Error::InsufficientCredits { .. } => Some(402),
            Error::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
}

/// Status codes the client retries: transient rate limiting and upstream failures.
pub(crate) fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Best-effort extraction of `error.message` from a `{"error": {...}}` envelope.
fn parse_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_should_parse_error_envelope_message() {
        let body = r#"{"error":{"code":400,"message":"bad request"}}"#;
        let err = Error::from_api_response(400, body, None);
        match err {
            Error::Api {
                status,
                message,
                body,
            } => {
                assert_eq!(status, 400);
                assert_eq!(message, "bad request");
                assert!(body.contains("bad request"));
            }
            other => panic!("expected Api variant, got {other:?}"),
        }
    }

    #[test]
    fn test_should_fall_back_to_raw_body_when_envelope_missing() {
        let err = Error::from_api_response(500, "upstream exploded", None);
        match err {
            Error::Api { message, .. } => assert_eq!(message, "upstream exploded"),
            other => panic!("expected Api variant, got {other:?}"),
        }
    }

    #[test]
    fn test_should_map_402_to_insufficient_credits() {
        let body = r#"{"error":{"message":"top up your account"}}"#;
        let err = Error::from_api_response(402, body, None);
        assert!(matches!(err, Error::InsufficientCredits { .. }));
        assert_eq!(err.status(), Some(402));
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_should_map_429_to_rate_limited_with_retry_after() {
        let err = Error::from_api_response(429, "{}", Some(3));
        match err {
            Error::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, Some(3)),
            other => panic!("expected RateLimited variant, got {other:?}"),
        }
        assert!(err.is_retryable());
    }

    #[test]
    fn test_should_truncate_long_error_body_on_char_boundary() {
        let body = "€".repeat(4000); // 3 bytes each => 12000 bytes.
        let err = Error::from_api_response(500, &body, None);
        match err {
            Error::Api {
                body: truncated, ..
            } => {
                assert!(truncated.len() <= MAX_ERROR_BODY_BYTES);
                // Must still be valid UTF-8 (guaranteed by type) and a true prefix.
                assert!(body.starts_with(&truncated));
            }
            other => panic!("expected Api variant, got {other:?}"),
        }
    }

    #[rstest]
    #[case(400, false)]
    #[case(401, false)]
    #[case(402, false)]
    #[case(403, false)]
    #[case(404, false)]
    #[case(413, false)]
    #[case(429, true)]
    #[case(500, true)]
    #[case(502, true)]
    #[case(503, true)]
    #[case(504, true)]
    fn test_should_classify_status_retryability(#[case] status: u16, #[case] retryable: bool) {
        assert_eq!(is_retryable_status(status), retryable);
    }

    #[test]
    fn test_should_not_retry_invalid_or_parse_errors() {
        assert!(
            !Error::Invalid {
                field: "x".into(),
                reason: "y".into()
            }
            .is_retryable()
        );
        assert!(!Error::Parse("bad".into()).is_retryable());
        assert!(!Error::MissingApiKey.is_retryable());
    }
}
