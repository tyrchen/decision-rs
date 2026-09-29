//! Wire-level integration tests against a local `wiremock` server: golden request
//! bodies, error mapping, and retry behavior. No test in this file touches the
//! network.

use jev::{
    Choice, ChoiceOptions, Client, Decision, Noul, Questions, RetryConfig, Score, ScoreLevels,
    Surface,
};
use secrecy::SecretString;
use serde_json::json;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

/// The triage decision from the crate-level docs, used as the golden fixture.
#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a software defect?
    #[jev(
        yes = "The customer describes broken or unexpected product behavior.",
        no = "The customer is asking a question or requesting a feature."
    )]
    is_bug: Noul,
    /// Which team should own this ticket?
    team: Choice<Team>,
    /// How urgent is this ticket?
    urgency: Score<Urgency>,
}

/// Owning team for a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Checkout, billing, or payment processing issues.
    Payments,
    /// Rendering, layout, or browser compatibility issues.
    Frontend,
    /// Login, permissions, or profile issues.
    Account,
}

/// Ordered urgency scale, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
enum Urgency {
    /// Can wait for the next release.
    NextRelease,
    /// Should be fixed this week.
    ThisWeek,
    /// Blocking revenue right now.
    BlockingRevenue,
}

fn client_for(server: &MockServer) -> Client {
    Client::with_http(reqwest::Client::new(), SecretString::from("test-key"))
        .with_api_base(&server.uri())
        .expect("mock server URI is a valid http://127.0.0.1 base")
        .with_retry(RetryConfig::default().with_max_attempts(1))
}

fn triage_response_body() -> serde_json::Value {
    json!({
        "model": "typesafe/jev-1.13",
        "id": "req_abc123",
        "provider": "openrouter",
        "usage": {"input_tokens": 120, "output_tokens": 8, "cost": 0.0012},
        "answers": {
            "is_bug": {"type": "noul", "noul": 0.95},
            "team": {
                "type": "choice",
                "choice": "payments",
                "confidence": 0.88,
                "probabilities": {"payments": 0.88, "frontend": 0.1, "account": 0.02}
            },
            "urgency": {
                "type": "score",
                "score": 2.0,
                "confidence": 0.8,
                "legend": {"0": "Can wait", "1": "This week", "2": "Blocking revenue"}
            }
        }
    })
}

#[tokio::test]
async fn test_should_send_exact_golden_request_body_on_decisions_surface() {
    let server = MockServer::start().await;

    let expected_body = json!({
        "model": "typesafe/jev-1.13",
        "state": "a checkout ticket",
        "questions": {
            "is_bug": {
                "type": "noul",
                "instructions": "Is the customer reporting a software defect?",
                "criteria": {
                    "true": "The customer describes broken or unexpected product behavior.",
                    "false": "The customer is asking a question or requesting a feature."
                }
            },
            "team": {
                "type": "choice",
                "instructions": "Which team should own this ticket?",
                "criteria": {
                    "payments": "Checkout, billing, or payment processing issues.",
                    "frontend": "Rendering, layout, or browser compatibility issues.",
                    "account": "Login, permissions, or profile issues."
                }
            },
            "urgency": {
                "type": "score",
                "instructions": "How urgent is this ticket?",
                "criteria": [
                    "Can wait for the next release.",
                    "Should be fixed this week.",
                    "Blocking revenue right now."
                ]
            }
        }
    });

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .and(body_json(&expected_body))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(triage_response_body()))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let triage: Triage = client.decide("a checkout ticket").await.unwrap();

    assert!(triage.is_bug.is_yes(0.9));
    assert_eq!(*triage.team.value(), Team::Payments);
    assert_eq!(triage.urgency.nearest(), Urgency::BlockingRevenue);
}

#[tokio::test]
async fn test_should_send_bare_model_id_on_system_one_surface() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/systemone"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body["model"], "jev-1.13");
            assert!(body.get("session_id").is_none());
            ResponseTemplate::new(200).set_body_json(triage_response_body())
        })
        .mount(&server)
        .await;

    let client = client_for(&server).with_surface(Surface::SystemOne);
    let _triage: Triage = client.decide("a checkout ticket").await.unwrap();
}

#[tokio::test]
async fn test_should_drop_session_id_on_system_one_surface() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/systemone"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert!(
                body.get("session_id").is_none(),
                "session_id must be dropped on System One"
            );
            ResponseTemplate::new(200).set_body_json(triage_response_body())
        })
        .mount(&server)
        .await;

    let client = client_for(&server)
        .with_surface(Surface::SystemOne)
        .with_session_id("sess-1");
    let _triage: Triage = client.decide("a checkout ticket").await.unwrap();
}

#[tokio::test]
async fn test_should_keep_session_id_on_decisions_surface() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body["session_id"], "sess-1");
            ResponseTemplate::new(200).set_body_json(triage_response_body())
        })
        .mount(&server)
        .await;

    let client = client_for(&server).with_session_id("sess-1");
    let _triage: Triage = client.decide("a checkout ticket").await.unwrap();
}

#[tokio::test]
async fn test_should_map_400_to_api_error_without_retry() {
    let server = MockServer::start().await;
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = call_count.clone();

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(move |_req: &Request| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": {"code": 400, "message": "bad question shape"}}))
        })
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server).with_retry(RetryConfig::default().with_max_attempts(3));
    let err = client.ask("state", &questions).await.unwrap_err();

    match err {
        jev::Error::Api {
            status, message, ..
        } => {
            assert_eq!(status, 400);
            assert_eq!(message, "bad question shape");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_eq!(
        call_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "400 must not be retried"
    );
}

#[tokio::test]
async fn test_should_map_402_to_insufficient_credits() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(
            ResponseTemplate::new(402).set_body_json(json!({"error": {"message": "top up"}})),
        )
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server);
    let err = client.ask("state", &questions).await.unwrap_err();
    assert!(matches!(err, jev::Error::InsufficientCredits { .. }));
}

#[tokio::test]
async fn test_should_map_429_and_respect_retry_after_header() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"error": {"message": "slow down"}})),
        )
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server).with_retry(RetryConfig::default().with_max_attempts(1));
    let err = client.ask("state", &questions).await.unwrap_err();
    assert!(matches!(
        err,
        jev::Error::RateLimited {
            retry_after_secs: Some(0)
        }
    ));
}

#[tokio::test]
#[cfg(feature = "retry")]
async fn test_should_retry_and_succeed_on_second_attempt() {
    let server = MockServer::start().await;
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = call_count.clone();

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(move |_req: &Request| {
            let attempt = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt == 0 {
                ResponseTemplate::new(503)
                    .set_body_json(json!({"error": {"message": "unavailable"}}))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "model": "jev-1.13",
                    "usage": {"input_tokens": 1, "output_tokens": 1},
                    "answers": {"flag": {"type": "noul", "noul": 0.5}}
                }))
            }
        })
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server).with_retry(
        RetryConfig::default()
            .with_max_attempts(3)
            .with_initial_backoff(std::time::Duration::from_millis(1)),
    );
    let response = client.ask("state", &questions).await.unwrap();
    assert_eq!(response.answers.noul("flag").unwrap(), 0.5);
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_should_exhaust_retries_and_return_last_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(
            ResponseTemplate::new(503).set_body_json(json!({"error": {"message": "down"}})),
        )
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server).with_retry(
        RetryConfig::default()
            .with_max_attempts(2)
            .with_initial_backoff(std::time::Duration::from_millis(1)),
    );
    let err = client.ask("state", &questions).await.unwrap_err();
    assert!(matches!(err, jev::Error::Api { status: 503, .. }));
}

#[tokio::test]
async fn test_should_reject_oversized_response_body() {
    let server = MockServer::start().await;

    // ~11 MiB of JSON, exceeding the 10 MiB cap.
    let huge_string = "x".repeat(11 * 1024 * 1024);
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.13",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "answers": {},
            "provider": huge_string
        })))
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server);
    let err = client.ask("state", &questions).await.unwrap_err();
    assert!(matches!(err, jev::Error::Parse(_)));
}

#[tokio::test]
async fn test_should_deserialize_unknown_answer_type_as_other_without_failing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.13",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "answers": {
                "flag": {"type": "noul", "noul": 0.5},
                "future_primitive": {"type": "vibe", "vibe": "immaculate"}
            }
        })))
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server);
    let response = client.ask("state", &questions).await.unwrap();
    assert_eq!(response.answers.len(), 2);
    match response.answers.get("future_primitive").unwrap() {
        jev::Answer::Other { r#type, .. } => assert_eq!(r#type, "vibe"),
        other => panic!("expected Other, got {other:?}"),
    }
}

#[tokio::test]
async fn test_should_reject_state_too_large_for_known_model_before_sending() {
    let server = MockServer::start().await;
    // No mock registered: if the client made an HTTP call at all, this test would
    // fail with a connection/"no matching mock" error instead of `StateTooLarge`.

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server);
    // `jev-1.13` has a 32_000 token budget; ~8 bytes/token estimate margin, so use a
    // wildly oversized state to be unambiguous regardless of estimator rounding.
    let huge_state = "x".repeat(32_000 * 4 + 10_000);
    let err = client.ask(huge_state, &questions).await.unwrap_err();
    assert!(matches!(err, jev::Error::StateTooLarge { .. }));
}

#[tokio::test]
async fn test_should_skip_context_check_when_disabled() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.13",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "answers": {"flag": {"type": "noul", "noul": 0.5}}
        })))
        .mount(&server)
        .await;

    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let client = client_for(&server).with_context_check(false);
    let huge_state = "x".repeat(32_000 * 4 + 10_000);
    let response = client.ask(huge_state, &questions).await.unwrap();
    assert_eq!(response.answers.noul("flag").unwrap(), 0.5);
}

#[tokio::test]
async fn test_should_return_decided_with_response_meta_from_decide_with() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(triage_response_body()))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let decided = client
        .decide_with::<Triage>(jev::RequestOptions::new("a ticket"))
        .await
        .unwrap();
    assert_eq!(decided.meta.id, Some("req_abc123".to_string()));
    assert_eq!(decided.meta.usage.input_tokens, 120);
    assert!(decided.value.is_bug.is_yes(0.9));
}

#[tokio::test]
async fn test_should_ask_single_noul_question_via_convenience_method() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body["questions"]["noul"]["type"], "noul");
            assert_eq!(
                body["questions"]["noul"]["instructions"],
                "Is this a refund request?"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"noul": {"type": "noul", "noul": 0.87}}
            }))
        })
        .mount(&server)
        .await;

    let client = client_for(&server);
    let is_refund = client
        .noul("Please refund my order.", "Is this a refund request?")
        .await
        .unwrap();
    assert!(is_refund.is_yes(0.8));
}

#[tokio::test]
async fn test_should_ask_single_choice_question_via_convenience_method() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body["questions"]["choice"]["type"], "choice");
            assert_eq!(
                body["questions"]["choice"]["criteria"]["payments"],
                "Checkout, billing, or payment processing issues."
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {
                    "choice": {"type": "choice", "choice": "payments", "confidence": 0.9}
                }
            }))
        })
        .mount(&server)
        .await;

    let client = client_for(&server);
    let team = client
        .choice::<Team>("a checkout ticket", "Which team owns this?")
        .await
        .unwrap();
    assert_eq!(*team.value(), Team::Payments);
    assert!(team.confident(0.8));
}

#[tokio::test]
async fn test_should_ask_single_score_question_via_convenience_method() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(|req: &Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body["questions"]["score"]["type"], "score");
            assert_eq!(
                body["questions"]["score"]["criteria"][0],
                "Can wait for the next release."
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"score": {"type": "score", "score": 2.0, "confidence": 0.8}}
            }))
        })
        .mount(&server)
        .await;

    let client = client_for(&server);
    let urgency = client
        .score::<Urgency>("a checkout ticket", "How urgent is this?")
        .await
        .unwrap();
    assert_eq!(urgency.nearest(), Urgency::BlockingRevenue);
}

#[tokio::test]
async fn test_should_send_no_authorization_header_when_built_without_api_key() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(|req: &Request| {
            assert!(
                req.headers.get("authorization").is_none(),
                "expected no Authorization header"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"flag": {"type": "noul", "noul": 0.5}}
            }))
        })
        .mount(&server)
        .await;

    let client = jev::Client::without_api_key(&server.uri()).unwrap();
    let questions = Questions::builder()
        .noul("flag", "Is this flagged?")
        .build()
        .unwrap();
    let response = client.ask("state", &questions).await.unwrap();
    assert_eq!(response.answers.noul("flag").unwrap(), 0.5);
}
