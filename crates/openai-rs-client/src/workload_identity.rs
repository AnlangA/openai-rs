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
pub type SubjectTokenFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SubjectToken, SubjectTokenProviderError>> + Send + 'a>>;

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
    OAuthRejected {
        status: StatusCode,
        body: BodyPreview,
    },
    #[error("workload token exchange failed with HTTP {status}")]
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
            Self::OAuthRejected { status, .. } => formatter
                .debug_struct("WorkloadIdentityError::OAuthRejected")
                .field("status", status)
                .field("body", &"[REDACTED]")
                .finish(),
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
            Self::OAuthRejected { status, .. } | Self::ExchangeRejected { status, .. } => {
                Some(*status)
            }
            Self::SubjectToken | Self::Transport | Self::InvalidResponse { .. } => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct TokenLease {
    pub header: HeaderValue,
    pub generation: Option<u64>,
}

impl fmt::Debug for TokenLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenLease")
            .field("header", &"[REDACTED]")
            .field("generation", &self.generation)
            .finish()
    }
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
                tokio::pin!(notified);
                notified.as_mut().enable();
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

    async fn exchange(&self, generation: u64) -> Result<CachedToken, Arc<WorkloadIdentityError>> {
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
            let body = BodyPreview::from_bytes(&bytes, truncated);
            return Err(Arc::new(if matches!(status.as_u16(), 400 | 401 | 403) {
                WorkloadIdentityError::OAuthRejected { status, body }
            } else {
                WorkloadIdentityError::ExchangeRejected { status, body }
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
        let lifetime_seconds = response
            .expires_in
            .unwrap_or(DEFAULT_TOKEN_LIFETIME.as_secs_f64());
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
        || value.chars().any(char::is_whitespace)
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

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{CreateFileRequest, FilePurpose, ReplayableMultipartSource};
    use serde_json::Value;
    use tokio::net::TcpListener;
    use url::Url;

    use super::*;
    use crate::{Client, CreateFileOneShotRequest, OneShotMultipartSource};

    #[derive(Clone)]
    struct Reply {
        status: StatusCode,
        body: String,
        location: Option<String>,
    }

    async fn exchange_server(
        replies: Vec<Reply>,
    ) -> (Url, Arc<AtomicUsize>, Arc<StdMutex<Vec<Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind exchange server");
        let address = listener.local_addr().expect("exchange address");
        let replies = Arc::new(StdMutex::new(VecDeque::from(replies)));
        let count = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let server_count = Arc::clone(&count);
        let server_requests = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let replies = Arc::clone(&replies);
                let count = Arc::clone(&server_count);
                let requests = Arc::clone(&server_requests);
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let replies = Arc::clone(&replies);
                        let count = Arc::clone(&count);
                        let requests = Arc::clone(&requests);
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            let bytes = request
                                .into_body()
                                .collect()
                                .await
                                .expect("read exchange request")
                                .to_bytes();
                            let value = serde_json::from_slice(&bytes).expect("exchange JSON");
                            requests.lock().expect("exchange requests lock").push(value);
                            let reply = {
                                let mut replies = replies.lock().expect("exchange replies lock");
                                match replies.len() {
                                    0 => Reply {
                                        status: StatusCode::INTERNAL_SERVER_ERROR,
                                        body: "{}".to_owned(),
                                        location: None,
                                    },
                                    1 => replies.front().expect("one reply").clone(),
                                    _ => replies.pop_front().expect("queued reply"),
                                }
                            };
                            let mut response = hyper::Response::builder()
                                .status(reply.status)
                                .header(http::header::CONTENT_TYPE, "application/json");
                            if let Some(location) = reply.location {
                                response = response.header(http::header::LOCATION, location);
                            }
                            Ok::<_, Infallible>(
                                response
                                    .body(Full::new(Bytes::from(reply.body)))
                                    .expect("exchange response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (
            Url::parse(&format!("http://{address}/oauth/token")).expect("exchange URL"),
            count,
            requests,
        )
    }

    async fn api_server(
        reject_first: bool,
        success_body: &'static str,
    ) -> (Url, Arc<AtomicUsize>, Arc<StdMutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind API server");
        let address = listener.local_addr().expect("API address");
        let count = Arc::new(AtomicUsize::new(0));
        let headers = Arc::new(StdMutex::new(Vec::new()));
        let server_count = Arc::clone(&count);
        let server_headers = Arc::clone(&headers);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let count = Arc::clone(&server_count);
                let headers = Arc::clone(&server_headers);
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let count = Arc::clone(&count);
                        let headers = Arc::clone(&headers);
                        async move {
                            let attempt = count.fetch_add(1, Ordering::SeqCst);
                            let authorization = request
                                .headers()
                                .get(http::header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or_default()
                                .to_owned();
                            headers
                                .lock()
                                .expect("API headers lock")
                                .push(authorization);
                            let (status, body) = if reject_first && attempt == 0 {
                                (
                                    StatusCode::UNAUTHORIZED,
                                    r#"{"error":{"message":"expired","type":"auth","code":"invalid_token"}}"#,
                                )
                            } else {
                                (StatusCode::OK, success_body)
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .body(Full::new(Bytes::from_static(body.as_bytes())))
                                    .expect("API response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (
            Url::parse(&format!("http://{address}/v1/")).expect("API URL"),
            count,
            headers,
        )
    }

    fn config(exchange_url: Url, provider_count: Arc<AtomicUsize>) -> WorkloadIdentityConfig {
        let provider = SubjectTokenProviderFn::new(SubjectTokenType::Jwt, move || {
            let provider_count = Arc::clone(&provider_count);
            async move {
                provider_count.fetch_add(1, Ordering::SeqCst);
                SubjectToken::new("subject.jwt.token").map_err(|_| SubjectTokenProviderError::new())
            }
        });
        WorkloadIdentityConfig::new("idp_test", "svc_test", provider)
            .expect("workload config")
            .with_client_id("client_test")
            .expect("client id")
            .with_token_exchange_url(exchange_url)
    }

    #[tokio::test]
    async fn concurrent_api_calls_share_one_exchange() {
        let (exchange_url, exchanges, exchange_requests) = exchange_server(vec![Reply {
            status: StatusCode::OK,
            body: r#"{"access_token":"access_one","expires_in":3600}"#.to_owned(),
            location: None,
        }])
        .await;
        let (api_url, api_calls, headers) =
            api_server(false, r#"{"object":"list","data":[]}"#).await;
        let providers = Arc::new(AtomicUsize::new(0));
        let client =
            Client::workload_identity_builder(config(exchange_url, Arc::clone(&providers)))
                .base_url(api_url)
                .allow_insecure_loopback(true)
                .build()
                .expect("workload client");

        let mut tasks = Vec::new();
        for _ in 0..16 {
            let client = client.clone();
            tasks.push(tokio::spawn(async move { client.models().list().await }));
        }
        for task in tasks {
            task.await
                .expect("join API call")
                .expect("workload API call");
        }
        assert_eq!(providers.load(Ordering::SeqCst), 1);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(api_calls.load(Ordering::SeqCst), 16);
        assert!(
            headers
                .lock()
                .expect("headers lock")
                .iter()
                .all(|header| header == "Bearer access_one")
        );
        let requests = exchange_requests.lock().expect("exchange requests");
        assert_eq!(requests[0]["grant_type"], TOKEN_EXCHANGE_GRANT_TYPE);
        assert_eq!(requests[0]["subject_token_type"], JWT_TOKEN_TYPE);
        assert_eq!(requests[0]["identity_provider_id"], "idp_test");
        assert_eq!(requests[0]["service_account_id"], "svc_test");
        assert_eq!(requests[0]["client_id"], "client_test");
    }

    #[tokio::test]
    async fn concurrent_waiters_share_one_provider_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider_calls = Arc::clone(&calls);
        let provider = SubjectTokenProviderFn::new(SubjectTokenType::Id, move || {
            let provider_calls = Arc::clone(&provider_calls);
            async move {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err(SubjectTokenProviderError::new())
            }
        });
        let config =
            WorkloadIdentityConfig::new("idp_test", "svc_test", provider).expect("workload config");
        let auth =
            WorkloadIdentityAuth::new(config, None, Duration::from_secs(1), Duration::from_secs(1))
                .expect("workload auth");
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let auth = Arc::clone(&auth);
            tasks.push(tokio::spawn(async move { auth.token().await }));
        }
        for task in tasks {
            assert!(task.await.expect("join provider waiter").is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn api_401_invalidates_generation_and_replays_once() {
        let (exchange_url, exchanges, _) = exchange_server(vec![
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_one","expires_in":3600}"#.to_owned(),
                location: None,
            },
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_two","expires_in":3600}"#.to_owned(),
                location: None,
            },
        ])
        .await;
        let (api_url, api_calls, headers) =
            api_server(true, r#"{"object":"list","data":[]}"#).await;
        let client =
            Client::workload_identity_builder(config(exchange_url, Arc::new(AtomicUsize::new(0))))
                .base_url(api_url)
                .allow_insecure_loopback(true)
                .build()
                .expect("workload client");
        client.models().list().await.expect("401 replay succeeds");

        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(api_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            headers.lock().expect("headers lock").as_slice(),
            ["Bearer access_one", "Bearer access_two"]
        );
    }

    #[tokio::test]
    async fn late_old_generation_cannot_evict_a_new_token() {
        let (exchange_url, exchanges, _) = exchange_server(vec![
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_one","expires_in":3600}"#.to_owned(),
                location: None,
            },
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_two","expires_in":3600}"#.to_owned(),
                location: None,
            },
        ])
        .await;
        let auth = WorkloadIdentityAuth::new(
            config(exchange_url, Arc::new(AtomicUsize::new(0))),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("workload auth");
        let old = auth.token().await.expect("old token");
        assert!(
            auth.invalidate_if_generation(old.generation.expect("workload generation"))
                .await
        );
        let new = auth.token().await.expect("new token");
        assert_eq!(new.header.to_str().ok(), Some("Bearer access_two"));
        assert!(
            !auth
                .invalidate_if_generation(old.generation.expect("old generation"))
                .await
        );
        let still_new = auth.token().await.expect("still-new token");
        assert_eq!(still_new.header.to_str().ok(), Some("Bearer access_two"));
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn multipart_replayable_refreshes_once_but_one_shot_never_replays() {
        const FILE_RESPONSE: &str = r#"{"id":"file_1","object":"file","bytes":3,"created_at":1,"filename":"input.jsonl","purpose":"batch","status":"processed"}"#;
        let (exchange_url, exchanges, _) = exchange_server(vec![
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_one","expires_in":3600}"#.to_owned(),
                location: None,
            },
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_two","expires_in":3600}"#.to_owned(),
                location: None,
            },
        ])
        .await;
        let (api_url, api_calls, headers) = api_server(true, FILE_RESPONSE).await;
        let client =
            Client::workload_identity_builder(config(exchange_url, Arc::new(AtomicUsize::new(0))))
                .base_url(api_url)
                .allow_insecure_loopback(true)
                .build()
                .expect("workload client");
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"abc"[..]));
        client
            .files()
            .create(CreateFileRequest::new(source, FilePurpose::Batch))
            .await
            .expect("replayable multipart refresh");
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(api_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            headers.lock().expect("headers lock").as_slice(),
            ["Bearer access_one", "Bearer access_two"]
        );

        let (exchange_url, exchanges, _) = exchange_server(vec![Reply {
            status: StatusCode::OK,
            body: r#"{"access_token":"access_one","expires_in":3600}"#.to_owned(),
            location: None,
        }])
        .await;
        let (api_url, api_calls, _) = api_server(true, FILE_RESPONSE).await;
        let client =
            Client::workload_identity_builder(config(exchange_url, Arc::new(AtomicUsize::new(0))))
                .base_url(api_url)
                .allow_insecure_loopback(true)
                .build()
                .expect("workload client");
        let source = OneShotMultipartSource::from_reader(tokio::io::empty());
        let result = client
            .files()
            .create_one_shot(CreateFileOneShotRequest::new(source, FilePurpose::Batch))
            .await;
        assert!(result.is_err());
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(api_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn proactive_refresh_uses_half_life_cap_and_returns_stale_token() {
        let (exchange_url, exchanges, _) = exchange_server(vec![
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_one","expires_in":0.2}"#.to_owned(),
                location: None,
            },
            Reply {
                status: StatusCode::OK,
                body: r#"{"access_token":"access_two","expires_in":3600}"#.to_owned(),
                location: None,
            },
        ])
        .await;
        let auth = WorkloadIdentityAuth::new(
            config(exchange_url, Arc::new(AtomicUsize::new(0)))
                .with_refresh_buffer(Duration::from_secs(10)),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("workload auth");
        let first = auth.token().await.expect("first token");
        assert_eq!(first.header, "Bearer access_one");
        tokio::time::sleep(Duration::from_millis(120)).await;
        let stale = auth.token().await.expect("stale token");
        assert_eq!(stale.header, "Bearer access_one");
        let mut saw_refreshed = false;
        for _ in 0..40 {
            let refreshed = auth.token().await.expect("refreshed token");
            if refreshed.header.to_str().ok() == Some("Bearer access_two") {
                saw_refreshed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(saw_refreshed);
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn bad_status_token_and_redirect_fail_closed_without_leaks() {
        let (exchange_url, _, _) = exchange_server(vec![Reply {
            status: StatusCode::BAD_REQUEST,
            body: r#"{"error":"invalid_subject","subject_token":"subject.jwt.token"}"#.to_owned(),
            location: None,
        }])
        .await;
        let auth = WorkloadIdentityAuth::new(
            config(exchange_url, Arc::new(AtomicUsize::new(0))),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("workload auth");
        let error = match auth.token().await {
            Err(error) => error,
            Ok(_) => panic!("rejected exchange unexpectedly succeeded"),
        };
        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        let debug = format!("{error:?}");
        assert!(!debug.contains("subject.jwt.token"));
        assert!(!debug.contains("idp_test"));

        let (bad_url, _, _) = exchange_server(vec![Reply {
            status: StatusCode::OK,
            body: r#"{"access_token":" bad token ","expires_in":3600}"#.to_owned(),
            location: None,
        }])
        .await;
        let auth = WorkloadIdentityAuth::new(
            config(bad_url, Arc::new(AtomicUsize::new(0))),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("workload auth");
        assert!(auth.token().await.is_err());

        let target = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("redirect target");
        let target_url = format!(
            "http://{}/stolen",
            target.local_addr().expect("target addr")
        );
        let (redirect_url, _, _) = exchange_server(vec![Reply {
            status: StatusCode::TEMPORARY_REDIRECT,
            body: String::new(),
            location: Some(target_url),
        }])
        .await;
        let auth = WorkloadIdentityAuth::new(
            config(redirect_url, Arc::new(AtomicUsize::new(0))),
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("workload auth");
        assert!(auth.token().await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), target.accept())
                .await
                .is_err(),
            "redirect target must never receive the subject token"
        );
    }
}
