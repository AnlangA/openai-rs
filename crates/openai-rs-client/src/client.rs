use std::{fmt, sync::Arc, time::Duration};

use http::HeaderValue;
use url::{Host, Url};

#[cfg(feature = "alpha-graders")]
use crate::AlphaGraders;
#[cfg(feature = "beta-responses-multi-agent")]
use crate::BetaResponses;
#[cfg(feature = "beta-chatkit")]
use crate::ChatKit;
#[cfg(feature = "legacy-completions")]
use crate::Completions;
#[cfg(feature = "legacy-evals")]
use crate::Evals;
#[cfg(feature = "legacy-realtime")]
#[allow(deprecated)]
use crate::LegacyRealtimeSessions;
#[cfg(feature = "realtime")]
use crate::Realtime;
#[cfg(feature = "custom-voice")]
use crate::Voices;
use crate::{
    ApiKey, Audio, Batches, ChatCompletions, Containers, ContentProvenanceChecks, Conversations,
    Embeddings, Error, Files, FineTuning, Images, Models, Moderations, Responses, RetryPolicy,
    Skills, Uploads, VectorStores, auth::AuthProvider, multipart::MultipartTransport,
    sse::SseLimits, transport::Transport,
};
#[cfg(feature = "workload-identity")]
use crate::{WorkloadIdentityConfig, workload_identity::WorkloadIdentityAuth};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1/";
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_MAX_JSON_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// TLS implementation selected for the reqwest transport.
///
/// Variants exist only when their matching crate feature is enabled. When both
/// are compiled, rustls remains the default and callers may select native TLS
/// explicitly at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TlsBackend {
    #[cfg(feature = "rustls-tls")]
    Rustls,
    #[cfg(feature = "native-tls")]
    Native,
}

/// A cheap-to-clone OpenAI Platform client.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    transport: Transport,
    multipart: MultipartTransport,
    /// Cloneable snapshot of every transport construction input, kept so
    /// [`Client::with_request_timeout`] can rebuild both transports with one
    /// budget overridden while sharing the connection pool and credential.
    derivation: TransportDerivation,
}

/// The inputs [`ClientBuilder::build`] used to assemble the two transports.
///
/// [`Transport`] deliberately owns its timeout privately (and is not
/// `Clone`), so a derived client is rebuilt through the `pub(crate)`
/// constructors instead of mutating an existing transport in place.
#[derive(Clone)]
struct TransportDerivation {
    http: reqwest::Client,
    base_url: Url,
    auth: AuthProvider,
    organization: Option<HeaderValue>,
    project: Option<HeaderValue>,
    client_request_id: Option<HeaderValue>,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    retry_policy: RetryPolicy,
    sse_limits: SseLimits,
    tls_backend: Option<TlsBackend>,
}

impl Inner {
    fn from_derivation(derivation: TransportDerivation, request_timeout: Duration) -> Self {
        Self {
            transport: Transport::new(
                derivation.http.clone(),
                derivation.base_url.clone(),
                derivation.auth.clone(),
                derivation.organization.clone(),
                derivation.project.clone(),
                derivation.client_request_id.clone(),
                derivation.max_json_body_bytes,
                derivation.max_error_body_bytes,
                derivation.retry_policy,
                request_timeout,
                derivation.sse_limits,
                derivation.tls_backend,
            ),
            multipart: MultipartTransport::new(
                derivation.http.clone(),
                derivation.base_url.clone(),
                derivation.auth.clone(),
                derivation.organization.clone(),
                derivation.project.clone(),
                derivation.client_request_id.clone(),
                derivation.max_json_body_bytes,
                derivation.max_error_body_bytes,
                derivation.retry_policy,
                request_timeout,
            ),
            derivation,
        }
    }
}

impl Client {
    /// Starts a builder using a validated Platform API key.
    #[must_use]
    pub fn builder(api_key: ApiKey) -> ClientBuilder {
        ClientBuilder::new(api_key)
    }

    /// Builds a client with secure defaults and the official Platform base URL.
    pub fn new(api_key: ApiKey) -> Result<Self, Error> {
        Self::builder(api_key).build()
    }

    /// Builds a Platform client backed by RFC 8693 workload identity.
    #[cfg(feature = "workload-identity")]
    pub fn from_workload_identity(config: WorkloadIdentityConfig) -> Result<Self, Error> {
        Self::workload_identity_builder(config).build()
    }

    /// Starts a builder that never accepts or stores an API key.
    #[cfg(feature = "workload-identity")]
    #[must_use]
    pub fn workload_identity_builder(config: WorkloadIdentityConfig) -> ClientBuilder {
        ClientBuilder::from_workload_identity(config)
    }

    /// Returns the Responses resource facade.
    #[must_use]
    pub fn responses(&self) -> Responses {
        Responses::new(self.clone())
    }

    /// Returns the explicitly feature-gated multi-agent Responses preview.
    #[cfg(feature = "beta-responses-multi-agent")]
    #[must_use]
    pub fn beta_responses(&self) -> BetaResponses {
        BetaResponses::new(self.clone())
    }

    /// Returns the legacy Completions resource facade.
    #[cfg(feature = "legacy-completions")]
    #[must_use]
    pub fn completions(&self) -> Completions {
        Completions::new(self.clone())
    }

    /// Returns deprecated pre-GA Realtime session-token operations.
    #[cfg(feature = "legacy-realtime")]
    #[allow(deprecated)]
    #[deprecated(
        since = "0.1.0",
        note = "use GA Realtime client_secrets and calls APIs instead"
    )]
    #[must_use]
    pub fn legacy_realtime_sessions(&self) -> LegacyRealtimeSessions {
        LegacyRealtimeSessions::new(self.clone())
    }

    /// Returns access-controlled custom voice operations.
    #[cfg(feature = "custom-voice")]
    #[must_use]
    pub fn voices(&self) -> Voices {
        Voices::new(self.clone())
    }

    /// Returns access-controlled experimental grader operations.
    #[cfg(feature = "alpha-graders")]
    #[must_use]
    pub fn alpha_graders(&self) -> AlphaGraders {
        AlphaGraders::new(self.clone())
    }

    /// Returns access-controlled Beta ChatKit operations.
    #[cfg(feature = "beta-chatkit")]
    #[must_use]
    pub fn chatkit(&self) -> ChatKit {
        ChatKit::new(self.clone())
    }

    /// Returns the GA Realtime API facade.
    #[cfg(feature = "realtime")]
    #[must_use]
    pub fn realtime(&self) -> Realtime {
        Realtime::new(self.clone())
    }

    /// Returns the Chat Completions resource facade.
    #[must_use]
    pub fn chat_completions(&self) -> ChatCompletions {
        ChatCompletions::new(self.clone())
    }

    /// Returns the Batch API resource facade.
    #[must_use]
    pub fn batches(&self) -> Batches {
        Batches::new(self.clone())
    }

    /// Returns the Evals resource facade.
    ///
    /// The OpenAI Evals platform will become read-only on 2026-10-31 and shut down on 2026-11-30.
    #[cfg(feature = "legacy-evals")]
    #[must_use]
    pub fn evals(&self) -> Evals {
        Evals::new(self.clone())
    }

    /// Returns the Containers resource facade.
    #[must_use]
    pub fn containers(&self) -> Containers {
        Containers::new(self.clone())
    }

    /// Returns the Skills resource facade.
    #[must_use]
    pub fn skills(&self) -> Skills {
        Skills::new(self.clone())
    }

    /// Returns the Fine-tuning resource facade.
    #[must_use]
    pub fn fine_tuning(&self) -> FineTuning {
        FineTuning::new(self.clone())
    }

    /// Returns the Conversations resource facade.
    #[must_use]
    pub fn conversations(&self) -> Conversations {
        Conversations::new(self.clone())
    }

    /// Returns the Content Provenance Checks resource facade.
    #[must_use]
    pub fn content_provenance_checks(&self) -> ContentProvenanceChecks {
        ContentProvenanceChecks::new(self.clone())
    }

    /// Returns the Vector Stores resource facade.
    #[must_use]
    pub fn vector_stores(&self) -> VectorStores {
        VectorStores::new(self.clone())
    }

    /// Returns the Models resource facade.
    #[must_use]
    pub fn models(&self) -> Models {
        Models::new(self.clone())
    }

    /// Returns the Embeddings resource facade.
    #[must_use]
    pub fn embeddings(&self) -> Embeddings {
        Embeddings::new(self.clone())
    }

    /// Returns the Moderations resource facade.
    #[must_use]
    pub fn moderations(&self) -> Moderations {
        Moderations::new(self.clone())
    }

    /// Returns the Files resource facade.
    #[must_use]
    pub fn files(&self) -> Files {
        Files::new(self.clone())
    }

    /// Returns speech, transcription, and translation operations.
    #[must_use]
    pub fn audio(&self) -> Audio {
        Audio::new(self.clone())
    }

    /// Returns image generation and editing operations.
    #[must_use]
    pub fn images(&self) -> Images {
        Images::new(self.clone())
    }

    /// Returns the multipart Uploads resource facade.
    #[must_use]
    pub fn uploads(&self) -> Uploads {
        Uploads::new(self.clone())
    }

    /// The configured API base URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.inner.transport.base_url()
    }

    /// Returns a client that runs every operation under one overridden total
    /// request budget.
    ///
    /// The default budget is 600s (see [`ClientBuilder::request_timeout`] for
    /// the D0199 total-budget semantics). This is the escape hatch for
    /// long-running work — a large upload or a deliberately slow stream can
    /// derive a wider budget (or a tight caller a narrower one) without
    /// loosening the budget every other call runs under. The derived client
    /// shares this client's credential, connection pool, TLS backend, retry
    /// policy, and body limits; the original client is unaffected.
    ///
    /// The knob is deliberately client-shaped rather than request-shaped:
    /// typed resource methods take no per-request timeout parameters, so
    /// budgets stay a property of the client that owns them. Derive one
    /// client per budget instead.
    ///
    /// Unlike [`ClientBuilder::build`], a zero duration cannot be rejected
    /// here (no fallible surface); a client derived with zero fails every
    /// request immediately with [`Error::DeadlineExceeded`].
    #[must_use]
    pub fn with_request_timeout(&self, request_timeout: Duration) -> Client {
        Client {
            inner: Arc::new(Inner::from_derivation(
                self.inner.derivation.clone(),
                request_timeout,
            )),
        }
    }

    pub(crate) fn transport(&self) -> &Transport {
        &self.inner.transport
    }

    pub(crate) fn multipart_transport(&self) -> &MultipartTransport {
        &self.inner.multipart
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base_origin = self.base_url().origin().ascii_serialization();
        formatter
            .debug_struct("Client")
            .field("base_origin", &base_origin)
            .finish_non_exhaustive()
    }
}

/// Secure-by-default builder for [`Client`].
pub struct ClientBuilder {
    credential: ClientCredential,
    base_url: Option<Url>,
    allow_insecure_loopback: bool,
    organization: Option<String>,
    project: Option<String>,
    client_request_id: Option<String>,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    tls_backend: Option<TlsBackend>,
    retry_policy: RetryPolicy,
    sse_limits: SseLimits,
    proxy: Option<reqwest::Proxy>,
}

enum ClientCredential {
    ApiKey(ApiKey),
    #[cfg(feature = "workload-identity")]
    Workload(WorkloadIdentityConfig),
}

impl ClientBuilder {
    #[must_use]
    pub fn new(api_key: ApiKey) -> Self {
        Self {
            credential: ClientCredential::ApiKey(api_key),
            base_url: None,
            allow_insecure_loopback: false,
            organization: None,
            project: None,
            client_request_id: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_json_body_bytes: DEFAULT_MAX_JSON_BODY_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
            tls_backend: default_tls_backend(),
            retry_policy: RetryPolicy::default(),
            sse_limits: SseLimits::default(),
            proxy: None,
        }
    }

    #[cfg(feature = "workload-identity")]
    #[must_use]
    pub fn from_workload_identity(config: WorkloadIdentityConfig) -> Self {
        Self {
            credential: ClientCredential::Workload(config),
            base_url: None,
            allow_insecure_loopback: false,
            organization: None,
            project: None,
            client_request_id: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_json_body_bytes: DEFAULT_MAX_JSON_BODY_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
            tls_backend: default_tls_backend(),
            retry_policy: RetryPolicy::default(),
            sse_limits: SseLimits::default(),
            proxy: None,
        }
    }

    /// Replaces the official base URL. Routes are still selected by typed
    /// resource methods; this does not enable per-request raw URLs.
    #[must_use]
    pub fn base_url(mut self, base_url: Url) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Permits plain HTTP only when the configured host is a literal loopback
    /// IP address. Intended for local tests and local emulators.
    #[must_use]
    pub const fn allow_insecure_loopback(mut self, allow: bool) -> Self {
        self.allow_insecure_loopback = allow;
        self
    }

    #[must_use]
    pub fn organization(mut self, organization: impl Into<String>) -> Self {
        self.organization = Some(organization.into());
        self
    }

    #[must_use]
    pub fn project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets `X-Client-Request-Id` on every request for timeout reconciliation.
    ///
    /// The value must be non-empty ASCII with no surrounding whitespace and at
    /// most 512 bytes.
    #[must_use]
    pub fn client_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.client_request_id = Some(request_id.into());
        self
    }

    /// Sets the per-attempt connection budget (TCP plus TLS handshake).
    ///
    /// The default is 10s, a deliberate middle ground between the two official
    /// baselines: openai-python sets a 5s connect budget while openai-node has
    /// no SDK-level connect timeout and inherits the transport default of 10s
    /// (see decisions D0163/D0199). The connect budget is independent of
    /// [`ClientBuilder::request_timeout`] and applies to every dial, including
    /// retried attempts and workload-identity token exchanges. Must be non-zero;
    /// zero values are rejected by [`ClientBuilder::build`].
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the total budget for one logical request.
    ///
    /// This budget covers connection, request write, server processing, body
    /// streaming, and any in-budget retries from start to finish — it is a
    /// *total* budget, matching openai-node. openai-python's 600s
    /// `DEFAULT_TIMEOUT` is instead applied by httpx per I/O operation, so the
    /// same number buys less there (D0199 corrects the attribution; the 600s
    /// default itself matches both SDKs). Long-running operations (large
    /// uploads, slow streams) should derive a wider budget with
    /// [`Client::with_request_timeout`] instead of raising this value for every
    /// call. Must be non-zero; zero values are rejected by
    /// [`ClientBuilder::build`].
    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets one explicit forward proxy for every connection this client makes.
    ///
    /// By default the client disables all proxying — including `HTTP_PROXY` /
    /// `HTTPS_PROXY` / `ALL_PROXY` environment variables — to match openai-node
    /// and to keep credentials off hops the caller cannot see. Passing
    /// `Some(proxy)` routes all API traffic (and, for workload-identity
    /// credentials, the token exchange) through that single declared proxy
    /// instead; passing `None` restores the default no-proxy posture. There is
    /// no way to enable the environment-variable proxies.
    #[must_use]
    pub fn proxy(mut self, proxy: Option<reqwest::Proxy>) -> Self {
        self.proxy = proxy;
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

    /// Selects one of the TLS backends compiled into this crate.
    #[must_use]
    pub const fn tls_backend(mut self, backend: TlsBackend) -> Self {
        self.tls_backend = Some(backend);
        self
    }

    /// Replaces the automatic retry policy.
    #[must_use]
    pub const fn retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Sets parser-owned memory limits for all SSE streams (Responses, chat
    /// completions, legacy completions, and media streams alike).
    #[must_use]
    pub const fn sse_limits(mut self, sse_limits: SseLimits) -> Self {
        self.sse_limits = sse_limits;
        self
    }

    pub fn build(self) -> Result<Client, Error> {
        if self.connect_timeout.is_zero() {
            return Err(invalid_configuration("connect timeout must be non-zero"));
        }
        if self.request_timeout.is_zero() {
            return Err(invalid_configuration("request timeout must be non-zero"));
        }
        if self.max_json_body_bytes == 0 {
            return Err(invalid_configuration(
                "JSON response body limit must be non-zero",
            ));
        }
        if self.max_error_body_bytes == 0 {
            return Err(invalid_configuration(
                "error response body limit must be non-zero",
            ));
        }

        let mut base_url = match self.base_url {
            Some(url) => url,
            None => Url::parse(DEFAULT_BASE_URL).map_err(|error| {
                invalid_configuration(format!("invalid built-in base URL: {error}"))
            })?,
        };
        validate_base_url(&base_url, self.allow_insecure_loopback)?;
        if base_url.scheme() == "https" && self.tls_backend.is_none() {
            return Err(invalid_configuration(
                "HTTPS base URL requires the rustls-tls or native-tls feature",
            ));
        }
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_owned();
            path.push('/');
            base_url.set_path(&path);
        }

        let organization = optional_sensitive_header(self.organization, "organization")?;
        let project = optional_sensitive_header(self.project, "project")?;
        let client_request_id = optional_client_request_id(self.client_request_id)?;

        let http = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("openai-rs/", env!("CARGO_PKG_VERSION")));
        // Proxy posture (aligned with openai-node): environment proxies are
        // never read, so credentials cannot traverse undeclared hops. Only the
        // explicitly supplied proxy is used; without one the client stays
        // direct via no_proxy().
        let http = match self.proxy.clone() {
            Some(proxy) => http.proxy(proxy),
            None => http.no_proxy(),
        };
        let http = match self.tls_backend {
            #[cfg(feature = "rustls-tls")]
            Some(TlsBackend::Rustls) => http.use_rustls_tls(),
            #[cfg(feature = "native-tls")]
            Some(TlsBackend::Native) => http.use_native_tls(),
            None => http,
        }
        .build()
        .map_err(Error::from_reqwest)?;

        let auth = match self.credential {
            ClientCredential::ApiKey(api_key) => AuthProvider::api_key(api_key),
            #[cfg(feature = "workload-identity")]
            ClientCredential::Workload(config) => {
                AuthProvider::workload(WorkloadIdentityAuth::new(
                    config,
                    self.tls_backend,
                    self.connect_timeout,
                    self.request_timeout,
                    // The token exchange shares the explicit proxy (if any):
                    // a declared hop covers both API traffic and exchange, and
                    // env proxies stay unread either way.
                    self.proxy,
                )?)
            }
        };

        let derivation = TransportDerivation {
            http,
            base_url,
            auth,
            organization,
            project,
            client_request_id,
            max_json_body_bytes: self.max_json_body_bytes,
            max_error_body_bytes: self.max_error_body_bytes,
            retry_policy: self.retry_policy,
            sse_limits: self.sse_limits,
            tls_backend: self.tls_backend,
        };
        Ok(Client {
            inner: Arc::new(Inner::from_derivation(derivation, self.request_timeout)),
        })
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base_origin = self
            .base_url
            .as_ref()
            .map(|url| url.origin().ascii_serialization());
        formatter
            .debug_struct("ClientBuilder")
            .field("credential", &"[REDACTED]")
            .field("base_origin", &base_origin)
            .field("allow_insecure_loopback", &self.allow_insecure_loopback)
            .field(
                "organization",
                &self.organization.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project.as_ref().map(|_| "[REDACTED]"))
            .field("client_request_id", &self.client_request_id)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .field("tls_backend", &self.tls_backend)
            .field("retry_policy", &self.retry_policy)
            .field("sse_limits", &self.sse_limits)
            .field("proxy", &self.proxy.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

fn validate_base_url(base_url: &Url, allow_insecure_loopback: bool) -> Result<(), Error> {
    if !base_url.username().is_empty() || base_url.password().is_some() {
        return Err(invalid_configuration(
            "base URL must not contain user information",
        ));
    }
    if base_url.query().is_some() || base_url.fragment().is_some() {
        return Err(invalid_configuration(
            "base URL must not contain a query or fragment",
        ));
    }
    if base_url.cannot_be_a_base() || base_url.host().is_none() {
        return Err(invalid_configuration(
            "base URL must be an absolute hierarchical URL",
        ));
    }

    match base_url.scheme() {
        "https" => Ok(()),
        "http" if allow_insecure_loopback && is_literal_loopback(base_url) => Ok(()),
        "http" => Err(invalid_configuration(
            "plain HTTP requires allow_insecure_loopback(true) and a literal loopback IP",
        )),
        _ => Err(invalid_configuration("base URL scheme must be HTTPS")),
    }
}

fn is_literal_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(_)) | None => false,
    }
}

fn optional_sensitive_header(
    value: Option<String>,
    name: &'static str,
) -> Result<Option<HeaderValue>, Error> {
    value
        .map(|value| {
            if value.is_empty() || value.trim() != value {
                return Err(invalid_configuration(format!(
                    "{name} header must be non-empty and have no surrounding whitespace"
                )));
            }
            let mut header = HeaderValue::from_str(&value).map_err(|_| {
                invalid_configuration(format!("{name} is not a valid HTTP header value"))
            })?;
            header.set_sensitive(true);
            Ok(header)
        })
        .transpose()
}

fn optional_client_request_id(value: Option<String>) -> Result<Option<HeaderValue>, Error> {
    value
        .map(|value| {
            if value.is_empty() || value.trim() != value {
                return Err(invalid_configuration(
                    "X-Client-Request-Id must be non-empty and have no surrounding whitespace",
                ));
            }
            if value.len() > 512 || !value.is_ascii() {
                return Err(invalid_configuration(
                    "X-Client-Request-Id must be ASCII and at most 512 bytes",
                ));
            }
            HeaderValue::from_str(&value).map_err(|_| {
                invalid_configuration("X-Client-Request-Id is not a valid HTTP header value")
            })
        })
        .transpose()
}

fn invalid_configuration(message: impl Into<Box<str>>) -> Error {
    Error::InvalidConfiguration(message.into())
}

const fn default_tls_backend() -> Option<TlsBackend> {
    #[cfg(feature = "rustls-tls")]
    {
        Some(TlsBackend::Rustls)
    }
    #[cfg(all(not(feature = "rustls-tls"), feature = "native-tls"))]
    {
        Some(TlsBackend::Native)
    }
    #[cfg(all(not(feature = "rustls-tls"), not(feature = "native-tls")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use bytes::Bytes;
    use http_body_util::Full;
    use hyper::{Request, StatusCode, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;

    use super::*;

    fn key() -> ApiKey {
        ApiKey::new("test-placeholder-key").expect("valid test key")
    }

    /// Serves an empty model list after `delay`, counting request arrivals.
    async fn delayed_models_server(delay: Duration) -> (Url, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind API server");
        let address = listener.local_addr().expect("API address");
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let requests = Arc::clone(&server_requests);
                tokio::spawn(async move {
                    let service = service_fn(move |_: Request<Incoming>| {
                        let requests = Arc::clone(&requests);
                        async move {
                            requests.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(delay).await;
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(StatusCode::OK)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .body(Full::new(Bytes::from_static(
                                        br#"{"object":"list","data":[]}"#,
                                    )))
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
            Url::parse(&format!("http://{address}/v1/")).expect("test API URL"),
            requests,
        )
    }

    #[test]
    #[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
    fn default_base_url_is_official_platform() {
        let client = Client::new(key()).expect("client builds");
        assert_eq!(client.base_url().as_str(), DEFAULT_BASE_URL);
    }

    #[test]
    #[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
    fn https_base_fails_without_a_tls_backend() {
        assert!(Client::new(key()).is_err());
    }

    #[test]
    fn insecure_base_requires_literal_loopback_and_opt_in() {
        let loopback = Url::parse("http://127.0.0.1:1234/v1/").expect("test URL");
        assert!(
            Client::builder(key())
                .base_url(loopback.clone())
                .build()
                .is_err()
        );
        assert!(
            Client::builder(key())
                .base_url(loopback)
                .allow_insecure_loopback(true)
                .build()
                .is_ok()
        );

        let localhost = Url::parse("http://localhost:1234/v1/").expect("test URL");
        assert!(
            Client::builder(key())
                .base_url(localhost)
                .allow_insecure_loopback(true)
                .build()
                .is_err()
        );
    }

    #[test]
    fn builder_debug_redacts_credentials_and_tenant_headers() {
        let builder = Client::builder(key())
            .organization("org-private")
            .project("proj-private");
        let debug = format!("{builder:?}");
        assert!(!debug.contains("test-placeholder-key"));
        assert!(!debug.contains("org-private"));
        assert!(!debug.contains("proj-private"));
    }

    #[test]
    fn client_request_id_rejects_non_ascii_and_oversize_values() {
        let loopback = Url::parse("http://127.0.0.1:1234/v1/").expect("test URL");
        assert!(
            Client::builder(key())
                .base_url(loopback.clone())
                .allow_insecure_loopback(true)
                .client_request_id("corr-1")
                .build()
                .is_ok()
        );
        assert!(
            Client::builder(key())
                .base_url(loopback.clone())
                .allow_insecure_loopback(true)
                .client_request_id(" corr-1")
                .build()
                .is_err()
        );
        assert!(
            Client::builder(key())
                .base_url(loopback.clone())
                .allow_insecure_loopback(true)
                .client_request_id("编号")
                .build()
                .is_err()
        );
        assert!(
            Client::builder(key())
                .base_url(loopback)
                .allow_insecure_loopback(true)
                .client_request_id("a".repeat(513))
                .build()
                .is_err()
        );
    }

    #[test]
    fn explicit_proxy_builds_and_none_restores_the_no_proxy_default() {
        let loopback = Url::parse("http://127.0.0.1:1234/v1/").expect("test URL");
        let proxy = reqwest::Proxy::all("http://127.0.0.1:1").expect("test proxy");
        assert!(
            Client::builder(key())
                .base_url(loopback.clone())
                .allow_insecure_loopback(true)
                .proxy(Some(proxy))
                .build()
                .is_ok(),
            "an explicit proxy must replace the no_proxy default"
        );
        assert!(
            Client::builder(key())
                .base_url(loopback)
                .allow_insecure_loopback(true)
                .proxy(None)
                .build()
                .is_ok(),
            "passing None must restore the no_proxy default"
        );
    }

    #[tokio::test]
    async fn explicit_proxy_carries_traffic_so_no_proxy_no_longer_applies() {
        let (api_url, requests) = delayed_models_server(Duration::ZERO).await;
        let dead_proxy = reqwest::Proxy::all("http://127.0.0.1:1").expect("test proxy");
        let client = Client::builder(key())
            .base_url(api_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .proxy(Some(dead_proxy))
            .build()
            .expect("client with an explicit proxy");
        assert!(
            matches!(client.models().list().await, Err(Error::Transport(_))),
            "the unreachable proxy must fail the request"
        );
        assert_eq!(
            requests.load(Ordering::SeqCst),
            0,
            "traffic must be routed to the proxy instead of the origin"
        );
    }

    #[tokio::test]
    async fn with_request_timeout_narrows_only_the_derived_client() {
        let (api_url, requests) = delayed_models_server(Duration::from_millis(400)).await;
        let client = Client::builder(key())
            .base_url(api_url)
            .allow_insecure_loopback(true)
            .request_timeout(Duration::from_secs(5))
            .build()
            .expect("base client");

        let narrowed = client.with_request_timeout(Duration::from_millis(100));
        assert!(
            matches!(narrowed.models().list().await, Err(Error::Timeout(_))),
            "the derived budget must expire before the delayed server answers"
        );
        client
            .models()
            .list()
            .await
            .expect("the original budget still covers the delay");
        narrowed
            .with_request_timeout(Duration::from_secs(5))
            .models()
            .list()
            .await
            .expect("deriving again widens the budget back");
        // One arrival per issued request: the timed-out attempt still reaches
        // the server, then the two successful calls.
        assert_eq!(requests.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn zero_derived_budget_fails_closed_immediately() {
        let (api_url, _) = delayed_models_server(Duration::ZERO).await;
        let client = Client::builder(key())
            .base_url(api_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("base client");
        assert!(
            matches!(
                client
                    .with_request_timeout(Duration::ZERO)
                    .models()
                    .list()
                    .await,
                Err(Error::DeadlineExceeded)
            ),
            "a zero derived budget has no time to spend"
        );
    }
}
