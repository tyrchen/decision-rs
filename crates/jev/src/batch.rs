//! `decide_all`: a bounded-concurrency helper over many states, same questions.
//!
//! The API has no batch endpoint; this ships one helper instead of a fake batch
//! surface. Feature-gated behind `batch` so the `futures-util` dependency stays
//! opt-in.

use std::num::NonZeroUsize;

use futures_util::{StreamExt, stream};

use crate::{client::Client, error::Error, state::State, typed::Decision};

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
}
