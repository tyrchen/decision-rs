//! Stable, redacted errors shared by every decision backend.

use crate::metadata::ResponseMeta;

/// Invalid schema diagnostics, safe to cache and display.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid schema: {0}")]
#[non_exhaustive]
pub struct SchemaError(pub(crate) &'static str);

/// Failures rebuilding a typed decision from validated answers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// One or more required questions were refused.
    #[error("required questions refused")]
    Refused(Vec<String>),
    /// A handwritten trait implementation disagrees with its schema.
    #[error("invalid typed mapping for question {key}")]
    InvalidMapping {
        /// The schema question key.
        key: String,
    },
}

/// Failures calling or configuring any decision backend.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Input violates a documented resource limit.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    /// Question schema is invalid.
    #[error(transparent)]
    InvalidSchema(#[from] SchemaError),
    /// Client configuration is invalid.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(&'static str),
    /// More than one provider is possible.
    #[error("set JEV_PROVIDER to select credentials")]
    AmbiguousProvider,
    /// The selected provider has no key.
    #[error("credentials missing for selected provider")]
    MissingCredentials,
    /// A built-in model belongs to a different provider.
    #[error("model is incompatible with selected backend")]
    UnsupportedModel,
    /// Authentication failed.
    #[error("authentication failed")]
    Authentication,
    /// Access is forbidden.
    #[error("permission denied")]
    PermissionDenied,
    /// Credits, spending, or usage limits prevent requests.
    #[error("quota exhausted")]
    QuotaExceeded,
    /// Transient rate limiting.
    #[error("rate limited")]
    RateLimited {
        /// Server-requested delay in seconds.
        retry_after_secs: Option<u64>,
    },
    /// Overall network deadline expired.
    #[error("request deadline expired")]
    Timeout,
    /// HTTP transport failure; display does not expose URLs or remote text.
    #[error("HTTP transport failed")]
    Transport(#[source] reqwest::Error),
    /// Case file I/O failed.
    #[error("case file I/O failed")]
    Io(#[source] std::io::Error),
    /// The remote result failed the schema contract.
    #[error("invalid response: {reason}")]
    Protocol {
        /// Optional schema question key.
        key: Option<String>,
        /// Non-secret structural diagnostic.
        reason: &'static str,
    },
    /// Required fields were refused; accounting is retained.
    #[error("required questions refused")]
    Refused {
        /// Nonempty, declaration-ordered question keys.
        keys: Vec<String>,
        /// Metadata from the completed request.
        meta: Box<ResponseMeta>,
    },
    /// A distribution needed for an operation is absent.
    #[error("probability distribution unavailable")]
    StatisticsUnavailable,
    /// An otherwise unclassified HTTP status.
    #[error("API returned HTTP {status}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Bounded machine-readable server error code.
        code: Option<String>,
        /// Optional diagnostic request identifier.
        request_id: Option<String>,
    },
}

impl Error {
    pub(crate) fn protocol(reason: &'static str) -> Self {
        Self::Protocol { key: None, reason }
    }

    pub(crate) fn for_question(key: &str, reason: &'static str) -> Self {
        Self::Protocol {
            key: Some(key.to_owned()),
            reason,
        }
    }

    /// Whether a definite transient response or connection failure can be retried.
    ///
    /// ```
    /// assert!(!jev::Error::QuotaExceeded.is_retryable());
    /// ```
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } => true,
            Self::Api { status, .. } => matches!(status, 500 | 502 | 503 | 504),
            Self::Transport(source) => source.is_connect(),
            _ => false,
        }
    }
}
