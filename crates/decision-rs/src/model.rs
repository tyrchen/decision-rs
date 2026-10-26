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
    /// # fn example(value: &decision_rs::ModelId) -> Result<(), decision_rs::Error> {
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
    /// Pinned Jev 1.14 version; the default for `OpenRouter` and `TypeSafe`.
    Jev114,
    /// Explicit rolling Jev alias.
    JevLatest,
    /// `OpenAI` GPT-6 Luna; the alias does not promise fixed weights.
    Gpt6Luna,
    /// Cloudflare Clef rolling alias.
    Clef,
    /// Cloudflare Clef Flash rolling alias.
    ClefFlash,
    /// Cloudflare Clef Omni rolling alias (text input in this SDK).
    ClefOmni,
    /// A validated, uninterpreted model identifier.
    Custom(ModelId),
}
impl Model {
    pub(crate) fn resolve(&self, provider: Option<Provider>) -> Result<(Self, String), Error> {
        let model = if *self == Self::Default {
            match provider {
                Some(Provider::OpenAi) => Self::Gpt6Luna,
                Some(Provider::OpenRouter | Provider::TypeSafe) => Self::Jev114,
                Some(Provider::Cloudflare) => Self::Clef,
                None => return Err(Error::UnsupportedModel),
            }
        } else {
            self.clone()
        };
        let wire = match (&model, provider) {
            (Self::Gpt6Luna, Some(Provider::OpenAi)) => "gpt-6-luna".to_owned(),
            (Self::Jev113, Some(Provider::OpenRouter)) => "typesafe/jev-1.13".to_owned(),
            (Self::Jev113, Some(Provider::TypeSafe)) => "jev-1.13.0".to_owned(),
            (Self::Jev114, Some(Provider::OpenRouter)) => "typesafe/jev-1.14".to_owned(),
            (Self::Jev114, Some(Provider::TypeSafe)) => "jev-1.14.0".to_owned(),
            (Self::JevLatest, Some(Provider::OpenRouter)) => "~typesafe/jev-latest".to_owned(),
            (Self::JevLatest, Some(Provider::TypeSafe)) => "jev-latest".to_owned(),
            (Self::Clef, Some(Provider::Cloudflare)) => "clef".to_owned(),
            (Self::ClefFlash, Some(Provider::Cloudflare)) => "clef-flash".to_owned(),
            (Self::ClefOmni, Some(Provider::Cloudflare)) => "clef-omni".to_owned(),
            (Self::Custom(_), Some(Provider::Cloudflare)) => return Err(Error::UnsupportedModel),
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
        "clef" | "@cf/cloudflare/clef" => Some(Model::Clef),
        "clef-flash" | "@cf/cloudflare/clef-flash" => Some(Model::ClefFlash),
        "clef-omni" | "@cf/cloudflare/clef-omni" => Some(Model::ClefOmni),
        "gpt-6-luna" => Some(Model::Gpt6Luna),
        "jev-1.13" | "jev-1.13.0" | "typesafe/jev-1.13" => Some(Model::Jev113),
        "jev-1.14" | "jev-1.14.0" | "typesafe/jev-1.14" => Some(Model::Jev114),
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
            Self::Clef => "clef",
            Self::ClefFlash => "clef-flash",
            Self::ClefOmni => "clef-omni",
            Self::Gpt6Luna => "gpt-6-luna",
            Self::Jev113 => "jev-1.13",
            Self::Jev114 => "jev-1.14",
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
    #[case(Provider::OpenRouter, Model::Default, "typesafe/jev-1.14")]
    #[case(Provider::TypeSafe, Model::Default, "jev-1.14.0")]
    #[case(Provider::OpenRouter, Model::Jev113, "typesafe/jev-1.13")]
    #[case(Provider::TypeSafe, Model::Jev113, "jev-1.13.0")]
    #[case(Provider::OpenRouter, Model::Jev114, "typesafe/jev-1.14")]
    #[case(Provider::TypeSafe, Model::Jev114, "jev-1.14.0")]
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
    #[case("jev-1.14")]
    #[case("jev-1.14.0")]
    #[case("typesafe/jev-1.14")]
    #[case("jev-latest")]
    #[case("~typesafe/jev-latest")]
    #[case("default")]
    fn test_should_reject_builtin_custom_bypass(#[case] id: &str) {
        let model = Model::Custom(ModelId::try_from(id).unwrap());
        assert!(model.resolve(Some(Provider::OpenAi)).is_err());
        assert!(model.resolve(None).is_err());
    }
    #[rstest]
    #[case("jev-1.14")]
    #[case("jev-1.14.0")]
    #[case("typesafe/jev-1.14")]
    fn test_should_parse_and_serialize_jev_114(#[case] id: &str) {
        let model: Model = id.parse().unwrap();
        assert_eq!(model, Model::Jev114);
        assert_eq!(model.to_string(), "jev-1.14");
        assert_eq!(
            serde_json::from_value::<Model>(serde_json::json!(id)).unwrap(),
            model
        );
        assert_eq!(serde_json::to_value(&model).unwrap(), "jev-1.14");
        for provider in [Some(Provider::OpenAi), Some(Provider::Cloudflare), None] {
            assert!(matches!(
                model.resolve(provider),
                Err(Error::UnsupportedModel)
            ));
        }
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

#[cfg(test)]
mod clef_tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("clef", Model::Clef)]
    #[case("clef-flash", Model::ClefFlash)]
    #[case("clef-omni", Model::ClefOmni)]
    fn test_should_parse_clef_aliases_and_reject_incompatible_providers(
        #[case] name: &str,
        #[case] model: Model,
    ) {
        assert_eq!(name.parse::<Model>().unwrap(), model);
        assert_eq!(
            format!("@cf/cloudflare/{name}").parse::<Model>().unwrap(),
            model
        );
        assert_eq!(model.to_string(), name);
        assert_eq!(
            serde_json::from_value::<Model>(serde_json::to_value(&model).unwrap()).unwrap(),
            model
        );
        assert_eq!(model.resolve(Some(Provider::Cloudflare)).unwrap().1, name);
        for provider in [
            Some(Provider::OpenAi),
            Some(Provider::OpenRouter),
            Some(Provider::TypeSafe),
            None,
        ] {
            assert!(matches!(
                model.resolve(provider),
                Err(Error::UnsupportedModel)
            ));
            assert!(
                Model::Custom(ModelId::try_from(name).unwrap())
                    .resolve(provider)
                    .is_err()
            );
        }
    }
    #[test]
    fn test_should_reject_non_clef_hosted_models_and_preserve_checkpoint_ids() {
        for model in [
            Model::Jev113,
            Model::Jev114,
            Model::JevLatest,
            Model::Gpt6Luna,
            "other".parse().unwrap(),
            "Cloudflare/clef".parse().unwrap(),
        ] {
            assert!(matches!(
                model.resolve(Some(Provider::Cloudflare)),
                Err(Error::UnsupportedModel)
            ));
        }
        assert_eq!(
            "Cloudflare/clef"
                .parse::<Model>()
                .unwrap()
                .resolve(None)
                .unwrap()
                .1,
            "Cloudflare/clef"
        );
        assert!("@cf/cloudflare/unknown".parse::<Model>().is_err());
    }
}
