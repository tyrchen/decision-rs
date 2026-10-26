//! Shared Jev Decisions and System One codec.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Decoded, NumericPolicy, WireUsage, bounded_identifier, choice, predicate, refusal, score,
};
use crate::{Answers, Error, Input, Probability, Questions, json, question::Kind};

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    state: &'a str,
    questions: IndexMap<&'a str, Question<'a>>,
}
#[derive(Serialize)]
#[serde(tag = "type")]
enum Question<'a> {
    #[serde(rename = "noul")]
    Predicate {
        instructions: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<Criteria<'a>>,
    },
    #[serde(rename = "choice")]
    Choice {
        instructions: &'a str,
        criteria: IndexMap<&'a str, &'a str>,
    },
    #[serde(rename = "score")]
    Score {
        instructions: &'a str,
        criteria: Vec<&'a str>,
    },
}
#[derive(Serialize)]
struct Criteria<'a> {
    #[serde(rename = "true")]
    yes: &'a str,
    #[serde(rename = "false")]
    no: &'a str,
}
pub(super) fn prepare(
    input: &Input,
    schema: &Questions,
    model: &str,
    cap: usize,
) -> Result<Vec<u8>, Error> {
    let questions = schema
        .iter()
        .map(|(key, q)| {
            let instructions = q.instructions.as_str();
            let question = match &q.kind {
                Kind::Predicate { criteria } => Question::Predicate {
                    instructions,
                    criteria: criteria.as_ref().map(|(yes, no)| Criteria {
                        yes: yes.as_str(),
                        no: no.as_str(),
                    }),
                },
                Kind::Choice(options) => Question::Choice {
                    instructions,
                    criteria: options
                        .iter()
                        .map(|(k, d)| (k.as_str(), d.as_str()))
                        .collect(),
                },
                Kind::Score(levels) => Question::Score {
                    instructions,
                    criteria: levels.iter().map(|(_, d)| d.as_str()).collect(),
                },
            };
            (key.as_str(), question)
        })
        .collect();
    json::encode(
        &Request {
            model,
            state: input.as_str(),
            questions,
        },
        cap,
    )
    .map_err(|_| Error::InvalidInput("request body limit"))
}
#[derive(Deserialize)]
struct Response {
    model: String,
    answers: IndexMap<String, Value>,
    usage: WireUsage,
    #[serde(default)]
    id: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "type")]
enum WireAnswer {
    #[serde(rename = "noul")]
    Predicate { noul: Probability },
    #[serde(rename = "choice")]
    Choice {
        choice: String,
        confidence: Option<Probability>,
        probabilities: Option<IndexMap<String, Probability>>,
    },
    #[serde(rename = "score")]
    Score {
        score: f64,
        confidence: Option<Probability>,
        probabilities: Option<IndexMap<String, Probability>>,
        legend: Option<IndexMap<String, Value>>,
    },
    #[serde(rename = "refusal")]
    Refusal,
}
pub(super) fn decode(
    value: Value,
    questions: &Questions,
    policy: NumericPolicy,
) -> Result<Decoded, Error> {
    let mut response: Response =
        serde_json::from_value(value).map_err(|_| Error::protocol("Jev envelope"))?;
    if response.answers.len() != questions.len()
        || response.answers.keys().any(|k| questions.get(k).is_none())
    {
        return Err(Error::protocol("answer names differ from schema"));
    }
    let mut answers = IndexMap::new();
    for (key, question) in questions {
        let value = response
            .answers
            .shift_remove(key)
            .ok_or_else(|| Error::for_question(key, "missing answer"))?;
        let wire: WireAnswer = serde_json::from_value(value)
            .map_err(|_| Error::for_question(key, "invalid Jev answer"))?;
        let answer = match (&question.kind, wire) {
            (_, WireAnswer::Refusal) => refusal(key),
            (Kind::Predicate { .. }, WireAnswer::Predicate { noul }) => predicate(noul),
            (
                Kind::Choice(options),
                WireAnswer::Choice {
                    choice: selected,
                    confidence,
                    probabilities,
                },
            ) => choice(
                key,
                selected,
                options.iter().map(|(k, _)| k.clone()).collect(),
                confidence,
                probabilities.map(|p| p.into_iter().collect()),
                policy,
            )?,
            (
                Kind::Score(levels),
                WireAnswer::Score {
                    score: raw,
                    confidence,
                    probabilities,
                    legend,
                },
            ) => {
                if let Some(legend) = legend
                    && (legend.len() != levels.len()
                        || !(0..levels.len()).all(|i| legend.contains_key(&i.to_string())))
                {
                    return Err(Error::for_question(
                        key,
                        "legend indices differ from schema",
                    ));
                }
                score(
                    key,
                    raw,
                    levels.iter().map(|(k, _)| k.clone()).collect(),
                    confidence,
                    probabilities.map(|p| p.into_iter().collect()),
                    policy,
                )?
            }
            _ => return Err(Error::for_question(key, "answer kind differs from schema")),
        };
        answers.insert(key.clone(), answer);
    }
    Ok(Decoded {
        answers: Answers(answers),
        model: bounded_identifier(&response.model, 256)?,
        id: response
            .id
            .as_deref()
            .map(|s| bounded_identifier(s, 256))
            .transpose()?,
        usage: response.usage.validate()?,
    })
}
