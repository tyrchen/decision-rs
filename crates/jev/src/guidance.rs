//! `Guidance`: the repeated `string | object | array` shape used for `instructions`
//! and for each `choice`/`score` criterion.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Free-form guidance text sent to the model: a question's `instructions`, or one
/// option's criteria.
///
/// The API accepts a string, an object, or an array for these fields. Callers who
/// only ever pass strings — the overwhelming majority — never need to name this type,
/// because every constructor that accepts guidance takes `impl Into<Guidance>` and
/// `&str` / `String` convert directly.
///
/// # Examples
///
/// ```
/// use jev::Guidance;
///
/// let g: Guidance = "Is the customer asking for a refund?".into();
/// assert_eq!(g.to_string(), "Is the customer asking for a refund?");
/// assert!(!g.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Guidance {
    /// The common case: plain-text instructions or criteria.
    Text(String),
    /// A structured object, passed through verbatim.
    Object(serde_json::Map<String, Value>),
    /// A structured array, passed through verbatim.
    Array(Vec<Value>),
}

impl Guidance {
    /// The guidance as `&str`, when it is the [`Guidance::Text`] variant.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Guidance::Text(s) => Some(s.as_str()),
            Guidance::Object(_) | Guidance::Array(_) => None,
        }
    }

    /// The serialized byte length, used for boundary size checks.
    ///
    /// For [`Guidance::Text`] this is the string's UTF-8 byte length; for the
    /// structured variants it is the byte length of the compact JSON encoding.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self {
            Guidance::Text(s) => s.len(),
            Guidance::Object(_) | Guidance::Array(_) => {
                serde_json::to_string(self).map_or(usize::MAX, |s| s.len())
            }
        }
    }

    /// Whether the guidance carries no content at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Guidance::Text(s) => s.trim().is_empty(),
            Guidance::Object(m) => m.is_empty(),
            Guidance::Array(a) => a.is_empty(),
        }
    }
}

impl std::fmt::Display for Guidance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Guidance::Text(s) => f.write_str(s),
            Guidance::Object(_) | Guidance::Array(_) => {
                // Structured guidance has no natural plain-text rendering; fall back to
                // its JSON form rather than panicking or silently dropping content.
                let json = serde_json::to_string(self).unwrap_or_default();
                f.write_str(&json)
            }
        }
    }
}

impl From<&str> for Guidance {
    fn from(s: &str) -> Self {
        Guidance::Text(s.to_string())
    }
}

impl From<String> for Guidance {
    fn from(s: String) -> Self {
        Guidance::Text(s)
    }
}

impl From<Value> for Guidance {
    /// Converts an arbitrary [`serde_json::Value`] into `Guidance`.
    ///
    /// A JSON string becomes [`Guidance::Text`]; an object or array becomes the
    /// matching structured variant; any other JSON scalar (number, bool, null) is
    /// rendered as its compact string form so no caller input is silently dropped.
    fn from(value: Value) -> Self {
        match value {
            Value::String(s) => Guidance::Text(s),
            Value::Object(m) => Guidance::Object(m),
            Value::Array(a) => Guidance::Array(a),
            other => Guidance::Text(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_convert_str_and_string_into_text_guidance() {
        let a: Guidance = "hello".into();
        let b: Guidance = String::from("hello").into();
        assert_eq!(a, Guidance::Text("hello".to_string()));
        assert_eq!(b, Guidance::Text("hello".to_string()));
    }

    #[test]
    fn test_should_display_text_guidance_as_plain_string() {
        let g: Guidance = "plain text".into();
        assert_eq!(g.to_string(), "plain text");
    }

    #[test]
    fn test_should_round_trip_serde_for_each_variant() {
        let text: Guidance = "hi".into();
        let object = Guidance::from(json!({"k": "v"}));
        let array = Guidance::from(json!(["a", "b"]));

        for g in [text, object, array] {
            let encoded = serde_json::to_string(&g).unwrap();
            let decoded: Guidance = serde_json::from_str(&encoded).unwrap();
            assert_eq!(g, decoded);
        }
    }

    #[test]
    fn test_should_report_emptiness_per_variant() {
        assert!(Guidance::Text("   ".to_string()).is_empty());
        assert!(!Guidance::Text("x".to_string()).is_empty());
        assert!(Guidance::Object(serde_json::Map::new()).is_empty());
        assert!(Guidance::Array(vec![]).is_empty());
    }

    #[test]
    fn test_should_convert_non_string_scalar_json_to_text() {
        let g = Guidance::from(json!(42));
        assert_eq!(g.as_str(), Some("42"));
        assert_eq!(g.to_string(), "42");
    }
}
