//! The only asynchronous HTTP path: bounded reads, deadlines and explicit retry.

use std::{
    fmt,
    net::IpAddr,
    time::{Duration, SystemTime},
};

use reqwest::{
    Client as ReqwestClient, Request, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use secrecy::{ExposeSecret, SecretString};
use tokio::{
    sync::Semaphore,
    time::{Instant, sleep, timeout_at},
};

use crate::{Error, Settings, backend::Backend};

/// Shared SDK-controlled connection pool. Redirects and implicit retries are disabled.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HttpClient(ReqwestClient);
impl HttpClient {
    /// Build the standard pool.
    /// # Errors
    /// Returns Transport if HTTP initialization fails.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let pool = jev::HttpClient::new()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Result<Self, Error> {
        Self::with_pool(Duration::from_secs(90), 8)
    }
    /// Configure pool idle timeout and idle connections per host (1..=128).
    /// # Errors
    /// Returns invalid configuration for out-of-range parameters or Transport on initialization.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let pool = jev::HttpClient::with_pool(std::time::Duration::from_secs(90), 8)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_pool(idle_timeout: Duration, idle_per_host: usize) -> Result<Self, Error> {
        if idle_timeout.is_zero()
            || idle_timeout > Duration::from_secs(600)
            || !(1..=128).contains(&idle_per_host)
        {
            return Err(Error::InvalidConfiguration("connection pool bounds"));
        }
        let inner = ReqwestClient::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .pool_idle_timeout(idle_timeout)
            .pool_max_idle_per_host(idle_per_host)
            .user_agent(concat!("jev-typed/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(Error::Transport)?;
        Ok(Self(inner))
    }
    /// Read-only access for application requests sharing the connection pool.
    ///
    /// ```no_run
    /// # fn example(pool: &jev::HttpClient) -> Result<(), jev::Error> {
    /// let http = pool.as_reqwest();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_reqwest(&self) -> &ReqwestClient {
        &self.0
    }
}
/// Authentication for an explicitly configured endpoint.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Auth {
    /// Bearer credential, redacted in Debug.
    ApiKey(SecretString),
    /// Anonymous authentication, only available with a custom endpoint.
    None,
}
impl Auth {
    pub(crate) fn header(&self) -> Result<Option<HeaderValue>, Error> {
        match self {
            Self::None => Ok(None),
            Self::ApiKey(secret) => {
                let key = secret.expose_secret();
                if key.is_empty()
                    || key.len() > 4096
                    || key.chars().any(|c| c.is_whitespace() || c.is_control())
                {
                    return Err(Error::InvalidConfiguration("API key bounds"));
                }
                let mut header = HeaderValue::from_str(&format!("Bearer {key}"))
                    .map_err(|_| Error::InvalidConfiguration("API key encoding"))?;
                header.set_sensitive(true);
                Ok(Some(header))
            }
        }
    }
}
/// Complete custom URL paired with an explicit protocol.
#[derive(Clone)]
#[non_exhaustive]
pub struct Endpoint {
    pub(crate) url: Url,
    pub(crate) backend: Backend,
}
impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("protocol", &self.backend)
            .finish_non_exhaustive()
    }
}
impl Endpoint {
    fn new(url: &str, backend: Backend) -> Result<Self, Error> {
        if url.len() > 2048 {
            return Err(Error::InvalidConfiguration("endpoint length"));
        }
        let parsed = Url::parse(url).map_err(|_| Error::InvalidConfiguration("endpoint URL"))?;
        let local = parsed
            .host_str()
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !(parsed.scheme() == "https" || parsed.scheme() == "http" && local)
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::InvalidConfiguration(
                "endpoint requires HTTPS or literal loopback HTTP, without credentials, query or \
                 fragment",
            ));
        }
        Ok(Self {
            url: parsed,
            backend,
        })
    }
    /// Declare a complete System One endpoint.
    /// # Errors
    /// Rejects invalid or insecure URLs.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let endpoint = jev::Endpoint::system_one("http://127.0.0.1:8080/decisions")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn system_one(url: &str) -> Result<Self, Error> {
        Self::new(url, Backend::SystemOne)
    }
    /// Declare a complete Jev Decisions endpoint.
    /// # Errors
    /// Rejects invalid or insecure URLs.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let endpoint = jev::Endpoint::jev_decisions("http://127.0.0.1:8080/decisions")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn jev_decisions(url: &str) -> Result<Self, Error> {
        Self::new(url, Backend::Jev)
    }
    /// Declare a complete `OpenAI` Decisions endpoint.
    /// # Errors
    /// Rejects invalid or insecure URLs.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), jev::Error> {
    /// let endpoint = jev::Endpoint::openai_decisions("http://127.0.0.1:8080/decisions")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn openai_decisions(url: &str) -> Result<Self, Error> {
        Self::new(url, Backend::OpenAi)
    }
}
#[derive(Debug)]
pub(crate) struct Transport {
    pub http: HttpClient,
    pub permits: Semaphore,
    pub settings: Settings,
}
struct AttemptError {
    error: Error,
    retry_after: Option<Duration>,
}
impl From<Error> for AttemptError {
    fn from(error: Error) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }
}
fn retry_delay(value: &str) -> Option<Duration> {
    if value.len() > 128 {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(SystemTime::now()).unwrap_or_default())
        })
}
pub(crate) struct Received {
    pub body: Vec<u8>,
    pub request_id: Option<String>,
    pub attempts: u32,
    pub latency: Duration,
}
impl Transport {
    pub fn request(
        &self,
        endpoint: &Endpoint,
        auth: &Auth,
        body: Vec<u8>,
    ) -> Result<Request, Error> {
        let mut builder = self
            .http
            .0
            .post(endpoint.url.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(header) = auth.header()? {
            builder = builder.header(AUTHORIZATION, header);
        }
        builder.build().map_err(Error::Transport)
    }
    pub async fn send(&self, request: Request, backend: Backend) -> Result<Received, Error> {
        // A byte-backed request must be clonable before the first send.
        if request.try_clone().is_none() {
            return Err(Error::InvalidConfiguration("request cannot be cloned"));
        }
        let start = Instant::now();
        let deadline = start + self.settings.timeout();
        let mut attempts = 0_u32;
        loop {
            attempts = attempts.saturating_add(1);
            let next = request
                .try_clone()
                .ok_or(Error::InvalidConfiguration("request cannot be cloned"))?;
            let result = timeout_at(deadline, self.attempt(next, backend))
                .await
                .map_err(|_| Error::Timeout)?;
            match result {
                Ok((body, request_id)) => {
                    return Ok(Received {
                        body,
                        request_id,
                        attempts,
                        latency: start.elapsed(),
                    });
                }
                Err(failure) => {
                    let error = failure.error;
                    if !cfg!(feature = "retry")
                        || !error.is_retryable()
                        || attempts >= self.settings.retry.max_attempts
                    {
                        return Err(error);
                    }
                    let computed = self
                        .settings
                        .retry
                        .initial_backoff_ms
                        .saturating_mul(2_u64.saturating_pow(attempts.saturating_sub(1)))
                        .min(self.settings.retry.max_backoff_ms);
                    let wait = failure
                        .retry_after
                        .unwrap_or_else(|| Duration::from_millis(computed));
                    if wait >= deadline.saturating_duration_since(Instant::now()) {
                        return Err(Error::Timeout);
                    }
                    timeout_at(deadline, sleep(wait))
                        .await
                        .map_err(|_| Error::Timeout)?;
                }
            }
        }
    }
    async fn attempt(
        &self,
        request: Request,
        backend: Backend,
    ) -> Result<(Vec<u8>, Option<String>), AttemptError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| Error::InvalidConfiguration("closed concurrency pool"))?;
        let mut response = self
            .http
            .0
            .execute(request)
            .await
            .map_err(|e| Error::Transport(e.without_url()))?;
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .map(|value| {
                value
                    .to_str()
                    .map_err(|_| Error::protocol("request ID encoding"))
                    .and_then(|value| crate::backend::bounded_identifier(value, 256))
            })
            .transpose()?;
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(retry_delay);
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Error::Transport(e.without_url()))?
        {
            let size = body
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| Error::protocol("response size"))?;
            if size > self.settings.limits.response_bytes {
                return Err(Error::protocol("response size").into());
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let error = backend.http_error(
                status.as_u16(),
                &body,
                retry_after.map(|d| d.as_secs().saturating_add(u64::from(d.subsec_nanos() != 0))),
                request_id,
            );
            return Err(AttemptError { error, retry_after });
        }
        Ok((body, request_id))
    }
}
