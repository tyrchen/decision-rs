//! `Config`, `Surface`, `RetryConfig`, and `Thresholds`.

use std::{collections::HashMap, str::FromStr, time::Duration};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    error::Error,
    model::{DEFAULT_MODEL, Model},
};

/// Default per-request timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default maximum retry attempts, including the first.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default initial backoff before the first retry.
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(200);
/// Default backoff cap.
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(2);

/// Which HTTP surface a [`crate::Client`] talks to.
///
/// Both surfaces answer the same questions with the same semantics; see the
/// crate-level docs for the field differences. [`Surface::Decisions`] is the
/// default because it is the one documented for non-official-SDK clients and
/// supports `session_id` / `trace` for cross-service observability.
///
/// # Examples
///
/// ```
/// use jev::Surface;
///
/// assert_eq!(Surface::default(), Surface::Decisions);
/// assert_eq!(Surface::Decisions.path(), "/api/alpha/decisions");
/// assert_eq!(Surface::SystemOne.path(), "/api/v1/systemone");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Surface {
    /// `POST /api/alpha/decisions`. Alpha. Adds `provider`, `session_id`, `trace`, `user`.
    #[default]
    Decisions,
    /// `POST /api/v1/systemone`. Stable `v1` path. Accepts bare model IDs.
    SystemOne,
}

impl Surface {
    /// The HTTP path this surface is served at.
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Surface::Decisions => "/api/alpha/decisions",
            Surface::SystemOne => "/api/v1/systemone",
        }
    }
}

/// Bounded retry configuration honouring `Retry-After`.
///
/// The endpoint has no idempotency key, so a retry can double-bill a request that
/// the server actually processed but whose response was lost to a transport error.
/// Set `max_attempts: 1` to disable retry entirely when that risk is unacceptable.
///
/// # Examples
///
/// ```
/// use jev::RetryConfig;
/// use std::time::Duration;
///
/// let no_retry = RetryConfig::disabled();
/// assert_eq!(no_retry.max_attempts, 1);
///
/// let custom = RetryConfig::default().with_max_attempts(5).with_initial_backoff(Duration::from_millis(50));
/// assert_eq!(custom.max_attempts, 5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetryConfig {
    /// Maximum number of attempts, including the first. `1` disables retry.
    pub max_attempts: u32,
    /// Backoff before the first retry.
    #[serde(with = "duration_millis", rename = "initialBackoffMs")]
    pub initial_backoff: Duration,
    /// Backoff cap; `Retry-After` is clamped to this.
    #[serde(with = "duration_millis", rename = "maxBackoffMs")]
    pub max_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        }
    }
}

impl RetryConfig {
    /// A configuration that never retries.
    #[must_use]
    pub fn disabled() -> Self {
        RetryConfig {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Sets the maximum attempt count.
    #[must_use]
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Sets the initial backoff.
    #[must_use]
    pub fn with_initial_backoff(mut self, backoff: Duration) -> Self {
        self.initial_backoff = backoff;
        self
    }

    /// Sets the backoff cap.
    #[must_use]
    pub fn with_max_backoff(mut self, backoff: Duration) -> Self {
        self.max_backoff = backoff;
        self
    }
}

/// `serde` helper for `Duration <-> milliseconds` fields.
mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Named, runtime-tunable probability thresholds.
///
/// The SDK never picks a threshold itself; gates like [`crate::Noul::is_yes`] and
/// [`crate::Choice::confident`] take a threshold as an explicit argument. `Thresholds`
/// exists so an application can load those numbers from configuration instead of
/// hard-coding them, and swap them without a redeploy (wrap `Config` in `ArcSwap`;
/// the SDK imposes no reload mechanism of its own).
///
/// Every value is validated to be finite and within `0.0..=1.0` when the map is
/// built or deserialized.
///
/// # Examples
///
/// ```
/// use jev::Thresholds;
///
/// let mut thresholds = Thresholds::new();
/// thresholds.insert("autoRouteConfidence", 0.7).unwrap();
/// assert_eq!(thresholds.get("autoRouteConfidence"), Some(0.7));
/// assert!(thresholds.require("missing").is_err());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Thresholds(HashMap<String, f64>);

impl Thresholds {
    /// An empty threshold set.
    #[must_use]
    pub fn new() -> Self {
        Thresholds(HashMap::new())
    }

    /// Inserts or replaces a named threshold.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] if `value` is not finite or not within `0.0..=1.0`.
    pub fn insert(&mut self, name: impl Into<String>, value: f64) -> Result<(), Error> {
        validate_threshold(value)?;
        self.0.insert(name.into(), value);
        Ok(())
    }

    /// Reads a named threshold, if present.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<f64> {
        self.0.get(name).copied()
    }

    /// Reads a named threshold, or a named [`Error::Invalid`] if absent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when `name` has no configured value.
    pub fn require(&self, name: &str) -> Result<f64, Error> {
        self.get(name).ok_or_else(|| Error::Invalid {
            field: format!("thresholds.{name}"),
            reason: "no threshold configured for this name".to_string(),
        })
    }
}

/// Validates a threshold is finite and within `0.0..=1.0`.
fn validate_threshold(value: f64) -> Result<(), Error> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::Invalid {
            field: "threshold".to_string(),
            reason: format!("must be finite and within 0.0..=1.0, got {value}"),
        });
    }
    Ok(())
}

impl<'de> Deserialize<'de> for Thresholds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<String, f64>::deserialize(deserializer)?;
        let mut thresholds = Thresholds::new();
        for (name, value) in map {
            thresholds.insert(name, value).map_err(D::Error::custom)?;
        }
        Ok(thresholds)
    }
}

/// Deserializable client configuration, for the config-file path.
///
/// Nests under an application's YAML with `#[serde(rename_all = "camelCase", deny_unknown_fields)]`
/// so a typo in a threshold name or config key is rejected at load time instead of
/// silently ignored:
///
/// ```yaml
/// jev:
///   model: typesafe/jev-1.13
///   surface: decisions
///   timeoutMs: 10000
///   retry:
///     maxAttempts: 3
///     initialBackoffMs: 200
///     maxBackoffMs: 2000
///   thresholds:
///     autoRouteConfidence: 0.7
///     isBugYes: 0.9
/// ```
///
/// # Examples
///
/// ```
/// let yaml = r#"
/// model: typesafe/jev-1.13
/// surface: decisions
/// timeoutMs: 5000
/// thresholds:
///   isBugYes: 0.9
/// "#;
/// let config: jev::Config = serde_norway::from_str(yaml).unwrap();
/// assert_eq!(config.surface, jev::Surface::Decisions);
/// assert_eq!(config.thresholds.get("isBugYes"), Some(0.9));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Config {
    /// The model to request. Defaults to [`crate::DEFAULT_MODEL`].
    #[serde(with = "model_as_string")]
    pub model: Model,
    /// The HTTP surface to call.
    pub surface: Surface,
    /// Override for the API base URL (scheme + host), mainly for tests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base: Option<url::Url>,
    /// Per-request timeout.
    #[serde(with = "duration_millis", rename = "timeoutMs")]
    pub timeout: Duration,
    /// Retry policy.
    pub retry: RetryConfig,
    /// Named probability thresholds.
    pub thresholds: Thresholds,
    /// Whether to run the cheap pre-flight context-size estimate.
    pub context_check: bool,
    /// Default session identifier applied to every request unless overridden by
    /// `RequestOptions::with_session_id` (Decisions surface only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_session_id: Option<String>,
    /// Default user identifier applied to every request unless overridden by
    /// `RequestOptions::with_user` (Decisions surface only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_user: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: DEFAULT_MODEL,
            surface: Surface::default(),
            api_base: None,
            timeout: DEFAULT_TIMEOUT,
            retry: RetryConfig::default(),
            thresholds: Thresholds::default(),
            context_check: true,
            default_session_id: None,
            default_user: None,
        }
    }
}

/// `serde` helper for `Model <-> String` via [`FromStr`] / [`std::fmt::Display`].
mod model_as_string {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{FromStr, Model};

    pub(super) fn serialize<S: Serializer>(m: &Model, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&m.to_string())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Model, D::Error> {
        let s = String::deserialize(d)?;
        Model::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_default_config_to_pinned_model_and_decisions_surface() {
        let config = Config::default();
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.surface, Surface::Decisions);
        assert!(config.context_check);
    }

    #[test]
    fn test_should_deserialize_config_from_yaml() {
        let yaml = r#"
model: typesafe/jev-1.13
surface: systemOne
timeoutMs: 5000
retry:
  maxAttempts: 5
  initialBackoffMs: 100
  maxBackoffMs: 1000
thresholds:
  isBugYes: 0.9
contextCheck: false
"#;
        let config: Config = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.model, Model::Jev113);
        assert_eq!(config.surface, Surface::SystemOne);
        assert_eq!(config.timeout, Duration::from_millis(5000));
        assert_eq!(config.retry.max_attempts, 5);
        assert_eq!(config.thresholds.get("isBugYes"), Some(0.9));
        assert!(!config.context_check);
    }

    #[test]
    fn test_should_reject_unknown_config_field() {
        let yaml = "model: jev-1.13\nbogusField: 1\n";
        assert!(serde_norway::from_str::<Config>(yaml).is_err());
    }

    #[test]
    fn test_should_reject_out_of_range_threshold_in_config() {
        let yaml = "thresholds:\n  bad: 1.5\n";
        assert!(serde_norway::from_str::<Config>(yaml).is_err());
    }

    #[test]
    fn test_should_reject_out_of_range_threshold_on_insert() {
        let mut thresholds = Thresholds::new();
        assert!(thresholds.insert("bad", 1.5).is_err());
        assert!(thresholds.insert("bad", f64::NAN).is_err());
        assert!(thresholds.insert("good", 0.5).is_ok());
    }

    #[test]
    fn test_should_require_return_error_for_missing_threshold() {
        let thresholds = Thresholds::new();
        assert!(matches!(
            thresholds.require("missing"),
            Err(Error::Invalid { .. })
        ));
    }

    #[test]
    fn test_should_build_retry_config_via_builder_methods() {
        let retry = RetryConfig::default()
            .with_max_attempts(5)
            .with_initial_backoff(Duration::from_millis(50))
            .with_max_backoff(Duration::from_secs(1));
        assert_eq!(retry.max_attempts, 5);
        assert_eq!(retry.initial_backoff, Duration::from_millis(50));
        assert_eq!(retry.max_backoff, Duration::from_secs(1));
    }

    #[test]
    fn test_should_disable_retry_with_one_attempt() {
        assert_eq!(RetryConfig::disabled().max_attempts, 1);
    }

    #[test]
    fn test_surface_path_matches_documented_endpoints() {
        assert_eq!(Surface::Decisions.path(), "/api/alpha/decisions");
        assert_eq!(Surface::SystemOne.path(), "/api/v1/systemone");
    }
}
