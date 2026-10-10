//! Real HTTP contract tests against local endpoints; no external requests.

use std::time::Duration;

use jev::{
    Auth, Choice, ChoiceOptions, Client, Decision, Endpoint, Error, HttpClient, Input, Model,
    Outcome, Predicate, Probability, Provider, Questions, RequestOptions, RetryConfig, Score,
    ScoreLevels, Settings, Verdict,
};
use rstest::rstest;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Payments and refunds.
    Billing,
    /// Technical failures.
    Technical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait.
    Low,
    /// Handle this week.
    Medium,
    /// Immediate help.
    High,
}
#[derive(Debug, Decision)]
struct Triage {
    /// Is this a defect?
    defect: Predicate,
    /// Is a refund requested?
    #[jev(yes = "Money back requested.", no = "No money back requested.")]
    refund: Predicate,
    /// Which team?
    team: Choice<Team>,
    /// How urgent?
    urgency: Score<Urgency>,
}
#[derive(Debug, Decision)]
struct Refusable {
    /// Required condition one.
    first: Predicate,
    /// Required condition two.
    second: Predicate,
    /// Explicitly refusable condition.
    third: Outcome<Predicate>,
}
fn p(value: f64) -> Probability {
    Probability::try_from(value).unwrap()
}
fn settings() -> Settings {
    let mut s = Settings::default();
    s.retry = RetryConfig::disabled();
    s
}
fn client(server: &MockServer, openai: bool, s: Settings) -> Client {
    let url = format!("{}/decisions", server.uri());
    let endpoint = if openai {
        Endpoint::openai_decisions(&url)
    } else {
        Endpoint::system_one(&url)
    }
    .unwrap();
    Client::for_endpoint_with(
        endpoint,
        Auth::ApiKey("fixture-key".into()),
        "fixture-model".parse().unwrap(),
        HttpClient::new().unwrap(),
        s,
    )
    .unwrap()
}
fn envelope(_openai: bool, answers: Value) -> Value {
    json!({"model":"returned-model", "id":"response-123", "answers":answers,
        "usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15,"cost":0.01}})
}
fn triage_body(openai: bool) -> Value {
    let answers = if openai {
        json!([
            {"name":"urgency","type":"score","score":0.8,"confidence":0.9,"probabilities":[
                {"value":2,"label":"high","probability":0.25}, {"value":0,"label":"low","probability":0.45}, {"value":1,"label":"medium","probability":0.3}]},
            {"name":"team","type":"choice","choice":"billing","confidence":0.95,"probabilities":[{"value":"technical","probability":0.4},{"value":"billing","probability":0.6}]},
            {"name":"refund","type":"choice","choice":false,"confidence":0.99,"probabilities":[{"value":false,"probability":0.7},{"value":true,"probability":0.3}]},
            {"name":"defect","type":"predicate","probability":0.9}
        ])
    } else {
        json!({
            "defect":{"type":"noul","noul":0.9}, "refund":{"type":"noul","noul":0.3},
            "team":{"type":"choice","choice":"billing","confidence":0.95,"probabilities":{"billing":0.6,"technical":0.4}},
            "urgency":{"type":"score","score":0.8,"confidence":0.9,"probabilities":{"0":0.45,"1":0.3,"2":0.25},"legend":{"0":"Can wait.","1":"Handle this week.","2":"Immediate help."}}
        })
    };
    envelope(openai, answers)
}
async fn mount(server: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path("/decisions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(body)
                .insert_header("x-request-id", "request-456"),
        )
        .mount(server)
        .await;
}
#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_should_preserve_typed_semantics_across_protocols(#[case] openai: bool) {
    let server = MockServer::start().await;
    mount(&server, triage_body(openai)).await;
    let client = client(&server, openai, settings());
    let response = client
        .decide_with::<Triage>(RequestOptions::new("ticket").unwrap())
        .await
        .unwrap();
    assert!(response.value.defect.is_yes(p(0.8)));
    assert_eq!(response.value.refund.probability(), p(0.3)); // P(true), not chosen confidence.
    assert_eq!(
        response.value.refund.decide(p(0.2), p(0.2)),
        Verdict::Unsure
    );
    assert_eq!(response.value.team.value(), &Team::Billing);
    assert!(response.value.team.accept(p(0.8)).is_none());
    assert_eq!(
        response.value.team.accept_confidence(p(0.8)),
        Some(&Team::Billing)
    );
    assert_eq!(response.value.team.sample(p(0.9)).unwrap(), Team::Technical);
    assert_eq!(response.value.urgency.raw(), 0.8);
    assert_eq!(response.value.urgency.nearest().unwrap(), Urgency::Medium); // Mode is Low.
    assert!(!response.value.urgency.at_least(&Urgency::High).unwrap());
    assert_eq!(response.meta.request_id.as_deref(), Some("request-456"));
    assert_eq!(response.meta.response_id.as_deref(), Some("response-123"));
    assert_eq!(response.meta.usage.total_tokens, 15);
    assert_eq!(response.meta.attempts, 1);
    assert_eq!(response.meta.wire_model, "fixture-model");
    assert_eq!(response.meta.returned_model, "returned-model");
    let dynamic = client
        .ask("ticket", Triage::questions().unwrap())
        .await
        .unwrap();
    assert_eq!(
        dynamic
            .answers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>(),
        ["defect", "refund", "team", "urgency"]
    );
}
#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_should_compile_exact_golden_requests(#[case] openai: bool) {
    let server = MockServer::start().await;
    let expected = if openai {
        json!({"model":"fixture-model","input":"{\"a\":1,\"z\":2}","questions":[
            {"name":"defect","type":"predicate","instructions":"Is this a defect?"},
            {"name":"refund","type":"choice","instructions":"Is a refund requested?","choices":[{"value":true,"description":"Money back requested."},{"value":false,"description":"No money back requested."}]},
            {"name":"team","type":"choice","instructions":"Which team?","choices":[{"value":"billing","description":"Payments and refunds."},{"value":"technical","description":"Technical failures."}]},
            {"name":"urgency","type":"score","instructions":"How urgent?","levels":[{"label":"low","description":"Can wait."},{"label":"medium","description":"Handle this week."},{"label":"high","description":"Immediate help."}]}
        ]})
    } else {
        json!({"model":"fixture-model","state":"{\"a\":1,\"z\":2}","questions":{
            "defect":{"type":"noul","instructions":"Is this a defect?"},
            "refund":{"type":"noul","instructions":"Is a refund requested?","criteria":{"true":"Money back requested.","false":"No money back requested."}},
            "team":{"type":"choice","instructions":"Which team?","criteria":{"billing":"Payments and refunds.","technical":"Technical failures."}},
            "urgency":{"type":"score","instructions":"How urgent?","criteria":["Can wait.","Handle this week.","Immediate help."]}
        }})
    };
    Mock::given(method("POST"))
        .and(body_json(expected))
        .and(header("authorization", "Bearer fixture-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(triage_body(openai)))
        .expect(1)
        .mount(&server)
        .await;
    client(&server, openai, settings())
        .decide::<Triage>(Input::json(&json!({"z":2,"a":1})).unwrap())
        .await
        .unwrap();
}
#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_should_collect_required_refusals_and_preserve_outcomes(#[case] openai: bool) {
    let server = MockServer::start().await;
    let answers = if openai {
        json!([{"name":"first","type":"refusal"},{"name":"second","type":"refusal"},{"name":"third","type":"refusal"}])
    } else {
        json!({"first":{"type":"refusal"},"second":{"type":"refusal"},"third":{"type":"refusal"}})
    };
    mount(&server, envelope(openai, answers)).await;
    let client = client(&server, openai, settings());
    let response = client
        .ask("ticket", Refusable::questions().unwrap())
        .await
        .unwrap();
    assert!(
        matches!(response.answers.decode::<Outcome<Predicate>>("third").unwrap(), Outcome::Refused(r) if r.key() == "third")
    );
    let error = client.decide::<Refusable>("ticket").await.unwrap_err();
    assert!(
        matches!(error, Error::Refused { keys, meta } if keys == ["first", "second"] && meta.usage.input_tokens == 12)
    );
}
#[rstest]
#[case("name_null")]
#[case("name_missing")]
#[case("name_unknown")]
#[case("name_duplicate")]
#[case("answer_missing")]
#[case("wrong_type")]
#[case("probability_range")]
#[case("boolean_string")]
#[case("distribution_empty")]
#[case("distribution_sum")]
#[case("distribution_unknown")]
#[case("distribution_duplicate")]
#[case("confidence_missing")]
#[case("score_label")]
#[case("score_index")]
#[case("score_mean")]
#[case("score_range")]
#[case("negative_usage")]
#[case("token_overflow")]
#[case("model_length")]
#[tokio::test]
async fn test_should_reject_invalid_openai_responses(#[case] corruption: &str) {
    let server = MockServer::start().await;
    let mut body = triage_body(true);
    match corruption {
        "name_null" => body["answers"][3]["name"] = Value::Null,
        "name_missing" => {
            body["answers"][3].as_object_mut().unwrap().remove("name");
        }
        "name_unknown" => body["answers"][3]["name"] = json!("unknown"),
        "name_duplicate" => body["answers"][3]["name"] = json!("team"),
        "answer_missing" => {
            body["answers"].as_array_mut().unwrap().pop();
        }
        "wrong_type" => body["answers"][3]["type"] = json!("future_kind"),
        "probability_range" => body["answers"][3]["probability"] = json!(1.1),
        "boolean_string" => body["answers"][2]["probabilities"][0]["value"] = json!("false"),
        "distribution_empty" => body["answers"][1]["probabilities"] = json!([]),
        "distribution_sum" => body["answers"][1]["probabilities"][0]["probability"] = json!(0.1),
        "distribution_unknown" => body["answers"][1]["probabilities"][0]["value"] = json!("other"),
        "distribution_duplicate" => {
            body["answers"][1]["probabilities"][0]["value"] = json!("billing")
        }
        "confidence_missing" => {
            body["answers"][1]
                .as_object_mut()
                .unwrap()
                .remove("confidence");
        }
        "score_label" => body["answers"][0]["probabilities"][0]["label"] = json!("wrong"),
        "score_index" => body["answers"][0]["probabilities"][0]["value"] = json!(9),
        "score_mean" => body["answers"][0]["score"] = json!(1.9),
        "score_range" => body["answers"][0]["score"] = json!(3),
        "negative_usage" => body["usage"]["input_tokens"] = json!(-1),
        "token_overflow" => {
            body["usage"]["input_tokens"] = json!(u64::MAX);
            body["usage"]["output_tokens"] = json!(1);
        }
        "model_length" => body["model"] = json!("x".repeat(257)),
        _ => panic!("unknown test corruption"),
    }
    mount(&server, body).await;
    assert!(matches!(
        client(&server, true, settings())
            .decide::<Triage>("ticket")
            .await,
        Err(Error::Protocol { .. })
    ));
}
#[tokio::test]
async fn test_should_preserve_unavailable_jev_statistics() {
    let server = MockServer::start().await;
    let mut body = triage_body(false);
    body["answers"]["team"] = json!({"type":"choice","choice":"billing"});
    body["answers"]["urgency"] = json!({"type":"score","score":1.5});
    mount(&server, body).await;
    let value = client(&server, false, settings())
        .decide::<Triage>("ticket")
        .await
        .unwrap();
    assert!(value.team.confidence().is_none());
    assert!(value.team.probability_of(&Team::Billing).is_none());
    assert!(matches!(
        value.team.sample(p(0.5)),
        Err(Error::StatisticsUnavailable)
    ));
    assert_eq!(value.urgency.nearest().unwrap(), Urgency::High);
}
#[rstest]
#[case("empty")]
#[case("incomplete")]
#[case("unknown")]
#[case("legend")]
#[case("kind")]
#[tokio::test]
async fn test_should_reject_invalid_jev_statistics(#[case] corruption: &str) {
    let server = MockServer::start().await;
    let mut body = triage_body(false);
    match corruption {
        "empty" => body["answers"]["team"]["probabilities"] = json!({}),
        "incomplete" => body["answers"]["team"]["probabilities"] = json!({"billing":1.0}),
        "unknown" => body["answers"]["team"]["choice"] = json!("unknown"),
        "legend" => body["answers"]["urgency"]["legend"] = json!({"4":"bad"}),
        "kind" => body["answers"]["defect"] = json!({"type":"choice","choice":"billing"}),
        _ => panic!("unknown test corruption"),
    }
    mount(&server, body).await;
    assert!(matches!(
        client(&server, false, settings())
            .decide::<Triage>("ticket")
            .await,
        Err(Error::Protocol { .. })
    ));
}
#[rstest]
#[case("insufficient_quota")]
#[case("credit_balance_exhausted")]
#[case("organization_spend_limit_exceeded")]
#[case("project_spend_limit_exceeded")]
#[case("organization_usage_limit_exceeded")]
#[tokio::test]
async fn test_should_not_retry_exhausted_quota(#[case] code: &str) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({"error":{"code":code}})))
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.retry = RetryConfig::default();
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::QuotaExceeded)
    ));
}
#[rstest]
#[case(401, "authentication")]
#[case(403, "permission")]
#[case(402, "quota")]
#[case(400, "input")]
#[tokio::test]
async fn test_should_map_http_failures(#[case] status: u16, #[case] category: &str) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status))
        .expect(1)
        .mount(&server)
        .await;
    let error = client(&server, false, settings())
        .decide::<Triage>("ticket")
        .await
        .unwrap_err();
    assert!(match category {
        "authentication" => matches!(error, Error::Authentication),
        "permission" => matches!(error, Error::PermissionDenied),
        "quota" => matches!(error, Error::QuotaExceeded),
        "input" => matches!(error, Error::InvalidInput(_)),
        _ => false,
    });
}
#[rstest]
#[case(429)]
#[case(500)]
#[case(502)]
#[case(503)]
#[case(504)]
#[cfg(feature = "retry")]
#[tokio::test]
async fn test_should_retry_only_definite_transient_failures(#[case] status: u16) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status))
        .expect(3)
        .mount(&server)
        .await;
    let mut s = settings();
    s.retry = RetryConfig::default();
    s.retry.initial_backoff_ms = 1;
    assert!(
        client(&server, true, s)
            .decide::<Triage>("ticket")
            .await
            .is_err()
    );
}
#[tokio::test]
async fn test_should_enforce_deadline_without_replaying_sent_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(triage_body(true))
                .set_delay(Duration::from_millis(150)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.timeout_ms = 30;
    s.retry = RetryConfig::default();
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::Timeout)
    ));
}
#[tokio::test]
async fn test_should_honor_retry_after_beyond_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "5"))
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.timeout_ms = 50;
    s.retry = RetryConfig::default();
    let error = client(&server, true, s)
        .decide::<Triage>("ticket")
        .await
        .unwrap_err();
    if cfg!(feature = "retry") {
        assert!(matches!(error, Error::Timeout));
    } else {
        assert!(matches!(error, Error::RateLimited { .. }));
    }
}
#[tokio::test]
async fn test_should_enforce_body_caps_and_duplicate_key_rejection() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"model":"one","model":"two","answers":[],"usage":{}}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server, true, settings())
            .decide::<Triage>("ticket")
            .await,
        Err(Error::Protocol { .. })
    ));
    let mut s = settings();
    s.limits.response_bytes = 10;
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::Protocol { .. })
    ));
    let mut s = settings();
    s.limits.request_bytes = 10;
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::InvalidInput(_))
    ));
    let mut s = settings();
    s.limits.input_bytes = 2;
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::InvalidInput(_))
    ));
}
#[tokio::test]
async fn test_should_disable_redirects() {
    let server = MockServer::start().await;
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/decisions", target.uri())),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server, true, settings())
            .decide::<Triage>("ticket")
            .await,
        Err(Error::Api { status: 307, .. })
    ));
    assert!(target.received_requests().await.unwrap().is_empty());
}
#[test]
fn test_should_resolve_hosted_defaults_and_reject_known_mismatches() {
    assert_eq!(Client::openai("key").unwrap().model(), &Model::Gpt6Luna);
    assert_eq!(Client::openrouter("key").unwrap().model(), &Model::Jev113);
    assert_eq!(Client::typesafe("key").unwrap().model(), &Model::Jev113);
    assert!(matches!(
        Client::builder(Provider::OpenAi)
            .api_key("key")
            .model(Model::Jev113)
            .build(),
        Err(Error::UnsupportedModel)
    ));
    assert!(matches!(
        Client::builder(Provider::TypeSafe)
            .api_key("key")
            .model(Model::Gpt6Luna)
            .build(),
        Err(Error::UnsupportedModel)
    ));
    assert!(Client::builder(Provider::OpenAi).build().is_err());
}
#[tokio::test]
async fn test_should_reject_bad_overrides_before_http_and_keep_default_model() {
    let server = MockServer::start().await;
    let client = client(&server, true, settings());
    assert!(matches!(
        client
            .decide_with::<Triage>(RequestOptions::new("ticket").unwrap().model(Model::Jev113))
            .await,
        Err(Error::UnsupportedModel)
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    mount(&server, triage_body(true)).await;
    let result = client
        .decide_with::<Triage>(
            RequestOptions::new("ticket")
                .unwrap()
                .model("another-model".parse().unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(result.meta.wire_model, "another-model");
    assert_eq!(client.model(), &"fixture-model".parse::<Model>().unwrap());
}
#[test]
fn test_should_validate_endpoint_security_and_redact_sensitive_debug() {
    for url in [
        "http://example.com/decisions",
        "https://key@example.com/decisions",
        "https://example.com/decisions?a=b",
        "https://example.com/decisions#frag",
        "http://localhost/decisions",
    ] {
        assert!(Endpoint::openai_decisions(url).is_err());
    }
    assert!(Endpoint::system_one("http://[::1]:9000/decisions").is_ok());
    assert!(Endpoint::system_one("https://internal.example/decisions").is_ok());
    let secret = "uniquely-sensitive-key";
    assert!(!format!("{:?}", Client::openai(secret).unwrap()).contains(secret));
    assert!(!format!("{:?}", Auth::ApiKey(secret.into())).contains(secret));
    assert!(
        !format!("{:?}", Input::try_from("uniquely-sensitive-input").unwrap())
            .contains("uniquely-sensitive-input")
    );
}
#[test]
fn test_should_validate_input_schema_and_probabilities_at_boundaries() {
    for v in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(Probability::try_from(v).is_err());
    }
    assert!(Input::try_from(" ").is_err());
    assert!(Input::try_from("x".repeat((1 << 20) + 1)).is_err());
    assert!(Input::json(&json!("scalar")).is_err());
    assert!(Input::json(&json!({"a":"x".repeat((256<<10)+1)})).is_err());
    assert!(
        Questions::builder()
            .predicate("a", "Question")
            .predicate("a", "Question")
            .build()
            .is_err()
    );
    assert!(Questions::builder().predicate("a", "x\0y").build().is_err());
    assert!(
        Questions::builder()
            .choice("a", "Question", [("one", "Only")])
            .build()
            .is_err()
    );
    assert!(
        Questions::builder()
            .score("a", "Question", [("one", "First"), ("one", "Second")])
            .build()
            .is_err()
    );
    let mut nested = json!({});
    for _ in 0..34 {
        nested = json!({"inner":nested});
    }
    assert!(Input::json(&nested).is_err());
    let input = Input::json(&json!({"z":{"y":2,"a":1},"a":[{"z":2,"b":3}]})).unwrap();
    assert_eq!(input.as_str(), r#"{"a":[{"b":3,"z":2}],"z":{"a":1,"y":2}}"#);
}

#[tokio::test]
async fn test_should_decode_mixed_refusal_without_losing_other_fields() {
    let server = MockServer::start().await;
    let body = envelope(
        true,
        json!([
            {"name":"first","type":"predicate","probability":0.8},
            {"name":"second","type":"predicate","probability":0.2},
            {"name":"third","type":"refusal"}
        ]),
    );
    mount(&server, body).await;
    let value = client(&server, true, settings())
        .decide::<Refusable>("ticket")
        .await
        .unwrap();
    assert_eq!(value.first.probability(), p(0.8));
    assert_eq!(value.second.probability(), p(0.2));
    assert!(matches!(value.third, Outcome::Refused(_)));
}
#[tokio::test]
async fn test_should_include_semaphore_wait_in_deadline_across_clones() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(triage_body(true))
                .set_delay(Duration::from_millis(200)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.max_inflight = 1;
    s.timeout_ms = 60;
    let client = client(&server, true, s);
    let clone = client.clone();
    let (first, second) = tokio::join!(
        client.decide::<Triage>("one"),
        clone.decide::<Triage>("two")
    );
    assert!(matches!(first, Err(Error::Timeout)));
    assert!(matches!(second, Err(Error::Timeout)));
}
#[cfg(feature = "retry")]
#[tokio::test]
async fn test_should_stop_retries_when_cancelled() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.retry = RetryConfig::default();
    let client = client(&server, true, s);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), client.decide::<Triage>("ticket"))
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
#[cfg(feature = "batch")]
#[tokio::test]
async fn test_should_keep_batch_order_and_per_item_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_partial_json(
            json!({"input":"first"}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(triage_body(true))
                .set_delay(Duration::from_millis(20)),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_partial_json(
            json!({"input":"second"}),
        ))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let client = client(&server, true, settings());
    let inputs = [
        Input::try_from("first").unwrap(),
        Input::try_from("second").unwrap(),
    ];
    let results = jev::decide_all::<Triage>(&client, &inputs, 2)
        .await
        .unwrap();
    assert!(results[0].is_ok());
    assert!(matches!(results[1], Err(Error::PermissionDenied)));
    assert!(
        jev::decide_all::<Triage>(&client, &inputs, 0)
            .await
            .is_err()
    );
    assert!(
        jev::decide_all::<Triage>(&client, &inputs, 129)
            .await
            .is_err()
    );
    let results = jev::decide_all::<Triage>(&client, &[], 1).await.unwrap();
    assert!(results.is_empty());
}

#[cfg(feature = "retry")]
#[tokio::test]
async fn test_should_honor_http_date_retry_after_for_service_unavailable() {
    let server = MockServer::start().await;
    let future = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(10));
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", future))
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.timeout_ms = 100;
    s.retry = RetryConfig::default();
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::Timeout)
    ));
}
#[tokio::test]
async fn test_should_not_retry_insufficient_quota_type_without_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({"error":{"type":"insufficient_quota"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.retry = RetryConfig::default();
    assert!(matches!(
        client(&server, true, s).decide::<Triage>("ticket").await,
        Err(Error::QuotaExceeded)
    ));
}

#[tokio::test]
async fn test_should_not_replay_a_post_when_the_server_drops_the_response() {
    use tokio::{io::AsyncReadExt, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut received = Vec::new();
        let mut chunk = [0_u8; 2048];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }
            received.extend_from_slice(&chunk[..count]);
            if received.ends_with(b"}}}") || received.ends_with(b"]}") {
                break;
            }
            assert!(received.len() < 8192);
        }
        assert!(String::from_utf8_lossy(&received).contains("ticket"));
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let endpoint = Endpoint::openai_decisions(&format!("http://{address}/decisions")).unwrap();
    let mut s = settings();
    s.retry = RetryConfig::default();
    let client = Client::for_endpoint_with(
        endpoint,
        Auth::None,
        "fixture-model".parse().unwrap(),
        HttpClient::new().unwrap(),
        s,
    )
    .unwrap();
    let error = client.decide::<Triage>("ticket").await.unwrap_err();
    assert!(matches!(error, Error::Transport(ref source) if !source.is_connect()));
    assert!(!error.is_retryable());
    server.await.unwrap();
}

#[cfg(not(feature = "retry"))]
#[rstest]
#[case(429)]
#[case(503)]
#[tokio::test]
async fn test_should_return_transient_failures_without_retry(#[case] status: u16) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status))
        .expect(1)
        .mount(&server)
        .await;
    let mut s = settings();
    s.retry = RetryConfig::default();
    let error = client(&server, true, s)
        .decide::<Triage>("ticket")
        .await
        .unwrap_err();
    assert!(error.is_retryable()); // Classification stays stable; this build sends once.
}

#[tokio::test]
async fn test_should_decode_unordered_choice_keys_with_checked_enum_mapping() {
    let server = MockServer::start().await;
    mount(&server, envelope(false, json!({"team":{"type":"choice","choice":"billing","probabilities":{"billing":0.6,"technical":0.4}}}))).await;
    let questions = Questions::builder()
        .choice(
            "team",
            "Which team?",
            [
                ("technical", "Technical failures."),
                ("billing", "Payments and refunds."),
            ],
        )
        .build()
        .unwrap();
    let response = client(&server, false, settings())
        .ask("ticket", &questions)
        .await
        .unwrap();
    let team: Choice<Team> = response.answers.decode("team").unwrap();
    assert_eq!(team.value(), &Team::Billing);
}
