//! Provider-aware logical models and validated custom identifiers.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{error::Error, provider::Provider};

/// Validated custom model ID.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelId(String);
impl TryFrom<&str> for ModelId {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:/~+-".contains(&b))
        {
            return Err(Error::InvalidConfiguration("invalid model ID"));
        }
        Ok(Self(value.to_owned()))
    }
}
impl ModelId {
    /// Exact custom model string.
    ///
    /// ```no_run
    /// # fn example(value: &jev::ModelId) -> Result<(), jev::Error> {
    /// let id = value.as_str();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// A logical model; Default is resolved once against the selected provider.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Model {
    /// The selected hosted provider's default.
    #[default]
    Default,
    /// Pinned Jev 1.13 version.
    Jev113,
    /// Explicit rolling Jev alias.
    JevLatest,
    /// `OpenAI` GPT-6 Luna; the alias does not promise fixed weights.
    Gpt6Luna,
    /// A validated, uninterpreted model identifier.
    Custom(ModelId),
}
impl Model {
    pub(crate) fn resolve(&self, provider: Option<Provider>) -> Result<(Self, String), Error> {
        let model = if *self == Self::Default {
            match provider {
                Some(Provider::OpenAi) => Self::Gpt6Luna,
                Some(_) => Self::Jev113,
                None => return Err(Error::UnsupportedModel),
            }
        } else {
            self.clone()
        };
        let wire = match (&model, provider) {
            (Self::Gpt6Luna, Some(Provider::OpenAi)) => "gpt-6-luna".to_owned(),
            (Self::Jev113, Some(Provider::OpenRouter)) => "typesafe/jev-1.13".to_owned(),
            (Self::Jev113, Some(Provider::TypeSafe)) => "jev-1.13.0".to_owned(),
            (Self::JevLatest, Some(Provider::OpenRouter)) => "~typesafe/jev-latest".to_owned(),
            (Self::JevLatest, Some(Provider::TypeSafe)) => "jev-latest".to_owned(),
            (Self::Custom(id), _) => {
                if built_in(id.as_str()).is_some() {
                    return Err(Error::InvalidConfiguration(
                        "use built-in variant for known model",
                    ));
                }
                id.as_str().to_owned()
            }
            _ => return Err(Error::UnsupportedModel),
        };
        Ok((model, wire))
    }
}
fn built_in(value: &str) -> Option<Model> {
    match value {
        "default" => Some(Model::Default),
        "gpt-6-luna" => Some(Model::Gpt6Luna),
        "jev-1.13" | "jev-1.13.0" | "typesafe/jev-1.13" => Some(Model::Jev113),
        "jev-latest" | "~typesafe/jev-latest" => Some(Model::JevLatest),
        _ => None,
    }
}
impl FromStr for Model {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        built_in(value).map_or_else(|| ModelId::try_from(value).map(Self::Custom), Ok)
    }
}
impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::Gpt6Luna => "gpt-6-luna",
            Self::Jev113 => "jev-1.13",
            Self::JevLatest => "jev-latest",
            Self::Custom(id) => id.as_str(),
        })
    }
}
impl Serialize for Model {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Model {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(|_| serde::de::Error::custom("invalid model"))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    #[rstest]
    #[case(Provider::OpenAi, Model::Default, "gpt-6-luna")]
    #[case(Provider::OpenRouter, Model::Default, "typesafe/jev-1.13")]
    #[case(Provider::TypeSafe, Model::Default, "jev-1.13.0")]
    #[case(Provider::OpenRouter, Model::JevLatest, "~typesafe/jev-latest")]
    #[case(Provider::TypeSafe, Model::JevLatest, "jev-latest")]
    fn test_should_resolve_model_namespaces(
        #[case] provider: Provider,
        #[case] model: Model,
        #[case] expected: &str,
    ) {
        assert_eq!(model.resolve(Some(provider)).unwrap().1, expected);
    }
    #[rstest]
    #[case("gpt-6-luna")]
    #[case("jev-1.13")]
    #[case("jev-1.13.0")]
    #[case("typesafe/jev-1.13")]
    #[case("jev-latest")]
    #[case("~typesafe/jev-latest")]
    #[case("default")]
    fn test_should_reject_builtin_custom_bypass(#[case] id: &str) {
        let model = Model::Custom(ModelId::try_from(id).unwrap());
        assert!(model.resolve(Some(Provider::OpenAi)).is_err());
        assert!(model.resolve(None).is_err());
    }
    #[test]
    fn test_should_preserve_custom_models_and_validate_identifiers() {
        let model: Model = "vendor/custom-1.0".parse().unwrap();
        for provider in [
            Some(Provider::OpenAi),
            Some(Provider::OpenRouter),
            Some(Provider::TypeSafe),
            None,
        ] {
            assert_eq!(model.resolve(provider).unwrap().1, "vendor/custom-1.0");
        }
        for id in ["", "bad model", "a?b", "模型", "\n"] {
            assert!(id.parse::<Model>().is_err());
        }
        assert!("a".repeat(129).parse::<Model>().is_err());
    }
}
