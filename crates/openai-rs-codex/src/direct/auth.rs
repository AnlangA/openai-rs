use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
#[cfg(feature = "experimental-direct-device")]
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use url::Url;

use super::jwt::{ChatGptAccountId, JsonWebKeySet, OidcVerifier};
use super::{CancellationToken, DirectError, secure_equal};

// Compatibility constants derived from anomalyco/opencode codex.ts at
// d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83 (MIT). These private experimental
// endpoints are not represented as stable OpenAI Platform API contracts.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE_ENDPOINT: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const JWKS_ENDPOINT: &str = "https://auth.openai.com/.well-known/jwks.json";
const AUTHORIZE_SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";
#[cfg(feature = "experimental-direct-device")]
const DEVICE_CODE_ENDPOINT: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
#[cfg(feature = "experimental-direct-device")]
const DEVICE_TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
#[cfg(feature = "experimental-direct-device")]
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
#[cfg(feature = "experimental-direct-device")]
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";

const CALLBACK_PATH: &str = "/auth/callback";
const DEFAULT_CALLBACK_PORT: u16 = 1455;
const FALLBACK_CALLBACK_PORT: u16 = 1457;
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const MAX_AUTH_BODY_BYTES: usize = 256 * 1024;
const DEFAULT_EXPIRES_IN: u64 = 3_600;
const REFRESH_SKEW: u64 = 60;
const SUCCESS_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization complete</title></head><body><h1>Authorization complete</h1><p>You may close this window.</p></body></html>";
const ERROR_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization failed</title></head><body><h1>Authorization failed</h1><p>Return to the application and try again.</p></body></html>";

/// A verified subscription session. Secret material is shared in protected
/// allocations and never implements Serde or Display.
#[derive(Clone)]
pub struct StoredCodexSession {
    pub(crate) access_token: Arc<SecretString>,
    pub(crate) refresh_token: Arc<SecretString>,
    pub(crate) expires_at: u64,
    pub(crate) account_id: ChatGptAccountId,
    pub(crate) generation: u64,
}

impl StoredCodexSession {
    #[must_use]
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    #[must_use]
    pub fn account_id(&self) -> &ChatGptAccountId {
        &self.account_id
    }

    pub(crate) fn access_token(&self) -> &str {
        self.access_token.expose_secret()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn refresh_token(&self) -> &str {
        self.refresh_token.expose_secret()
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        access_token: &str,
        refresh_token: &str,
        expires_at: u64,
        account_id: ChatGptAccountId,
    ) -> Self {
        Self {
            access_token: Arc::new(SecretString::from(access_token.to_owned())),
            refresh_token: Arc::new(SecretString::from(refresh_token.to_owned())),
            expires_at,
            account_id,
            generation: 0,
        }
    }
}

impl std::fmt::Debug for StoredCodexSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredCodexSession")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account_id", &"<redacted>")
            .field("generation", &self.generation)
            .finish()
    }
}

/// Async persistence boundary for one subscription account.
#[async_trait]
pub trait CredentialStore: Send + Sync + 'static {
    async fn load(&self) -> Result<Option<StoredCodexSession>, DirectError>;
    async fn save(&self, session: &StoredCodexSession) -> Result<(), DirectError>;
    async fn delete(&self) -> Result<(), DirectError>;
}

/// Process-local credential store for tests and explicit non-persistent use.
#[derive(Debug, Default)]
pub struct EphemeralStore {
    session: RwLock<Option<StoredCodexSession>>,
}

#[async_trait]
impl CredentialStore for EphemeralStore {
    async fn load(&self) -> Result<Option<StoredCodexSession>, DirectError> {
        Ok(self.session.read().await.clone())
    }

    async fn save(&self, session: &StoredCodexSession) -> Result<(), DirectError> {
        *self.session.write().await = Some(session.clone());
        Ok(())
    }

    async fn delete(&self) -> Result<(), DirectError> {
        self.session.write().await.take();
        Ok(())
    }
}

#[derive(Clone)]
struct AuthEndpoints {
    issuer: String,
    authorize: Url,
    token: Url,
    jwks: Url,
    #[cfg(feature = "experimental-direct-device")]
    device_code: Url,
    #[cfg(feature = "experimental-direct-device")]
    device_token: Url,
    #[cfg(feature = "experimental-direct-device")]
    device_verification: Url,
    #[cfg(feature = "experimental-direct-device")]
    device_redirect: Url,
}

impl AuthEndpoints {
    fn production() -> Result<Self, DirectError> {
        Ok(Self {
            issuer: ISSUER.to_owned(),
            authorize: parse_fixed_url(AUTHORIZE_ENDPOINT)?,
            token: parse_fixed_url(TOKEN_ENDPOINT)?,
            jwks: parse_fixed_url(JWKS_ENDPOINT)?,
            #[cfg(feature = "experimental-direct-device")]
            device_code: parse_fixed_url(DEVICE_CODE_ENDPOINT)?,
            #[cfg(feature = "experimental-direct-device")]
            device_token: parse_fixed_url(DEVICE_TOKEN_ENDPOINT)?,
            #[cfg(feature = "experimental-direct-device")]
            device_verification: parse_fixed_url(DEVICE_VERIFICATION_URL)?,
            #[cfg(feature = "experimental-direct-device")]
            device_redirect: parse_fixed_url(DEVICE_REDIRECT_URI)?,
        })
    }
}

/// OAuth client for the private experimental subscription backend.
#[derive(Clone)]
pub struct DirectAuthClient {
    http: reqwest::Client,
    endpoints: AuthEndpoints,
    callback_timeout: Duration,
    #[cfg(feature = "experimental-direct-device")]
    device_deadline: Duration,
}

impl std::fmt::Debug for DirectAuthClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectAuthClient")
            .field("issuer", &self.endpoints.issuer)
            .field("callback_timeout", &self.callback_timeout)
            .finish_non_exhaustive()
    }
}

impl DirectAuthClient {
    pub fn new() -> Result<Self, DirectError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("openai-rs/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            endpoints: AuthEndpoints::production()?,
            callback_timeout: Duration::from_secs(5 * 60),
            #[cfg(feature = "experimental-direct-device")]
            device_deadline: Duration::from_secs(15 * 60),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_token_endpoint(token: Url) -> Result<Self, DirectError> {
        let mut client = Self::new()?;
        client.endpoints.token = token;
        Ok(client)
    }

    /// Test-only override of both token-exchange endpoints used by the
    /// browser-login e2e chain (8-22): the OAuth token endpoint and the JWKS
    /// endpoint, both pointed at a scripted loopback server.
    #[cfg(test)]
    pub(crate) fn with_test_auth_urls(token: Url, jwks: Url) -> Result<Self, DirectError> {
        let mut client = Self::new()?;
        client.endpoints.token = token;
        client.endpoints.jwks = jwks;
        Ok(client)
    }

    #[cfg(all(test, feature = "experimental-direct-device"))]
    fn with_test_device_base(base: &Url, deadline: Duration) -> Result<Self, DirectError> {
        let mut client = Self::new()?;
        client.endpoints.device_code = base
            .join("deviceauth/usercode")
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        client.endpoints.device_token = base
            .join("deviceauth/token")
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        client.endpoints.device_verification = base
            .join("codex/device")
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        client.endpoints.device_redirect = base
            .join("deviceauth/callback")
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        client.device_deadline = deadline;
        Ok(client)
    }

    /// Bind a registered IPv4 loopback port and build a PKCE+state+nonce URL.
    pub async fn begin_browser_login(&self) -> Result<BrowserLogin, DirectError> {
        let listener = bind_callback_listener().await?;
        let port = listener
            .local_addr()
            .map_err(|error| DirectError::OAuth(format!("loopback address failed: {error}")))?
            .port();
        let redirect_uri = Url::parse(&format!("http://localhost:{port}{CALLBACK_PATH}"))
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        let verifier = random_base64url(32)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_base64url(32)?;
        let nonce = random_base64url(32)?;
        let mut authorize_url = self.endpoints.authorize.clone();
        authorize_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", redirect_uri.as_str())
            .append_pair("scope", AUTHORIZE_SCOPE)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", super::CODEX_ORIGINATOR);
        Ok(BrowserLogin {
            authorize_url,
            redirect_uri,
            listener,
            verifier: SecretString::from(verifier),
            state: SecretString::from(state),
            nonce: SecretString::from(nonce),
            auth: self.clone(),
        })
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &Url,
        verifier: &str,
    ) -> Result<TokenResponse, DirectError> {
        let response = self
            .http
            .post(self.endpoints.token.clone())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri.as_str()),
                ("client_id", CLIENT_ID),
                ("code_verifier", verifier),
            ])
            .send()
            .await?;
        decode_auth_response(response).await
    }

    async fn refresh(
        &self,
        session: &StoredCodexSession,
    ) -> Result<StoredCodexSession, DirectError> {
        let response = self
            .http
            .post(self.endpoints.token.clone())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", session.refresh_token()),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await?;
        let tokens: RefreshResponse = decode_auth_response(response).await?;
        let refresh_token = tokens
            .refresh_token
            .map(|token| Arc::new(SecretString::from(token)))
            .unwrap_or_else(|| Arc::clone(&session.refresh_token));
        Ok(StoredCodexSession {
            access_token: Arc::new(SecretString::from(tokens.access_token)),
            refresh_token,
            expires_at: now_epoch()?
                .saturating_add(tokens.expires_in.unwrap_or(DEFAULT_EXPIRES_IN)),
            account_id: session.account_id.clone(),
            generation: session.generation.saturating_add(1),
        })
    }

    async fn verifier(&self) -> Result<OidcVerifier, DirectError> {
        let response = self.http.get(self.endpoints.jwks.clone()).send().await?;
        let jwks: JsonWebKeySet = decode_auth_response(response).await?;
        OidcVerifier::new(self.endpoints.issuer.clone(), CLIENT_ID, jwks)
    }

    #[cfg(feature = "experimental-direct-device")]
    pub async fn begin_device_login(&self) -> Result<DeviceCodeLogin, DirectError> {
        let nonce = random_base64url(32)?;
        let response = self
            .http
            .post(self.endpoints.device_code.clone())
            .json(&DeviceCodeRequest {
                client_id: CLIENT_ID,
                nonce: &nonce,
            })
            .send()
            .await?;
        let response: DeviceCodeResponse = decode_auth_response(response).await?;
        let interval = response.interval.seconds().clamp(1, 30);
        Ok(DeviceCodeLogin {
            verification_url: self.endpoints.device_verification.clone(),
            user_code: response.user_code,
            device_auth_id: SecretString::from(response.device_auth_id),
            nonce: SecretString::from(nonce),
            interval: Duration::from_secs(interval),
            auth: self.clone(),
        })
    }
}

async fn bind_callback_listener() -> Result<TcpListener, DirectError> {
    match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, DEFAULT_CALLBACK_PORT)).await {
        Ok(listener) => Ok(listener),
        Err(primary) if primary.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, FALLBACK_CALLBACK_PORT))
                .await
                .map_err(|fallback| {
                    DirectError::OAuth(format!(
                        "registered loopback ports {DEFAULT_CALLBACK_PORT} and {FALLBACK_CALLBACK_PORT} are unavailable: {primary}; {fallback}"
                    ))
                })
        }
        Err(error) => Err(DirectError::OAuth(format!(
            "loopback bind on registered port {DEFAULT_CALLBACK_PORT} failed: {error}"
        ))),
    }
}

/// In-progress browser flow. Debug never includes state, nonce, or verifier.
pub struct BrowserLogin {
    pub authorize_url: Url,
    pub redirect_uri: Url,
    listener: TcpListener,
    verifier: SecretString,
    state: SecretString,
    nonce: SecretString,
    auth: DirectAuthClient,
}

impl std::fmt::Debug for BrowserLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let authorize_origin = format!(
            "{}://{}{}",
            self.authorize_url.scheme(),
            self.authorize_url.host_str().unwrap_or("<invalid>"),
            self.authorize_url.path()
        );
        formatter
            .debug_struct("BrowserLogin")
            .field("authorize_url", &authorize_origin)
            .field("redirect_uri", &self.redirect_uri)
            .field("oauth_secrets", &"<redacted>")
            .finish()
    }
}

impl BrowserLogin {
    pub async fn complete<S: CredentialStore>(
        self,
        store: &S,
        cancellation: &CancellationToken,
    ) -> Result<StoredCodexSession, DirectError> {
        let timeout = self.auth.callback_timeout;
        tokio::select! {
            () = cancellation.cancelled() => Err(DirectError::Cancelled),
            result = tokio::time::timeout(timeout, self.complete_inner(store)) => {
                result.map_err(|_| DirectError::Timeout)?
            }
        }
    }

    async fn complete_inner<S: CredentialStore>(
        self,
        store: &S,
    ) -> Result<StoredCodexSession, DirectError> {
        let expected_host = self
            .redirect_uri
            .host_str()
            .zip(self.redirect_uri.port())
            .map(|(host, port)| format!("{host}:{port}"))
            .ok_or_else(|| {
                DirectError::Configuration("invalid loopback redirect URI".to_owned())
            })?;
        let (mut stream, callback) = loop {
            let (mut stream, peer) =
                self.listener.accept().await.map_err(|error| {
                    DirectError::OAuth(format!("callback accept failed: {error}"))
                })?;
            if !peer.ip().is_loopback() {
                let _ = write_not_found(&mut stream).await;
                continue;
            }
            match read_callback(&mut stream, &expected_host).await {
                Ok(Some(callback)) => break (stream, callback),
                Ok(None) => {
                    let _ = write_not_found(&mut stream).await;
                }
                Err(error) => {
                    let _ = write_html(&mut stream, false).await;
                    return Err(error);
                }
            }
        };
        if !secure_equal(
            callback.state.as_bytes(),
            self.state.expose_secret().as_bytes(),
        ) {
            let _ = write_html(&mut stream, false).await;
            return Err(DirectError::OAuth("callback state mismatch".to_owned()));
        }
        if callback.error || callback.code.is_empty() {
            let _ = write_html(&mut stream, false).await;
            return Err(DirectError::OAuth(
                "authorization was not granted".to_owned(),
            ));
        }

        let result = async {
            let tokens = self
                .auth
                .exchange_code(
                    &callback.code,
                    &self.redirect_uri,
                    self.verifier.expose_secret(),
                )
                .await?;
            let verifier = self.auth.verifier().await?;
            let account_id =
                verifier.verify(&tokens.id_token, self.nonce.expose_secret(), now_epoch()?)?;
            let refresh_token = tokens.refresh_token.ok_or_else(|| {
                DirectError::OAuth("initial token response omitted refresh token".to_owned())
            })?;
            let session = StoredCodexSession {
                access_token: Arc::new(SecretString::from(tokens.access_token)),
                refresh_token: Arc::new(SecretString::from(refresh_token)),
                expires_at: now_epoch()?
                    .saturating_add(tokens.expires_in.unwrap_or(DEFAULT_EXPIRES_IN)),
                account_id,
                generation: 0,
            };
            store.save(&session).await?;
            Ok::<_, DirectError>(session)
        };
        match result.await {
            Ok(session) => {
                write_html(&mut stream, true).await?;
                Ok(session)
            }
            Err(error) => {
                let _ = write_html(&mut stream, false).await;
                Err(error)
            }
        }
    }
}

/// In-progress device-code flow.
#[cfg(feature = "experimental-direct-device")]
pub struct DeviceCodeLogin {
    pub verification_url: Url,
    pub user_code: String,
    device_auth_id: SecretString,
    nonce: SecretString,
    interval: Duration,
    auth: DirectAuthClient,
}

#[cfg(not(feature = "experimental-direct-device"))]
#[derive(Debug)]
pub struct DeviceCodeLogin {
    _private: (),
}

#[cfg(feature = "experimental-direct-device")]
impl std::fmt::Debug for DeviceCodeLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceCodeLogin")
            .field("verification_url", &self.verification_url)
            .field("user_code", &"<redacted>")
            .finish()
    }
}

#[cfg(feature = "experimental-direct-device")]
impl DeviceCodeLogin {
    pub async fn complete<S: CredentialStore>(
        self,
        store: &S,
        cancellation: &CancellationToken,
    ) -> Result<StoredCodexSession, DirectError> {
        let deadline = tokio::time::Instant::now() + self.auth.device_deadline;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(DirectError::Timeout);
            }
            let response = tokio::select! {
                () = cancellation.cancelled() => return Err(DirectError::Cancelled),
                result = self.auth.http.post(self.auth.endpoints.device_token.clone()).json(
                    &DevicePollRequest {
                        device_auth_id: self.device_auth_id.expose_secret(),
                        user_code: &self.user_code,
                    }
                ).send() => result?,
            };
            let status = response.status();
            if status.is_success() {
                let code: DeviceCodeSuccess = decode_auth_response(response).await?;
                let expected_challenge =
                    URL_SAFE_NO_PAD.encode(Sha256::digest(code.code_verifier.as_bytes()));
                if !secure_equal(
                    expected_challenge.as_bytes(),
                    code.code_challenge.as_bytes(),
                ) {
                    return Err(DirectError::OAuth(
                        "device PKCE challenge mismatch".to_owned(),
                    ));
                }
                let tokens = self
                    .auth
                    .exchange_code(
                        &code.authorization_code,
                        &self.auth.endpoints.device_redirect,
                        &code.code_verifier,
                    )
                    .await?;
                let verifier = self.auth.verifier().await?;
                let account_id =
                    verifier.verify(&tokens.id_token, self.nonce.expose_secret(), now_epoch()?)?;
                let refresh_token = tokens.refresh_token.ok_or_else(|| {
                    DirectError::OAuth("initial token response omitted refresh token".to_owned())
                })?;
                let session = StoredCodexSession {
                    access_token: Arc::new(SecretString::from(tokens.access_token)),
                    refresh_token: Arc::new(SecretString::from(refresh_token)),
                    expires_at: now_epoch()?
                        .saturating_add(tokens.expires_in.unwrap_or(DEFAULT_EXPIRES_IN)),
                    account_id,
                    generation: 0,
                };
                store.save(&session).await?;
                return Ok(session);
            }
            let wait = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                retry_after(response.headers()).unwrap_or(self.interval)
            } else if matches!(
                status,
                reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::NOT_FOUND
            ) {
                self.interval.saturating_add(Duration::from_secs(3))
            } else {
                return Err(http_status_error(response).await);
            };
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::select! {
                () = cancellation.cancelled() => return Err(DirectError::Cancelled),
                () = tokio::time::sleep(wait.min(remaining)) => {}
            }
        }
    }
}

/// Single-account, refresh-skewed, singleflight token manager.
pub struct TokenManager<S: CredentialStore> {
    store: Arc<S>,
    auth: DirectAuthClient,
    cached: RwLock<Option<StoredCodexSession>>,
    refresh_gate: Mutex<()>,
}

impl<S: CredentialStore> std::fmt::Debug for TokenManager<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TokenManager(<credentials redacted>)")
    }
}

impl<S: CredentialStore> TokenManager<S> {
    #[must_use]
    pub fn new(store: Arc<S>, auth: DirectAuthClient) -> Self {
        Self {
            store,
            auth,
            cached: RwLock::new(None),
            refresh_gate: Mutex::new(()),
        }
    }

    pub async fn session(&self) -> Result<StoredCodexSession, DirectError> {
        let now = now_epoch()?;
        if let Some(session) = self.cached.read().await.clone()
            && session.expires_at > now.saturating_add(REFRESH_SKEW)
        {
            return Ok(session);
        }
        let _guard = self.refresh_gate.lock().await;
        let now = now_epoch()?;
        let current = match self.cached.read().await.clone() {
            Some(session) => session,
            None => self
                .store
                .load()
                .await?
                .ok_or(DirectError::ReauthenticationRequired)?,
        };
        if current.expires_at > now.saturating_add(REFRESH_SKEW) {
            *self.cached.write().await = Some(current.clone());
            return Ok(current);
        }
        let refreshed = self.auth.refresh(&current).await?;
        self.store.save(&refreshed).await?;
        *self.cached.write().await = Some(refreshed.clone());
        Ok(refreshed)
    }

    pub(crate) async fn refresh_after_unauthorized(
        &self,
        failed_generation: u64,
    ) -> Result<StoredCodexSession, DirectError> {
        let _guard = self.refresh_gate.lock().await;
        if let Some(current) = self.cached.read().await.clone()
            && current.generation != failed_generation
        {
            return Ok(current);
        }
        let current = self
            .store
            .load()
            .await?
            .ok_or(DirectError::ReauthenticationRequired)?;
        if current.generation != failed_generation {
            *self.cached.write().await = Some(current.clone());
            return Ok(current);
        }
        let refreshed = self.auth.refresh(&current).await?;
        self.store.save(&refreshed).await?;
        *self.cached.write().await = Some(refreshed.clone());
        Ok(refreshed)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

struct CallbackParams {
    code: String,
    state: String,
    error: bool,
}

async fn read_callback(
    stream: &mut TcpStream,
    expected_host: &str,
) -> Result<Option<CallbackParams>, DirectError> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| DirectError::OAuth(format!("callback read failed: {error}")))?;
        if read == 0 {
            return Ok(None);
        }
        if request.len().saturating_add(read) > MAX_CALLBACK_BYTES {
            return Err(DirectError::OAuth("callback request too large".to_owned()));
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request)
        .map_err(|_| DirectError::OAuth("callback was not UTF-8".to_owned()))?;
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| DirectError::OAuth("callback request line missing".to_owned()))?;
    let mut parts = request_line.split_ascii_whitespace();
    if parts.next() != Some("GET") {
        return Ok(None);
    }
    let Some(target) = parts.next() else {
        return Ok(None);
    };
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() {
        return Ok(None);
    }
    let mut host = None;
    for line in request.lines().skip(1) {
        if line.is_empty() || line == "\r" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(None);
        };
        if name.eq_ignore_ascii_case("host") {
            if host.is_some() {
                return Err(DirectError::OAuth(
                    "duplicate callback Host header".to_owned(),
                ));
            }
            host = Some(value.trim());
        }
    }
    if host != Some(expected_host) {
        return Err(DirectError::OAuth(
            "callback Host header mismatch".to_owned(),
        ));
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| DirectError::OAuth("invalid callback URL".to_owned()))?;
    if url.path() != CALLBACK_PATH {
        return Ok(None);
    }
    let mut code = None;
    let mut state = None;
    let mut error = false;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "code" => {
                return Err(DirectError::OAuth(
                    "duplicate callback code parameter".to_owned(),
                ));
            }
            "state" if state.is_none() => state = Some(value.into_owned()),
            "state" => {
                return Err(DirectError::OAuth(
                    "duplicate callback state parameter".to_owned(),
                ));
            }
            "error" => error = true,
            _ => {}
        }
    }
    let state = state.ok_or_else(|| DirectError::OAuth("callback state missing".to_owned()))?;
    Ok(Some(CallbackParams {
        code: code.unwrap_or_default(),
        state,
        error,
    }))
}

async fn write_not_found(stream: &mut TcpStream) -> Result<(), DirectError> {
    stream
        .write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        )
        .await
        .map_err(|error| DirectError::OAuth(format!("callback response failed: {error}")))?;
    stream
        .shutdown()
        .await
        .map_err(|error| DirectError::OAuth(format!("callback shutdown failed: {error}")))
}

async fn write_html(stream: &mut TcpStream, success: bool) -> Result<(), DirectError> {
    let (status, body) = if success {
        ("200 OK", SUCCESS_HTML)
    } else {
        ("400 Bad Request", ERROR_HTML)
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|error| DirectError::OAuth(format!("callback response failed: {error}")))?;
    stream
        .shutdown()
        .await
        .map_err(|error| DirectError::OAuth(format!("callback shutdown failed: {error}")))
}

async fn decode_auth_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, DirectError> {
    if response.status().is_redirection() {
        return Err(DirectError::RedirectRejected);
    }
    if !response.status().is_success() {
        return Err(http_status_error(response).await);
    }
    let body = read_limited(response, MAX_AUTH_BODY_BYTES).await?;
    serde_json::from_slice(&body).map_err(DirectError::Json)
}

async fn http_status_error(response: reqwest::Response) -> DirectError {
    let status = response.status().as_u16();
    let message = read_limited(response, 8 * 1024)
        .await
        .ok()
        .and_then(|body| sanitized_error_code(&body))
        .unwrap_or_else(|| "authentication request failed".to_owned());
    DirectError::HttpStatus { status, message }
}

fn sanitized_error_code(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let code = value.get("error")?.as_str()?;
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    Some(code.to_owned())
}

async fn read_limited(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, DirectError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(DirectError::BodyTooLarge);
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(DirectError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_fixed_url(value: &str) -> Result<Url, DirectError> {
    Url::parse(value).map_err(|error| DirectError::Configuration(error.to_string()))
}

fn random_base64url(bytes: usize) -> Result<String, DirectError> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value).map_err(|_| DirectError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn now_epoch() -> Result<u64, DirectError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| DirectError::Configuration("system clock is before Unix epoch".to_owned()))
}

#[cfg(feature = "experimental-direct-device")]
#[derive(Serialize)]
struct DeviceCodeRequest<'a> {
    client_id: &'a str,
    nonce: &'a str,
}

#[cfg(feature = "experimental-direct-device")]
#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    #[serde(alias = "usercode")]
    user_code: String,
    interval: StringOrNumber,
}

#[cfg(feature = "experimental-direct-device")]
#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrNumber {
    String(String),
    Number(u64),
}

#[cfg(feature = "experimental-direct-device")]
impl StringOrNumber {
    fn seconds(&self) -> u64 {
        match self {
            Self::String(value) => value.parse().unwrap_or(5),
            Self::Number(value) => *value,
        }
    }
}

#[cfg(feature = "experimental-direct-device")]
#[derive(Serialize)]
struct DevicePollRequest<'a> {
    device_auth_id: &'a str,
    user_code: &'a str,
}

#[cfg(feature = "experimental-direct-device")]
#[derive(Deserialize)]
struct DeviceCodeSuccess {
    authorization_code: String,
    code_challenge: String,
    code_verifier: String,
}

#[cfg(feature = "experimental-direct-device")]
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 60)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use static_assertions::assert_not_impl_any;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use url::Url;

    use super::{
        CredentialStore, DirectAuthClient, EphemeralStore, StoredCodexSession, TokenManager,
        now_epoch, read_callback, sanitized_error_code,
    };

    assert_not_impl_any!(StoredCodexSession: serde::Serialize, std::fmt::Display);

    static BROWSER_LOGIN_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn browser_url_uses_registered_localhost_callback_and_security_parameters()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = BROWSER_LOGIN_TEST_LOCK.lock().await;
        let login = DirectAuthClient::new()?.begin_browser_login().await?;
        assert_eq!(login.redirect_uri.host_str(), Some("localhost"));
        assert!(matches!(
            login.redirect_uri.port(),
            Some(super::DEFAULT_CALLBACK_PORT | super::FALLBACK_CALLBACK_PORT)
        ));
        let params: std::collections::HashMap<_, _> =
            login.authorize_url.query_pairs().into_owned().collect();
        assert_eq!(
            params.get("scope").map(String::as_str),
            Some(super::AUTHORIZE_SCOPE)
        );
        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(params.get("state").is_some_and(|value| !value.is_empty()));
        assert!(params.get("nonce").is_some_and(|value| !value.is_empty()));
        assert_eq!(
            params.get("originator").map(String::as_str),
            Some(super::super::CODEX_ORIGINATOR)
        );
        let state = params.get("state").ok_or("missing state")?;
        let nonce = params.get("nonce").ok_or("missing nonce")?;
        let debug = format!("{login:?}");
        assert!(!debug.contains(state));
        assert!(!debug.contains(nonce));
        assert!(!debug.contains('?'));
        Ok(())
    }

    #[tokio::test]
    async fn browser_callback_has_deadline_and_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = BROWSER_LOGIN_TEST_LOCK.lock().await;
        let mut auth = DirectAuthClient::new()?;
        auth.callback_timeout = Duration::from_millis(20);
        let store = EphemeralStore::default();
        let login = auth.begin_browser_login().await?;
        assert!(matches!(
            login
                .complete(&store, &super::CancellationToken::default())
                .await,
            Err(super::DirectError::Timeout)
        ));

        let login = auth.begin_browser_login().await?;
        let cancellation = super::CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            login.complete(&store, &cancellation).await,
            Err(super::DirectError::Cancelled)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn ephemeral_store_round_trip_empty() -> Result<(), super::DirectError> {
        let store = EphemeralStore::default();
        assert!(super::CredentialStore::load(&store).await?.is_none());
        Ok(())
    }

    async fn callback_request(
        request: &str,
        expected_host: &str,
    ) -> Result<Result<Option<super::CallbackParams>, super::DirectError>, Box<dyn std::error::Error>>
    {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let request = request.to_owned();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await?;
            stream.write_all(request.as_bytes()).await?;
            Ok::<_, std::io::Error>(())
        });
        let (mut server, _) = listener.accept().await?;
        let result = read_callback(&mut server, expected_host).await;
        client.await??;
        Ok(result)
    }

    #[tokio::test]
    async fn callback_ignores_favicon_and_rejects_duplicate_or_wrong_host()
    -> Result<(), Box<dyn std::error::Error>> {
        let favicon = callback_request(
            "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "127.0.0.1:1234",
        )
        .await??;
        assert!(favicon.is_none());

        let duplicate = callback_request(
            "GET /auth/callback?code=one&code=two&state=s HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "127.0.0.1:1234",
        )
        .await?;
        assert!(duplicate.is_err());

        let duplicate_state = callback_request(
            "GET /auth/callback?code=one&state=s&state=t HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "127.0.0.1:1234",
        )
        .await?;
        assert!(duplicate_state.is_err());

        let wrong_host = callback_request(
            "GET /auth/callback?code=one&state=s HTTP/1.1\r\nHost: attacker.test\r\n\r\n",
            "127.0.0.1:1234",
        )
        .await?;
        assert!(wrong_host.is_err());
        Ok(())
    }

    #[test]
    fn auth_error_body_only_exposes_sanitized_code() {
        assert_eq!(
            sanitized_error_code(br#"{"error":"invalid_grant","token":"secret"}"#).as_deref(),
            Some("invalid_grant")
        );
        assert!(sanitized_error_code(br#"{"error":"email@example.com"}"#).is_none());
    }

    #[tokio::test]
    async fn concurrent_expiry_refreshes_once() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut count = 0_u64;
            if let Ok((mut stream, _)) = listener.accept().await {
                count += 1;
                let mut request = vec![0_u8; 8 * 1024];
                let _ = stream.readable().await;
                let _ = stream.try_read(&mut request);
                let body = br#"{"access_token":"new-access","expires_in":3600}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await?;
                stream.write_all(body).await?;
            }
            if let Ok(Ok((_stream, _))) =
                tokio::time::timeout(Duration::from_millis(250), listener.accept()).await
            {
                count += 1;
            }
            Ok::<_, std::io::Error>(count)
        });

        let store = Arc::new(EphemeralStore::default());
        let expired = StoredCodexSession::fixture(
            "old-access",
            "refresh-secret",
            now_epoch()?.saturating_sub(1),
            super::ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&expired).await?;
        let token_endpoint = Url::parse(&format!("http://{address}/oauth/token"))?;
        let auth = DirectAuthClient::with_test_token_endpoint(token_endpoint)?;
        let manager = Arc::new(TokenManager::new(store, auth));
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let manager = Arc::clone(&manager);
            tasks.push(tokio::spawn(async move { manager.session().await }));
        }
        for task in tasks {
            assert_eq!(task.await??.access_token(), "new-access");
        }
        assert_eq!(server.await??, 1);
        Ok(())
    }

    #[cfg(feature = "experimental-direct-device")]
    #[tokio::test]
    async fn device_poll_is_bounded_and_cancellable() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            for response in [
                Some(
                    br#"{"device_auth_id":"device-1","user_code":"ABCD-1234","interval":"1"}"#
                        .as_slice(),
                ),
                None,
            ] {
                let (mut stream, _) = listener.accept().await?;
                let mut request = vec![0_u8; 8 * 1024];
                let _ = stream.readable().await;
                let _ = stream.try_read(&mut request);
                match response {
                    Some(body) => {
                        let headers = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        stream.write_all(headers.as_bytes()).await?;
                        stream.write_all(body).await?;
                    }
                    None => {
                        stream
                            .write_all(
                                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await?;
                    }
                }
            }
            Ok::<_, std::io::Error>(())
        });
        let base = Url::parse(&format!("http://{address}/"))?;
        let auth = DirectAuthClient::with_test_device_base(&base, Duration::from_secs(5))?;
        let login = auth.begin_device_login().await?;
        let store = Arc::new(EphemeralStore::default());
        let cancellation = super::CancellationToken::default();
        let task_cancellation = cancellation.clone();
        let task_store = Arc::clone(&store);
        let task = tokio::spawn(async move {
            login
                .complete(task_store.as_ref(), &task_cancellation)
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancellation.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), task).await??;
        assert!(matches!(result, Err(super::DirectError::Cancelled)));
        server.await??;
        Ok(())
    }

    /// Read one HTTP request (headers plus a Content-Length body) from a
    /// loopback stream.
    async fn read_http_request(
        stream: &mut TcpStream,
    ) -> Result<(String, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buffer.len() >= header_end + 4 + content_length {
                let body = buffer[header_end + 4..].to_vec();
                return Ok((headers, body));
            }
        }
        Err("loopback HTTP request ended early".into())
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).await?;
        stream.write_all(&body).await?;
        stream.shutdown().await?;
        Ok(())
    }

    /// Drive one scripted loopback "browser" callback against `login` and
    /// return the HTTP response bytes the client served.
    async fn scripted_callback(
        port: u16,
        query: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let request = format!(
            "GET /auth/callback?{query} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
        );
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response).await;
        Ok(response)
    }

    /// 8-22: the full browser-login success chain against a scripted loopback
    /// IdP — callback with the matching state, the authorization-code
    /// exchange (PKCE verifier honored), JWKS discovery, id_token signature
    /// and nonce verification, and the final `store.save`.
    #[tokio::test]
    async fn browser_login_completes_the_exchange_jwks_and_verification_chain()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use sha2::Digest;

        let _guard = BROWSER_LOGIN_TEST_LOCK.lock().await;
        let fixture = crate::direct::jwt::test_support::rsa_fixture()?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let base = Url::parse(&format!("http://{address}/"))?;
        let auth = DirectAuthClient::with_test_auth_urls(
            base.join("oauth/token")?,
            base.join(".well-known/jwks.json")?,
        )?;

        let login = auth.begin_browser_login().await?;
        let params: std::collections::HashMap<_, _> =
            login.authorize_url.query_pairs().into_owned().collect();
        let state = params.get("state").ok_or("authorize URL state")?.clone();
        let nonce = params.get("nonce").ok_or("authorize URL nonce")?.clone();
        let challenge = params
            .get("code_challenge")
            .ok_or("authorize URL code_challenge")?
            .clone();
        let port = login.redirect_uri.port().ok_or("redirect port")?;

        let now = now_epoch()?;
        let id_token = crate::direct::jwt::test_support::token(
            &fixture.pair,
            serde_json::json!({
                "iss": super::ISSUER,
                "aud": super::CLIENT_ID,
                "exp": now + 3_600,
                "iat": now,
                "nonce": nonce,
                "chatgpt_account_id": "acct-e2e"
            }),
        )?;
        let token_body = serde_json::json!({
            "id_token": id_token,
            "access_token": "access-e2e",
            "refresh_token": "refresh-e2e",
            "expires_in": 3_600
        })
        .to_string()
        .into_bytes();
        let jwks_body = serde_json::to_vec(&fixture.jwks_json)?;

        let server = tokio::spawn(async move {
            // 1: the authorization-code exchange, 2: JWKS discovery.
            let (mut stream, _) = listener.accept().await?;
            let (headers, body) = read_http_request(&mut stream).await?;
            let exchange = format!("{headers}\n{}", String::from_utf8_lossy(&body));
            let exchange_body = body;
            write_http_response(&mut stream, "application/json", token_body).await?;
            let (mut stream, _) = listener.accept().await?;
            let (jwks_headers, _) = read_http_request(&mut stream).await?;
            assert!(jwks_headers.starts_with("GET /.well-known/jwks.json"));
            write_http_response(&mut stream, "application/json", jwks_body).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((exchange, exchange_body))
        });

        let store = Arc::new(EphemeralStore::default());
        let verify_store = Arc::clone(&store);
        let complete = tokio::spawn(async move {
            login
                .complete(store.as_ref(), &super::CancellationToken::default())
                .await
        });
        let callback = scripted_callback(port, &format!("code=code-e2e&state={state}")).await?;
        let callback_html = String::from_utf8_lossy(&callback);
        assert!(
            callback_html.contains("200 OK"),
            "the success page must reach the browser: {callback_html}"
        );
        assert!(callback_html.contains("Authorization complete"));

        let session = complete.await??;
        assert_eq!(session.access_token(), "access-e2e");
        assert_eq!(session.account_id().as_str(), "acct-e2e");
        assert!(session.expires_at() >= now + 3_000);
        let stored = CredentialStore::load(verify_store.as_ref()).await?;
        assert_eq!(
            stored.map(|stored| stored.access_token().to_owned()),
            Some("access-e2e".to_owned()),
            "complete() must persist the session"
        );

        let (exchange, exchange_body) = server.await??;
        let lower = exchange.to_ascii_lowercase();
        assert!(lower.contains("post /oauth/token"));
        assert!(lower.contains("grant_type=authorization_code"));
        assert!(exchange.contains("code=code-e2e"));
        assert!(lower.contains("code_verifier="));
        // The PKCE pair must be internally consistent: the verifier presented
        // at exchange time hashes to the challenge the authorize URL carried.
        let exchange_form = String::from_utf8_lossy(&exchange_body).into_owned();
        let verifier = exchange_form
            .split('&')
            .find_map(|field| field.strip_prefix("code_verifier="))
            .ok_or("code_verifier form field")?;
        use base64::Engine;
        let derived = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(verifier.as_bytes()));
        assert_eq!(
            derived, challenge,
            "the PKCE challenge must match the verifier"
        );
        Ok(())
    }

    /// 8-22: a callback whose state does not match the one sent in the
    /// authorize URL is rejected, and an OAuth `error` parameter (with the
    /// correct state) is reported as a denied authorization — neither
    /// reaches the token endpoint.
    #[tokio::test]
    async fn browser_login_rejects_state_mismatch_and_error_callbacks()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _guard = BROWSER_LOGIN_TEST_LOCK.lock().await;
        let mut auth = DirectAuthClient::new()?;
        auth.callback_timeout = Duration::from_secs(5);

        let login = auth.begin_browser_login().await?;
        let port = login.redirect_uri.port().ok_or("redirect port")?;
        let store = EphemeralStore::default();
        let complete = tokio::spawn(async move {
            login
                .complete(&store, &super::CancellationToken::default())
                .await
        });
        let response = scripted_callback(port, "code=code-x&state=tampered-state").await?;
        assert!(
            response.starts_with(b"HTTP/1.1 400"),
            "a state mismatch must serve the failure page"
        );
        match complete.await? {
            Err(super::DirectError::OAuth(message)) => {
                assert_eq!(message, "callback state mismatch");
            }
            other => return Err(format!("unexpected mismatch result: {other:?}").into()),
        }

        let login = auth.begin_browser_login().await?;
        let params: std::collections::HashMap<_, _> =
            login.authorize_url.query_pairs().into_owned().collect();
        let state = params.get("state").ok_or("authorize URL state")?.clone();
        let port = login.redirect_uri.port().ok_or("redirect port")?;
        let store = EphemeralStore::default();
        let complete = tokio::spawn(async move {
            login
                .complete(&store, &super::CancellationToken::default())
                .await
        });
        scripted_callback(port, &format!("error=access_denied&state={state}")).await?;
        match complete.await? {
            Err(super::DirectError::OAuth(message)) => {
                assert_eq!(message, "authorization was not granted");
            }
            other => return Err(format!("unexpected error-callback result: {other:?}").into()),
        }
        Ok(())
    }

    /// 8-22: the 401-recovery lane — `refresh_after_unauthorized` performs
    /// exactly one refresh for the failed generation and persists it, while
    /// a caller whose generation is already stale (another caller refreshed
    /// in the meantime) gets the newer session back without any further
    /// network round trip.
    #[tokio::test]
    async fn refresh_after_unauthorized_refreshes_once_and_skips_stale_generations()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut served = 0_u64;
            // Serve at most one refresh; a second request would prove the
            // skip branch wrong, so park instead of answering it.
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_http_request(&mut stream).await?;
                served += 1;
                let body = br#"{"access_token":"access-refreshed","expires_in":3600}"#;
                write_http_response(&mut stream, "application/json", body.to_vec()).await?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(served)
        });

        let store = Arc::new(EphemeralStore::default());
        let expired = StoredCodexSession::fixture(
            "access-old",
            "refresh-secret",
            now_epoch()?.saturating_sub(1),
            super::ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&expired).await?;
        let token_endpoint = Url::parse(&format!("http://{address}/oauth/token"))?;
        let auth = DirectAuthClient::with_test_token_endpoint(token_endpoint)?;
        let manager = TokenManager::new(Arc::clone(&store), auth);

        // The failed generation triggers the single refresh and persists it.
        let refreshed = manager.refresh_after_unauthorized(0).await?;
        assert_eq!(refreshed.access_token(), "access-refreshed");
        assert_eq!(refreshed.generation(), 1);
        let stored = CredentialStore::load(store.as_ref()).await?;
        assert_eq!(
            stored.map(|stored| stored.generation()),
            Some(1),
            "the refreshed session must be persisted"
        );

        // The same failed generation is now stale (the cache already holds
        // generation 1), so the newer session comes back with no refresh.
        let skipped = manager.refresh_after_unauthorized(0).await?;
        assert_eq!(skipped.access_token(), "access-refreshed");
        assert_eq!(skipped.generation(), 1);

        assert_eq!(
            server.await??,
            1,
            "exactly one refresh request may leave the client"
        );
        Ok(())
    }
}
