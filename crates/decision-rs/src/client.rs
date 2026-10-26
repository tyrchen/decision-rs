//! Provider-neutral orchestration. No wire DTOs are exposed to applications.

use std::{fmt, str::FromStr, sync::Arc};

use secrecy::SecretString;
use tokio::sync::Semaphore;

use crate::{
    Auth, CloudflareAccountId, Config, Decision, DecisionResponse, DecodeError, Endpoint, Error,
    HttpClient, Input, IntoInput, Model, Provider, Questions, ResponseMeta, Settings,
    backend::ResponsePlan,
    target::{HostedSelection, ResolvedCall, Target},
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
    target: Target,
    auth: Auth,
    requested_model: Model,
    resolved_model: Model,
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
    selection: HostedSelection,
    key: Option<SecretString>,
    model: Model,
    http: Option<HttpClient>,
    settings: Settings,
}
impl ClientBuilder {
    /// Bind a Cloudflare account. Other providers reject this setting at build time.
    ///
    /// ```no_run
    /// let account = "0123456789abcdef0123456789abcdef".parse()?;
    /// let builder = decision_rs::Client::builder(decision_rs::Provider::Cloudflare)
    ///     .cloudflare_account(account);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn cloudflare_account(mut self, account: CloudflareAccountId) -> Self {
        self.selection.account = Some(account);
        self
    }

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
        let provider = self.selection.provider;
        let target = self.selection.into_target()?;
        Client::construct(
            Some(provider),
            target,
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
        target: Target,
        auth: Auth,
        model: Model,
        http: Option<HttpClient>,
        settings: Settings,
    ) -> Result<Self, Error> {
        settings.validate()?;
        auth.header()?;
        let resolved_model = target.resolve(&model, provider)?.model;
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
                target,
                auth,
                requested_model: model,
                resolved_model,
                transport,
            }),
        })
    }
    /// Construct a hosted client using its provider-aware default model.
    /// # Errors
    /// Rejects an invalid key, missing Cloudflare account, or HTTP initialization failure.
    /// Use `Client::cloudflare` or the builder to provide a Cloudflare account.
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
            selection: HostedSelection {
                provider,
                account: None,
            },
            key: None,
            model: Model::Default,
            http: None,
            settings: Settings::default(),
        }
    }
    /// Connect to Cloudflare Workers AI using its default Clef model.
    /// # Errors
    /// Rejects an invalid token or HTTP initialization failure.
    ///
    /// ```no_run
    /// let account = "0123456789abcdef0123456789abcdef".parse()?;
    /// let client = decision_rs::Client::cloudflare(account, "application-loaded-token")?;
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn cloudflare(
        account: CloudflareAccountId,
        token: impl Into<SecretString>,
    ) -> Result<Self, Error> {
        Self::builder(Provider::Cloudflare)
            .cloudflare_account(account)
            .api_key(token)
            .build()
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
        config.validate()?;
        let mut builder = Self::builder(config.provider)
            .api_key(key)
            .model(config.model)
            .settings(config.settings);
        builder.selection.account = config.cloudflare_account_id;
        builder.build()
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
            for provider in [
                Provider::OpenAi,
                Provider::OpenRouter,
                Provider::TypeSafe,
                Provider::Cloudflare,
            ] {
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
        let mut config = Config::new(provider);
        config.model = model;
        if provider == Provider::Cloudflare {
            config.cloudflare_account_id = read("CLOUDFLARE_ACCOUNT_ID")?
                .map(|value| value.parse())
                .transpose()?;
        }
        Self::from_config(config, key)
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
        Self::construct(
            None,
            Target::Fixed(endpoint),
            auth,
            model,
            Some(http),
            settings,
        )
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
        let call = self.inner.target.resolve(&requested, self.inner.provider)?;
        self.execute_call(call, requested, &options.input, questions)
            .await
    }
    async fn execute_call(
        &self,
        call: ResolvedCall,
        requested: Model,
        input: &Input,
        questions: &Questions,
    ) -> Result<DecisionResponse, Error> {
        let backend = call.endpoint.backend;
        let plan = ResponsePlan { questions };
        let body = backend.prepare(
            input,
            questions,
            &call.wire_model,
            self.inner.transport.settings.limits.request_bytes,
        )?;
        let request = self
            .inner
            .transport
            .request(&call.endpoint, &self.inner.auth, body)?;
        let received = self.inner.transport.send(request, backend).await?;
        let decoded = backend.decode(&received.body, &plan)?;
        let meta = ResponseMeta {
            provider: self.inner.provider,
            requested_model: requested,
            resolved_model: call.model,
            wire_model: call.wire_model,
            routing_model: call.routing_model,
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

#[cfg(test)]
mod cloudflare_tests {
    use std::time::Duration;

    use rstest::rstest;
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    use super::*;
    #[cfg(feature = "batch")]
    use crate::run_cases;
    use crate::{Answer, CaseSet, RetryConfig, backend::Backend};

    const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
    fn hosted(settings: Settings) -> Client {
        Client::builder(Provider::Cloudflare)
            .cloudflare_account(ACCOUNT.parse().unwrap())
            .api_key("fixture-token")
            .settings(settings)
            .build()
            .unwrap()
    }
    fn environment(entries: &[(&str, &str)]) -> Result<Client, Error> {
        Client::from_environment(|name| {
            Ok(entries
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned()))
        })
    }
    #[test]
    fn test_should_validate_cloudflare_constructors_without_environment_fallback() {
        assert!(matches!(
            Client::new(Provider::Cloudflare, "token"),
            Err(Error::InvalidConfiguration("missing Cloudflare account ID"))
        ));
        assert!(
            Client::builder(Provider::OpenAi)
                .cloudflare_account(ACCOUNT.parse().unwrap())
                .api_key("token")
                .build()
                .is_err()
        );
        assert!(Client::from_config(Config::new(Provider::Cloudflare), "token").is_err());
        let client = Client::cloudflare(ACCOUNT.parse().unwrap(), "token").unwrap();
        assert_eq!(client.provider(), Some(Provider::Cloudflare));
        assert_eq!(client.model(), &Model::Clef);
        assert!(!format!("{client:?}").contains(ACCOUNT));
        assert!(
            !format!(
                "{:?}",
                Client::builder(Provider::Cloudflare)
                    .cloudflare_account(ACCOUNT.parse().unwrap())
                    .api_key("hidden-token")
            )
            .contains("hidden-token")
        );
    }
    #[test]
    fn test_should_resolve_cloudflare_environment_deterministically() {
        let inferred = environment(&[
            ("CLOUDFLARE_API_TOKEN", "token"),
            ("CLOUDFLARE_ACCOUNT_ID", ACCOUNT),
        ])
        .unwrap();
        assert_eq!(inferred.provider(), Some(Provider::Cloudflare));
        let explicit = environment(&[
            ("DECISION_PROVIDER", "cloudflare"),
            ("DECISION_API_KEY", "generic"),
            ("CLOUDFLARE_API_TOKEN", "ignored"),
            ("CLOUDFLARE_ACCOUNT_ID", ACCOUNT),
            ("DECISION_MODEL", "@cf/cloudflare/clef-flash"),
        ])
        .unwrap();
        assert_eq!(explicit.model(), &Model::ClefFlash);
        assert_eq!(
            explicit.inner.auth.header().unwrap().unwrap(),
            "Bearer generic"
        );
        assert!(matches!(
            environment(&[("CLOUDFLARE_ACCOUNT_ID", ACCOUNT)]),
            Err(Error::MissingCredentials)
        ));
        assert!(matches!(
            environment(&[("CLOUDFLARE_API_TOKEN", "token")]),
            Err(Error::InvalidConfiguration(_))
        ));
        assert!(matches!(
            environment(&[
                ("CLOUDFLARE_API_TOKEN", "token"),
                ("OPENAI_API_KEY", "other")
            ]),
            Err(Error::AmbiguousProvider)
        ));
        assert!(matches!(
            environment(&[
                ("DECISION_API_KEY", "token"),
                ("CLOUDFLARE_ACCOUNT_ID", ACCOUNT)
            ]),
            Err(Error::AmbiguousProvider)
        ));
        assert!(matches!(
            environment(&[
                ("DECISION_PROVIDER", "cloudflare"),
                ("OPENAI_API_KEY", "other"),
                ("CLOUDFLARE_ACCOUNT_ID", ACCOUNT)
            ]),
            Err(Error::MissingCredentials)
        ));
        for (name, value) in [
            ("CLOUDFLARE_API_TOKEN", " "),
            ("CLOUDFLARE_ACCOUNT_ID", " "),
            ("DECISION_MODEL", " "),
        ] {
            let mut entries = vec![
                ("DECISION_PROVIDER", "cloudflare"),
                ("CLOUDFLARE_API_TOKEN", "token"),
                ("CLOUDFLARE_ACCOUNT_ID", ACCOUNT),
            ];
            entries.retain(|(key, _)| *key != name);
            entries.push((name, value));
            assert!(environment(&entries).is_err());
        }
        let other = environment(&[
            ("DECISION_PROVIDER", "openai"),
            ("OPENAI_API_KEY", "token"),
            ("CLOUDFLARE_API_TOKEN", " "),
            ("CLOUDFLARE_ACCOUNT_ID", "bad"),
        ])
        .unwrap();
        assert_eq!(other.provider(), Some(Provider::OpenAi));
    }
    fn schema() -> Questions {
        Questions::builder()
            .predicate("p", "Condition?")
            .predicate_with("refund", "Refund?", "Money back", "No refund")
            .choice("team", "Team?", [("a", "Team A"), ("b", "Team B")])
            .score("urgency", "Urgency?", [("low", "Low"), ("high", "High")])
            .build()
            .unwrap()
    }
    // Synthetic REST envelope, including deliberately different confidence/probability.
    fn response(model: &str) -> Value {
        json!({"success":true,"errors":[],"result":{"id":"body-id","model":model,"answers":{
            "urgency":{"type":"score","score":0.4,"confidence":0.8,"probabilities":{"1":0.4,"0":0.6},"legend":{"0":"Low","1":"High"}},
            "team":{"type":"choice","choice":"a","confidence":0.9,"probabilities":{"b":0.3,"a":0.7}},
            "refund":{"type":"noul","noul":0.2},"p":{"type":"noul","noul":0.8}},
            "usage":{"input_tokens":24,"output_tokens":0}}})
    }
    fn local_call(client: &Client, model: &Model, server: &MockServer) -> ResolvedCall {
        let mut call = client
            .inner
            .target
            .resolve(model, client.provider())
            .unwrap();
        // Only remap the origin in this private test. Preserve the production resolver's path.
        let url = format!("{}{}", server.uri(), call.endpoint.url.path());
        call.endpoint = Endpoint::new(&url, call.endpoint.backend).unwrap();
        call
    }
    async fn send_model(
        client: &Client,
        server: &MockServer,
        model: Model,
    ) -> Result<DecisionResponse, Error> {
        let call = local_call(client, &model, server);
        let input = Input::json(&json!({"z":2,"a":1})).unwrap();
        client.execute_call(call, model, &input, &schema()).await
    }
    async fn mount_model(server: &MockServer, model: &str) {
        Mock::given(method("POST")).and(path(format!("/client/v4/accounts/{ACCOUNT}/ai/run/@cf/cloudflare/{model}")))
            .and(header("authorization", "Bearer fixture-token"))
            .and(header("content-type", "application/json"))
            .and(body_partial_json(json!({"model":model,"state":"{\"a\":1,\"z\":2}","questions":{
                "p":{"type":"noul"}, "refund":{"criteria":{"true":"Money back","false":"No refund"}},
                "team":{"type":"choice","criteria":{"a":"Team A","b":"Team B"}}, "urgency":{"type":"score","criteria":["Low","High"]}}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(response(model)).insert_header("cf-ray", "ray-id"))
            .mount(server).await;
    }
    #[tokio::test]
    async fn test_should_keep_concurrent_clef_routes_and_metadata_isolated() {
        let server = MockServer::start().await;
        for name in ["clef", "clef-flash", "clef-omni"] {
            mount_model(&server, name).await;
        }
        let client = hosted(Settings::default());
        let clone = client.clone();
        assert!(Arc::ptr_eq(&client.inner, &clone.inner));
        let (flash, omni) = tokio::join!(
            send_model(&client, &server, Model::ClefFlash),
            send_model(&clone, &server, Model::ClefOmni)
        );
        for (result, name) in [
            (flash.unwrap(), "clef-flash"),
            (omni.unwrap(), "clef-omni"),
            (
                send_model(&client, &server, Model::Default).await.unwrap(),
                "clef",
            ),
        ] {
            assert_eq!(result.meta.wire_model, name);
            assert_eq!(result.meta.returned_model, name);
            assert_eq!(
                result.meta.routing_model,
                Some(format!("@cf/cloudflare/{name}"))
            );
            assert_eq!(result.meta.provider, Some(Provider::Cloudflare));
            assert_eq!(result.meta.request_id.as_deref(), Some("ray-id"));
            assert_eq!(result.meta.response_id.as_deref(), Some("body-id"));
            assert_eq!(result.meta.attempts, 1);
            assert_eq!(result.meta.usage.reported_cost_usd, None);
            assert_eq!(
                result
                    .answers
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>(),
                ["p", "refund", "team", "urgency"]
            );
            let Some(Answer::Choice(choice)) = result.answers.get("team") else {
                panic!("choice expected");
            };
            assert_eq!(choice.confidence().unwrap().get(), 0.9);
            assert_eq!(choice.probabilities().get("a").unwrap().get(), 0.7);
        }
        assert_eq!(client.model(), &Model::Clef);
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }
    #[rstest]
    #[case(3036, false)]
    #[case(3040, true)]
    #[tokio::test]
    async fn test_should_retry_capacity_but_not_exhausted_cloudflare_quota(
        #[case] code: u32,
        #[case] transient: bool,
    ) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_json(json!({"errors":[{"code":code,"message":"redacted"}]})),
            )
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        mount_model(&server, "clef").await;
        let client = hosted(Settings::default());
        let result = send_model(&client, &server, Model::Default).await;
        if transient && cfg!(feature = "retry") {
            assert_eq!(result.unwrap().meta.attempts, 2);
            assert_eq!(server.received_requests().await.unwrap().len(), 2);
        } else {
            assert!(
                matches!(result, Err(Error::QuotaExceeded))
                    || transient && matches!(result, Err(Error::RateLimited { .. }))
            );
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn test_should_apply_cloudflare_header_precedence_and_limits() {
        let server = MockServer::start().await;
        let client = hosted(Settings::default());
        for (headers, valid, expected) in [
            (
                vec![("x-request-id", "primary"), ("cf-ray", "secondary")],
                true,
                Some("primary"),
            ),
            (vec![], true, None),
            (vec![("cf-ray", "")], false, None),
            (
                vec![("x-request-id", ""), ("cf-ray", "secondary")],
                false,
                None,
            ),
        ] {
            server.reset().await;
            let mut template = ResponseTemplate::new(200).set_body_json(response("clef"));
            for (key, value) in headers {
                template = template.insert_header(key, value);
            }
            Mock::given(method("POST"))
                .respond_with(template)
                .mount(&server)
                .await;
            let result = send_model(&client, &server, Model::Default).await;
            if valid {
                assert_eq!(result.unwrap().meta.request_id.as_deref(), expected);
            } else {
                assert!(matches!(result, Err(Error::Protocol { .. })));
            }
        }
        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(response("clef"))
                    .insert_header("cf-ray", "x".repeat(257)),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            send_model(&client, &server, Model::Default).await,
            Err(Error::Protocol { .. })
        ));
    }
    #[tokio::test]
    async fn test_should_bound_cloudflare_wait_body_and_unsuccessful_envelopes() {
        let server = MockServer::start().await;
        let settings = Settings {
            timeout_ms: 40,
            max_inflight: 1,
            ..Settings::default()
        };
        let client = hosted(settings);
        let permit = client.inner.transport.permits.acquire().await.unwrap();
        assert!(matches!(
            send_model(&client, &server, Model::Default).await,
            Err(Error::Timeout)
        ));
        assert!(server.received_requests().await.unwrap().is_empty());
        drop(permit);
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(response("clef"))
                    .set_delay(Duration::from_millis(200)),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            send_model(&client, &server, Model::Default).await,
            Err(Error::Timeout)
        ));
        server.reset().await;
        let mut settings = Settings::default();
        settings.limits.response_bytes = 100;
        let small = hosted(settings);
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(101)))
            .mount(&server)
            .await;
        assert!(matches!(
            send_model(&small, &server, Model::Default).await,
            Err(Error::Protocol { .. })
        ));
        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"success":false,"errors":[{"code":3040}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(matches!(
            send_model(&hosted(Settings::default()), &server, Model::Default).await,
            Err(Error::Protocol { .. })
        ));
    }
    #[tokio::test]
    async fn test_should_reject_clef_invalid_requests_before_network() {
        let mut settings = Settings::default();
        settings.limits.input_bytes = 1;
        let client = hosted(settings);
        assert!(matches!(
            client.ask("too large", &schema()).await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            client
                .execute(
                    RequestOptions::new("x").unwrap().model(Model::Jev113),
                    &schema()
                )
                .await,
            Err(Error::UnsupportedModel)
        ));
    }
    #[tokio::test]
    async fn test_should_run_clef_cases_through_the_shared_codec() {
        let server = MockServer::start().await;
        let endpoint =
            Endpoint::new(&format!("{}/decisions", server.uri()), Backend::Cloudflare).unwrap();
        let settings = Settings {
            retry: RetryConfig::disabled(),
            ..Settings::default()
        };
        let client = Client::construct(
            Some(Provider::Cloudflare),
            Target::Fixed(endpoint),
            Auth::ApiKey("fixture-token".into()),
            Model::Default,
            None,
            settings,
        )
        .unwrap();
        let case_document = json!({"version":2,"cases":[
            {"id":"one","input":"refund","questions":{"p":{"type":"predicate","instructions":"Condition?"}}},
            {"id":"two","input":"other","questions":{"p":{"type":"predicate","instructions":"Condition?"}}}
        ]});
        let cases = CaseSet::from_json_str(&case_document.to_string()).unwrap();
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":{"model":"clef","answers":{"p":{"type":"noul","noul":0.8}},"usage":{"input_tokens":4,"output_tokens":0}}}))).mount(&server).await;
        let case = cases.iter().next().unwrap();
        let result = client.run_case(case).await.unwrap();
        assert_eq!(result.id, "one");
        #[cfg(feature = "batch")]
        {
            let results = run_cases(&client, &cases, 2).await.unwrap();
            assert_eq!(results.len(), 2);
            assert_eq!(
                results
                    .into_iter()
                    .map(|result| result.unwrap().id)
                    .collect::<Vec<_>>(),
                ["one", "two"]
            );
        }
    }
    #[tokio::test]
    async fn test_should_not_forward_cloudflare_auth_on_redirect() {
        let server = MockServer::start().await;
        let destination = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", destination.uri()))
            .expect(1)
            .mount(&server)
            .await;
        let result = send_model(&hosted(Settings::default()), &server, Model::Default).await;
        assert!(matches!(result, Err(Error::Api { status: 302, .. })));
        assert!(destination.received_requests().await.unwrap().is_empty());
    }
}
