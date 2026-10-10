//! Provider-neutral orchestration. No wire DTOs are exposed to applications.

use std::{fmt, str::FromStr, sync::Arc};

use secrecy::SecretString;
use tokio::sync::Semaphore;

use crate::{
    Auth, Config, Decision, DecisionResponse, DecodeError, Endpoint, Error, HttpClient, Input,
    IntoInput, Model, Provider, Questions, ResponseMeta, Settings, backend::ResponsePlan,
    transport::Transport,
};

/// A typed decision together with accounting and provenance.
#[derive(Debug)]
#[non_exhaustive]
pub struct Decided<D> {
    /// Decoded business value.
    pub value: D,
    /// Metadata from the same response.
    pub meta: ResponseMeta,
}
/// Prepared input and an optional per-call model override.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestOptions {
    input: Input,
    model: Option<Model>,
}
impl RequestOptions {
    /// Validate an input once.
    /// # Errors
    /// Returns `InvalidInput` for empty or oversized evidence.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let options = decision_rs::RequestOptions::new("Please refund me.")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<I: IntoInput>(input: I) -> Result<Self, Error> {
        Ok(Self {
            input: input.try_into_input()?,
            model: None,
        })
    }
    /// Override the logical model for this call without changing the client.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let options = decision_rs::RequestOptions::new("Please refund me.")?.model(decision_rs::Model::Default);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn model(mut self, model: Model) -> Self {
        self.model = Some(model);
        self
    }
}
/// Immutable backend binding. Clones share a pool and concurrency permits.
#[derive(Clone)]
#[non_exhaustive]
pub struct Client {
    inner: Arc<ClientInner>,
}
#[derive(Debug)]
struct ClientInner {
    provider: Option<Provider>,
    endpoint: Endpoint,
    auth: Auth,
    requested_model: Model,
    resolved_model: Model,
    wire_model: String,
    transport: Transport,
}
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("provider", &self.inner.provider)
            .field("model", &self.inner.resolved_model)
            .finish_non_exhaustive()
    }
}
/// Builder with an explicitly required hosted provider and credential.
#[derive(Debug)]
#[non_exhaustive]
pub struct ClientBuilder {
    provider: Provider,
    key: Option<SecretString>,
    model: Model,
    http: Option<HttpClient>,
    settings: Settings,
}
impl ClientBuilder {
    /// Supply a credential; Debug always redacts it.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::OpenAi).api_key("application-loaded-key");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn api_key(mut self, key: impl Into<SecretString>) -> Self {
        self.key = Some(key.into());
        self
    }
    /// Select a logical model.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::OpenAi).model(decision_rs::Model::Gpt6Luna);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn model(mut self, model: Model) -> Self {
        self.model = model;
        self
    }
    /// Share an SDK-controlled connection pool.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let pool = decision_rs::HttpClient::new()?;
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::OpenAi).http(pool);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn http(mut self, http: HttpClient) -> Self {
        self.http = Some(http);
        self
    }
    /// Supply runtime settings, validated by build.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::OpenAi).settings(decision_rs::Settings::default());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }
    /// Freeze and validate all configuration before any HTTP request.
    /// # Errors
    /// Rejects missing/invalid credentials, invalid settings or an incompatible model.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::builder(decision_rs::Provider::OpenAi).api_key("application-loaded-key").build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Result<Client, Error> {
        let endpoint = match self.provider {
            Provider::OpenAi => Endpoint::openai_decisions("https://api.openai.com/v1/decisions")?,
            Provider::OpenRouter => {
                Endpoint::jev_decisions("https://openrouter.ai/api/alpha/decisions")?
            }
            Provider::TypeSafe => Endpoint::system_one("https://api.typesafe.ai/v1/systemone")?,
        };
        Client::construct(
            Some(self.provider),
            endpoint,
            Auth::ApiKey(self.key.ok_or(Error::MissingCredentials)?),
            self.model,
            self.http,
            self.settings,
        )
    }
}
impl Client {
    fn construct(
        provider: Option<Provider>,
        endpoint: Endpoint,
        auth: Auth,
        model: Model,
        http: Option<HttpClient>,
        settings: Settings,
    ) -> Result<Self, Error> {
        settings.validate()?;
        auth.header()?;
        let (resolved_model, wire_model) = model.resolve(provider)?;
        let http = match http {
            Some(http) => http,
            None => HttpClient::new()?,
        };
        let transport = Transport {
            http,
            permits: Semaphore::new(settings.max_inflight),
            settings,
        };
        Ok(Self {
            inner: Arc::new(ClientInner {
                provider,
                endpoint,
                auth,
                requested_model: model,
                resolved_model,
                wire_model,
                transport,
            }),
        })
    }
    /// Construct a hosted client using its provider-aware default model.
    /// # Errors
    /// Rejects an invalid key or HTTP initialization failure.
    /// ```no_run
    /// let client = decision_rs::Client::new(decision_rs::Provider::OpenAi, "application-loaded-key")?;
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn new(provider: Provider, key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::builder(provider).api_key(key).build()
    }
    /// Start configuring a hosted client.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::OpenAi);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder(provider: Provider) -> ClientBuilder {
        ClientBuilder {
            provider,
            key: None,
            model: Model::Default,
            http: None,
            settings: Settings::default(),
        }
    }
    /// Connect to `OpenAI` Decisions.
    /// # Errors
    /// Rejects an invalid key or HTTP initialization failure.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::openai("application-loaded-key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn openai(key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::new(Provider::OpenAi, key)
    }
    /// Connect to `OpenRouter` Jev Decisions.
    /// # Errors
    /// Rejects an invalid key or HTTP initialization failure.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::openrouter("application-loaded-key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn openrouter(key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::new(Provider::OpenRouter, key)
    }
    /// Connect to `TypeSafe` System One.
    /// # Errors
    /// Rejects an invalid key or HTTP initialization failure.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::typesafe("application-loaded-key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn typesafe(key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::new(Provider::TypeSafe, key)
    }
    /// Bind a secret-free configuration to an application-loaded key.
    /// # Errors
    /// Rejects invalid configuration, key or model/provider combinations.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::from_config(decision_rs::Config::new(decision_rs::Provider::OpenAi), "application-loaded-key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_config(config: Config, key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::builder(config.provider)
            .api_key(key)
            .model(config.model)
            .settings(config.settings)
            .build()
    }
    /// Infer credentials only when selection is unambiguous. `DECISION_PROVIDER` selects
    /// `DECISION_API_KEY` or that provider's own key; other provider keys never fall back.
    /// # Errors
    /// Rejects ambiguity, blank/invalid environment values and missing selected credentials.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let client = decision_rs::Client::from_env()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_env() -> Result<Self, Error> {
        Self::from_environment(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(Error::InvalidConfiguration("environment encoding")),
        })
    }
    fn from_environment<F: Fn(&str) -> Result<Option<String>, Error>>(
        read: F,
    ) -> Result<Self, Error> {
        let generic = read("DECISION_API_KEY")?;
        if let Some(key) = &generic {
            Auth::ApiKey(SecretString::from(key.clone())).header()?;
        }
        let (provider, key) = if let Some(raw) = read("DECISION_PROVIDER")? {
            let provider = Provider::from_str(&raw)?;
            let key = match generic {
                Some(key) => key,
                None => read(provider.key_env())?.ok_or(Error::MissingCredentials)?,
            };
            (provider, key)
        } else {
            if generic.is_some() {
                return Err(Error::AmbiguousProvider);
            }
            let mut selected = None;
            for provider in [Provider::OpenAi, Provider::OpenRouter, Provider::TypeSafe] {
                if let Some(key) = read(provider.key_env())? {
                    Auth::ApiKey(SecretString::from(key.clone())).header()?;
                    if selected.is_some() {
                        return Err(Error::AmbiguousProvider);
                    }
                    selected = Some((provider, key));
                }
            }
            selected.ok_or(Error::MissingCredentials)?
        };
        let model = read("DECISION_MODEL")?
            .map(|m| Model::from_str(&m))
            .transpose()?
            .unwrap_or_default();
        Self::builder(provider).api_key(key).model(model).build()
    }
    /// Connect to an explicit custom endpoint; requires a Custom model ID.
    /// # Errors
    /// Rejects invalid auth, model or HTTP initialization.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let endpoint = decision_rs::Endpoint::system_one("http://127.0.0.1:8080/v1/systemone")?;
    /// let client = decision_rs::Client::for_endpoint(endpoint, decision_rs::Auth::None, "local-model".parse()?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn for_endpoint(endpoint: Endpoint, auth: Auth, model: Model) -> Result<Self, Error> {
        Self::for_endpoint_with(
            endpoint,
            auth,
            model,
            HttpClient::new()?,
            Settings::default(),
        )
    }
    /// Configure a custom endpoint's pool and runtime settings.
    /// # Errors
    /// Rejects invalid auth, model or settings.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), decision_rs::Error> {
    /// let endpoint = decision_rs::Endpoint::system_one("http://127.0.0.1:8080/v1/systemone")?;
    /// let client = decision_rs::Client::for_endpoint_with(endpoint, decision_rs::Auth::None, "local-model".parse()?, decision_rs::HttpClient::new()?, decision_rs::Settings::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn for_endpoint_with(
        endpoint: Endpoint,
        auth: Auth,
        model: Model,
        http: HttpClient,
        settings: Settings,
    ) -> Result<Self, Error> {
        Self::construct(None, endpoint, auth, model, Some(http), settings)
    }
    /// The bound provider, or None for a custom endpoint.
    ///
    /// ```no_run
    /// # fn example(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let value = client.provider();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn provider(&self) -> Option<Provider> {
        self.inner.provider
    }
    /// Resolved default model for this client.
    ///
    /// ```no_run
    /// # fn example(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let value = client.model();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn model(&self) -> &Model {
        &self.inner.resolved_model
    }
    /// Cloneable shared connection pool.
    ///
    /// ```no_run
    /// # fn example(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let value = client.http();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn http(&self) -> &HttpClient {
        &self.inner.transport.http
    }
    /// Immutable validated settings.
    ///
    /// ```no_run
    /// # fn example(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let value = client.settings();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.inner.transport.settings
    }
    /// Ask a typed question set with the client's default model.
    /// # Errors
    /// Returns schema, transport, protocol or required-field refusal errors.
    ///
    /// ```no_run
    /// # async fn example<D: decision_rs::Decision>(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let answer: D = client.decide("Please refund me.").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn decide<D: Decision>(&self, input: impl IntoInput) -> Result<D, Error> {
        Ok(self
            .decide_with::<D>(RequestOptions::new(input)?)
            .await?
            .value)
    }
    /// Ask a typed question set and retain accounting.
    /// # Errors
    /// Returns schema, transport, protocol or required-field refusal errors.
    ///
    /// ```no_run
    /// # async fn example<D: decision_rs::Decision>(client: &decision_rs::Client) -> Result<(), decision_rs::Error> {
    /// let answer = client.decide_with::<D>(decision_rs::RequestOptions::new("Please refund me.")?).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn decide_with<D: Decision>(
        &self,
        options: RequestOptions,
    ) -> Result<Decided<D>, Error> {
        let response = self.execute(options, D::questions()?).await?;
        let value = D::from_answers(&response.answers).map_err(|error| match error {
            DecodeError::Refused(keys) if !keys.is_empty() => Error::Refused {
                keys,
                meta: Box::new(response.meta.clone()),
            },
            DecodeError::Refused(_) => Error::protocol("empty typed refusal mapping"),
            DecodeError::InvalidMapping { key } => {
                Error::for_question(&key, "invalid typed mapping")
            }
        })?;
        Ok(Decided {
            value,
            meta: response.meta,
        })
    }
    /// Ask a dynamic schema with provider-neutral answers and explicit refusals.
    /// # Errors
    /// Returns input, transport or response-validation errors.
    ///
    /// ```no_run
    /// # async fn example(client: &decision_rs::Client, questions: &decision_rs::Questions) -> Result<(), decision_rs::Error> {
    /// let response = client.ask("Please refund me.", questions).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ask(
        &self,
        input: impl IntoInput,
        questions: &Questions,
    ) -> Result<DecisionResponse, Error> {
        self.execute(RequestOptions::new(input)?, questions).await
    }
    async fn execute(
        &self,
        options: RequestOptions,
        questions: &Questions,
    ) -> Result<DecisionResponse, Error> {
        if options.input.as_str().len() > self.inner.transport.settings.limits.input_bytes {
            return Err(Error::InvalidInput("configured evidence limit"));
        }
        let requested = options
            .model
            .unwrap_or_else(|| self.inner.requested_model.clone());
        let (resolved, wire) = if requested == self.inner.requested_model {
            (
                self.inner.resolved_model.clone(),
                self.inner.wire_model.clone(),
            )
        } else {
            requested.resolve(self.inner.provider)?
        };
        let backend = self.inner.endpoint.backend;
        let plan = ResponsePlan { questions };
        let body = backend.prepare(
            &options.input,
            questions,
            &wire,
            self.inner.transport.settings.limits.request_bytes,
        )?;
        let request = self
            .inner
            .transport
            .request(&self.inner.endpoint, &self.inner.auth, body)?;
        let received = self.inner.transport.send(request, backend).await?;
        let decoded = backend.decode(&received.body, &plan)?;
        let meta = ResponseMeta {
            provider: self.inner.provider,
            requested_model: requested,
            resolved_model: resolved,
            wire_model: wire,
            returned_model: decoded.model,
            request_id: received.request_id,
            response_id: decoded.id,
            sdk_version: env!("CARGO_PKG_VERSION"),
            usage: decoded.usage,
            latency: received.latency,
            attempts: received.attempts,
        };
        tracing::debug!(provider = ?meta.provider, model = %meta.wire_model, questions = questions.len(), attempts = meta.attempts, latency_ms = meta.latency.as_millis(), input_tokens = meta.usage.input_tokens, output_tokens = meta.usage.output_tokens, request_id = ?meta.request_id, "decision completed");
        Ok(DecisionResponse {
            answers: decoded.answers,
            meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    fn environment(entries: &[(&str, &str)]) -> Result<Client, Error> {
        Client::from_environment(|name| {
            Ok(entries
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned()))
        })
    }
    #[rstest]
    #[case("openai", Provider::OpenAi)]
    #[case("openrouter", Provider::OpenRouter)]
    #[case("typesafe", Provider::TypeSafe)]
    fn test_should_read_decision_environment_variables(
        #[case] provider_name: &str,
        #[case] provider: Provider,
    ) {
        let client = environment(&[
            ("DECISION_PROVIDER", provider_name),
            ("DECISION_API_KEY", "application-loaded-key"),
            ("DECISION_MODEL", "custom-model"),
        ])
        .unwrap();
        assert_eq!(client.provider(), Some(provider));
        assert_eq!(client.model(), &"custom-model".parse::<Model>().unwrap());
    }
    #[test]
    fn test_should_reject_ambiguous_credentials() {
        assert!(matches!(
            environment(&[("OPENAI_API_KEY", "a"), ("OPENROUTER_API_KEY", "b")]),
            Err(Error::AmbiguousProvider)
        ));
        assert!(matches!(
            environment(&[("DECISION_API_KEY", "a")]),
            Err(Error::AmbiguousProvider)
        ));
    }
    #[test]
    fn test_should_use_only_selected_provider_credentials() {
        assert!(matches!(
            environment(&[("DECISION_PROVIDER", "openai"), ("OPENROUTER_API_KEY", "b")]),
            Err(Error::MissingCredentials)
        ));
        assert!(matches!(
            environment(&[
                ("DECISION_PROVIDER", "openai"),
                ("DECISION_API_KEY", " "),
                ("OPENAI_API_KEY", "b")
            ]),
            Err(Error::InvalidConfiguration(_))
        ));
        assert_eq!(
            environment(&[("OPENAI_API_KEY", "a")]).unwrap().provider(),
            Some(Provider::OpenAi)
        );
    }
}
