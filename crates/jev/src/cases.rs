//! Version 2 provider-neutral case files. Inputs and schemas share SDK validation.

use std::{collections::HashSet, path::Path, time::Duration};

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use tokio::{io::AsyncReadExt, time::timeout};

use crate::{Client, DecisionResponse, Error, Input, Questions, json};

const MAX_FILE_BYTES: usize = 16 << 20;
/// An immutable, validated input and question set.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Case {
    id: String,
    name: Option<String>,
    input: Input,
    questions: Questions,
}
impl Case {
    /// Case identifier.
    ///
    /// ```no_run
    /// # fn example(case: &jev::Case) -> Result<(), jev::Error> {
    /// let value = case.id();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Optional display name.
    ///
    /// ```no_run
    /// # fn example(case: &jev::Case) -> Result<(), jev::Error> {
    /// let value = case.name();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    /// Prepared evidence.
    ///
    /// ```no_run
    /// # fn example(case: &jev::Case) -> Result<(), jev::Error> {
    /// let value = case.input();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn input(&self) -> &Input {
        &self.input
    }
    /// Shared provider-neutral schema.
    ///
    /// ```no_run
    /// # fn example(case: &jev::Case) -> Result<(), jev::Error> {
    /// let value = case.questions();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn questions(&self) -> &Questions {
        &self.questions
    }
}
/// A case result with stable case identity.
#[derive(Debug)]
#[non_exhaustive]
pub struct CaseResult {
    /// Case identifier.
    pub id: String,
    /// Checked decision response.
    pub response: DecisionResponse,
}
/// Validated cases with unique identifiers, at most 10000 entries.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CaseSet(Vec<Case>);
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct File {
    version: u32,
    cases: Vec<CaseDto>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaseDto {
    id: String,
    name: Option<String>,
    input: Value,
    questions: IndexMap<String, QuestionDto>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum QuestionDto {
    Predicate {
        instructions: String,
        criteria: Option<Criteria>,
    },
    Choice {
        instructions: String,
        options: Vec<OptionDto>,
    },
    Score {
        instructions: String,
        levels: Vec<OptionDto>,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Criteria {
    yes: String,
    no: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OptionDto {
    key: String,
    description: String,
}
impl CaseSet {
    fn validate(file: File) -> Result<Self, Error> {
        if file.version != 2 || file.cases.is_empty() || file.cases.len() > 10_000 {
            return Err(Error::InvalidInput("case version or count"));
        }
        let mut ids = HashSet::new();
        let mut cases = Vec::new();
        for case in file.cases {
            if case.id.is_empty()
                || case.id.len() > 256
                || !case
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !ids.insert(case.id.clone())
            {
                return Err(Error::InvalidInput("invalid or duplicate case ID"));
            }
            if case.name.as_ref().is_some_and(|n| {
                n.trim().is_empty() || n.len() > 256 || n.chars().any(char::is_control)
            }) {
                return Err(Error::InvalidInput("case name"));
            }
            let input = match case.input {
                Value::String(text) => Input::try_from(text)?,
                value => Input::json(&value)?,
            };
            let mut builder = Questions::builder();
            for (key, question) in case.questions {
                builder = match question {
                    QuestionDto::Predicate {
                        instructions,
                        criteria: None,
                    } => builder.predicate(&key, &instructions),
                    QuestionDto::Predicate {
                        instructions,
                        criteria: Some(criteria),
                    } => builder.predicate_with(&key, &instructions, &criteria.yes, &criteria.no),
                    QuestionDto::Choice {
                        instructions,
                        options,
                    } => builder.choice(
                        &key,
                        &instructions,
                        options.into_iter().map(|o| (o.key, o.description)),
                    ),
                    QuestionDto::Score {
                        instructions,
                        levels,
                    } => builder.score(
                        &key,
                        &instructions,
                        levels.into_iter().map(|l| (l.key, l.description)),
                    ),
                };
            }
            cases.push(Case {
                id: case.id,
                name: case.name,
                input,
                questions: builder.build()?,
            });
        }
        Ok(Self(cases))
    }
    /// Load version 2 JSON, rejecting duplicates and bounded before deserialization.
    /// # Errors
    /// Rejects malformed, oversized or invalid cases.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let cases = jev::CaseSet::from_json_str(r#"{"version":2,"cases":[{"id":"ticket-1","input":"Refund please.","questions":{"refund":{"type":"predicate","instructions":"Refund requested?"}}}]}"#)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_json_str(text: &str) -> Result<Self, Error> {
        if text.len() > MAX_FILE_BYTES {
            return Err(Error::InvalidInput("case file byte limit"));
        }
        let value = json::parse_file(text.as_bytes())?;
        let file = serde_json::from_value(value)
            .map_err(|_| Error::InvalidInput("invalid case document"))?;
        Self::validate(file)
    }
    /// Load strict version 2 YAML without aliases or merge keys.
    /// # Errors
    /// Rejects malformed, oversized or invalid cases.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let cases = jev::CaseSet::from_yaml_str(r#"{"version":2,"cases":[{"id":"ticket-1","input":"Refund please.","questions":{"refund":{"type":"predicate","instructions":"Refund requested?"}}}]}"#)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(text: &str) -> Result<Self, Error> {
        Self::validate(crate::yaml::parse(text, MAX_FILE_BYTES)?)
    }
    /// Read a bounded file within ten seconds. File extensions select JSON or YAML.
    /// # Errors
    /// Returns I/O, timeout or case validation errors; unsupported formats are rejected.
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), jev::Error> {
    /// let cases = jev::CaseSet::from_file("cases.json").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let read = async {
            let file = tokio::fs::File::open(path).await.map_err(Error::Io)?;
            let mut bytes = Vec::new();
            file.take((MAX_FILE_BYTES as u64).saturating_add(1))
                .read_to_end(&mut bytes)
                .await
                .map_err(Error::Io)?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(Error::InvalidInput("case file byte limit"));
            }
            String::from_utf8(bytes).map_err(|_| Error::InvalidInput("case encoding"))
        };
        let text = timeout(Duration::from_secs(10), read)
            .await
            .map_err(|_| Error::Timeout)??;
        match path.extension().and_then(|s| s.to_str()) {
            Some("json") => Self::from_json_str(&text),
            #[cfg(feature = "yaml")]
            Some("yaml" | "yml") => Self::from_yaml_str(&text),
            _ => Err(Error::InvalidInput("unsupported case file extension")),
        }
    }
    /// Iterate validated cases in file order.
    ///
    /// ```no_run
    /// # fn example(cases: &jev::CaseSet) -> Result<(), jev::Error> {
    /// let value = cases.iter();
    /// # Ok(())
    /// # }
    /// ```
    pub fn iter(&self) -> std::slice::Iter<'_, Case> {
        self.0.iter()
    }
    /// Number of cases.
    ///
    /// ```no_run
    /// # fn example(cases: &jev::CaseSet) -> Result<(), jev::Error> {
    /// let value = cases.len();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether there are no cases.
    ///
    /// ```no_run
    /// # fn example(cases: &jev::CaseSet) -> Result<(), jev::Error> {
    /// let value = cases.is_empty();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl<'a> IntoIterator for &'a CaseSet {
    type Item = &'a Case;
    type IntoIter = std::slice::Iter<'a, Case>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Client {
    /// Execute a validated case through the normal sending path.
    /// # Errors
    /// Returns the same errors as ask.
    ///
    /// ```no_run
    /// # async fn example(client: &jev::Client, case: &jev::Case) -> Result<(), jev::Error> {
    /// let result = client.run_case(case).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_case(&self, case: &Case) -> Result<CaseResult, Error> {
        Ok(CaseResult {
            id: case.id.clone(),
            response: self.ask(&case.input, &case.questions).await?,
        })
    }
}
