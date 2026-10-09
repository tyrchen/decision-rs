//! Model identifiers and surface-aware wire-ID normalization.

use std::{borrow::Cow, fmt, str::FromStr};

use crate::{config::Surface, error::Error};

/// Bare (unnamespaced) System One ID for the pinned default model.
const JEV_1_13_BARE: &str = "jev-1.13";
/// Decisions-surface (namespaced) ID for the pinned default model.
const JEV_1_13_NAMESPACED: &str = "typesafe/jev-1.13";
/// Bare (unnamespaced) System One ID for the rolling-latest alias.
const JEV_LATEST_BARE: &str = "jev-latest";
/// Decisions-surface (namespaced) ID for the rolling-latest alias.
const JEV_LATEST_NAMESPACED: &str = "~typesafe/jev-latest";

/// The context window, in tokens, shared by every known Jev model as of this release.
const KNOWN_MODEL_CONTEXT_TOKENS: usize = 32_000;

/// A Jev / System One model identifier.
///
/// `Model` normalizes the documented ID mapping between the two API surfaces: the
/// Decisions surface expects an author-namespaced ID (`typesafe/jev-1.13`), while the
/// System One surface accepts and returns bare IDs (`jev-1.13`). Use
/// [`Model::wire_id`] to get the ID to send for a given [`Surface`].
/// [`crate::Client`] additionally maps the pinned default to `jev-1.13.0` for
/// [`crate::Provider::TypeSafe`], following the direct API's version convention.
///
/// # Examples
///
/// ```
/// use jev::{Model, Surface};
///
/// let model = Model::Jev113;
/// assert_eq!(model.wire_id(Surface::Decisions), "typesafe/jev-1.13");
/// assert_eq!(model.wire_id(Surface::SystemOne), "jev-1.13");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Model {
    /// `typesafe/jev-1.13` on the Decisions surface, `jev-1.13` on System One.
    ///
    /// This is [`DEFAULT_MODEL`]: pinned, not auto-updating. Thresholds are tuned
    /// against one model's calibration, so silently following `jev-latest` would
    /// silently re-tune every gate in production.
    Jev113,
    /// `~typesafe/jev-latest` on the Decisions surface, `jev-latest` on System One.
    ///
    /// Tracks the newest release. Intended for exploratory work, not for a service
    /// with tuned thresholds.
    Latest,
    /// Any other System One model ID, passed through unchanged on every surface.
    Custom(String),
}

/// The pinned default model. A bump is one reviewable line, deliberately never
/// [`Model::Latest`]. See [`Model::Jev113`] for the rationale.
pub const DEFAULT_MODEL: Model = Model::Jev113;

impl Model {
    /// The identifier to send on the wire for the given [`Surface`].
    ///
    /// [`Model::Custom`] is never rewritten: the caller owns the exact string they
    /// provided, on every surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::{Model, Surface};
    ///
    /// let custom = Model::Custom("acme/experimental".to_string());
    /// assert_eq!(custom.wire_id(Surface::Decisions), "acme/experimental");
    /// assert_eq!(custom.wire_id(Surface::SystemOne), "acme/experimental");
    /// ```
    #[must_use]
    pub fn wire_id(&self, surface: Surface) -> Cow<'_, str> {
        match (self, surface) {
            (Model::Jev113, Surface::Decisions) => Cow::Borrowed(JEV_1_13_NAMESPACED),
            (Model::Jev113, Surface::SystemOne) => Cow::Borrowed(JEV_1_13_BARE),
            (Model::Latest, Surface::Decisions) => Cow::Borrowed(JEV_LATEST_NAMESPACED),
            (Model::Latest, Surface::SystemOne) => Cow::Borrowed(JEV_LATEST_BARE),
            (Model::Custom(id), _) => Cow::Borrowed(id.as_str()),
        }
    }

    /// The model's context window, in tokens, used for a cheap pre-flight size check.
    ///
    /// Returns `None` for [`Model::Custom`], since the SDK has no data for it; the
    /// context-size guard is skipped in that case and the server remains authoritative.
    #[must_use]
    pub fn context_tokens(&self) -> Option<usize> {
        match self {
            Model::Jev113 | Model::Latest => Some(KNOWN_MODEL_CONTEXT_TOKENS),
            Model::Custom(_) => None,
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Model::Jev113 => f.write_str(JEV_1_13_NAMESPACED),
            Model::Latest => f.write_str(JEV_LATEST_NAMESPACED),
            Model::Custom(id) => f.write_str(id),
        }
    }
}

impl FromStr for Model {
    type Err = Error;

    /// Parses a model ID in either its bare or namespaced form.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::Model;
    ///
    /// assert_eq!("jev-1.13".parse::<Model>().unwrap(), Model::Jev113);
    /// assert_eq!("typesafe/jev-1.13".parse::<Model>().unwrap(), Model::Jev113);
    /// assert_eq!("~typesafe/jev-latest".parse::<Model>().unwrap(), Model::Latest);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when `s` is empty.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(Error::Invalid {
                field: "model".to_string(),
                reason: "model id must not be empty".to_string(),
            });
        }
        Ok(match s {
            JEV_1_13_BARE | JEV_1_13_NAMESPACED => Model::Jev113,
            JEV_LATEST_BARE | JEV_LATEST_NAMESPACED => Model::Latest,
            other => Model::Custom(other.to_string()),
        })
    }
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Surface::Decisions => f.write_str("decisions"),
            Surface::SystemOne => f.write_str("systemOne"),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn test_should_map_jev113_wire_id_per_surface() {
        assert_eq!(
            Model::Jev113.wire_id(Surface::Decisions),
            "typesafe/jev-1.13"
        );
        assert_eq!(Model::Jev113.wire_id(Surface::SystemOne), "jev-1.13");
    }

    #[test]
    fn test_should_map_latest_wire_id_per_surface() {
        assert_eq!(
            Model::Latest.wire_id(Surface::Decisions),
            "~typesafe/jev-latest"
        );
        assert_eq!(Model::Latest.wire_id(Surface::SystemOne), "jev-latest");
    }

    #[test]
    fn test_should_never_rewrite_custom_model() {
        let custom = Model::Custom("acme/model-x".to_string());
        assert_eq!(custom.wire_id(Surface::Decisions), "acme/model-x");
        assert_eq!(custom.wire_id(Surface::SystemOne), "acme/model-x");
    }

    #[test]
    fn test_should_default_to_pinned_jev113_not_latest() {
        assert_eq!(DEFAULT_MODEL, Model::Jev113);
    }

    #[test]
    fn test_should_parse_bare_and_namespaced_ids() {
        assert_eq!("jev-1.13".parse::<Model>().unwrap(), Model::Jev113);
        assert_eq!("typesafe/jev-1.13".parse::<Model>().unwrap(), Model::Jev113);
        assert_eq!("jev-latest".parse::<Model>().unwrap(), Model::Latest);
        assert_eq!(
            "~typesafe/jev-latest".parse::<Model>().unwrap(),
            Model::Latest
        );
    }

    #[test]
    fn test_should_parse_unknown_id_as_custom() {
        assert_eq!(
            "acme/model-x".parse::<Model>().unwrap(),
            Model::Custom("acme/model-x".to_string())
        );
    }

    #[test]
    fn test_should_reject_empty_model_string() {
        assert!("".parse::<Model>().is_err());
    }

    #[test]
    fn test_should_report_context_tokens_for_known_models_only() {
        assert_eq!(Model::Jev113.context_tokens(), Some(32_000));
        assert_eq!(Model::Latest.context_tokens(), Some(32_000));
        assert_eq!(Model::Custom("x".into()).context_tokens(), None);
    }

    proptest! {
        #[test]
        fn test_should_round_trip_display_and_from_str_for_custom(id in "[a-z][a-z0-9_/.~-]{1,40}") {
            // Avoid colliding with the two reserved canonical forms, which is not a
            // round-trip failure but a deliberate, documented normalization.
            prop_assume!(!matches!(id.as_str(), "jev-1.13" | "typesafe/jev-1.13" | "jev-latest" | "~typesafe/jev-latest"));
            let model = Model::Custom(id.clone());
            let displayed = model.to_string();
            let parsed: Model = displayed.parse().unwrap();
            prop_assert_eq!(parsed, Model::Custom(id));
        }

        #[test]
        fn test_should_round_trip_display_and_from_str_for_canonical_models(pick in 0usize..2) {
            let model = if pick == 0 { Model::Jev113 } else { Model::Latest };
            let displayed = model.to_string();
            let parsed: Model = displayed.parse().unwrap();
            prop_assert_eq!(parsed, model);
        }
    }
}
