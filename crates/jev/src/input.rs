//! Validated, immutable text evidence shared by every backend.

use std::{fmt, sync::Arc};

use serde::Serialize;
use serde_json::Value;

use crate::{error::Error, json};

/// Maximum input length in UTF-8 bytes.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Prepared evidence. JSON evidence is sorted compact text on every backend.
///
/// ```
/// let input = jev::Input::try_from("a support ticket")?;
/// assert_eq!(input.as_str(), "a support ticket");
/// # Ok::<(), jev::Error>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Input(Arc<str>);
impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}
impl Input {
    /// Serialize an object or array as bounded, canonical JSON evidence.
    ///
    /// # Errors
    /// Rejects scalars, excessive nesting, strings, nodes, or encoded size.
    ///
    /// ```
    /// let input = jev::Input::json(&serde_json::json!({"z": 2, "a": 1}))?;
    /// assert_eq!(input.as_str(), r#"{"a":1,"z":2}"#);
    /// # Ok::<(), jev::Error>(())
    /// ```
    pub fn json<T: Serialize>(value: &T) -> Result<Self, Error> {
        let bytes = json::encode(value, MAX_INPUT_BYTES)?;
        let mut value = json::parse(&bytes, 256 * 1024, 256)
            .map_err(|_| Error::InvalidInput("JSON evidence exceeds structural limits"))?;
        if !matches!(value, Value::Object(_) | Value::Array(_)) {
            return Err(Error::InvalidInput(
                "JSON evidence must be an object or array",
            ));
        }
        value.sort_all_objects();
        let text = serde_json::to_string(&value)
            .map_err(|_| Error::InvalidInput("JSON encoding failed"))?;
        Self::try_from(text)
    }
    /// Borrow the exact evidence text sent to every provider.
    ///
    /// ```no_run
    /// # fn example(value: &jev::Input) -> Result<(), jev::Error> {
    /// let evidence = value.as_str();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for Input {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self, Error> {
        if value.trim().is_empty() || value.len() > MAX_INPUT_BYTES {
            return Err(Error::InvalidInput(
                "input must be nonempty and at most 1 MiB",
            ));
        }
        Ok(Self(Arc::from(value)))
    }
}
impl TryFrom<String> for Input {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        Self::try_from(value.as_str())
    }
}
mod sealed {
    pub trait Sealed {}
    impl Sealed for &str {}
    impl Sealed for String {}
    impl Sealed for super::Input {}
    impl Sealed for &super::Input {}
}
/// Supported fallible conversions into prepared evidence.
pub trait IntoInput: sealed::Sealed {
    /// Validate or cheaply share evidence.
    ///
    /// # Errors
    /// Returns the validation errors documented by Input.
    /// ```
    /// use jev::IntoInput;
    /// let input = "A support ticket".try_into_input()?;
    /// assert_eq!(input.as_str(), "A support ticket");
    /// # Ok::<(), jev::Error>(())
    /// ```
    fn try_into_input(self) -> Result<Input, Error>;
}
impl IntoInput for &str {
    fn try_into_input(self) -> Result<Input, Error> {
        Input::try_from(self)
    }
}
impl IntoInput for String {
    fn try_into_input(self) -> Result<Input, Error> {
        Input::try_from(self)
    }
}
impl IntoInput for Input {
    fn try_into_input(self) -> Result<Input, Error> {
        Ok(self)
    }
}
impl IntoInput for &Input {
    fn try_into_input(self) -> Result<Input, Error> {
        Ok(self.clone())
    }
}
