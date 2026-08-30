use std::{fmt, sync::Arc, time::Duration};

use http::HeaderValue;
use url::{Host, Url};

use crate::{
    ApiKey, Embeddings, Error, Models, Moderations, Responses, RetryPolicy, transport::Transport,
};

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

    /// Returns the Responses resource facade.
    #[must_use]
    pub fn responses(&self) -> Responses {
        Responses::new(self.clone())
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

    /// The configured API base URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.inner.transport.base_url()
    }

    pub(crate) fn transport(&self) -> &Transport {
        &self.inner.transport
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
    api_key: ApiKey,
    base_url: Option<Url>,
    allow_insecure_loopback: bool,
    organization: Option<String>,
    project: Option<String>,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    tls_backend: Option<TlsBackend>,
    retry_policy: RetryPolicy,
}

impl ClientBuilder {
    #[must_use]
    pub fn new(api_key: ApiKey) -> Self {
        Self {
            api_key,
            base_url: None,
            allow_insecure_loopback: false,
            organization: None,
            project: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_json_body_bytes: DEFAULT_MAX_JSON_BODY_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
            tls_backend: default_tls_backend(),
            retry_policy: RetryPolicy::default(),
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
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_owned();
            path.push('/');
            base_url.set_path(&path);
        }

        let authorization = self
            .api_key
            .authorization_header()
            .map_err(|error| invalid_configuration(error.to_string()))?;
        let organization = optional_sensitive_header(self.organization, "organization")?;
        let project = optional_sensitive_header(self.project, "project")?;

        let http = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(concat!("openai-rs/", env!("CARGO_PKG_VERSION")));
        let http = match self.tls_backend {
            #[cfg(feature = "rustls-tls")]
            Some(TlsBackend::Rustls) => http.use_rustls_tls(),
            #[cfg(feature = "native-tls")]
            Some(TlsBackend::Native) => http.use_native_tls(),
            None => http,
        }
        .build()
        .map_err(Error::from_reqwest)?;

        Ok(Client {
            inner: Arc::new(Inner {
                transport: Transport::new(
                    http,
                    base_url,
                    authorization,
                    organization,
                    project,
                    self.max_json_body_bytes,
                    self.max_error_body_bytes,
                    self.retry_policy,
                    self.request_timeout,
                ),
            }),
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
            .field("api_key", &"[REDACTED]")
            .field("base_origin", &base_origin)
            .field("allow_insecure_loopback", &self.allow_insecure_loopback)
            .field(
                "organization",
                &self.organization.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project.as_ref().map(|_| "[REDACTED]"))
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .field("tls_backend", &self.tls_backend)
            .field("retry_policy", &self.retry_policy)
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
    use super::*;

    fn key() -> ApiKey {
        ApiKey::new("test-placeholder-key").expect("valid test key")
    }

    #[test]
    fn default_base_url_is_official_platform() {
        let client = Client::new(key()).expect("client builds");
        assert_eq!(client.base_url().as_str(), DEFAULT_BASE_URL);
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
}
