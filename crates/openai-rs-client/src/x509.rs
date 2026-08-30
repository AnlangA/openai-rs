//! Preview X.509 workload-identity client.
//!
//! This client is intentionally separate from [`crate::Client`]. It only uses
//! pinned OpenAI mTLS origins, owns no custom transport escape hatch, and does
//! not expose Realtime or arbitrary raw requests.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use http::{HeaderValue, Method, StatusCode, header};
use openai_rs_types::{
    Model, ModelId, ModelList, ResponseId,
    responses::{
        CompactResponseRequest, CompactedResponse, CountInputTokensRequest, CreateResponseRequest,
        InputTokenCountResponse, Response,
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error as ThisError;
use tokio::sync::{Mutex, Notify};
use url::Url;
use zeroize::Zeroizing;

use crate::{ApiError, ApiResponse, BodyPreview, Error, ResponseMeta, transport::deserialize_json};

const GLOBAL_API_BASE: &str = "https://mtls.api.openai.com/v1/";
const US_API_BASE: &str = "https://mtls-us.api.openai.com/v1/";
const EU_API_BASE: &str = "https://mtls-eu.api.openai.com/v1/";
const TOKEN_EXCHANGE_URL: &str = "https://mtls.auth.openai.com/oauth/token";
const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const X509_SUBJECT_TOKEN_TYPE: &str = "urn:openai:params:oauth:token-type:x509";
const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";
const DEFAULT_REFRESH_BUFFER: Duration = Duration::from_secs(1_200);
const TOKEN_EXCHANGE_DEADLINE: Duration = Duration::from_secs(5);
const MAX_TOKEN_LIFETIME: Duration = Duration::from_secs(3_600);
const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_MAX_JSON_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const JSON_MIME: &str = "application/json";
const DECODE_PREVIEW_BYTES: usize = 8 * 1024;

type ExchangeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ExchangedToken, X509Error>> + Send + 'a>>;

/// Pinned OpenAI mTLS data-residency origin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum X509Region {
    #[default]
    Global,
    Us,
    Eu,
}

impl X509Region {
    const fn base_url(self) -> &'static str {
        match self {
            Self::Global => GLOBAL_API_BASE,
            Self::Us => US_API_BASE,
            Self::Eu => EU_API_BASE,
        }
    }
}

/// Caller-owned PEM bundle containing a certificate chain and exactly one
/// unencrypted private key.
pub struct X509IdentityPem(Zeroizing<Vec<u8>>);

impl X509IdentityPem {
    /// Performs a structural preflight. Full cryptographic parsing happens when
    /// [`X509ClientBuilder::build`] creates the rustls identity.
    pub fn new(pem: impl Into<Vec<u8>>) -> Result<Self, X509Error> {
        let pem = Zeroizing::new(pem.into());
        let text = std::str::from_utf8(&pem).map_err(|_| X509Error::InvalidIdentity)?;
        let certificates = text.matches("-----BEGIN CERTIFICATE-----").count();
        let private_keys = [
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----BEGIN EC PRIVATE KEY-----",
        ]
        .iter()
        .map(|marker| text.matches(marker).count())
        .sum::<usize>();
        if certificates == 0
            || private_keys != 1
            || text.contains("-----BEGIN ENCRYPTED PRIVATE KEY-----")
        {
            return Err(X509Error::InvalidIdentity);
        }
        Ok(Self(pem))
    }

    fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for X509IdentityPem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("X509IdentityPem([REDACTED])")
    }
}

/// Builder for the isolated X.509 preview client.
pub struct X509ClientBuilder {
    identity: X509IdentityPem,
    identity_provider_id: Box<str>,
    service_account_id: Box<str>,
    region: X509Region,
    refresh_buffer: Duration,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
}

impl X509ClientBuilder {
    /// Creates a builder from a combined PEM identity and enrolled OpenAI IDs.
    pub fn new(
        identity: X509IdentityPem,
        identity_provider_id: impl Into<Box<str>>,
        service_account_id: impl Into<Box<str>>,
    ) -> Result<Self, X509Error> {
        let identity_provider_id = identity_provider_id.into();
        let service_account_id = service_account_id.into();
        validate_selector(&identity_provider_id)?;
        validate_selector(&service_account_id)?;
        Ok(Self {
            identity,
            identity_provider_id,
            service_account_id,
            region: X509Region::Global,
            refresh_buffer: DEFAULT_REFRESH_BUFFER,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_json_body_bytes: DEFAULT_MAX_JSON_BODY_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
        })
    }

    /// Selects one of the three pinned mTLS API origins.
    #[must_use]
    pub const fn region(mut self, region: X509Region) -> Self {
        self.region = region;
        self
    }

    /// Sets proactive refresh lead time. It is capped at half of each issued
    /// token's actual lifetime.
    #[must_use]
    pub const fn refresh_buffer(mut self, refresh_buffer: Duration) -> Self {
        self.refresh_buffer = refresh_buffer;
        self
    }

    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn max_json_body_bytes(mut self, limit: usize) -> Self {
        self.max_json_body_bytes = limit;
        self
    }

    #[must_use]
    pub const fn max_error_body_bytes(mut self, limit: usize) -> Self {
        self.max_error_body_bytes = limit;
        self
    }

    /// Builds a rustls client whose certificate identity is shared by token
    /// exchange and every API request.
    pub fn build(self) -> Result<X509Client, X509Error> {
        if self.connect_timeout.is_zero()
            || self.request_timeout.is_zero()
            || self.max_json_body_bytes == 0
            || self.max_error_body_bytes == 0
        {
            return Err(X509Error::InvalidConfiguration);
        }
        let identity = reqwest::Identity::from_pem(self.identity.expose())
            .map_err(|_| X509Error::InvalidIdentity)?;
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .identity(identity)
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(concat!(
                "openai-rs-x509-preview/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| X509Error::InvalidIdentity)?;
        let exchange = Arc::new(HttpX509Exchange {
            http: http.clone(),
            identity_provider_id: self.identity_provider_id,
            service_account_id: self.service_account_id,
        });
        let token_manager = Arc::new(X509TokenManager::new(exchange, self.refresh_buffer));
        let base_url =
            Url::parse(self.region.base_url()).map_err(|_| X509Error::InvalidConfiguration)?;
        Ok(X509Client {
            inner: Arc::new(X509Inner {
                http,
                base_url,
                region: self.region,
                token_manager,
                request_timeout: self.request_timeout,
                max_json_body_bytes: self.max_json_body_bytes,
                max_error_body_bytes: self.max_error_body_bytes,
            }),
        })
    }
}

impl fmt::Debug for X509ClientBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("X509ClientBuilder")
            .field("identity", &"[REDACTED]")
            .field("identity_provider_id", &"[REDACTED]")
            .field("service_account_id", &"[REDACTED]")
            .field("region", &self.region)
            .field("refresh_buffer", &self.refresh_buffer)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .finish()
    }
}

/// Isolated preview client for certificate-authenticated Platform REST calls.
#[derive(Clone)]
pub struct X509Client {
    inner: Arc<X509Inner>,
}

struct X509Inner {
    http: reqwest::Client,
    base_url: Url,
    region: X509Region,
    token_manager: Arc<X509TokenManager>,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
}

impl X509Client {
    pub fn builder(
        identity: X509IdentityPem,
        identity_provider_id: impl Into<Box<str>>,
        service_account_id: impl Into<Box<str>>,
    ) -> Result<X509ClientBuilder, X509Error> {
        X509ClientBuilder::new(identity, identity_provider_id, service_account_id)
    }

    #[must_use]
    pub fn region(&self) -> X509Region {
        self.inner.region
    }

    /// Non-streaming Responses operations supported by the preview boundary.
    #[must_use]
    pub fn responses(&self) -> X509Responses {
        X509Responses {
            client: self.clone(),
        }
    }

    /// Read-only Models operations supported by the preview boundary.
    #[must_use]
    pub fn models(&self) -> X509Models {
        X509Models {
            client: self.clone(),
        }
    }

    async fn execute_json<Q, R>(
        &self,
        method: Method,
        path: &[RouteSegment<'_>],
        body: Option<&Q>,
    ) -> Result<ApiResponse<R>, X509Error>
    where
        Q: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = operation_url(&self.inner.base_url, path)?;
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(Error::Encode)?;
        let started = Instant::now();
        let mut auth_replayed = false;
        loop {
            let remaining = self
                .inner
                .request_timeout
                .checked_sub(started.elapsed())
                .filter(|value| !value.is_zero())
                .ok_or_else(|| X509Error::from(Error::DeadlineExceeded))?;
            let lease = self.inner.token_manager.lease().await?;
            let mut request = self
                .inner
                .http
                .request(method.clone(), url.clone())
                .timeout(remaining)
                .header(header::AUTHORIZATION, lease.header.clone())
                .header(header::ACCEPT, JSON_MIME);
            if let Some(encoded) = &encoded {
                request = request
                    .header(header::CONTENT_TYPE, JSON_MIME)
                    .body(encoded.clone());
            }
            let response = request.send().await.map_err(safe_transport_error)?;
            if response.status() == StatusCode::UNAUTHORIZED && !auth_replayed {
                drop(response);
                let _ = self
                    .inner
                    .token_manager
                    .invalidate_if_generation(lease.generation)
                    .await;
                auth_replayed = true;
                continue;
            }
            if response.status() == StatusCode::OK {
                return decode_api_response(response, self.inner.max_json_body_bytes).await;
            }
            return Err(read_api_error(response, self.inner.max_error_body_bytes).await);
        }
    }
}

impl fmt::Debug for X509Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("X509Client")
            .field("region", &self.inner.region)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct X509Responses {
    client: X509Client,
}

impl X509Responses {
    pub async fn create(
        &self,
        request: CreateResponseRequest,
    ) -> Result<ApiResponse<Response>, X509Error> {
        self.client
            .execute_json(
                Method::POST,
                &[RouteSegment::literal("responses")],
                Some(&request),
            )
            .await
    }

    pub async fn retrieve(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<Response>, X509Error> {
        self.client
            .execute_json::<(), _>(
                Method::GET,
                &[
                    RouteSegment::literal("responses"),
                    RouteSegment::parameter("response_id", response_id.as_str())?,
                ],
                None,
            )
            .await
    }

    pub async fn cancel(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<Response>, X509Error> {
        self.client
            .execute_json::<(), _>(
                Method::POST,
                &[
                    RouteSegment::literal("responses"),
                    RouteSegment::parameter("response_id", response_id.as_str())?,
                    RouteSegment::literal("cancel"),
                ],
                None,
            )
            .await
    }

    pub async fn compact(
        &self,
        request: CompactResponseRequest,
    ) -> Result<ApiResponse<CompactedResponse>, X509Error> {
        self.client
            .execute_json(
                Method::POST,
                &[
                    RouteSegment::literal("responses"),
                    RouteSegment::literal("compact"),
                ],
                Some(&request),
            )
            .await
    }

    pub async fn count_input_tokens(
        &self,
        request: CountInputTokensRequest,
    ) -> Result<ApiResponse<InputTokenCountResponse>, X509Error> {
        self.client
            .execute_json(
                Method::POST,
                &[
                    RouteSegment::literal("responses"),
                    RouteSegment::literal("input_tokens"),
                ],
                Some(&request),
            )
            .await
    }
}

#[derive(Clone, Debug)]
pub struct X509Models {
    client: X509Client,
}

impl X509Models {
    pub async fn list(&self) -> Result<ApiResponse<ModelList>, X509Error> {
        self.client
            .execute_json::<(), _>(Method::GET, &[RouteSegment::literal("models")], None)
            .await
    }

    pub async fn retrieve(&self, model: &ModelId) -> Result<ApiResponse<Model>, X509Error> {
        self.client
            .execute_json::<(), _>(
                Method::GET,
                &[
                    RouteSegment::literal("models"),
                    RouteSegment::parameter("model", model.as_str())?,
                ],
                None,
            )
            .await
    }
}

#[derive(Clone)]
struct TokenLease {
    header: HeaderValue,
    generation: u64,
}

struct CachedToken {
    token: SecretString,
    generation: u64,
    expires_at: Instant,
    refresh_at: Instant,
}

struct TokenState {
    generation: u64,
    next_attempt: u64,
    active_attempt: Option<u64>,
    completed_attempt: Option<(u64, Result<TokenLease, X509Error>)>,
    cached: Option<CachedToken>,
}

struct X509TokenManager {
    exchange: Arc<dyn X509Exchange>,
    refresh_buffer: Duration,
    state: Mutex<TokenState>,
    notify: Notify,
}

impl X509TokenManager {
    fn new(exchange: Arc<dyn X509Exchange>, refresh_buffer: Duration) -> Self {
        Self {
            exchange,
            refresh_buffer,
            state: Mutex::new(TokenState {
                generation: 0,
                next_attempt: 0,
                active_attempt: None,
                completed_attempt: None,
                cached: None,
            }),
            notify: Notify::new(),
        }
    }

    async fn lease(self: &Arc<Self>) -> Result<TokenLease, X509Error> {
        enum Action {
            Wait(u64),
            Start {
                attempt: u64,
                generation: u64,
                fallback: Option<(SecretString, u64, Instant)>,
            },
        }

        loop {
            let action = {
                let mut state = self.state.lock().await;
                if let Some(cached) = &state.cached
                    && Instant::now() < cached.refresh_at
                {
                    return token_lease(cached);
                }
                if let Some(attempt) = state.active_attempt {
                    Action::Wait(attempt)
                } else {
                    state.next_attempt = state.next_attempt.wrapping_add(1);
                    let attempt = state.next_attempt;
                    state.active_attempt = Some(attempt);
                    state.completed_attempt = None;
                    let fallback = state.cached.as_ref().and_then(|cached| {
                        (Instant::now() < cached.expires_at)
                            .then(|| (cached.token.clone(), cached.generation, cached.expires_at))
                    });
                    Action::Start {
                        attempt,
                        generation: state.generation,
                        fallback,
                    }
                }
            };

            let attempt = match action {
                Action::Wait(attempt) => attempt,
                Action::Start {
                    attempt,
                    generation,
                    fallback,
                } => {
                    let manager = Arc::clone(self);
                    tokio::spawn(async move {
                        let exchanged = manager.exchange.exchange().await;
                        manager
                            .complete_attempt(attempt, generation, fallback, exchanged)
                            .await;
                    });
                    attempt
                }
            };
            match self.wait_for_attempt(attempt).await {
                Ok(lease) => return Ok(lease),
                Err(X509Error::Invalidated) => continue,
                Err(error) => return Err(error),
            }
        }
    }

    async fn wait_for_attempt(&self, attempt: u64) -> Result<TokenLease, X509Error> {
        loop {
            let notified = self.notify.notified();
            {
                let state = self.state.lock().await;
                if let Some((completed, result)) = &state.completed_attempt
                    && *completed == attempt
                {
                    return result.clone();
                }
                if state.active_attempt != Some(attempt) {
                    return Err(X509Error::Invalidated);
                }
            }
            notified.await;
        }
    }

    async fn complete_attempt(
        &self,
        attempt: u64,
        generation: u64,
        fallback: Option<(SecretString, u64, Instant)>,
        exchanged: Result<ExchangedToken, X509Error>,
    ) {
        let mut state = self.state.lock().await;
        let result = if state.generation != generation || state.active_attempt != Some(attempt) {
            Err(X509Error::Invalidated)
        } else {
            match exchanged {
                Ok(exchanged) => {
                    let now = Instant::now();
                    let expires_at = exchanged.started_at + exchanged.lifetime;
                    if now >= expires_at {
                        state.cached = None;
                        Err(X509Error::InvalidTokenLifetime)
                    } else {
                        state.generation = state.generation.wrapping_add(1);
                        let generation = state.generation;
                        let refresh_buffer =
                            self.refresh_buffer.min(exchanged.lifetime.div_f64(2.0));
                        let refresh_at = expires_at - refresh_buffer;
                        state.cached = Some(CachedToken {
                            token: exchanged.token,
                            generation,
                            expires_at,
                            refresh_at,
                        });
                        match state.cached.as_ref() {
                            Some(cached) => token_lease(cached),
                            None => Err(X509Error::Invalidated),
                        }
                    }
                }
                Err(error) if error.is_retryable() => {
                    if let Some((token, generation, expires_at)) = fallback
                        && Instant::now() < expires_at
                    {
                        let refresh_at = Instant::now()
                            .checked_add(Duration::from_millis(500))
                            .unwrap_or(expires_at)
                            .min(expires_at);
                        state.cached = Some(CachedToken {
                            token,
                            generation,
                            expires_at,
                            refresh_at,
                        });
                        match state.cached.as_ref() {
                            Some(cached) => token_lease(cached),
                            None => Err(X509Error::Invalidated),
                        }
                    } else {
                        Err(error)
                    }
                }
                Err(error) => Err(error),
            }
        };
        if state.active_attempt == Some(attempt) {
            state.active_attempt = None;
        }
        state.completed_attempt = Some((attempt, result));
        drop(state);
        self.notify.notify_waiters();
    }

    async fn invalidate_if_generation(&self, generation: u64) -> bool {
        let mut state = self.state.lock().await;
        if state.cached.as_ref().map(|cached| cached.generation) != Some(generation) {
            return false;
        }
        state.generation = state.generation.wrapping_add(1);
        state.cached = None;
        state.active_attempt = None;
        state.completed_attempt = None;
        drop(state);
        self.notify.notify_waiters();
        true
    }
}

fn token_lease(cached: &CachedToken) -> Result<TokenLease, X509Error> {
    let value = Zeroizing::new(format!("Bearer {}", cached.token.expose_secret()));
    let mut header = HeaderValue::from_str(&value).map_err(|_| X509Error::InvalidAccessToken)?;
    header.set_sensitive(true);
    Ok(TokenLease {
        header,
        generation: cached.generation,
    })
}

trait X509Exchange: Send + Sync {
    fn exchange(&self) -> ExchangeFuture<'_>;
}

struct HttpX509Exchange {
    http: reqwest::Client,
    identity_provider_id: Box<str>,
    service_account_id: Box<str>,
}

impl X509Exchange for HttpX509Exchange {
    fn exchange(&self) -> ExchangeFuture<'_> {
        Box::pin(async move {
            let started_at = Instant::now();
            let body = TokenExchangeRequest {
                grant_type: TOKEN_EXCHANGE_GRANT,
                subject_token_type: X509_SUBJECT_TOKEN_TYPE,
                identity_provider_id: &self.identity_provider_id,
                service_account_id: &self.service_account_id,
            };
            let response = self
                .http
                .post(TOKEN_EXCHANGE_URL)
                .timeout(TOKEN_EXCHANGE_DEADLINE)
                .header(header::CONTENT_TYPE, JSON_MIME)
                .json(&body)
                .send()
                .await
                .map_err(|error| {
                    if error.is_timeout() {
                        X509Error::ExchangeTimeout
                    } else {
                        X509Error::ExchangeTransport
                    }
                })?;
            let status = response.status();
            let bytes = read_bounded(response, MAX_TOKEN_RESPONSE_BYTES)
                .await
                .map_err(|_| X509Error::ExchangeTransport)?;
            if matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return Err(X509Error::OAuth {
                    status,
                    code: safe_oauth_code(&bytes),
                });
            }
            if !status.is_success() {
                return Err(X509Error::ExchangeStatus(status));
            }
            validate_exchange_response(&bytes, started_at)
        })
    }
}

#[derive(Serialize)]
struct TokenExchangeRequest<'a> {
    grant_type: &'static str,
    subject_token_type: &'static str,
    identity_provider_id: &'a str,
    service_account_id: &'a str,
}

#[derive(Deserialize)]
struct TokenExchangeResponse {
    access_token: String,
    token_type: String,
    issued_token_type: String,
    expires_in: f64,
}

struct ExchangedToken {
    token: SecretString,
    lifetime: Duration,
    started_at: Instant,
}

fn validate_exchange_response(
    bytes: &[u8],
    started_at: Instant,
) -> Result<ExchangedToken, X509Error> {
    let response: TokenExchangeResponse =
        serde_json::from_slice(bytes).map_err(|_| X509Error::InvalidExchangeResponse)?;
    if !response.token_type.eq_ignore_ascii_case("bearer")
        || response.issued_token_type != ACCESS_TOKEN_TYPE
    {
        return Err(X509Error::InvalidTokenType);
    }
    if !response.expires_in.is_finite()
        || response.expires_in <= 0.0
        || response.expires_in > MAX_TOKEN_LIFETIME.as_secs_f64()
    {
        return Err(X509Error::InvalidTokenLifetime);
    }
    if !is_safe_bearer_token(&response.access_token) {
        return Err(X509Error::InvalidAccessToken);
    }
    let lifetime = Duration::try_from_secs_f64(response.expires_in)
        .map_err(|_| X509Error::InvalidTokenLifetime)?;
    Ok(ExchangedToken {
        token: SecretString::from(response.access_token),
        lifetime,
        started_at,
    })
}

fn is_safe_bearer_token(value: &str) -> bool {
    let prefix = value.trim_end_matches('=');
    !prefix.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'~' | b'+' | b'/' | b'-' | b'=')
        })
        && !prefix.contains('=')
}

fn safe_oauth_code(bytes: &[u8]) -> Option<X509OAuthCode> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let error = value.get("error")?;
    let code = error
        .as_str()
        .or_else(|| error.get("code").and_then(serde_json::Value::as_str))?;
    match code {
        "invalid_grant" => Some(X509OAuthCode::InvalidGrant),
        "invalid_subject_token" => Some(X509OAuthCode::InvalidSubjectToken),
        "token_exchange_server_error" => Some(X509OAuthCode::TokenExchangeServerError),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum X509OAuthCode {
    InvalidGrant,
    InvalidSubjectToken,
    TokenExchangeServerError,
}

/// Sanitized failures from the X.509 preview boundary.
#[derive(Clone, Debug, ThisError)]
#[non_exhaustive]
pub enum X509Error {
    #[error("invalid X.509 client identity")]
    InvalidIdentity,
    #[error("invalid X.509 client configuration")]
    InvalidConfiguration,
    #[error("invalid X.509 identity selector")]
    InvalidSelector,
    #[error("X.509 token exchange timed out")]
    ExchangeTimeout,
    #[error("X.509 token exchange transport failed")]
    ExchangeTransport,
    #[error("X.509 token exchange returned OAuth status {status}")]
    OAuth {
        status: StatusCode,
        code: Option<X509OAuthCode>,
    },
    #[error("X.509 token exchange returned HTTP {0}")]
    ExchangeStatus(StatusCode),
    #[error("X.509 token exchange returned invalid JSON")]
    InvalidExchangeResponse,
    #[error("X.509 token exchange returned an invalid access token")]
    InvalidAccessToken,
    #[error("X.509 token exchange returned an invalid token type")]
    InvalidTokenType,
    #[error("X.509 token exchange returned an invalid token lifetime")]
    InvalidTokenLifetime,
    #[error("X.509 token generation was invalidated")]
    Invalidated,
    #[error(transparent)]
    Api(Arc<Error>),
    #[error("X.509 API response body could not be read (status {0})")]
    ApiResponseBody(StatusCode),
    #[error("X.509 API response body exceeded the configured limit")]
    ApiBodyTooLarge,
}

impl From<Error> for X509Error {
    fn from(error: Error) -> Self {
        Self::Api(Arc::new(error))
    }
}

impl X509Error {
    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ExchangeTimeout
                | Self::ExchangeTransport
                | Self::ExchangeStatus(StatusCode::REQUEST_TIMEOUT)
                | Self::ExchangeStatus(StatusCode::CONFLICT)
                | Self::ExchangeStatus(StatusCode::TOO_MANY_REQUESTS)
        ) || matches!(self, Self::ExchangeStatus(status) if status.is_server_error())
    }
}

#[derive(Clone, Copy)]
enum RouteSegment<'a> {
    Literal(&'static str),
    Parameter(&'a str),
}

impl<'a> RouteSegment<'a> {
    const fn literal(value: &'static str) -> Self {
        Self::Literal(value)
    }

    fn parameter(_name: &'static str, value: &'a str) -> Result<Self, X509Error> {
        if value.is_empty() || matches!(value, "." | "..") || value.chars().any(char::is_control) {
            return Err(X509Error::InvalidConfiguration);
        }
        Ok(Self::Parameter(value))
    }
}

fn operation_url(base: &Url, path: &[RouteSegment<'_>]) -> Result<Url, X509Error> {
    let mut url = base.clone();
    let mut segments = url
        .path_segments_mut()
        .map_err(|()| X509Error::InvalidConfiguration)?;
    segments.pop_if_empty();
    for segment in path {
        match segment {
            RouteSegment::Literal(value) => segments.push(value),
            RouteSegment::Parameter(value) => segments.push(value),
        };
    }
    drop(segments);
    if url.origin() != base.origin() {
        return Err(X509Error::InvalidConfiguration);
    }
    Ok(url)
}

fn validate_selector(value: &str) -> Result<(), X509Error> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(|character| character.is_control())
    {
        Err(X509Error::InvalidSelector)
    } else {
        Ok(())
    }
}

async fn decode_api_response<T>(
    response: reqwest::Response,
    limit: usize,
) -> Result<ApiResponse<T>, X509Error>
where
    T: DeserializeOwned,
{
    let meta = ResponseMeta::from_headers(response.status(), response.headers());
    let body = read_success(response, limit, &meta).await?;
    let decoded = deserialize_json(&body).map_err(|failure| Error::Decode {
        source: failure.source,
        path: failure.path,
        meta_status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(
            &body[..body.len().min(DECODE_PREVIEW_BYTES)],
            body.len() > DECODE_PREVIEW_BYTES,
        ),
    })?;
    Ok(ApiResponse::new(decoded, meta))
}

async fn read_api_error(response: reqwest::Response, limit: usize) -> X509Error {
    let meta = ResponseMeta::from_headers(response.status(), response.headers());
    match read_up_to(response, limit).await {
        Ok((body, truncated)) => {
            X509Error::from(Error::from(ApiError::from_body(meta, &body, truncated)))
        }
        Err(_) => X509Error::ApiResponseBody(meta.status()),
    }
}

async fn read_success(
    response: reqwest::Response,
    limit: usize,
    meta: &ResponseMeta,
) -> Result<Vec<u8>, X509Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(X509Error::ApiBodyTooLarge);
    }
    let (body, truncated) = read_up_to(response, limit)
        .await
        .map_err(|_| X509Error::ApiResponseBody(meta.status()))?;
    if truncated {
        Err(X509Error::ApiBodyTooLarge)
    } else {
        Ok(body)
    }
}

async fn read_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ()> {
    let (body, truncated) = read_up_to(response, limit).await.map_err(|_| ())?;
    (!truncated).then_some(body).ok_or(())
}

async fn read_up_to(
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

fn safe_transport_error(error: reqwest::Error) -> X509Error {
    X509Error::from(Error::from_reqwest(error))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(X509Client: Send, Sync, Clone);
    assert_impl_all!(X509Region: Send, Sync, Copy);

    struct FakeExchange {
        calls: AtomicUsize,
        delay: Duration,
        results: StdMutex<VecDeque<Result<(&'static str, Duration), X509Error>>>,
    }

    impl FakeExchange {
        fn new(
            delay: Duration,
            results: impl IntoIterator<Item = Result<(&'static str, Duration), X509Error>>,
        ) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                delay,
                results: StdMutex::new(results.into_iter().collect()),
            }
        }
    }

    impl X509Exchange for FakeExchange {
        fn exchange(&self) -> ExchangeFuture<'_> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if !self.delay.is_zero() {
                    tokio::time::sleep(self.delay).await;
                }
                let result = self
                    .results
                    .lock()
                    .expect("fake exchange lock")
                    .pop_front()
                    .unwrap_or(Err(X509Error::ExchangeTransport))?;
                Ok(ExchangedToken {
                    token: SecretString::from(result.0.to_owned()),
                    lifetime: result.1,
                    started_at: Instant::now(),
                })
            })
        }
    }

    fn structural_pem() -> Vec<u8> {
        b"-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----\n\
-----BEGIN PRIVATE KEY-----\nZmFrZQ==\n-----END PRIVATE KEY-----\n"
            .to_vec()
    }

    #[test]
    fn pem_preflight_and_debug_are_secret_safe() {
        let identity = X509IdentityPem::new(structural_pem()).expect("structural PEM");
        let debug = format!("{identity:?}");
        assert_eq!(debug, "X509IdentityPem([REDACTED])");
        assert!(!debug.contains("PRIVATE KEY"));

        assert!(X509IdentityPem::new(b"-----BEGIN CERTIFICATE-----\n".to_vec()).is_err());
        assert!(
            X509IdentityPem::new(
                b"-----BEGIN CERTIFICATE-----\n-----BEGIN ENCRYPTED PRIVATE KEY-----\n".to_vec()
            )
            .is_err()
        );

        let structurally_valid_but_fake =
            X509IdentityPem::new(structural_pem()).expect("structural fake PEM");
        let builder = X509ClientBuilder::new(structurally_valid_but_fake, "idp_test", "svc_test")
            .expect("safe selectors");
        assert!(matches!(builder.build(), Err(X509Error::InvalidIdentity)));
    }

    #[test]
    fn region_and_exchange_origins_are_fixed() {
        assert_eq!(X509Region::Global.base_url(), GLOBAL_API_BASE);
        assert_eq!(X509Region::Us.base_url(), US_API_BASE);
        assert_eq!(X509Region::Eu.base_url(), EU_API_BASE);
        assert_eq!(
            TOKEN_EXCHANGE_URL,
            "https://mtls.auth.openai.com/oauth/token"
        );

        let base = Url::parse(X509Region::Eu.base_url()).expect("EU mTLS URL");
        let operation = operation_url(
            &base,
            &[
                RouteSegment::literal("models"),
                RouteSegment::parameter("model", "model/a b").expect("model id"),
            ],
        )
        .expect("operation URL");
        assert_eq!(
            operation.as_str(),
            "https://mtls-eu.api.openai.com/v1/models/model%2Fa%20b"
        );
    }

    #[test]
    fn exchange_payload_matches_x509_token_exchange_contract() {
        let payload = TokenExchangeRequest {
            grant_type: TOKEN_EXCHANGE_GRANT,
            subject_token_type: X509_SUBJECT_TOKEN_TYPE,
            identity_provider_id: "idp_1",
            service_account_id: "svc_1",
        };
        assert_eq!(
            serde_json::to_value(payload).expect("serialize exchange payload"),
            serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
                "subject_token_type": "urn:openai:params:oauth:token-type:x509",
                "identity_provider_id": "idp_1",
                "service_account_id": "svc_1"
            })
        );
    }

    #[test]
    fn token_response_requires_bearer_access_token_and_short_lifetime() {
        let valid = serde_json::json!({
            "access_token": "abc.DEF_123-+/=",
            "token_type": "Bearer",
            "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
            "expires_in": 3600
        });
        let token = validate_exchange_response(
            &serde_json::to_vec(&valid).expect("encode valid token response"),
            Instant::now(),
        )
        .expect("valid exchange response");
        assert_eq!(token.lifetime, Duration::from_secs(3_600));

        for invalid in [
            serde_json::json!({
                "access_token":"bad token","token_type":"Bearer",
                "issued_token_type":ACCESS_TOKEN_TYPE,"expires_in":3600
            }),
            serde_json::json!({
                "access_token":"abc","token_type":"MAC",
                "issued_token_type":ACCESS_TOKEN_TYPE,"expires_in":3600
            }),
            serde_json::json!({
                "access_token":"abc","token_type":"Bearer",
                "issued_token_type":"urn:wrong","expires_in":3600
            }),
            serde_json::json!({
                "access_token":"abc","token_type":"Bearer",
                "issued_token_type":ACCESS_TOKEN_TYPE,"expires_in":3601
            }),
        ] {
            assert!(
                validate_exchange_response(
                    &serde_json::to_vec(&invalid).expect("encode invalid response"),
                    Instant::now(),
                )
                .is_err()
            );
        }
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_exchange() {
        let exchange = Arc::new(FakeExchange::new(
            Duration::from_millis(20),
            [Ok(("token-one", Duration::from_secs(3_600)))],
        ));
        let manager = Arc::new(X509TokenManager::new(exchange.clone(), Duration::ZERO));
        let (first, second, third) =
            tokio::join!(manager.lease(), manager.lease(), manager.lease());
        let first = first.expect("first lease");
        let second = second.expect("second lease");
        let third = third.expect("third lease");
        assert_eq!(first.generation, second.generation);
        assert_eq!(second.generation, third.generation);
        assert_eq!(exchange.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelling_first_waiter_does_not_poison_singleflight() {
        let exchange = Arc::new(FakeExchange::new(
            Duration::from_millis(50),
            [Ok(("token-one", Duration::from_secs(3_600)))],
        ));
        let manager = Arc::new(X509TokenManager::new(exchange.clone(), Duration::ZERO));
        let first_manager = Arc::clone(&manager);
        let first = tokio::spawn(async move { first_manager.lease().await });
        while exchange.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        first.abort();

        let replacement = tokio::time::timeout(Duration::from_secs(1), manager.lease())
            .await
            .expect("singleflight must not hang")
            .expect("shared exchange succeeds");
        assert!(replacement.generation > 0);
        assert_eq!(exchange.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_generation_cannot_invalidate_replacement_token() {
        let exchange = Arc::new(FakeExchange::new(
            Duration::ZERO,
            [
                Ok(("token-one", Duration::from_secs(3_600))),
                Ok(("token-two", Duration::from_secs(3_600))),
            ],
        ));
        let manager = Arc::new(X509TokenManager::new(exchange.clone(), Duration::ZERO));
        let first = manager.lease().await.expect("first lease");
        assert!(manager.invalidate_if_generation(first.generation).await);
        let second = manager.lease().await.expect("replacement lease");
        assert_ne!(first.generation, second.generation);
        assert!(!manager.invalidate_if_generation(first.generation).await);
        let current = manager.lease().await.expect("current lease");
        assert_eq!(current.generation, second.generation);
        assert_eq!(exchange.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn oauth_error_only_preserves_allowlisted_codes() {
        assert_eq!(
            safe_oauth_code(br#"{"error":"invalid_grant"}"#),
            Some(X509OAuthCode::InvalidGrant)
        );
        assert_eq!(
            safe_oauth_code(br#"{"error":"private_internal_detail"}"#),
            None
        );
        assert_eq!(safe_oauth_code(b"not-json"), None);
    }
}
