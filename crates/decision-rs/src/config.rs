//! Validated, secret-free runtime configuration.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{Error, Model, Probability, Provider, question::validate_key};

/// Retry policy. Retry-After is honored even beyond the computed backoff cap.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[non_exhaustive]
pub struct RetryConfig {
    /// Maximum attempts including the first (1..=10).
    pub max_attempts: u32,
    /// Initial computed backoff in milliseconds.
    pub initial_backoff_ms: u64,
    /// Maximum computed backoff in milliseconds (at most 30000).
    pub max_backoff_ms: u64,
}
impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_ms: 200,
            max_backoff_ms: 2000,
        }
    }
}
impl RetryConfig {
    /// Disable retries.
    /// ```
    /// assert_eq!(decision_rs::RetryConfig::disabled().max_attempts, 1);
    /// ```
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }
    fn validate(self) -> Result<(), Error> {
        if !(1..=10).contains(&self.max_attempts)
            || self.initial_backoff_ms == 0
            || self.initial_backoff_ms > self.max_backoff_ms
            || self.max_backoff_ms > 30_000
        {
            return Err(Error::InvalidConfiguration("retry bounds"));
        }
        Ok(())
    }
}
/// Byte limits can only be lowered from these hard ceilings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Limits {
    /// Evidence bytes, at most 1 MiB.
    pub input_bytes: usize,
    /// Serialized request bytes, at most 2 MiB.
    pub request_bytes: usize,
    /// Streamed response bytes, at most 4 MiB.
    pub response_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            request_bytes: 2 << 20,
            response_bytes: 4 << 20,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<(), Error> {
        let hard = Self::default();
        if self.input_bytes == 0
            || self.input_bytes > hard.input_bytes
            || self.request_bytes == 0
            || self.request_bytes > hard.request_bytes
            || self.response_bytes == 0
            || self.response_bytes > hard.response_bytes
        {
            return Err(Error::InvalidConfiguration("byte limits"));
        }
        Ok(())
    }
}
/// Validated named probability thresholds.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Thresholds(BTreeMap<String, Probability>);
impl Thresholds {
    /// Create an empty threshold collection.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let gates = decision_rs::Thresholds::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Insert a validated probability. Names follow question key rules; at most 64 names.
    /// # Errors
    /// Returns an invalid configuration error for an invalid name or count.
    /// ```
    /// let mut gates = decision_rs::Thresholds::new();
    /// gates.insert("route", decision_rs::Probability::try_from(0.8)? )?;
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn insert(&mut self, name: &str, probability: Probability) -> Result<(), Error> {
        validate_key(name).map_err(|_| Error::InvalidConfiguration("threshold name"))?;
        if !self.0.contains_key(name) && self.0.len() >= 64 {
            return Err(Error::InvalidConfiguration("threshold count"));
        }
        self.0.insert(name.to_owned(), probability);
        Ok(())
    }
    /// Look up a named threshold.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let mut gates = decision_rs::Thresholds::new();
    /// gates.insert("route", decision_rs::Probability::try_from(0.9)?)?;
    /// let gate = gates.get("route");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Probability> {
        self.0.get(name).copied()
    }
    /// Require a named threshold.
    /// # Errors
    /// Returns an invalid configuration error if missing.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let mut gates = decision_rs::Thresholds::new();
    /// gates.insert("route", decision_rs::Probability::try_from(0.9)?)?;
    /// let gate = gates.require("route")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn require(&self, name: &str) -> Result<Probability, Error> {
        self.get(name)
            .ok_or(Error::InvalidConfiguration("missing threshold"))
    }
}
impl<'de> Deserialize<'de> for Thresholds {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = BTreeMap::<String, Probability>::deserialize(deserializer)?;
        let mut thresholds = Self::new();
        for (name, value) in entries {
            thresholds
                .insert(&name, value)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(thresholds)
    }
}
/// Settings frozen and validated when constructing a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Settings {
    /// Entire network deadline including queueing, reads and backoff (1..=120000 ms).
    pub timeout_ms: u64,
    /// Retry configuration.
    pub retry: RetryConfig,
    /// Downward-adjustable byte limits.
    pub limits: Limits,
    /// Application-defined acceptance gates.
    pub thresholds: Thresholds,
    /// Shared client/clone concurrency ceiling (1..=128).
    pub max_inflight: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            timeout_ms: 10_000,
            retry: RetryConfig::default(),
            limits: Limits::default(),
            thresholds: Thresholds::default(),
            max_inflight: 64,
        }
    }
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !(1..=120_000).contains(&self.timeout_ms) || !(1..=128).contains(&self.max_inflight) {
            return Err(Error::InvalidConfiguration(
                "deadline or concurrency bounds",
            ));
        }
        self.retry.validate()?;
        self.limits.validate()
    }
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}
/// Secret-free configuration; a provider must always be specified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    /// Hosted provider.
    pub provider: Provider,
    /// Logical model; Default resolves for the provider.
    #[serde(default)]
    pub model: Model,
    /// Runtime settings, flattened into configuration.
    #[serde(flatten)]
    pub settings: Settings,
}
impl Config {
    /// Build default settings for a provider.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let config = decision_rs::Config::new(decision_rs::Provider::OpenAi);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            model: Model::Default,
            settings: Settings::default(),
        }
    }
    /// Load bounded YAML with duplicate keys, aliases and unknown fields rejected.
    /// # Errors
    /// Returns invalid configuration for malformed or invalid settings.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let config = decision_rs::Config::from_yaml_str("provider: openai\ntimeoutMs: 10000")?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(text: &str) -> Result<Self, Error> {
        let config: Self = crate::yaml::parse(text, 64 << 10)
            .map_err(|_| Error::InvalidConfiguration("YAML configuration"))?;
        config.settings.validate()?;
        config.model.resolve(Some(config.provider))?;
        Ok(config)
    }
}
