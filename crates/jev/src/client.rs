//! `Client`: the async HTTP client, retry loop, and request-building logic.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, instrument, warn};

use crate::{
    answer::Answers,
    cases::{Case, CaseResult},
    config::{Config, Surface},
    error::Error,
    model::Model,
    question::Questions,
    state::State,
    typed::{Choice, ChoiceOptions, Decision, Noul, Score, ScoreLevels},
    util::estimate_tokens,
    wire::{DecisionRequest, Response, ResponseMeta, TraceConfig},
};

/// Maximum bytes read from a response body before parsing.
///
/// This bounds memory use against a hostile or misbehaving gateway; it is well
/// above any legitimate Jev response, which is a small JSON object.
const MAX_RESPONSE_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Environment variable read first by [`Client::from_env`].
const ENV_API_KEY: &str = "JEV_API_KEY";
/// Fallback environment variable read by [`Client::from_env`].
const ENV_API_KEY_FALLBACK: &str = "OPENROUTER_API_KEY";
/// Optional environment variable overriding the API base URL.
const ENV_BASE_URL: &str = "JEV_BASE_URL";

/// Default API base URL (scheme + host), shared by both surfaces.
const DEFAULT_API_BASE: &str = "https://openrouter.ai";

/// A typed decision plus the response envelope: id, model snapshot, usage, and cost.
///
/// [`Client::decide`] is the 90% API and deliberately drops this envelope;
/// [`Client::decide_with`] returns `Decided<D>` for callers that record cost or the
/// dated model snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Decided<D> {
    /// The typed decision.
    pub value: D,
    /// The response envelope: id, model snapshot, usage, cost, latency.
    pub meta: ResponseMeta,
}

/// Per-call request options for [`Client::decide_with`] and [`Client::raw`].
///
/// Fields left unset fall back to the [`Client`]'s configured defaults
/// (`with_model`, `with_session_id`, `with_user`).
#[derive(Debug, Clone)]
pub struct RequestOptions<'a> {
    state: State,
    questions: Option<&'a Questions>,
    model: Option<Model>,
    session_id: Option<String>,
    user: Option<String>,
    trace: Option<TraceConfig>,
}

impl<'a> RequestOptions<'a> {
    /// Starts a `RequestOptions` for the given state.
    #[must_use]
    pub fn new(state: impl Into<State>) -> Self {
        RequestOptions {
            state: state.into(),
            questions: None,
            model: None,
            session_id: None,
            user: None,
            trace: None,
        }
    }

    /// Sets the questions to send. Required for [`Client::raw`]; ignored by
    /// [`Client::decide_with`], which always uses `D::questions()`.
    #[must_use]
    pub fn with_questions(mut self, questions: &'a Questions) -> Self {
        self.questions = Some(questions);
        self
    }

    /// Overrides the client's configured model for this call only.
    #[must_use]
    pub fn with_model(mut self, model: Model) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets a session identifier grouping this request with related ones
    /// (Decisions surface only; dropped on System One).
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Sets an application-defined user identifier (Decisions surface only; dropped
    /// on System One).
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Sets opaque trace configuration (Decisions surface only; dropped on System One).
    #[must_use]
    pub fn with_trace(mut self, trace: TraceConfig) -> Self {
        self.trace = Some(trace);
        self
    }
}

/// Async client for the Jev decision model.
///
/// `Client` is cheap to clone (an `Arc`-backed `reqwest::Client` plus one more
/// `Arc`), so it drops straight into shared application state — a `OnceLock`,
/// `Arc<Client>`, or framework extension.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> Result<(), jev::Error> {
/// use jev::Client;
///
/// let client = Client::from_env()?;
/// let questions = jev::Questions::builder()
///     .noul("refund", "Is the customer asking for money back?")
///     .build()
///     .unwrap();
/// let response = client.ask("a support ticket about a refund", &questions).await?;
/// println!("{}", response.answers.noul("refund")?);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: Option<SecretString>,
    config: Arc<Config>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let api_key: &dyn std::fmt::Debug = if self.api_key.is_some() {
            &"[REDACTED]"
        } else {
            &"none"
        };
        f.debug_struct("Client")
            .field("api_base", &self.api_base())
            .field("model", &self.config.model)
            .field("surface", &self.config.surface)
            .field("api_key", api_key)
            // `http` is deliberately omitted: `reqwest::Client`'s own `Debug` output
            // is an internal connection-pool dump, not useful application diagnostics.
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Builds a client with its own connection pool, using whichever TLS backend
    /// feature is enabled (`rustls-tls` by default).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when neither the `rustls-tls` nor `native-tls`
    /// feature is enabled: with no TLS backend compiled in, this constructor cannot
    /// build an HTTPS-capable `reqwest::Client`. Use [`Client::with_http`] with a
    /// caller-supplied client in that configuration.
    pub fn new(api_key: impl Into<SecretString>) -> Result<Self, Error> {
        Self::from_config(
            build_http_client(Config::default().timeout)?,
            api_key,
            Config::default(),
        )
    }

    /// Builds a client that sends no `Authorization` header at all, for a System
    /// One-compatible backend a caller hosts themselves.
    ///
    /// Some self-hosted backends (a local model server, a proxy that injects its
    /// own credentials, a demo endpoint that takes no key) neither expect nor accept
    /// a bearer token; every other `Client` constructor sends one unconditionally.
    /// `base_url` may use `http://` for a loopback address without the opt-in
    /// [`Client::with_api_base`] otherwise requires, since a backend a caller hosts
    /// on their own machine is exactly the case that check exists to allow.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when neither the `rustls-tls` nor `native-tls`
    /// feature is enabled (see [`Client::new`]), or when `base_url` fails to parse
    /// or uses a non-`https`, non-loopback scheme.
    ///
    /// # Examples
    ///
    /// ```
    /// use jev::Client;
    ///
    /// # fn run() -> Result<(), jev::Error> {
    /// let client = Client::without_api_key("http://127.0.0.1:8099")?;
    /// # let _ = client;
    /// # Ok(())
    /// # }
    /// ```
    pub fn without_api_key(base_url: &str) -> Result<Self, Error> {
        let http = build_http_client(Config::default().timeout)?;
        let client = Self {
            http,
            api_key: None,
            config: Arc::new(Config::default()),
        };
        client.with_api_base(base_url)
    }

    /// Builds a client that shares a caller-owned `reqwest::Client` and connection
    /// pool. This is the right choice inside a service that already owns an HTTP
    /// client, and the only way to supply a custom TLS configuration (for example a
    /// pinned internal CA), or to build with neither TLS feature enabled.
    pub fn with_http(http: reqwest::Client, api_key: impl Into<SecretString>) -> Self {
        Self {
            http,
            api_key: Some(api_key.into()),
            config: Arc::new(Config::default()),
        }
    }

    /// Builds a client from a caller-supplied HTTP client, API key, and [`Config`],
    /// for the config-file path.
    ///
    /// # Errors
    ///
    /// This constructor itself is infallible; it is fallible only so it can share an
    /// implementation with [`Client::new`], which validates its own `http` build.
    pub fn from_config(
        http: reqwest::Client,
        api_key: impl Into<SecretString>,
        config: Config,
    ) -> Result<Self, Error> {
        Ok(Self {
            http,
            api_key: Some(api_key.into()),
            config: Arc::new(config),
        })
    }

    /// Builds a client from `JEV_API_KEY` (falling back to `OPENROUTER_API_KEY`) and
    /// an optional `JEV_BASE_URL`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MissingApiKey`] when neither environment variable is set, or
    /// [`Error::Invalid`] when `JEV_BASE_URL` is set but not a valid `https://` URL
    /// (or `http://` explicitly opted into; see [`Client::with_api_base`]).
    pub fn from_env() -> Result<Self, Error> {
        let api_key = std::env::var(ENV_API_KEY)
            .or_else(|_| std::env::var(ENV_API_KEY_FALLBACK))
            .map_err(|_| Error::MissingApiKey)?;
        let mut client = Self::new(SecretString::from(api_key))?;
        if let Ok(base_url) = std::env::var(ENV_BASE_URL) {
            client = client.with_api_base(&base_url)?;
        }
        Ok(client)
    }

    /// Overrides the model sent on every request made by this client.
    #[must_use]
    pub fn with_model(mut self, model: Model) -> Self {
        self.config_mut().model = model;
        self
    }

    /// Overrides which HTTP surface this client calls.
    #[must_use]
    pub fn with_surface(mut self, surface: Surface) -> Self {
        self.config_mut().surface = surface;
        self
    }

    /// Overrides the API base URL (scheme + host).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when `base_url` fails to parse, or uses a scheme
    /// other than `https` (`http` is accepted only for local mocks; see the crate's
    /// security posture docs).
    pub fn with_api_base(mut self, base_url: &str) -> Result<Self, Error> {
        let url = url::Url::parse(base_url).map_err(|e| Error::Invalid {
            field: "api_base".to_string(),
            reason: e.to_string(),
        })?;
        if url.scheme() != "https" && !is_local_http_opt_in(&url) {
            return Err(Error::Invalid {
                field: "api_base".to_string(),
                reason: "must use https (http is only accepted for loopback addresses)".to_string(),
            });
        }
        self.config_mut().api_base = Some(url);
        Ok(self)
    }

    /// Overrides the per-request timeout (an overall deadline across all retry attempts).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config_mut().timeout = timeout;
        self
    }

    /// Overrides the retry policy.
    #[must_use]
    pub fn with_retry(mut self, retry: crate::config::RetryConfig) -> Self {
        self.config_mut().retry = retry;
        self
    }

    /// Enables or disables the pre-flight context-size estimate.
    #[must_use]
    pub fn with_context_check(mut self, enabled: bool) -> Self {
        self.config_mut().context_check = enabled;
        self
    }

    /// Sets a default session identifier applied to every request unless overridden
    /// per-call by [`RequestOptions::with_session_id`].
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.config_mut().default_session_id = Some(session_id.into());
        self
    }

    /// Sets a default user identifier applied to every request unless overridden
    /// per-call by [`RequestOptions::with_user`].
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.config_mut().default_user = Some(user.into());
        self
    }

    /// Sets the named probability thresholds this client carries for callers that
    /// want to read them back via [`Client::thresholds`].
    #[must_use]
    pub fn with_thresholds(mut self, thresholds: crate::config::Thresholds) -> Self {
        self.config_mut().thresholds = thresholds;
        self
    }

    /// The configured probability thresholds.
    #[must_use]
    pub fn thresholds(&self) -> &crate::config::Thresholds {
        &self.config.thresholds
    }

    /// The effective API base URL.
    #[must_use]
    pub fn api_base(&self) -> url::Url {
        self.config.api_base.clone().unwrap_or_else(|| {
            url::Url::parse(DEFAULT_API_BASE).unwrap_or_else(|_| unreachable_url())
        })
    }

    /// Typed path: sends `D::questions()`, parses the answers back into `D`.
    ///
    /// This is the 90% API. It deliberately drops the response envelope (id, usage,
    /// cost); use [`Client::decide_with`] when that data is needed.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the underlying request, or from [`Decision::from_answers`]
    /// if the server's answers do not match `D`'s shape.
    pub async fn decide<D: Decision>(&self, state: impl Into<State>) -> Result<D, Error> {
        Ok(self
            .decide_with::<D>(RequestOptions::new(state))
            .await?
            .value)
    }

    /// Typed path with per-call knobs (session, user, trace, model override).
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the underlying request, or from [`Decision::from_answers`].
    pub async fn decide_with<D: Decision>(
        &self,
        options: RequestOptions<'_>,
    ) -> Result<Decided<D>, Error> {
        let questions = D::questions();
        let (response, latency) = self.send(options, questions).await?;
        let meta = ResponseMeta::from_response(&response, latency);
        let value = D::from_answers(&response.answers)?;
        Ok(Decided { value, meta })
    }

    /// Dynamic path: sends caller-supplied `questions`, returns the raw answer map.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the underlying request.
    pub async fn ask(
        &self,
        state: impl Into<State>,
        questions: &Questions,
    ) -> Result<Response, Error> {
        self.send(RequestOptions::new(state), questions)
            .await
            .map(|(response, _)| response)
    }

    /// Single-question convenience: asks one `noul` question and returns the typed
    /// [`Noul`] answer directly, without a [`Decision`] struct.
    ///
    /// For a one-off yes/no gate that does not warrant its own struct. For more than
    /// one question in a single request, or to keep the question and its criteria
    /// next to a named type, use `#[derive(Decision)]` or [`Client::ask`] instead.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] if `instructions` fails [`Questions`]'s validation (empty,
    /// oversized); otherwise any [`Error`] from the underlying request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn run(client: &jev::Client) -> Result<(), jev::Error> {
    /// let is_refund = client.noul("Please refund my order.", "Is this a refund request?").await?;
    /// println!("{is_refund}"); // Display: "yes, 88% sure"
    /// # Ok(())
    /// # }
    /// ```
    pub async fn noul(
        &self,
        state: impl Into<State>,
        instructions: impl Into<crate::guidance::Guidance>,
    ) -> Result<Noul, Error> {
        const KEY: &str = "noul";
        let questions = Questions::builder().noul(KEY, instructions).build()?;
        let response = self.ask(state, &questions).await?;
        Noul::try_new(response.answers.noul(KEY)?)
    }

    /// Single-question convenience: asks one `choice` question over `T::options()`
    /// and returns the typed [`Choice<T>`] answer directly, without a [`Decision`]
    /// struct.
    ///
    /// `T` supplies both the wire criteria (`T::options()`) and the typed result, the
    /// same as a `Choice<T>` field on a `#[derive(Decision)]` struct would.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] if `instructions` fails validation; [`Error::UnknownChoice`]
    /// if the server picks an option outside `T`; otherwise any [`Error`] from the
    /// underlying request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use jev::ChoiceOptions;
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
    /// enum Team {
    ///     /// Billing issues.
    ///     Payments,
    ///     /// UI issues.
    ///     Frontend,
    /// }
    ///
    /// # async fn run(client: &jev::Client) -> Result<(), jev::Error> {
    /// let team = client.choice::<Team>("a checkout ticket", "Which team owns this?").await?;
    /// println!("{:?}", team.value());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn choice<T: ChoiceOptions>(
        &self,
        state: impl Into<State>,
        instructions: impl Into<crate::guidance::Guidance>,
    ) -> Result<Choice<T>, Error> {
        const KEY: &str = "choice";
        let questions = Questions::builder()
            .choice(KEY, instructions, T::options().to_vec())
            .build()?;
        let response = self.ask(state, &questions).await?;
        let answer = response.answers.choice(KEY)?;
        Choice::try_new(
            KEY,
            &answer.choice,
            answer.confidence,
            answer.probabilities.clone(),
        )
    }

    /// Single-question convenience: asks one `score` question over `T::levels()` and
    /// returns the typed [`Score<T>`] answer directly, without a [`Decision`] struct.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] if `instructions` fails validation; [`Error::ScoreRange`] if
    /// the server's score falls outside `T`'s levels; otherwise any [`Error`] from the
    /// underlying request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use jev::ScoreLevels;
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, ScoreLevels)]
    /// enum Urgency {
    ///     /// Can wait.
    ///     Low,
    ///     /// Blocking revenue.
    ///     High,
    /// }
    ///
    /// # async fn run(client: &jev::Client) -> Result<(), jev::Error> {
    /// let urgency = client.score::<Urgency>("a checkout ticket", "How urgent is this?").await?;
    /// println!("{:?}", urgency.nearest());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn score<T: ScoreLevels>(
        &self,
        state: impl Into<State>,
        instructions: impl Into<crate::guidance::Guidance>,
    ) -> Result<Score<T>, Error> {
        const KEY: &str = "score";
        let questions = Questions::builder()
            .score(KEY, instructions, T::levels().to_vec())
            .build()?;
        let response = self.ask(state, &questions).await?;
        let answer = response.answers.score(KEY)?;
        Score::try_new(
            KEY,
            answer.score,
            answer.confidence,
            answer.probabilities.clone(),
        )
    }

    /// Runs one predefined [`Case`]: builds its questions, sends its state, and
    /// returns the response tagged with the case's `id`.
    ///
    /// This is the config-file counterpart of [`Client::ask`]: `ask` takes questions
    /// assembled in code, `run_case` takes both the state and the questions from a
    /// [`Case`] loaded via [`crate::CaseSet::from_yaml_str`] /
    /// [`crate::CaseSet::from_json_str`] / [`crate::CaseSet::from_file`]. For many
    /// cases at once with bounded concurrency, see [`crate::run_cases`] (the `batch`
    /// feature).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] if `case.questions` fails validation (this should not
    /// happen for a `Case` obtained from a successfully loaded [`crate::CaseSet`],
    /// which validates eagerly at load time); otherwise any [`Error`] from the
    /// underlying request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn run(client: &jev::Client, case: &jev::Case) -> Result<(), jev::Error> {
    /// let result = client.run_case(case).await?;
    /// println!("{}: {:?}", result.id, result.response.answers);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_case(&self, case: &Case) -> Result<CaseResult, Error> {
        let questions = case.build_questions()?;
        let response = self.ask(case.state.clone(), &questions).await?;
        Ok(CaseResult {
            id: case.id.clone(),
            response,
        })
    }

    /// Escape hatch: full request in ([`RequestOptions::with_questions`] required),
    /// full [`Response`] out.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] if `options` has no questions set; otherwise any [`Error`]
    /// from the underlying request.
    pub async fn raw(&self, options: RequestOptions<'_>) -> Result<Response, Error> {
        let questions = options.questions.ok_or_else(|| Error::Invalid {
            field: "questions".to_string(),
            reason: "Client::raw requires RequestOptions::with_questions".to_string(),
        })?;
        self.send(options, questions)
            .await
            .map(|(response, _)| response)
    }

    /// Mutable access to this client's private `Config` copy, cloning the `Arc`'s
    /// contents on first write (copy-on-write via `Arc::make_mut`).
    fn config_mut(&mut self) -> &mut Config {
        Arc::make_mut(&mut self.config)
    }

    /// Sends one decision request, applying the context-size guard and the retry
    /// loop, and returns the parsed response plus the measured latency.
    #[instrument(
        skip(self, options, questions),
        fields(
            model,
            surface = ?self.config.surface,
            questions = questions.len(),
            attempt,
            request_id,
            input_tokens,
            cost,
            latency_ms
        )
    )]
    async fn send(
        &self,
        options: RequestOptions<'_>,
        questions: &Questions,
    ) -> Result<(Response, Duration), Error> {
        let model = options.model.unwrap_or_else(|| self.config.model.clone());
        let wire_model = model.wire_id(self.config.surface).into_owned();
        tracing::Span::current().record("model", tracing::field::display(&wire_model));

        let session_id = options
            .session_id
            .or_else(|| self.config.default_session_id.clone());
        let user = options.user.or_else(|| self.config.default_user.clone());
        let trace = options.trace;

        if self.config.context_check {
            Self::check_context_size(&model, &options.state, questions)?;
        }

        let url = self.request_url();
        let start = Instant::now();
        let result = self
            .send_with_retry(
                &url,
                &wire_model,
                &options.state,
                questions,
                session_id,
                user,
                trace,
            )
            .await;
        let latency = start.elapsed();
        tracing::Span::current().record(
            "latency_ms",
            u64::try_from(latency.as_millis()).unwrap_or(u64::MAX),
        );

        let response = result?;
        tracing::Span::current().record("input_tokens", response.usage.input_tokens);
        if let Some(cost) = response.usage.cost {
            tracing::Span::current().record("cost", cost);
        }
        warn_on_low_confidence(&response.answers);

        Ok((response, latency))
    }

    /// The context-size guard: rejects a request before spending a billed round trip
    /// when the serialized payload is clearly too large for the model's context.
    ///
    /// This is a cheap heuristic (bytes / 4), not a tokenizer; the server remains
    /// authoritative. Disable with `Client::with_context_check(false)`.
    fn check_context_size(
        model: &Model,
        state: &State,
        questions: &Questions,
    ) -> Result<(), Error> {
        let Some(limit) = model.context_tokens() else {
            return Ok(());
        };
        let questions_bytes = serde_json::to_string(questions).map_or(usize::MAX, |s| s.len());
        let estimated = estimate_tokens(state.byte_len() + questions_bytes);
        if estimated > limit {
            return Err(Error::StateTooLarge { estimated, limit });
        }
        Ok(())
    }

    /// The full URL for this client's configured surface.
    fn request_url(&self) -> url::Url {
        let mut url = self.api_base();
        url.set_path(self.config.surface.path());
        url
    }

    /// Runs the bounded retry loop around a single logical request.
    #[allow(clippy::too_many_arguments)]
    async fn send_with_retry(
        &self,
        url: &url::Url,
        wire_model: &str,
        state: &State,
        questions: &Questions,
        session_id: Option<String>,
        user: Option<String>,
        trace: Option<TraceConfig>,
    ) -> Result<Response, Error> {
        let retry = self.config.retry;
        let max_attempts = effective_max_attempts(retry.max_attempts);

        let attempt_fut = async {
            let mut last_err: Option<Error> = None;
            for attempt in 1..=max_attempts {
                tracing::Span::current().record("attempt", attempt);
                let outcome = self
                    .send_once(
                        url,
                        wire_model,
                        state,
                        questions,
                        session_id.as_deref(),
                        user.as_deref(),
                        trace.as_ref(),
                    )
                    .await;

                match outcome {
                    Ok(response) => return Ok(response),
                    Err(err) => {
                        let retry_after = retry_after_from_error(&err);
                        let retryable = err.is_retryable();
                        last_err = Some(err);
                        if !retryable || attempt == max_attempts {
                            break;
                        }
                        sleep_before_retry(retry, attempt, retry_after).await;
                    }
                }
            }
            // `max_attempts >= 1` (enforced by `effective_max_attempts`), so the loop
            // above always runs at least once and `last_err` is always `Some` here.
            Err(last_err.unwrap_or(Error::Invalid {
                field: "retry".to_string(),
                reason: "retry loop exited without a result".to_string(),
            }))
        };

        run_with_deadline(self.config.timeout, attempt_fut).await
    }

    /// Sends exactly one HTTP request and parses its response, without retrying.
    #[allow(clippy::too_many_arguments)]
    async fn send_once(
        &self,
        url: &url::Url,
        wire_model: &str,
        state: &State,
        questions: &Questions,
        session_id: Option<&str>,
        user: Option<&str>,
        trace: Option<&TraceConfig>,
    ) -> Result<Response, Error> {
        let (session_id, user, trace) =
            drop_unsupported_fields(self.config.surface, session_id, user, trace);

        let request = DecisionRequest {
            model: wire_model,
            state,
            questions,
            session_id,
            user,
            trace,
        };
        let body = serde_json::to_vec(&request).map_err(|e| Error::Invalid {
            field: "request".to_string(),
            reason: e.to_string(),
        })?;

        let mut request = self
            .http
            .post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(api_key) = &self.api_key {
            request = request.header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", api_key.expose_secret()),
            );
        }
        let http_response = request.body(body).send().await?;

        let status = http_response.status();
        let retry_after_secs = parse_retry_after(http_response.headers());
        let bytes = read_capped_body(http_response).await?;

        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            return Err(Error::from_api_response(
                status.as_u16(),
                &text,
                retry_after_secs,
            ));
        }

        serde_json::from_slice(&bytes).map_err(|e| Error::Parse(e.to_string()))
    }
}

/// A `Client::new`/`Client::from_config`-friendly `reqwest::Client` builder.
///
/// Returns [`Error::Invalid`] when neither TLS feature is enabled, since a client
/// built this way cannot make an HTTPS request without one.
fn build_http_client(timeout: Duration) -> Result<reqwest::Client, Error> {
    #[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
    {
        log_tls_backend();
        reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(Error::Http)
    }
    #[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
    {
        let _ = timeout;
        Err(Error::Invalid {
            field: "tls".to_string(),
            reason: "Client::new requires the `rustls-tls` or `native-tls` feature; use \
                     Client::with_http with a caller-supplied client instead"
                .to_string(),
        })
    }
}

/// Names the effective TLS backend once, at `debug!` level, when both TLS features
/// are enabled (Cargo feature unification can legitimately request both).
#[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
fn log_tls_backend() {
    #[cfg(feature = "rustls-tls")]
    {
        debug!(
            backend = "rustls",
            "using rustls TLS backend (preferred when both are enabled)"
        );
    }
    #[cfg(all(not(feature = "rustls-tls"), feature = "native-tls"))]
    {
        debug!(backend = "native-tls", "using native-tls TLS backend");
    }
}

/// Clamps `max_attempts` to at least `1`, and to `1` unconditionally when the
/// `retry` feature is disabled (no backoff machinery is compiled in).
fn effective_max_attempts(configured: u32) -> u32 {
    #[cfg(feature = "retry")]
    {
        configured.max(1)
    }
    #[cfg(not(feature = "retry"))]
    {
        let _ = configured;
        1
    }
}

/// Runs `fut` under an overall deadline, mapping expiry to [`Error::Timeout`].
///
/// Without the `retry` feature, `tokio::time` is not pulled in as a dependency of
/// this crate's own retry loop; the per-attempt `reqwest` timeout (set at client
/// construction) is the only bound in that configuration.
async fn run_with_deadline<F, T>(timeout: Duration, fut: F) -> Result<T, Error>
where
    F: std::future::Future<Output = Result<T, Error>>,
{
    #[cfg(feature = "retry")]
    {
        let start = Instant::now();
        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => Err(Error::Timeout {
                elapsed_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            }),
        }
    }
    #[cfg(not(feature = "retry"))]
    {
        let _ = timeout;
        fut.await
    }
}

/// Sleeps before the next retry attempt, honouring `Retry-After` when present.
#[cfg(feature = "retry")]
async fn sleep_before_retry(
    retry: crate::config::RetryConfig,
    attempt: u32,
    retry_after: Option<Duration>,
) {
    let backoff = compute_backoff(retry, attempt, retry_after);
    tokio::time::sleep(backoff).await;
}

#[cfg(not(feature = "retry"))]
async fn sleep_before_retry(
    _retry: crate::config::RetryConfig,
    _attempt: u32,
    _retry_after: Option<Duration>,
) {
}

/// Computes the backoff before the next attempt: `Retry-After` takes precedence
/// (clamped to `max_backoff`); otherwise exponential backoff with full jitter,
/// capped at `max_backoff`.
#[cfg(feature = "retry")]
fn compute_backoff(
    retry: crate::config::RetryConfig,
    attempt: u32,
    retry_after: Option<Duration>,
) -> Duration {
    if let Some(retry_after) = retry_after {
        return retry_after.min(retry.max_backoff);
    }
    let exponent = attempt.saturating_sub(1).min(20); // bound the shift to avoid overflow
    let ceiling = retry
        .initial_backoff
        .saturating_mul(1u32.checked_shl(exponent).unwrap_or(u32::MAX))
        .min(retry.max_backoff);
    full_jitter(ceiling)
}

/// Full-jitter backoff: a uniformly random duration in `0..=ceiling`.
#[cfg(feature = "retry")]
fn full_jitter(ceiling: Duration) -> Duration {
    if ceiling.is_zero() {
        return ceiling;
    }
    // A dependency-free, non-cryptographic jitter source is sufficient here: backoff
    // timing has no security property, only a fairness one (avoiding a thundering
    // herd), and pulling in `rand` for it would cost more than it is worth.
    let nanos = Instant::now().elapsed().as_nanos() ^ u128::from(std::process::id());
    #[allow(clippy::cast_precision_loss)]
    let fraction = (nanos % 1_000_000) as f64 / 1_000_000.0;
    Duration::from_secs_f64(ceiling.as_secs_f64() * fraction)
}

/// Extracts the `retry_after_secs` carried by [`Error::RateLimited`], if any.
fn retry_after_from_error(err: &Error) -> Option<Duration> {
    match err {
        Error::RateLimited {
            retry_after_secs: Some(secs),
        } => Some(Duration::from_secs(*secs)),
        _ => None,
    }
}

/// Parses a `Retry-After` header, in either the seconds form or the HTTP-date form.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(secs) = raw.trim().parse::<u64>() {
        return Some(secs);
    }
    parse_http_date_seconds_from_now(raw.trim())
}

/// Parses an RFC 7231 IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) and returns the
/// number of seconds from now until that instant, saturating at zero if it is past.
fn parse_http_date_seconds_from_now(s: &str) -> Option<u64> {
    let epoch_secs = parse_imf_fixdate(s)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let now_i64 = now.cast_signed();
    Some(epoch_secs.saturating_sub(now_i64).max(0).cast_unsigned())
}

/// Parses `"Sun, 06 Nov 1994 08:49:37 GMT"` into Unix seconds. Returns `None` for any
/// other format; this SDK does not need to accept the obsolete RFC 850 / ANSI C forms.
fn parse_imf_fixdate(s: &str) -> Option<i64> {
    // "Sun, 06 Nov 1994 08:49:37 GMT"
    let s = s.strip_suffix(" GMT")?;
    let (_weekday, rest) = s.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: i64 = parts.next()?.parse().ok()?;
    let month = month_from_abbrev(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Maps a three-letter month abbreviation to `1..=12`.
fn month_from_abbrev(s: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    #[allow(clippy::cast_possible_wrap)] // MONTHS.len() == 12; the position always fits in i64
    MONTHS.iter().position(|m| *m == s).map(|i| i as i64 + 1)
}

/// Days since the Unix epoch for a civil (Gregorian) date, via Howard Hinnant's
/// `days_from_civil` algorithm. Valid for all proleptic Gregorian dates.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Reads a response body up to [`MAX_RESPONSE_BODY_BYTES`], erroring instead of
/// exhausting memory on an oversized or malformed gateway response.
async fn read_capped_body(mut response: reqwest::Response) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if buf.len() + chunk.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(Error::Parse(format!(
                "response body exceeds {MAX_RESPONSE_BODY_BYTES} byte cap"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Clears fields the System One surface does not define, logging the drop.
fn drop_unsupported_fields<'a>(
    surface: Surface,
    session_id: Option<&'a str>,
    user: Option<&'a str>,
    trace: Option<&'a TraceConfig>,
) -> (Option<&'a str>, Option<&'a str>, Option<&'a TraceConfig>) {
    if surface == Surface::SystemOne {
        if session_id.is_some() || user.is_some() || trace.is_some() {
            debug!(
                dropped_session_id = session_id.is_some(),
                dropped_user = user.is_some(),
                dropped_trace = trace.is_some(),
                "System One surface does not support session_id/user/trace; dropping"
            );
        }
        (None, None, None)
    } else {
        (session_id, user, trace)
    }
}

/// Fires a `warn!` when any returned `choice`/`score` confidence is below `0.5`, so
/// miscalibrated gates surface in dashboards without extra application code.
fn warn_on_low_confidence(answers: &Answers) {
    const LOW_CONFIDENCE: f64 = 0.5;
    for (key, answer) in answers {
        let confidence = match answer {
            crate::answer::Answer::Choice(c) => c.confidence,
            crate::answer::Answer::Score(s) => s.confidence,
            crate::answer::Answer::Noul(_) | crate::answer::Answer::Other { .. } => None,
        };
        if let Some(confidence) = confidence
            && confidence < LOW_CONFIDENCE
        {
            warn!(question = key, confidence, "low-confidence answer");
        }
    }
}

/// Whether `url` points at a loopback address, the only case `with_api_base` accepts
/// a non-`https` scheme for (local mocks such as `wiremock`).
fn is_local_http_opt_in(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// Infallible fallback for the default API base URL, used only if the literal
/// constant ever failed to parse (which `cargo test` guards against).
fn unreachable_url() -> url::Url {
    // Safety net, not a real fallible path: `DEFAULT_API_BASE` is a literal constant
    // covered by `test_should_parse_default_api_base`.
    url::Url::parse("https://openrouter.ai").unwrap_or_else(|_| {
        // `data:` URLs always parse; this line is unreachable in practice.
        url::Url::parse("data:,").expect("`data:,` is always a valid URL")
    })
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::CaseSet;

    #[tokio::test]
    async fn test_should_run_case_and_tag_result_with_case_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/alpha/decisions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "typesafe/jev-1.13",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {"refund": {"type": "noul", "noul": 0.9}}
            })))
            .mount(&server)
            .await;

        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"))
            .with_api_base(&server.uri())
            .unwrap();

        let cases = CaseSet::from_json_str(
            r#"{"cases": [{
                "id": "ticket-001",
                "state": "a refund ticket",
                "questions": {
                    "refund": {"type": "noul", "instructions": "Refund?"}
                }
            }]}"#,
        )
        .unwrap();
        let case = cases.iter().next().unwrap();

        let result = client.run_case(case).await.unwrap();
        assert_eq!(result.id, "ticket-001");
        assert_eq!(result.response.answers.noul("refund").unwrap(), 0.9);
    }

    #[test]
    fn test_should_redact_api_key_in_debug_output() {
        let client = Client::with_http(
            reqwest::Client::new(),
            SecretString::from("sk-super-secret"),
        );
        let debug = format!("{client:?}");
        assert!(!debug.contains("sk-super-secret"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn test_should_report_no_api_key_in_debug_output_for_without_api_key() {
        let client = Client::without_api_key("http://127.0.0.1:8099").unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("none"));
        assert!(!debug.contains("REDACTED"));
    }

    #[test]
    fn test_should_return_invalid_when_no_tls_feature_and_new_called() {
        #[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
        {
            let result = Client::new(SecretString::from("key"));
            assert!(matches!(result, Err(Error::Invalid { .. })));
        }
    }

    #[test]
    fn test_should_parse_default_api_base() {
        assert!(url::Url::parse(DEFAULT_API_BASE).is_ok());
    }

    #[test]
    fn test_should_reject_non_https_api_base() {
        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"));
        assert!(client.with_api_base("http://example.com").is_err());
    }

    #[test]
    fn test_should_accept_loopback_http_api_base_for_local_mocks() {
        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"));
        assert!(client.with_api_base("http://127.0.0.1:8080").is_ok());
        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"));
        assert!(client.with_api_base("http://localhost:8080").is_ok());
    }

    #[test]
    fn test_should_build_request_url_per_surface() {
        let client = Client::with_http(reqwest::Client::new(), SecretString::from("key"))
            .with_api_base("http://127.0.0.1:9999")
            .unwrap();
        assert_eq!(client.request_url().path(), "/api/alpha/decisions");
        let client = client.with_surface(Surface::SystemOne);
        assert_eq!(client.request_url().path(), "/api/v1/systemone");
    }

    #[test]
    fn test_should_parse_retry_after_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "120".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), Some(120));
    }

    #[test]
    fn test_should_parse_retry_after_http_date() {
        // Fixed historical date: the wait time will be zero (it is in the past), but
        // parsing itself must succeed rather than falling through to `None`.
        let epoch = parse_imf_fixdate("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        assert_eq!(epoch, 784_111_777);
    }

    #[test]
    fn test_should_return_none_for_unparsable_retry_after() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "not-a-date".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn test_should_drop_decisions_only_fields_on_system_one() {
        let trace = TraceConfig::new(serde_json::json!({}));
        let (session, user, trace) =
            drop_unsupported_fields(Surface::SystemOne, Some("s"), Some("u"), Some(&trace));
        assert_eq!(session, None);
        assert_eq!(user, None);
        assert_eq!(trace, None);
    }

    #[test]
    fn test_should_keep_decisions_only_fields_on_decisions_surface() {
        let trace = TraceConfig::new(serde_json::json!({}));
        let (session, user, trace) =
            drop_unsupported_fields(Surface::Decisions, Some("s"), Some("u"), Some(&trace));
        assert_eq!(session, Some("s"));
        assert_eq!(user, Some("u"));
        assert!(trace.is_some());
    }

    #[test]
    #[cfg(feature = "retry")]
    fn test_should_compute_backoff_from_retry_after_clamped_to_max() {
        let retry =
            crate::config::RetryConfig::default().with_max_backoff(Duration::from_millis(500));
        let backoff = compute_backoff(retry, 1, Some(Duration::from_secs(10)));
        assert_eq!(backoff, Duration::from_millis(500));
    }

    #[test]
    #[cfg(feature = "retry")]
    fn test_should_compute_backoff_within_exponential_ceiling() {
        let retry = crate::config::RetryConfig::default()
            .with_initial_backoff(Duration::from_millis(100))
            .with_max_backoff(Duration::from_secs(10));
        for attempt in 1..=5 {
            let backoff = compute_backoff(retry, attempt, None);
            let ceiling = retry.initial_backoff * 2u32.pow(attempt - 1);
            assert!(backoff <= ceiling.min(retry.max_backoff));
        }
    }

    #[test]
    #[cfg(feature = "retry")]
    fn test_should_cap_backoff_at_max_backoff_for_large_attempt_numbers() {
        let retry = crate::config::RetryConfig::default()
            .with_initial_backoff(Duration::from_millis(100))
            .with_max_backoff(Duration::from_secs(2));
        let backoff = compute_backoff(retry, 50, None);
        assert!(backoff <= Duration::from_secs(2));
    }

    #[test]
    fn test_should_force_single_attempt_without_retry_feature() {
        #[cfg(not(feature = "retry"))]
        assert_eq!(effective_max_attempts(5), 1);
        #[cfg(feature = "retry")]
        assert_eq!(effective_max_attempts(5), 5);
    }

    #[test]
    fn test_should_never_return_zero_max_attempts() {
        assert!(effective_max_attempts(0) >= 1);
    }
}
