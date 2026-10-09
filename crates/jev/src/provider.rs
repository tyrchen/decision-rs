//! Hosted providers and their endpoint conventions.

use std::{borrow::Cow, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{config::Surface, error::Error, model::Model};

/// The hosted service that receives requests and authenticates the API key.
///
/// `OpenRouter` supports both API surfaces. `TypeSafe` always uses System One,
/// including when a configuration leaves the surface at its default.
///
/// # Examples
///
/// ```
/// use jev::Provider;
///
/// assert_eq!(Provider::default(), Provider::OpenRouter);
/// assert_eq!("typesafe".parse::<Provider>()?, Provider::TypeSafe);
/// # Ok::<(), jev::Error>(())
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Provider {
    /// `OpenRouter`, authenticated with an `OpenRouter` API key.
    #[default]
    #[serde(rename = "openrouter", alias = "openRouter")]
    OpenRouter,
    /// `TypeSafe` AI's direct API, authenticated with a `TypeSafe` API key.
    #[serde(rename = "typesafe", alias = "typeSafe", alias = "typesafeai")]
    TypeSafe,
}

impl Provider {
    pub(crate) fn api_base(self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai",
            Self::TypeSafe => "https://api.typesafe.ai",
        }
    }

    pub(crate) fn surface(self, requested: Surface) -> Surface {
        match self {
            Self::OpenRouter => requested,
            Self::TypeSafe => Surface::SystemOne,
        }
    }

    pub(crate) fn path(self, requested: Surface) -> &'static str {
        match self {
            Self::OpenRouter => requested.path(),
            Self::TypeSafe => "/v1/systemone",
        }
    }

    pub(crate) fn api_key_env(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::TypeSafe => "TYPESAFE_API_KEY",
        }
    }

    pub(crate) fn wire_id(self, model: &Model, requested: Surface) -> Cow<'_, str> {
        match (self, model) {
            (Self::TypeSafe, Model::Jev113) => Cow::Borrowed("jev-1.13.0"),
            _ => model.wire_id(self.surface(requested)),
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenRouter => "openrouter",
            Self::TypeSafe => "typesafe",
        })
    }
}

impl FromStr for Provider {
    type Err = Error;

    /// Parses a provider name used by `JEV_PROVIDER` or configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] for an unknown provider name.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::Provider;
    ///
    /// assert_eq!("openrouter".parse::<Provider>()?, Provider::OpenRouter);
    /// assert_eq!("typesafeai".parse::<Provider>()?, Provider::TypeSafe);
    /// # Ok::<(), jev::Error>(())
    /// ```
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openrouter" | "openRouter" => Ok(Self::OpenRouter),
            "typesafe" | "typeSafe" | "typesafeai" => Ok(Self::TypeSafe),
            _ => Err(Error::Invalid {
                field: "provider".to_string(),
                reason: "must be openrouter or typesafe".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Provider::OpenRouter)]
    #[case(Provider::TypeSafe)]
    fn test_should_round_trip_provider_names(#[case] provider: Provider) {
        assert_eq!(provider.to_string().parse::<Provider>().unwrap(), provider);
        let json = serde_json::to_string(&provider).unwrap();
        assert_eq!(serde_json::from_str::<Provider>(&json).unwrap(), provider);
    }

    #[test]
    fn test_should_reject_unknown_provider() {
        assert!(matches!(
            "other".parse::<Provider>(),
            Err(Error::Invalid { .. })
        ));
        assert!(serde_json::from_str::<Provider>("\"other\"").is_err());
    }
}
