//! Hosted provider selection, independent of request wire formats.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Hosted provider; credentials never fall back across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Provider {
    /// `OpenAI` Decisions.
    OpenAi,
    /// Cloudflare Workers AI Clef decisions.
    Cloudflare,
    /// `OpenRouter` Jev Decisions.
    OpenRouter,
    /// `TypeSafe` AI System One.
    TypeSafe,
}
impl Provider {
    pub(crate) fn key_env(self) -> &'static str {
        match self {
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Cloudflare => "CLOUDFLARE_API_TOKEN",
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::TypeSafe => "TYPESAFE_API_KEY",
        }
    }
}
impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenAi => "openai",
            Self::Cloudflare => "cloudflare",
            Self::OpenRouter => "openrouter",
            Self::TypeSafe => "typesafe",
        })
    }
}
impl FromStr for Provider {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        match value {
            "openai" => Ok(Self::OpenAi),
            "cloudflare" => Ok(Self::Cloudflare),
            "openrouter" => Ok(Self::OpenRouter),
            "typesafe" => Ok(Self::TypeSafe),
            _ => Err(Error::InvalidConfiguration("unknown provider")),
        }
    }
}
