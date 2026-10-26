//! Workers AI REST envelope; the inner decision codec is shared with System One.

use serde_json::Value;

use super::{Decoded, NumericPolicy, jev};
use crate::{Error, Questions, json};

const MAX_DETAILS: usize = 32;

pub(super) fn decode(mut value: Value, questions: &Questions) -> Result<Decoded, Error> {
    let envelope = value
        .as_object_mut()
        .ok_or_else(|| Error::protocol("Cloudflare envelope"))?;
    if envelope
        .get("success")
        .is_some_and(|success| success != &Value::Bool(true))
    {
        return Err(Error::protocol("Cloudflare unsuccessful response"));
    }
    if let Some(errors) = envelope.get("errors") {
        let errors = details(errors)?;
        if !errors.is_empty() {
            return Err(Error::protocol("Cloudflare unsuccessful response"));
        }
    }
    if let Some(messages) = envelope.get("messages") {
        details(messages)?;
    }
    let result = envelope
        .remove("result")
        .filter(Value::is_object)
        .ok_or_else(|| Error::protocol("Cloudflare result missing or invalid"))?;
    jev::decode(result, questions, NumericPolicy::Clef)
}

fn details(value: &Value) -> Result<&[Value], Error> {
    let values = value
        .as_array()
        .filter(|values| values.len() <= MAX_DETAILS)
        .ok_or_else(|| Error::protocol("Cloudflare detail array bounds"))?;
    for value in values {
        let detail = value
            .as_object()
            .ok_or_else(|| Error::protocol("Cloudflare detail object"))?;
        if let Some(code) = detail.get("code") {
            error_code(code).ok_or_else(|| Error::protocol("Cloudflare error code bounds"))?;
        }
        if let Some(message) = detail.get("message") {
            message
                .as_str()
                .filter(|text| text.len() <= 4096)
                .ok_or_else(|| Error::protocol("Cloudflare message bounds"))?;
        }
    }
    Ok(values)
}
fn error_code(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|code| u32::try_from(code).ok())
            .map(|code| code.to_string()),
        Value::String(code)
            if !code.is_empty() && code.len() <= 16 && code.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Some(code.clone())
        }
        _ => None,
    }
}

pub(super) fn error_codes(bytes: &[u8]) -> Vec<String> {
    let Ok(value) = json::parse(bytes, 4096, 256) else {
        return Vec::new();
    };
    let Some(errors) = value.get("errors") else {
        return Vec::new();
    };
    let Ok(errors) = details(errors) else {
        return Vec::new();
    };
    errors
        .iter()
        .filter_map(|detail| detail.get("code").and_then(error_code))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde_json::{Map, json};

    use super::*;
    use crate::{
        Answer, Distribution, Probability,
        backend::{Backend, ResponsePlan, distribution, score},
    };

    // Synthetic fixtures based on the published System One schema, not hosted captures.
    fn questions() -> Questions {
        Questions::builder()
            .predicate("p", "Condition?")
            .build()
            .unwrap()
    }
    fn inner() -> Value {
        json!({"model":"clef", "answers":{"p":{"type":"noul","noul":0.9}}, "usage":{"input_tokens":12,"output_tokens":0}})
    }
    fn decode_fixture(value: Value) -> Result<Decoded, Error> {
        Backend::Cloudflare.decode(
            value.to_string().as_bytes(),
            &ResponsePlan {
                questions: &questions(),
            },
        )
    }
    #[test]
    fn test_should_accept_documented_envelopes_without_inventing_metadata() {
        for envelope in [
            json!({"result":inner()}),
            json!({"result":inner(),"success":true,"errors":[],"messages":[],"extension":"allowed"}),
        ] {
            let decoded = decode_fixture(envelope).unwrap();
            assert_eq!(decoded.model, "clef");
            assert_eq!(decoded.usage.input_tokens, 12);
            assert_eq!(decoded.usage.output_tokens, 0);
            assert_eq!(decoded.usage.total_tokens, 12);
            assert_eq!(decoded.usage.reported_cost_usd, None);
            assert_eq!(decoded.id, None);
        }
    }
    #[rstest]
    #[case("success", json!(false))]
    #[case("success", Value::Null)]
    #[case("success", json!("true"))]
    #[case("errors", json!([{"code":3036}]))]
    #[case("errors", json!({}))]
    #[case("errors", Value::Null)]
    #[case("messages", json!(vec![json!({}); 33]))]
    #[case("messages", json!([{"code":-1}]))]
    #[case("messages", json!([{"code":"not-numeric"}]))]
    #[case("messages", json!([{"message":true}]))]
    #[case("messages", json!(["invalid"]))]
    #[case("result", Value::Null)]
    #[case("result", json!([]))]
    fn test_should_reject_invalid_cloudflare_envelope(#[case] field: &str, #[case] value: Value) {
        let mut envelope = json!({"result":inner()});
        envelope[field] = value;
        assert!(matches!(
            decode_fixture(envelope),
            Err(Error::Protocol { .. })
        ));
    }
    #[test]
    fn test_should_reject_bare_missing_duplicate_or_unbounded_responses() {
        assert!(decode_fixture(inner()).is_err());
        assert!(decode_fixture(json!({})).is_err());
        let plan = ResponsePlan {
            questions: &questions(),
        };
        for bytes in [
            b"<html>failure</html>".as_slice(),
            br#"{"result":{},"result":{}}"#,
        ] {
            assert!(Backend::Cloudflare.decode(bytes, &plan).is_err());
        }
        let deep = format!("{}0{}", "[".repeat(40), "]".repeat(40));
        assert!(Backend::Cloudflare.decode(deep.as_bytes(), &plan).is_err());
        assert!(decode_fixture(json!({"result":inner(), "extension":"x".repeat(4097)})).is_err());
        for field in ["model", "usage", "answers"] {
            let mut value = inner();
            value.as_object_mut().unwrap().remove(field);
            assert!(decode_fixture(json!({"result":value})).is_err());
        }
    }
    #[test]
    fn test_should_reject_invalid_answers_and_usage() {
        for answer in [
            json!({"type":"noul","noul":-0.1}),
            json!({"type":"noul","noul":1.1}),
            json!({"type":"choice","choice":"unknown"}),
            json!({"type":"noul","noul":"NaN"}),
        ] {
            let mut value = inner();
            value["answers"]["p"] = answer;
            assert!(decode_fixture(json!({"result":value})).is_err());
        }
        for usage in [
            json!({"input_tokens":1}),
            json!({"input_tokens":u64::MAX,"output_tokens":1}),
            json!({"input_tokens":1,"output_tokens":0,"cost":-0.1}),
        ] {
            let mut value = inner();
            value["usage"] = usage;
            assert!(decode_fixture(json!({"result":value})).is_err());
        }
        let mut value = inner();
        value["answers"]["extra"] = json!({"type":"noul","noul":0.5});
        assert!(decode_fixture(json!({"result":value})).is_err());
    }
    #[test]
    fn test_should_preserve_rounded_mass_only_for_clef() {
        let keys: Vec<_> = (0..64).map(|i| format!("o{i}")).collect();
        let schema = Questions::builder()
            .choice("q", "Which?", keys.iter().map(|key| (key, "Option")))
            .build()
            .unwrap();
        let probabilities: Map<_, _> = keys
            .iter()
            .map(|key| (key.clone(), json!(0.0156)))
            .collect();
        let value = json!({"model":"clef", "answers":{"q":{"type":"choice","choice":"o0","confidence":0.7,"probabilities":probabilities}}, "usage":{"input_tokens":10,"output_tokens":0}});
        let plan = ResponsePlan { questions: &schema };
        assert!(
            Backend::SystemOne
                .decode(value.to_string().as_bytes(), &plan)
                .is_err()
        );
        for backend in [Backend::Cloudflare, Backend::ClefSystemOne] {
            let bytes = if matches!(backend, Backend::Cloudflare) {
                json!({"result":value}).to_string()
            } else {
                value.to_string()
            };
            let result = backend.decode(bytes.as_bytes(), &plan).unwrap();
            let Some(Answer::Choice(answer)) = result.answers.get("q") else {
                panic!("choice required");
            };
            let Distribution::Reported(probs) = answer.probabilities() else {
                panic!("distribution required");
            };
            assert!((probs.values().map(|p| p.get()).sum::<f64>() - 0.9984).abs() < 1e-12);
            assert_eq!(answer.confidence().unwrap().get(), 0.7);
        }
    }
    #[test]
    fn test_should_bound_mass_and_score_rounding_without_repair() {
        let keys = vec!["0".to_owned(), "1".to_owned()];
        let entries = |p| {
            Some(vec![
                ("0".to_owned(), Probability::try_from(0.5).unwrap()),
                ("1".to_owned(), Probability::try_from(p).unwrap()),
            ])
        };
        assert!(distribution("q", &keys, entries(0.5001), NumericPolicy::Clef).is_ok());
        assert!(distribution("q", &keys, entries(0.50011), NumericPolicy::Clef).is_err());
        assert!(
            score(
                "q",
                0.5001,
                keys.clone(),
                None,
                entries(0.5),
                NumericPolicy::Clef
            )
            .is_ok()
        );
        assert!(score("q", 0.50011, keys, None, entries(0.5), NumericPolicy::Clef).is_err());
    }
    #[rstest]
    #[case(401, json!({"errors":[{"code":3036}]}), "authentication", false)]
    #[case(403, json!({"errors":[]}), "permission", false)]
    #[case(402, json!({}), "quota", false)]
    #[case(429, json!({"errors":[{"code":3040},{"code":"003036"}]}), "quota", false)]
    #[case(429, json!({"errors":[{"code":"3040"}]}), "rate", true)]
    #[case(429, json!({"errors":[{"code":-1,"message":"quota"}]}), "rate", true)]
    #[case(400, json!({"errors":[{"code":5007}]}), "input", false)]
    #[case(413, json!({}), "input", false)]
    #[case(422, json!({}), "input", false)]
    #[case(404, json!({"errors":[{"code":3042}]}), "api", false)]
    #[case(408, json!({"errors":[{"code":3007}]}), "api", false)]
    #[case(503, json!({}), "api", true)]
    fn test_should_classify_cloudflare_errors(
        #[case] status: u16,
        #[case] value: Value,
        #[case] kind: &str,
        #[case] retryable: bool,
    ) {
        let error = Backend::Cloudflare.http_error(
            status,
            value.to_string().as_bytes(),
            Some(2),
            Some("ray".to_owned()),
        );
        let actual = match &error {
            Error::Authentication => "authentication",
            Error::PermissionDenied => "permission",
            Error::QuotaExceeded => "quota",
            Error::RateLimited {
                retry_after_secs: Some(2),
            } => "rate",
            Error::InvalidInput(_) => "input",
            Error::Api { .. } => "api",
            _ => "unexpected",
        };
        assert_eq!(actual, kind);
        assert_eq!(error.is_retryable(), retryable);
        assert!(!format!("{error:?}").contains("message"));
    }
    #[test]
    fn test_should_bound_error_details_and_retain_only_safe_codes() {
        for code in [
            json!(-1),
            json!(u64::MAX),
            json!(1.2),
            json!(""),
            json!("12345678901234567"),
            json!("3036\n"),
        ] {
            assert!(
                error_codes(json!({"errors":[{"code":code}]}).to_string().as_bytes()).is_empty()
            );
        }
        assert!(
            error_codes(
                json!({"errors": vec![json!({"code":3036});33]})
                    .to_string()
                    .as_bytes()
            )
            .is_empty()
        );
        assert!(matches!(
            Backend::Cloudflare.http_error(429, b"<html>", None, None),
            Error::RateLimited { .. }
        ));
        let error = Backend::Cloudflare.http_error(
            404,
            br#"{"errors":[{"code":3042,"message":"secret input"}]}"#,
            None,
            Some("ray".into()),
        );
        assert!(
            matches!(&error, Error::Api { code: Some(code), request_id: Some(id), .. } if code == "3042" && id == "ray")
        );
        assert!(!format!("{error:?}").contains("secret input"));
    }
    proptest! {
        #[test]
        fn test_should_accept_reference_rounded_distributions(weights in prop::collection::vec(1_u32..100_000, 2..65)) {
            let total: f64 = weights.iter().map(|w| f64::from(*w)).sum();
            let keys: Vec<_> = (0..weights.len()).map(|i| i.to_string()).collect();
            let rounded: Vec<_> = weights.iter().map(|w| (f64::from(*w) / total * 10000.0).round() / 10000.0).collect();
            let entries: Vec<_> = keys.iter().zip(&rounded).map(|(key, p)| (key.clone(), Probability::try_from(*p).unwrap())).collect();
            let result = distribution("q", &keys, Some(entries.clone()), NumericPolicy::Clef).unwrap();
            for (key, p) in &entries { prop_assert_eq!(result.get(key), Some(*p)); }
            if weights.len() <= 32 {
                let raw = weights.iter().enumerate().map(|(i,w)| f64::from(u32::try_from(i).unwrap()) * f64::from(*w) / total).sum::<f64>();
                let raw = (raw * 10000.0).round() / 10000.0;
                prop_assert!(score("q", raw, keys, None, Some(entries), NumericPolicy::Clef).is_ok());
            }
        }
    }
}
