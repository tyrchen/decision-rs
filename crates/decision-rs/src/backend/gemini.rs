//! Gemini Decisions codec: System One questions and answers under a Google envelope.
//!
//! The response carries no model or response identifier, reports usage with
//! `total_*` counters, and signals input-filter blocks either as HTTP 400 codes or
//! as `prompt_feedback.block_reason`. Google error bodies use numeric `code`,
//! symbolic `status` and typed `details`; invalid API keys arrive as HTTP 400.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Decoded, ErrorSummary, NumericPolicy, bounded_identifier, jev};
use crate::{Error, Input, Questions, Usage as DomainUsage, json};

const MAX_DETAILS: usize = 32;
const BLOCK_CODES: [&str; 5] = [
    "safety",
    "prohibited_content",
    "blocklist",
    "image_safety",
    "content_blocked",
];

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    input: &'a str,
    questions: IndexMap<&'a str, jev::Question<'a, Criteria<'a>>>,
}
#[derive(Serialize)]
struct Criteria<'a> {
    true_description: &'a str,
    false_description: &'a str,
}
pub(super) fn prepare(
    input: &Input,
    schema: &Questions,
    model: &str,
    cap: usize,
) -> Result<Vec<u8>, Error> {
    json::encode(
        &Request {
            model,
            input: input.as_str(),
            questions: jev::questions(schema, |yes, no| Criteria {
                true_description: yes,
                false_description: no,
            }),
        },
        cap,
    )
    .map_err(|_| Error::InvalidInput("request body limit"))
}
#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    answers: IndexMap<String, Value>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    prompt_feedback: Option<PromptFeedback>,
}
#[derive(Deserialize)]
struct PromptFeedback {
    #[serde(default)]
    block_reason: Option<String>,
}
#[derive(Deserialize)]
struct Usage {
    #[serde(rename = "total_input_tokens")]
    input: u64,
    // Gemini Decision generates no tokens; the guide documents output as always 0.
    #[serde(default, rename = "total_output_tokens")]
    output: u64,
    #[serde(default, rename = "total_tokens")]
    total: Option<u64>,
    #[serde(default, rename = "total_cached_tokens")]
    cached: Option<u64>,
    #[serde(default, rename = "total_thought_tokens")]
    thought: Option<u64>,
}
impl Usage {
    fn validate(self) -> Result<DomainUsage, Error> {
        let computed = self
            .input
            .checked_add(self.output)
            .ok_or_else(|| Error::protocol("token count overflow"))?;
        Ok(DomainUsage {
            input_tokens: self.input,
            output_tokens: self.output,
            total_tokens: self.total.unwrap_or(computed),
            cached_tokens: self.cached,
            cache_write_tokens: None,
            reasoning_tokens: self.thought,
            reported_cost_usd: None,
        })
    }
}
pub(super) fn decode(value: Value, questions: &Questions) -> Result<Decoded, Error> {
    let response: Response =
        serde_json::from_value(value).map_err(|_| Error::protocol("Gemini envelope"))?;
    if response
        .prompt_feedback
        .is_some_and(|feedback| feedback.block_reason.is_some())
    {
        return Err(Error::InvalidInput("upstream content filter blocked input"));
    }
    let usage = response
        .usage
        .ok_or_else(|| Error::protocol("Gemini usage missing"))?;
    Ok(Decoded {
        answers: jev::answers(response.answers, questions, NumericPolicy::Gemini)?,
        model: None,
        id: None,
        usage: usage.validate()?,
    })
}

pub(super) fn error_summary(bytes: &[u8]) -> ErrorSummary {
    let Ok(value) = json::parse(bytes, 4096, 256) else {
        return ErrorSummary::default();
    };
    let Some(error) = value.get("error").and_then(Value::as_object) else {
        return ErrorSummary::default();
    };
    // Filter blocks use a string `code`; ordinary Google errors use a numeric one.
    let filter_code = error.get("code").and_then(Value::as_str);
    let details = error
        .get("details")
        .and_then(Value::as_array)
        .filter(|details| details.len() <= MAX_DETAILS)
        .map_or(&[][..], Vec::as_slice);
    let invalid_key = typed(details, "ErrorInfo")
        .any(|info| info.get("reason").and_then(Value::as_str) == Some("API_KEY_INVALID"));
    let quota = typed(details, "QuotaFailure")
        .filter_map(|failure| failure.get("violations")?.as_array())
        .filter(|violations| violations.len() <= MAX_DETAILS)
        .flatten()
        .filter_map(|violation| violation.get("quotaId")?.as_str())
        .any(|id| id.len() <= 256 && id.contains("PerDay"));
    let retry_after_secs = typed(details, "RetryInfo")
        .find_map(|info| info.get("retryDelay")?.as_str().and_then(retry_delay));
    ErrorSummary {
        code: filter_code
            .or_else(|| error.get("status").and_then(Value::as_str))
            .and_then(|code| bounded_identifier(code, 128).ok()),
        quota,
        invalid_key,
        blocked: filter_code.is_some_and(|code| BLOCK_CODES.contains(&code)),
        retry_after_secs,
    }
}
fn typed<'a>(details: &'a [Value], kind: &'a str) -> impl Iterator<Item = &'a Map<String, Value>> {
    details
        .iter()
        .filter_map(Value::as_object)
        .filter(move |detail| {
            detail
                .get("@type")
                .and_then(Value::as_str)
                .and_then(|name| name.strip_prefix("type.googleapis.com/google.rpc."))
                == Some(kind)
        })
}
/// Protobuf JSON duration such as `43s` or `1.5s`, rounded up to whole seconds.
fn retry_delay(text: &str) -> Option<u64> {
    let digits = text
        .strip_suffix('s')
        .filter(|d| (1..=16).contains(&d.len()))?;
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let secs = whole.parse::<u64>().ok()?;
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(secs.saturating_add(u64::from(fraction.bytes().any(|b| b != b'0'))))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::{
        Answer, Distribution,
        backend::{Backend, ResponsePlan},
    };

    // Response fixtures are copied from the Decisions API guide (2026-11-11).
    fn triage() -> Questions {
        Questions::builder()
            .predicate("is_defect", "Does `message` describe a hardware defect?")
            .choice(
                "next_action",
                "What is the right resolution given `tier` and `order`?",
                [
                    ("instant_refund", "Refund without return."),
                    ("replacement_shipment", "Ship a replacement."),
                    ("troubleshooting_flow", "The issue may be fixable."),
                    ("human_escalation", "Escalate to a person."),
                ],
            )
            .score(
                "churn_risk",
                "How likely is the customer to churn?",
                [
                    ("none", "No churn signal"),
                    ("mild", "Mild frustration"),
                    ("alternatives", "Mentions alternatives"),
                    ("cancel", "Threatens to cancel"),
                ],
            )
            .build()
            .unwrap()
    }
    fn documented_triage() -> Value {
        json!({
          "answers": {
            "is_defect": { "type": "noul", "noul": 0.94, "confidence": 0.94 },
            "next_action": {
              "type": "choice",
              "choice": "human_escalation",
              "probabilities": {
                "human_escalation": 0.76,
                "replacement_shipment": 0.18,
                "troubleshooting_flow": 0.04,
                "instant_refund": 0.02
              },
              "confidence": 0.76
            },
            "churn_risk": {
              "type": "score",
              "score": 2.99,
              "legend": {
                "0": "No churn signal",
                "1": "Mild frustration",
                "2": "Mentions alternatives",
                "3": "Threatens to cancel"
              },
              "probabilities": { "0": 0.0, "1": 0.0, "2": 0.01, "3": 0.99 },
              "confidence": 0.99
            }
          },
          "usage": {
            "total_input_tokens": 324,
            "input_tokens_by_modality": [{ "modality": "text", "tokens": 324 }],
            "total_output_tokens": 0,
            "total_tokens": 324
          }
        })
    }
    fn decode_with(
        backend: Backend,
        questions: &Questions,
        value: &Value,
    ) -> Result<Decoded, Error> {
        backend.decode(value.to_string().as_bytes(), &ResponsePlan { questions })
    }
    #[test]
    fn test_should_compile_documented_request_shape() {
        let schema = Questions::builder()
            .predicate("is_damaged", "Did the item arrive damaged?")
            .predicate_with(
                "is_defect",
                "Is the customer reporting a product defect?",
                "The product itself is broken or faulty.",
                "Preference, sizing, shipping, or pricing complaint.",
            )
            .choice(
                "department",
                "Which department?",
                [("technical", "Problems."), ("billing", "Payments.")],
            )
            .score(
                "severity",
                "How severe?",
                [("low", "Cosmetic."), ("high", "Blocked.")],
            )
            .build()
            .unwrap();
        let input = Input::try_from("ticket").unwrap();
        let body = prepare(&input, &schema, "gemini-decision-1", 1 << 20).unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            json!({"model":"gemini-decision-1","input":"ticket","questions":{
                "is_damaged":{"type":"noul","instructions":"Did the item arrive damaged?"},
                "is_defect":{"type":"noul","instructions":"Is the customer reporting a product defect?",
                    "criteria":{"true_description":"The product itself is broken or faulty.",
                        "false_description":"Preference, sizing, shipping, or pricing complaint."}},
                "department":{"type":"choice","instructions":"Which department?",
                    "criteria":{"technical":"Problems.","billing":"Payments."}},
                "severity":{"type":"score","instructions":"How severe?","criteria":["Cosmetic.","Blocked."]}
            }})
        );
        let order: Vec<_> = value["questions"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(order, ["is_damaged", "is_defect", "department", "severity"]);
        assert!(matches!(
            prepare(&input, &schema, "gemini-decision-1", 64),
            Err(Error::InvalidInput("request body limit"))
        ));
    }
    #[test]
    fn test_should_decode_documented_multi_question_response() {
        let decoded = decode_with(Backend::Gemini, &triage(), &documented_triage()).unwrap();
        assert_eq!(decoded.model, None);
        assert_eq!(decoded.id, None);
        assert_eq!(decoded.usage.input_tokens, 324);
        assert_eq!(decoded.usage.output_tokens, 0);
        assert_eq!(decoded.usage.total_tokens, 324);
        assert_eq!(decoded.usage.reported_cost_usd, None);
        let keys: Vec<_> = decoded.answers.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["is_defect", "next_action", "churn_risk"]);
        let Some(Answer::Predicate(defect)) = decoded.answers.get("is_defect") else {
            panic!("predicate required");
        };
        assert_eq!(defect.probability().get(), 0.94);
        let Some(Answer::Choice(action)) = decoded.answers.get("next_action") else {
            panic!("choice required");
        };
        assert_eq!(action.choice(), "human_escalation");
        assert_eq!(action.confidence().unwrap().get(), 0.76);
        let Distribution::Reported(probs) = action.probabilities() else {
            panic!("distribution required");
        };
        // Reordered to schema order regardless of the server's alphabetical sort.
        assert_eq!(
            probs.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "instant_refund",
                "replacement_shipment",
                "troubleshooting_flow",
                "human_escalation"
            ]
        );
        let Some(Answer::Score(churn)) = decoded.answers.get("churn_risk") else {
            panic!("score required");
        };
        assert_eq!(churn.score(), 2.99);
        assert_eq!(churn.confidence().unwrap().get(), 0.99);
    }
    #[test]
    fn test_should_accept_documented_rounding_only_for_gemini() {
        // Guide example: mass 1.01 and expected value 1.13 reported as 1.12.
        let schema = Questions::builder()
            .score(
                "severity",
                "How severe?",
                [
                    ("cosmetic", "Cosmetic."),
                    ("workaround", "Workaround."),
                    ("blocked", "Blocked."),
                ],
            )
            .build()
            .unwrap();
        let answer = json!({"type":"score","score":1.12,
            "legend":{"0":"Cosmetic.","1":"Workaround.","2":"Blocked."},
            "probabilities":{"0":0.03,"1":0.83,"2":0.15},"confidence":0.83});
        let gemini = json!({"answers":{"severity":answer},"usage":{"total_input_tokens":9}});
        let decoded = decode_with(Backend::Gemini, &schema, &gemini).unwrap();
        assert_eq!(decoded.usage.total_tokens, 9);
        let jev = json!({"model":"m","answers":{"severity":answer},"usage":{"input_tokens":9,"output_tokens":0}});
        assert!(decode_with(Backend::SystemOne, &schema, &jev).is_err());
        let mut gross = gemini.clone();
        gross["answers"]["severity"]["probabilities"]["2"] = json!(0.05);
        assert!(decode_with(Backend::Gemini, &schema, &gross).is_err());
        let mut drift = gemini;
        drift["answers"]["severity"]["score"] = json!(1.2);
        assert!(decode_with(Backend::Gemini, &schema, &drift).is_err());
    }
    #[test]
    fn test_should_map_usage_details_without_inventing_cost() {
        let mut value = documented_triage();
        value["usage"] = json!({"total_input_tokens":10,"total_cached_tokens":4,
            "total_thought_tokens":0,"grounding_tool_count":[]});
        let usage = decode_with(Backend::Gemini, &triage(), &value)
            .unwrap()
            .usage;
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (10, 0, 10)
        );
        assert_eq!(usage.cached_tokens, Some(4));
        assert_eq!(usage.reasoning_tokens, Some(0));
        assert_eq!(usage.cache_write_tokens, None);
    }
    #[rstest]
    #[case(json!({"prompt_feedback":{"block_reason":"safety"},"answers":{}}))]
    #[case(json!({"prompt_feedback":{"block_reason":"other","block_reason_message":"x"},"usage":{"total_input_tokens":1}}))]
    fn test_should_report_prompt_feedback_blocks_as_rejected_input(#[case] value: Value) {
        assert!(matches!(
            decode_with(Backend::Gemini, &triage(), &value),
            Err(Error::InvalidInput("upstream content filter blocked input"))
        ));
    }
    #[test]
    fn test_should_reject_invalid_envelopes_answers_and_usage() {
        let mut cases = Vec::new();
        let mut missing_usage = documented_triage();
        missing_usage.as_object_mut().unwrap().remove("usage");
        cases.push(missing_usage);
        let mut missing_answer = documented_triage();
        missing_answer["answers"]
            .as_object_mut()
            .unwrap()
            .remove("is_defect");
        cases.push(missing_answer);
        let mut extra = documented_triage();
        extra["answers"]["extra"] = json!({"type":"noul","noul":0.5,"confidence":0.5});
        cases.push(extra);
        for (field, replacement) in [
            (
                "is_defect",
                json!({"type":"noul","noul":1.5,"confidence":1.0}),
            ),
            (
                "is_defect",
                json!({"type":"choice","choice":"x","probabilities":{},"confidence":1.0}),
            ),
            (
                "next_action",
                json!({"type":"choice","choice":"unknown","probabilities":{"unknown":1.0},"confidence":1.0}),
            ),
        ] {
            let mut value = documented_triage();
            value["answers"][field] = replacement;
            cases.push(value);
        }
        let mut legend = documented_triage();
        legend["answers"]["churn_risk"]["legend"] = json!({"0":"a"});
        cases.push(legend);
        let mut usage = documented_triage();
        usage["usage"] = json!({"total_input_tokens":u64::MAX,"total_output_tokens":1});
        cases.push(usage);
        cases.push(json!({"answers":[],"usage":{"total_input_tokens":1}}));
        for value in cases {
            assert!(
                matches!(
                    decode_with(Backend::Gemini, &triage(), &value),
                    Err(Error::Protocol { .. })
                ),
                "{value}"
            );
        }
        let plan = ResponsePlan {
            questions: &triage(),
        };
        for bytes in [b"<html>".as_slice(), br#"{"answers":{},"answers":{}}"#] {
            assert!(Backend::Gemini.decode(bytes, &plan).is_err());
        }
    }
    // Captured from POST /v1beta/decisions with a fake key on 2026-11-11.
    const INVALID_KEY: &str = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com","metadata":{"service":"generativelanguage.googleapis.com"}},{"@type":"type.googleapis.com/google.rpc.LocalizedMessage","locale":"en-US","message":"API key not valid. Please pass a valid API key."}]}}"#;
    #[rstest]
    #[case(400, INVALID_KEY, "authentication", false)]
    #[case(
        401,
        r#"{"error":{"code":401,"status":"UNAUTHENTICATED"}}"#,
        "authentication",
        false
    )]
    #[case(
        403,
        r#"{"error":{"code":403,"status":"PERMISSION_DENIED"}}"#,
        "permission",
        false
    )]
    #[case(
        400,
        r#"{"error":{"code":"safety","message":"secret input"}}"#,
        "blocked",
        false
    )]
    #[case(400, r#"{"error":{"code":"content_blocked"}}"#, "blocked", false)]
    #[case(
        400,
        r#"{"error":{"code":400,"status":"INVALID_ARGUMENT"}}"#,
        "input",
        false
    )]
    #[case(429, r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"42.5s"}]}}"#, "rate", true)]
    #[case(429, r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]}]}}"#, "quota", false)]
    #[case(429, r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerMinutePerProjectPerModel"}]}]}}"#, "rate", true)]
    // Captured with OAuth ADC on a prepaid project without credits, 2026-11-11.
    #[case(
        402,
        r#"{"error":{"code":402,"message":"Your prepayment credits are depleted.","status":"RESOURCE_EXHAUSTED"}}"#,
        "quota",
        false
    )]
    #[case(
        503,
        r#"{"error":{"code":503,"status":"UNAVAILABLE"}}"#,
        "unavailable",
        true
    )]
    #[case(500, "<html>", "api", true)]
    fn test_should_classify_google_errors(
        #[case] status: u16,
        #[case] body: &str,
        #[case] kind: &str,
        #[case] retryable: bool,
    ) {
        let error = Backend::Gemini.http_error(status, body.as_bytes(), None, None);
        let actual = match &error {
            Error::Authentication => "authentication",
            Error::PermissionDenied => "permission",
            Error::QuotaExceeded => "quota",
            Error::RateLimited { .. } => "rate",
            Error::InvalidInput("upstream content filter blocked input") => "blocked",
            Error::InvalidInput("upstream rejected request") => "input",
            Error::Api {
                code: Some(code), ..
            } if code == "UNAVAILABLE" => "unavailable",
            Error::Api { code: None, .. } => "api",
            _ => "unexpected",
        };
        assert_eq!(actual, kind);
        assert_eq!(error.is_retryable(), retryable);
        assert!(!format!("{error:?} {error}").contains("secret input"));
        assert!(!format!("{error:?}").contains("API key not valid"));
    }
    #[test]
    fn test_should_prefer_retry_after_header_over_body_retry_info() {
        let body = br#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"42.5s"}]}}"#;
        assert!(matches!(
            Backend::Gemini.http_error(429, body, None, None),
            Error::RateLimited {
                retry_after_secs: Some(43)
            }
        ));
        assert!(matches!(
            Backend::Gemini.http_error(429, body, Some(2), None),
            Error::RateLimited {
                retry_after_secs: Some(2)
            }
        ));
        // Details are only trusted under their documented Google type.
        let untyped =
            br#"{"error":{"code":429,"details":[{"retryDelay":"9s","reason":"API_KEY_INVALID"}]}}"#;
        assert!(matches!(
            Backend::Gemini.http_error(429, untyped, None, None),
            Error::RateLimited {
                retry_after_secs: None
            }
        ));
    }
    #[rstest]
    #[case("43s", Some(43))]
    #[case("0s", Some(0))]
    #[case("1.000s", Some(1))]
    #[case("1.001s", Some(2))]
    #[case("s", None)]
    #[case(".5s", None)]
    #[case("1.2.3s", None)]
    #[case("-1s", None)]
    #[case("43", None)]
    #[case("12345678901234567s", None)]
    fn test_should_parse_bounded_protobuf_durations(
        #[case] text: &str,
        #[case] expected: Option<u64>,
    ) {
        assert_eq!(retry_delay(text), expected);
    }
}
