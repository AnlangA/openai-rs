//! RFC 8693 workload-identity authentication for OpenAI Platform clients.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use http::{HeaderValue, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, Notify};
use zeroize::Zeroizing;

use crate::{BodyPreview, Error, TlsBackend};

const TOKEN_EXCHANGE_URL: &str = "https://auth.openai.com/oauth/token";
const TOKEN_EXCHANGE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:jwt";
const ID_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:id_token";
const DEFAULT_REFRESH_BUFFER: Duration = Duration::from_secs(1_200);
const DEFAULT_TOKEN_LIFETIME: Duration = Duration::from_secs(3_600);
const MAX_EXCHANGE_BODY_BYTES: usize = 64 * 1024;

/// Boxed future returned by [`SubjectTokenProvider`].
pub type SubjectTokenFuture<'a> = Pin<
    Box<dyn Future<Output = Result<SubjectToken, SubjectTokenProviderError>> + Send + 'a>,
>;

/// RFC 8693 subject-token kind accepted by OpenAI workload identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubjectTokenType {
    Jwt,
    Id,
}

impl SubjectTokenType {
    const fn as_urn(self) -> &'static str {
        match self {
            Self::Jwt => JWT_TOKEN_TYPE,
            Self::Id => ID_TOKEN_TYPE,
        }
    }
}

/// A redacting external subject token.
#[derive(Clone)]
pub struct SubjectToken(SecretString);

impl SubjectToken {
    pub fn new(token: impl Into<String>) -> Result<Self, SubjectTokenValidationError> {
        validate_bearer_material(&token.into()).map(|token| Self(SecretString::from(token)))
    }

    fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for SubjectToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SubjectToken([REDACTED])")
    }
}

/// Validation error for external subject tokens.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("subject token is empty or unsafe for RFC 8693 exchange")]
pub struct SubjectTokenValidationError;

/// Redacted error returned by a subject-token callback.
#[derive(Clone, Debug, Error)]
#[error("subject token provider failed")]
pub struct SubjectTokenProviderError;

impl SubjectTokenProviderError {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for SubjectTokenProviderError {
    fn default() -> Self {
        Self::new()
    }
}

mod provider_private {
    pub trait Sealed {}
}

/// Object-safe, sealed asynchronous source of external workload tokens.
///
/// Callers construct an implementation through [`SubjectTokenProviderFn`],
/// which snapshots the closure inside the client configuration.
pub trait SubjectTokenProvider: provider_private::Sealed + Send + Sync {
    fn token_type(&self) -> SubjectTokenType;
    fn subject_token(&self) -> SubjectTokenFuture<'_>;
}

/// Closure-backed implementation of [`SubjectTokenProvider`].
pub struct SubjectTokenProviderFn<F> {
    token_type: SubjectTokenType,
    provider: F,
}

impl<F> SubjectTokenProviderFn<F> {
    #[must_use]
    pub const fn new(token_type: SubjectTokenType, provider: F) -> Self {
        Self {
            token_type,
            provider,
        }
    }
}

impl<F> fmt::Debug for SubjectTokenProviderFn<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubjectTokenProviderFn")
            .field("token_type", &self.token_type)
            .field("provider", &"[REDACTED]")
            .finish()
    }
}

impl<F> provider_private::Sealed for SubjectTokenProviderFn<F> {}

impl<F, Fut> SubjectTokenProvider for SubjectTokenProviderFn<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Result<SubjectToken, SubjectTokenProviderError>> + Send + 'static,
{
    fn token_type(&self) -> SubjectTokenType {
        self.token_type
    }

    fn subject_token(&self) -> SubjectTokenFuture<'_> {
        Box::pin((self.provider)())
    }
}

/// Validated workload-identity configuration.
#[derive(Clone)]
pub struct WorkloadIdentityConfig {
    identity_provider_id: Box<str>,
    service_account_id: Box<str>,
    client_id: Option<Box<str>>,
    refresh_buffer: Duration,
    provider: Arc<dyn SubjectTokenProvider>,
    #[cfg(test)]
    token_exchange_url: Option<url::Url>,
}

impl WorkloadIdentityConfig {
    pub fn new<P>(
        identity_provider_id: impl Into<String>,
        service_account_id: impl Into<String>,
        provider: P,
    ) -> Result<Self, WorkloadIdentityConfigError>
    where
        P: SubjectTokenProvider + 'static,
    {
        Ok(Self {
            identity_provider_id: validate_identifier(identity_provider_id.into())?,
            service_account_id: validate_identifier(service_account_id.into())?,
            client_id: None,
            refresh_buffer: DEFAULT_REFRESH_BUFFER,
            provider: Arc::new(provider),
            #[cfg(test)]
            token_exchange_url: None,
        })
    }

    pub fn with_client_id(
        mut self,
        client_id: impl Into<String>,
    ) -> Result<Self, WorkloadIdentityConfigError> {
        self.client_id = Some(validate_identifier(client_id.into())?);
        Ok(self)
    }

    #[must_use]
    pub const fn with_refresh_buffer(mut self, refresh_buffer: Duration) -> Self {
        self.refresh_buffer = refresh_buffer;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_token_exchange_url(mut self, url: url::Url) -> Self {
        self.token_exchange_url = Some(url);
        self
    }

    fn exchange_url(&self) -> &str {
        #[cfg(test)]
        if let Some(url) = &self.token_exchange_url {
            return url.as_str();
        }
        TOKEN_EXCHANGE_URL
    }
}

impl fmt::Debug for WorkloadIdentityConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkloadIdentityConfig")
            .field("identity_provider_id", &"[REDACTED]")
            .field("service_account_id", &"[REDACTED]")
            .field("client_id", &self.client_id.as_ref().map(|_| "[REDACTED]"))
            .field("refresh_buffer", &self.refresh_buffer)
            .field("provider", &"[REDACTED]")
            .finish()
    }
}

/// Invalid workload configuration.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("workload identity identifier is empty or contains unsafe characters")]
pub struct WorkloadIdentityConfigError;

/// Workload token exchange and provider failures.
#[derive(Error)]
#[non_exhaustive]
pub enum WorkloadIdentityError {
    #[error("subject token provider failed")]
    SubjectToken,
    #[error("workload token exchange transport failed")]
    Transport,
    #[error("workload token exchange was rejected with HTTP {status}")]
    ExchangeRejected {
        status: StatusCode,
        body: BodyPreview,
    },
    #[error("workload token exchange returned an invalid response: {reason}")]
    InvalidResponse {
        reason: &'static str,
        body: BodyPreview,
    },
}

impl fmt::Debug for WorkloadIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectToken => formatter.write_str("WorkloadIdentityError::SubjectToken"),
            Self::Transport => formatter.write_str("WorkloadIdentityError::Transport"),
            Self::ExchangeRejected { status, .. } => formatter
                .debug_struct("WorkloadIdentityError::ExchangeRejected")
                .field("status", status)
                .field("body", &"[REDACTED]")
                .finish(),
            Self::InvalidResponse { reason, .. } => formatter
                .debug_struct("WorkloadIdentityError::InvalidResponse")
                .field("reason", reason)
                .field("body", &"[REDACTED]")
                .finish(),
        }
    }
}

impl WorkloadIdentityError {
    #[must_use]
    pub const fn status(&self) -> Option<StatusCode> {
        match self {
            Self::ExchangeRejected { status, .. } => Some(*status),
            Self::SubjectToken | Self::Transport | Self::InvalidResponse { .. } => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct TokenLease {
    pub header: HeaderValue,
    pub generation: Option<u64>,
}

#[derive(Clone)]
struct CachedToken {
    token: SecretString,
    expires_at: Instant,
    refresh_at: Instant,
    generation: u64,
}

struct RefreshInFlight {
    id: u64,
    generation: u64,
}

#[derive(Default)]
struct TokenState {
    cached: Option<CachedToken>,
    generation: u64,
    next_refresh_id: u64,
    refreshing: Option<RefreshInFlight>,
    completed_failure: Option<(u64, Arc<WorkloadIdentityError>)>,
}

pub(crate) struct WorkloadIdentityAuth {
    config: WorkloadIdentityConfig,
    http: reqwest::Client,
    state: Mutex<TokenState>,
    notify: Notify,
}

impl WorkloadIdentityAuth {
    pub(crate) fn new(
        config: WorkloadIdentityConfig,
        tls_backend: Option<TlsBackend>,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Arc<Self>, Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        let http = match tls_backend {
            #[cfg(feature = "rustls-tls")]
            Some(TlsBackend::Rustls) => http.use_rustls_tls(),
            #[cfg(feature = "native-tls")]
            Some(TlsBackend::Native) => http.use_native_tls(),
            None => http,
        }
        .build()
        .map_err(Error::from_reqwest)?;
        Ok(Arc::new(Self {
            config,
            http,
            state: Mutex::new(TokenState::default()),
            notify: Notify::new(),
        }))
    }

    pub(crate) async fn token(self: &Arc<Self>) -> Result<TokenLease, Error> {
        loop {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            if let Some(cached) = state.cached.clone()
                && now < cached.expires_at
            {
                if now >= cached.refresh_at && state.refreshing.is_none() {
                    let refresh = begin_refresh(&mut state);
                    let auth = Arc::clone(self);
                    tokio::spawn(async move {
                        let result = auth.exchange(refresh.generation).await;
                        auth.finish_refresh(refresh, result).await;
                    });
                }
                return token_lease(&cached);
            }

            if let Some(refreshing) = &state.refreshing {
                let refresh_id = refreshing.id;
                let notified = self.notify.notified();
                drop(state);
                notified.await;
                let state = self.state.lock().await;
                if let Some((completed_id, error)) = &state.completed_failure
                    && *completed_id == refresh_id
                {
                    return Err(Error::from(Arc::clone(error)));
                }
                drop(state);
                continue;
            }

            let refresh = begin_refresh(&mut state);
            drop(state);
            let result = self.exchange(refresh.generation).await;
            let caller_result = result.clone();
            self.finish_refresh(refresh, result).await;
            return match caller_result {
                Ok(token) => token_lease(&token),
                Err(error) => Err(Error::from(error)),
            };
        }
    }

    pub(crate) async fn invalidate_if_generation(&self, generation: u64) -> bool {
        let mut state = self.state.lock().await;
        if state.generation != generation {
            return false;
        }
        state.generation = state.generation.wrapping_add(1);
        state.cached = None;
        if state
            .refreshing
            .as_ref()
            .is_some_and(|refresh| refresh.generation == generation)
        {
            state.refreshing = None;
        }
        state.completed_failure = None;
        drop(state);
        self.notify.notify_waiters();
        true
    }

    async fn finish_refresh(
        &self,
        refresh: RefreshInFlight,
        result: Result<CachedToken, Arc<WorkloadIdentityError>>,
    ) {
        let mut state = self.state.lock().await;
        let still_current = state.generation == refresh.generation;
        if still_current {
            if let Ok(token) = &result {
                state.cached = Some(token.clone());
            }
        }
        if state
            .refreshing
            .as_ref()
            .is_some_and(|current| current.id == refresh.id)
        {
            state.refreshing = None;
            state.completed_failure = result.err().map(|error| (refresh.id, error));
        }
        drop(state);
        self.notify.notify_waiters();
    }

    async fn exchange(
        &self,
        generation: u64,
    ) -> Result<CachedToken, Arc<WorkloadIdentityError>> {
        let subject = self
            .config
            .provider
            .subject_token()
            .await
            .map_err(|_| Arc::new(WorkloadIdentityError::SubjectToken))?;
        let body = TokenExchangeRequest {
            grant_type: TOKEN_EXCHANGE_GRANT_TYPE,
            subject_token: subject.expose(),
            subject_token_type: self.config.provider.token_type().as_urn(),
            identity_provider_id: &self.config.identity_provider_id,
            service_account_id: &self.config.service_account_id,
            client_id: self.config.client_id.as_deref(),
        };
        let response = self
            .http
            .post(self.config.exchange_url())
            .header(http::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|_| Arc::new(WorkloadIdentityError::Transport))?;
        let status = response.status();
        let (bytes, truncated) = read_bounded(response, MAX_EXCHANGE_BODY_BYTES)
            .await
            .map_err(|_| Arc::new(WorkloadIdentityError::Transport))?;
        if !status.is_success() {
            return Err(Arc::new(WorkloadIdentityError::ExchangeRejected {
                status,
                body: BodyPreview::from_bytes(&bytes, truncated),
            }));
        }
        if truncated {
            return Err(Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response body exceeded the configured limit",
                body: BodyPreview::from_bytes(&bytes, true),
            }));
        }
        let response: TokenExchangeResponse = serde_json::from_slice(&bytes).map_err(|_| {
            Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response contains invalid JSON",
                body: BodyPreview::from_bytes(&bytes, false),
            })
        })?;
        let token = validate_bearer_material(&response.access_token).map_err(|_| {
            Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response contains an invalid access_token",
                body: BodyPreview::from_bytes(&bytes, false),
            })
        })?;
        let lifetime_seconds = response.expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME.as_secs_f64());
        let lifetime = Duration::try_from_secs_f64(lifetime_seconds).map_err(|_| {
            Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response contains an invalid expires_in",
                body: BodyPreview::from_bytes(&bytes, false),
            })
        })?;
        if lifetime.is_zero() {
            return Err(Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response contains an invalid expires_in",
                body: BodyPreview::from_bytes(&bytes, false),
            }));
        }
        let now = Instant::now();
        let expires_at = now.checked_add(lifetime).ok_or_else(|| {
            Arc::new(WorkloadIdentityError::InvalidResponse {
                reason: "response expiration overflows the monotonic clock",
                body: BodyPreview::from_bytes(&bytes, false),
            })
        })?;
        let effective_buffer = self.config.refresh_buffer.min(lifetime / 2);
        let refresh_at = expires_at.checked_sub(effective_buffer).unwrap_or(now);
        Ok(CachedToken {
            token: SecretString::from(token),
            expires_at,
            refresh_at,
            generation,
        })
    }
}

fn begin_refresh(state: &mut TokenState) -> RefreshInFlight {
    state.next_refresh_id = state.next_refresh_id.wrapping_add(1);
    let refresh = RefreshInFlight {
        id: state.next_refresh_id,
        generation: state.generation,
    };
    state.refreshing = Some(RefreshInFlight {
        id: refresh.id,
        generation: refresh.generation,
    });
    state.completed_failure = None;
    refresh
}

fn token_lease(token: &CachedToken) -> Result<TokenLease, Error> {
    let value = Zeroizing::new(format!("Bearer {}", token.token.expose_secret()));
    let mut header = HeaderValue::from_str(value.as_str()).map_err(|_| {
        Error::from(Arc::new(WorkloadIdentityError::InvalidResponse {
            reason: "cached access token cannot be encoded as an HTTP header",
            body: BodyPreview::from_bytes(b"", false),
        }))
    })?;
    header.set_sensitive(true);
    Ok(TokenLease {
        header,
        generation: Some(token.generation),
    })
}

fn validate_bearer_material(token: &str) -> Result<String, SubjectTokenValidationError> {
    if token.is_empty()
        || token.trim() != token
        || !token.is_ascii()
        || token.chars().any(char::is_whitespace)
        || token.chars().any(char::is_control)
    {
        Err(SubjectTokenValidationError)
    } else {
        Ok(token.to_owned())
    }
}

fn validate_identifier(value: String) -> Result<Box<str>, WorkloadIdentityConfigError> {
    if value.is_empty()
        || value.trim() != value
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        Err(WorkloadIdentityConfigError)
    } else {
        Ok(value.into_boxed_str())
    }
}

#[derive(Serialize)]
struct TokenExchangeRequest<'a> {
    grant_type: &'static str,
    subject_token: &'a str,
    subject_token_type: &'static str,
    identity_provider_id: &'a str,
    service_account_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<&'a str>,
}

#[derive(Deserialize)]
struct TokenExchangeResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<f64>,
}

async fn read_bounded(
    response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}
