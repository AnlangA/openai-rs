use std::fmt;

use http::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use zeroize::Zeroizing;

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
        if key.chars().any(char::is_control) {
            return Err(ApiKeyError::ControlCharacter);
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
    #[error("the API key contains a control character")]
    ControlCharacter,
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
            Err(ApiKeyError::ControlCharacter)
        ));
    }
}
