//! `OpenAI` Decisions codec. Boolean choices retain predicate branch descriptions.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    DataUrl, Decoded, NumericPolicy, WireUsage, bounded_identifier, choice, distribution,
    predicate, refusal, score,
};
use crate::{
    Answer, Answers, Distribution, Error, Input, Probability, Question as DomainQuestion,
    Questions, json, question::Kind,
};

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    input: RequestInput<'a>,
    questions: Vec<Question<'a>>,
}
/// Text-only evidence stays a string; images require one user message of parts.
#[derive(Serialize)]
#[serde(untagged)]
enum RequestInput<'a> {
    Text(&'a str),
    Messages([UserMessage<'a>; 1]),
}
#[derive(Serialize)]
struct UserMessage<'a> {
    role: &'static str,
    content: Vec<Part<'a>>,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Part<'a> {
    InputText {
        text: &'a str,
    },
    InputImage {
        image_url: DataUrl<'a>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<&'static str>,
    },
}
impl<'a> RequestInput<'a> {
    fn new(input: &'a Input) -> Self {
        if input.images().is_empty() {
            return Self::Text(input.as_str());
        }
        let content = std::iter::once(Part::InputText {
            text: input.as_str(),
        })
        .chain(input.images().iter().map(|image| Part::InputImage {
            image_url: DataUrl(image),
            detail: image.detail().wire(),
        }))
        .collect();
        Self::Messages([UserMessage {
            role: "user",
            content,
        }])
    }
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Question<'a> {
    Predicate {
        name: &'a str,
        instructions: &'a str,
    },
    Choice {
        name: &'a str,
        instructions: &'a str,
        choices: Vec<ChoiceSpec<'a>>,
    },
    Score {
        name: &'a str,
        instructions: &'a str,
        levels: Vec<Level<'a>>,
    },
}
#[derive(Serialize)]
struct ChoiceSpec<'a> {
    value: ChoiceValue<'a>,
    description: &'a str,
}
#[derive(Serialize)]
#[serde(untagged)]
enum ChoiceValue<'a> {
    Text(&'a str),
    Boolean(bool),
}
#[derive(Serialize)]
struct Level<'a> {
    label: &'a str,
    description: &'a str,
}
pub(super) fn prepare(
    input: &Input,
    schema: &Questions,
    model: &str,
    cap: usize,
) -> Result<Vec<u8>, Error> {
    let questions = schema
        .iter()
        .map(|(name, q)| {
            let name = name.as_str();
            let instructions = q.instructions.as_str();
            match &q.kind {
                Kind::Predicate { criteria: None } => Question::Predicate { name, instructions },
                Kind::Predicate {
                    criteria: Some((yes, no)),
                } => Question::Choice {
                    name,
                    instructions,
                    choices: vec![
                        ChoiceSpec {
                            value: ChoiceValue::Boolean(true),
                            description: yes.as_str(),
                        },
                        ChoiceSpec {
                            value: ChoiceValue::Boolean(false),
                            description: no.as_str(),
                        },
                    ],
                },
                Kind::Choice(options) => Question::Choice {
                    name,
                    instructions,
                    choices: options
                        .iter()
                        .map(|(k, d)| ChoiceSpec {
                            value: ChoiceValue::Text(k),
                            description: d.as_str(),
                        })
                        .collect(),
                },
                Kind::Score(levels) => Question::Score {
                    name,
                    instructions,
                    levels: levels
                        .iter()
                        .map(|(k, d)| Level {
                            label: k,
                            description: d.as_str(),
                        })
                        .collect(),
                },
            }
        })
        .collect();
    json::encode(
        &Request {
            model,
            input: RequestInput::new(input),
            questions,
        },
        cap,
    )
    .map_err(|_| Error::InvalidInput("request body limit"))
}
#[derive(Deserialize)]
struct Response {
    model: String,
    answers: Vec<NamedAnswer>,
    usage: WireUsage,
    #[serde(default)]
    id: Option<String>,
}
#[derive(Deserialize)]
struct NamedAnswer {
    name: String,
    #[serde(flatten)]
    answer: WireAnswer,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WireAnswer {
    Predicate {
        probability: Probability,
    },
    Choice {
        choice: Selected,
        confidence: Probability,
        probabilities: Vec<ChoiceProbability>,
    },
    Score {
        score: f64,
        confidence: Probability,
        probabilities: Vec<ScoreProbability>,
    },
    Refusal,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Selected {
    Text(String),
    Boolean(bool),
}
#[derive(Deserialize)]
struct ChoiceProbability {
    value: Selected,
    probability: Probability,
}
#[derive(Deserialize)]
struct ScoreProbability {
    value: usize,
    label: String,
    probability: Probability,
}
pub(super) fn decode(value: Value, questions: &Questions) -> Result<Decoded, Error> {
    let response: Response =
        serde_json::from_value(value).map_err(|_| Error::protocol("OpenAI envelope or answer"))?;
    if response.answers.len() != questions.len() {
        return Err(Error::protocol("answer count differs from schema"));
    }
    let mut named = IndexMap::new();
    for answer in response.answers {
        if questions.get(&answer.name).is_none()
            || named.insert(answer.name, answer.answer).is_some()
        {
            return Err(Error::protocol("unknown or duplicate answer name"));
        }
    }
    let mut answers = IndexMap::new();
    for (key, question) in questions {
        let wire = named
            .shift_remove(key)
            .ok_or_else(|| Error::for_question(key, "missing answer"))?;
        let answer = decode_answer(key, question, wire)?;
        answers.insert(key.clone(), answer);
    }
    Ok(Decoded {
        answers: Answers(answers),
        model: Some(bounded_identifier(&response.model, 256)?),
        id: response
            .id
            .as_deref()
            .map(|s| bounded_identifier(s, 256))
            .transpose()?,
        usage: response.usage.validate()?,
    })
}

fn decode_answer(key: &str, question: &DomainQuestion, wire: WireAnswer) -> Result<Answer, Error> {
    Ok(match (&question.kind, wire) {
        (_, WireAnswer::Refusal) => refusal(key),
        (Kind::Predicate { criteria: None }, WireAnswer::Predicate { probability }) => {
            predicate(probability)
        }
        (
            Kind::Predicate { criteria: Some(_) },
            WireAnswer::Choice {
                choice: Selected::Boolean(_),
                confidence: _,
                probabilities,
            },
        ) => {
            let entries = probabilities
                .into_iter()
                .map(|p| match p.value {
                    Selected::Boolean(value) => Ok((value.to_string(), p.probability)),
                    Selected::Text(_) => Err(Error::for_question(
                        key,
                        "boolean predicate received string value",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let probs = distribution(
                key,
                &["true".to_owned(), "false".to_owned()],
                Some(entries),
                NumericPolicy::Standard,
            )?;
            let Distribution::Reported(probs) = probs else {
                return Err(Error::for_question(key, "boolean distribution unavailable"));
            };
            predicate(
                *probs
                    .get("true")
                    .ok_or_else(|| Error::for_question(key, "true probability missing"))?,
            )
        }
        (
            Kind::Choice(options),
            WireAnswer::Choice {
                choice: Selected::Text(selected),
                confidence,
                probabilities,
            },
        ) => {
            let entries = probabilities
                .into_iter()
                .map(|p| match p.value {
                    Selected::Text(value) => Ok((value, p.probability)),
                    Selected::Boolean(_) => {
                        Err(Error::for_question(key, "choice received boolean value"))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            choice(
                key,
                selected,
                options.iter().map(|(k, _)| k.clone()).collect(),
                Some(confidence),
                Some(entries),
                NumericPolicy::Standard,
            )?
        }
        (
            Kind::Score(levels),
            WireAnswer::Score {
                score: raw,
                confidence,
                probabilities,
            },
        ) => {
            let entries = probabilities
                .into_iter()
                .map(|p| {
                    if levels.get(p.value).is_none_or(|(k, _)| k != &p.label) {
                        return Err(Error::for_question(
                            key,
                            "score label/index differs from schema",
                        ));
                    }
                    Ok((p.value.to_string(), p.probability))
                })
                .collect::<Result<Vec<_>, _>>()?;
            score(
                key,
                raw,
                levels.iter().map(|(k, _)| k.clone()).collect(),
                Some(confidence),
                Some(entries),
                NumericPolicy::Standard,
            )?
        }
        _ => return Err(Error::for_question(key, "answer kind differs from schema")),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{Image, ImageDetail};

    fn schema() -> Questions {
        Questions::builder()
            .predicate("shows_line", "Does the frame show the narrated line?")
            .build()
            .unwrap()
    }
    fn frames() -> Input {
        Input::try_from("Narration: revenue doubled.")
            .unwrap()
            .with_images([
                Image::try_from(&b"\x89PNG\r\n\x1a\nframe-a"[..]).unwrap(),
                Image::try_from(&b"\xff\xd8\xff\xe0frame-b"[..])
                    .unwrap()
                    .with_detail(ImageDetail::Low),
            ])
            .unwrap()
    }

    #[test]
    fn test_should_encode_images_as_user_message_parts() {
        let body = prepare(&frames(), &schema(), "gpt-6-luna", 1 << 20).unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            json!({"model":"gpt-6-luna","input":[{"role":"user","content":[
                {"type":"input_text","text":"Narration: revenue doubled."},
                {"type":"input_image","image_url":"data:image/png;base64,iVBORw0KGgpmcmFtZS1h"},
                {"type":"input_image","image_url":"data:image/jpeg;base64,/9j/4GZyYW1lLWI=","detail":"low"}
            ]}],"questions":[
                {"name":"shows_line","type":"predicate","instructions":"Does the frame show the narrated line?"}
            ]})
        );
    }

    #[test]
    fn test_should_keep_text_only_input_a_string() {
        let input = Input::try_from("ticket").unwrap();
        let body = prepare(&input, &schema(), "gpt-6-luna", 1 << 20).unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["input"], json!("ticket"));
    }
}
