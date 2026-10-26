//! Validated Cloudflare account identifiers, separate from secret API tokens.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de::Error as DeserializeError};

use crate::{Error, Provider};

/// A 32-byte ASCII hexadecimal Cloudflare account identifier.
///
/// ```
/// use decision_rs::CloudflareAccountId;
/// let account: CloudflareAccountId = "0123456789abcdef0123456789abcdef".parse()?;
/// assert_eq!(account.as_str().len(), 32);
/// # Ok::<(), decision_rs::Error>(())
/// ```
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct CloudflareAccountId(String);

impl fmt::Debug for CloudflareAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CloudflareAccountId([redacted])")
    }
}
impl CloudflareAccountId {
    /// Borrow the validated identifier for explicit application use.
    ///
    /// ```
    /// let account: decision_rs::CloudflareAccountId = "0123456789abcdef0123456789abcdef".parse()?;
    /// assert_eq!(account.as_str(), "0123456789abcdef0123456789abcdef");
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for CloudflareAccountId {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self, Error> {
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidConfiguration("invalid Cloudflare account ID"));
        }
        Ok(Self(value.to_owned()))
    }
}
impl FromStr for CloudflareAccountId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        Self::try_from(value)
    }
}
impl<'de> Deserialize<'de> for CloudflareAccountId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

pub(crate) fn validate_account(
    provider: Provider,
    account: Option<&CloudflareAccountId>,
) -> Result<(), Error> {
    match (provider, account) {
        (Provider::Cloudflare, None) => {
            Err(Error::InvalidConfiguration("missing Cloudflare account ID"))
        }
        (Provider::Cloudflare, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => Err(Error::InvalidConfiguration(
            "account ID requires Cloudflare provider",
        )),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("0123456789abcdef0123456789abcdef")]
    #[case("ABCDEF0123456789ABCDEF0123456789")]
    fn test_should_roundtrip_valid_accounts_without_debug_exposure(#[case] value: &str) {
        let account: CloudflareAccountId = value.parse().unwrap();
        let encoded = serde_json::to_string(&account).unwrap();
        assert_eq!(
            serde_json::from_str::<CloudflareAccountId>(&encoded).unwrap(),
            account
        );
        assert!(!format!("{account:?}").contains(value));
    }
    #[rstest]
    #[case("")]
    #[case("0123456789abcdef0123456789abcde")]
    #[case("0123456789abcdef0123456789abcdef0")]
    #[case(" 123456789abcdef0123456789abcdef")]
    #[case("/123456789abcdef0123456789abcdef")]
    #[case("%123456789abcdef0123456789abcdef")]
    #[case("?123456789abcdef0123456789abcdef")]
    #[case("g123456789abcdef0123456789abcdef")]
    #[case("\n123456789abcdef0123456789abcdef")]
    #[case("中文账户")]
    fn test_should_reject_invalid_accounts_at_every_boundary(#[case] value: &str) {
        assert!(CloudflareAccountId::try_from(value).is_err());
        assert!(value.parse::<CloudflareAccountId>().is_err());
        assert!(serde_json::from_value::<CloudflareAccountId>(serde_json::json!(value)).is_err());
    }
}
