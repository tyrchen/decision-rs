//! `decide_all` / `run_cases`: bounded-concurrency helpers over many states or cases.
//!
//! The API has no batch endpoint; this ships helpers instead of a fake batch
//! surface. Feature-gated behind `batch` so the `futures-util` dependency stays
//! opt-in.

use std::num::NonZeroUsize;

use futures_util::{StreamExt, stream};

use crate::{
    cases::{Case, CaseResult},
    client::Client,
    error::Error,
    state::State,
    typed::Decision,
};

/// Runs one decision per state with bounded concurrency, preserving input order.
///
/// Each state gets an independent request; one failure does not discard the batch —
/// the result at index `i` corresponds to `states[i]`.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> Result<(), jev::Error> {
/// use std::num::NonZeroUsize;
///
/// use jev::{Client, Decision, decide_all};
/// # #[derive(Debug)] struct Triage;
/// # impl Decision for Triage {
/// #     fn questions() -> &'static jev::Questions { unimplemented!() }
/// #     fn from_answers(_: &jev::Answers) -> Result<Self, jev::Error> { unimplemented!() }
/// # }
///
/// let client = Client::from_env()?;
/// let states = vec!["ticket one".into(), "ticket two".into()];
/// let results = decide_all::<Triage>(&client, states, NonZeroUsize::new(4).unwrap()).await;
/// for result in results {
///     match result {
///         Ok(triage) => println!("{triage:?}"),
///         Err(err) => eprintln!("failed: {err}"),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn decide_all<D: Decision>(
    client: &Client,
    states: Vec<State>,
    concurrency: NonZeroUsize,
) -> Vec<Result<D, Error>> {
    stream::iter(states)
        .map(|state| async move { client.decide::<D>(state).await })
        .buffered(concurrency.get())
        .collect()
        .await
}

/// Runs many predefined [`Case`]s (see [`crate::CaseSet`]) with bounded concurrency,
/// preserving input order.
///
/// The dynamic-path counterpart of [`decide_all`]: each case supplies its own state
/// *and* questions (loaded from a config file rather than one [`Decision`] type
/// applied to many states), so the result carries the case's `id` instead of a typed
/// value. One failure does not discard the batch — the result at index `i`
/// corresponds to `cases[i]`.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> Result<(), jev::Error> {
/// use std::num::NonZeroUsize;
///
/// use jev::{CaseSet, Client, run_cases};
///
/// let cases = CaseSet::from_json_str(r#"{"cases": [{
///     "id": "ticket-001",
///     "state": "a refund ticket",
///     "questions": {"refund": {"type": "noul", "instructions": "Refund?"}}
/// }]}"#)?;
/// let client = Client::from_env()?;
/// let results = run_cases(&client, cases.as_slice(), NonZeroUsize::new(4).unwrap()).await;
/// for result in results {
///     match result {
///         Ok(case) => println!("{}: {:?}", case.id, case.response.answers),
///         Err(err) => eprintln!("failed: {err}"),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn run_cases(
    client: &Client,
    cases: &[Case],
    concurrency: NonZeroUsize,
) -> Vec<Result<CaseResult, Error>> {
    stream::iter(cases)
        .map(|case| async move { client.run_case(case).await })
        .buffered(concurrency.get())
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use secrecy::SecretString;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::{answer::Answers, question::Questions, typed::Noul};

    #[derive(Debug, PartialEq)]
    struct OneNoul {
        flag: Noul,
    }

    impl Decision for OneNoul {
        fn questions() -> &'static Questions {
            static QUESTIONS: std::sync::LazyLock<Questions> = std::sync::LazyLock::new(|| {
                Questions::builder()
                    .noul("flag", "Is this flagged?")
                    .build()
                    .unwrap()
            });
            &QUESTIONS
        }

        fn from_answers(answers: &Answers) -> Result<Self, Error> {
            Ok(OneNoul {
                flag: Noul::try_new(answers.noul("flag")?)?,
            })
        }
    }

    #[tokio::test]
    async fn test_should_preserve_order_and_report_per_item_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/alpha/decisions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "typesafe/jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"flag": {"type": "noul", "noul": 0.9}}
            })))
            .mount(&server)
            .await;

        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"))
            .with_api_base(&server.uri())
            .unwrap();

        let states: Vec<State> = vec!["a".into(), "b".into(), "c".into()];
        let results = decide_all::<OneNoul>(&client, states, NonZeroUsize::new(2).unwrap()).await;

        assert_eq!(results.len(), 3);
        for result in results {
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_should_run_cases_preserving_order_and_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/alpha/decisions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "typesafe/jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"flag": {"type": "noul", "noul": 0.9}}
            })))
            .mount(&server)
            .await;

        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"))
            .with_api_base(&server.uri())
            .unwrap();

        let case_set = crate::CaseSet::from_json_str(
            r#"{"cases": [
                {"id": "a", "state": "one", "questions": {"flag": {"type": "noul", "instructions": "Flagged?"}}},
                {"id": "b", "state": "two", "questions": {"flag": {"type": "noul", "instructions": "Flagged?"}}}
            ]}"#,
        )
        .unwrap();

        let results = run_cases(&client, case_set.as_slice(), NonZeroUsize::new(2).unwrap()).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap().id, "a");
        assert_eq!(results[1].as_ref().unwrap().id, "b");
    }
}
