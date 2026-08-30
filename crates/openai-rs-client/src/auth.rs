use std::fmt;

use http::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use zeroize::Zeroizing;

#[cfg(feature = "workload-identity")]
use crate::workload_identity::{TokenLease, WorkloadIdentityAuth};

/// A Platform API key.
///
/// The inner value is never exposed through `Debug`, `Display`, or Serde. This
/// type is intentionally distinct from every credential used by Codex or
/// ChatGPT subscription authentication.
#[derive(Clone)]
pub struct ApiKey(SecretString);

impl ApiKey {
    /// Validates and wraps a Platform API key.
    pub fn new(key: impl Into<String>) -> Result<Self, ApiKeyError> {
        let key = key.into();
        if key.is_empty() {
            return Err(ApiKeyError::Empty);
        }
        if key.trim() != key {
            return Err(ApiKeyError::SurroundingWhitespace);
        }
        if key.chars().any(char::is_whitespace) {
            return Err(ApiKeyError::Whitespace);
        }
        if key.chars().any(char::is_control) {
            return Err(ApiKeyError::ControlCharacter);
        }
        if !key.is_ascii() {
            return Err(ApiKeyError::NonAscii);
        }
        Ok(Self(SecretString::from(key)))
    }

    pub(crate) fn authorization_header(&self) -> Result<HeaderValue, ApiKeyError> {
        let value = Zeroizing::new(format!("Bearer {}", self.0.expose_secret()));
        let mut header =
            HeaderValue::from_str(value.as_str()).map_err(|_| ApiKeyError::InvalidHeaderValue)?;
        header.set_sensitive(true);
        Ok(header)
    }
}

#[derive(Clone)]
pub(crate) enum AuthProvider {
    ApiKey(ApiKey),
    #[cfg(feature = "workload-identity")]
    Workload(std::sync::Arc<WorkloadIdentityAuth>),
}

impl AuthProvider {
    pub(crate) const fn api_key(api_key: ApiKey) -> Self {
        Self::ApiKey(api_key)
    }

    #[cfg(feature = "workload-identity")]
    pub(crate) fn workload(auth: std::sync::Arc<WorkloadIdentityAuth>) -> Self {
        Self::Workload(auth)
    }

    pub(crate) async fn authorization(&self) -> Result<AuthLease, crate::Error> {
        match self {
            Self::ApiKey(api_key) => api_key
                .authorization_header()
                .map(|header| AuthLease {
                    header,
                    generation: None,
                })
                .map_err(|error| crate::Error::InvalidConfiguration(error.to_string().into())),
            #[cfg(feature = "workload-identity")]
            Self::Workload(auth) => auth.token().await.map(AuthLease::from),
        }
    }

    pub(crate) async fn invalidate_if_generation(&self, generation: Option<u64>) -> bool {
        match (self, generation) {
            #[cfg(feature = "workload-identity")]
            (Self::Workload(auth), Some(generation)) => {
                auth.invalidate_if_generation(generation).await
            }
            _ => false,
        }
    }
}

impl fmt::Debug for AuthProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => formatter.write_str("AuthProvider::ApiKey([REDACTED])"),
            #[cfg(feature = "workload-identity")]
            Self::Workload(_) => formatter.write_str("AuthProvider::Workload([REDACTED])"),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AuthLease {
    pub header: HeaderValue,
    pub generation: Option<u64>,
}

#[cfg(feature = "workload-identity")]
impl From<TokenLease> for AuthLease {
    fn from(lease: TokenLease) -> Self {
        Self {
            header: lease.header,
            generation: lease.generation,
        }
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

/// Validation failures for [`ApiKey`].
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyError {
    #[error("the API key is empty")]
    Empty,
    #[error("the API key has leading or trailing whitespace")]
    SurroundingWhitespace,
    #[error("the API key contains whitespace")]
    Whitespace,
    #[error("the API key contains a control character")]
    ControlCharacter,
    #[error("the API key contains non-ASCII characters")]
    NonAscii,
    #[error("the API key cannot be represented as an HTTP authorization header")]
    InvalidHeaderValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let key = ApiKey::new("test-placeholder-key").expect("valid test key");
        let debug = format!("{key:?}");
        assert!(!debug.contains("test-placeholder-key"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn rejects_unsafe_header_input() {
        assert!(matches!(ApiKey::new(""), Err(ApiKeyError::Empty)));
        assert!(matches!(
            ApiKey::new(" key"),
            Err(ApiKeyError::SurroundingWhitespace)
        ));
        assert!(matches!(
            ApiKey::new("key\r\nheader: value"),
            Err(ApiKeyError::Whitespace)
        ));
        assert!(matches!(
            ApiKey::new("key with-space"),
            Err(ApiKeyError::Whitespace)
        ));
        assert!(matches!(ApiKey::new("密钥"), Err(ApiKeyError::NonAscii)));
    }
}
