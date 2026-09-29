//! `State`: the state a decision is made about, matching the schema's
//! `anyOf: [string, object, array]`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;

/// The state a [`crate::Question`] set is evaluated against.
///
/// State is never logged: it may contain PII. See the crate-level docs for the
/// full logging policy.
///
/// # Examples
///
/// ```
/// use jev::State;
///
/// let from_str: State = "a plain-text ticket body".into();
/// assert!(matches!(from_str, State::Text(_)));
///
/// #[derive(serde::Serialize)]
/// struct Ticket { subject: String }
/// let from_struct = State::json(&Ticket { subject: "billing issue".into() }).unwrap();
/// assert!(matches!(from_struct, State::Json(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum State {
    /// Plain-text state.
    Text(String),
    /// Structured (object or array) state.
    Json(Value),
}

impl State {
    /// Serializes any [`serde::Serialize`] value into a JSON [`State`].
    ///
    /// This is the common case for domain structs: `State::json(&ticket)` instead of
    /// hand-building a `serde_json::Value`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when `value` cannot be serialized to JSON.
    pub fn json<T: Serialize>(value: &T) -> Result<Self, Error> {
        serde_json::to_value(value)
            .map(State::Json)
            .map_err(|e| Error::Invalid {
                field: "state".to_string(),
                reason: e.to_string(),
            })
    }

    /// The serialized byte length, used for the pre-flight context-size estimate.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self {
            State::Text(s) => s.len(),
            State::Json(v) => serde_json::to_string(v).map_or(usize::MAX, |s| s.len()),
        }
    }
}

impl From<&str> for State {
    fn from(s: &str) -> Self {
        State::Text(s.to_string())
    }
}

impl From<String> for State {
    fn from(s: String) -> Self {
        State::Text(s)
    }
}

impl From<Value> for State {
    fn from(v: Value) -> Self {
        State::Json(v)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Serialize)]
    struct Ticket {
        subject: String,
    }

    #[test]
    fn test_should_convert_str_and_string_into_text_state() {
        let a: State = "hi".into();
        let b: State = String::from("hi").into();
        assert_eq!(a, State::Text("hi".to_string()));
        assert_eq!(b, State::Text("hi".to_string()));
    }

    #[test]
    fn test_should_convert_value_into_json_state() {
        let v = json!({"a": 1});
        let s: State = v.clone().into();
        assert_eq!(s, State::Json(v));
    }

    #[test]
    fn test_should_serialize_domain_struct_via_state_json() {
        let ticket = Ticket {
            subject: "billing".to_string(),
        };
        let state = State::json(&ticket).unwrap();
        assert_eq!(state, State::Json(json!({"subject": "billing"})));
    }

    #[test]
    fn test_should_round_trip_serde_for_each_variant() {
        for s in [State::Text("hi".to_string()), State::Json(json!(["a", 1]))] {
            let encoded = serde_json::to_string(&s).unwrap();
            let decoded: State = serde_json::from_str(&encoded).unwrap();
            assert_eq!(s, decoded);
        }
    }

    #[test]
    fn test_should_report_byte_len_for_text_and_json() {
        assert_eq!(State::Text("abcd".to_string()).byte_len(), 4);
        assert_eq!(State::Json(json!({})).byte_len(), 2); // "{}"
    }
}
