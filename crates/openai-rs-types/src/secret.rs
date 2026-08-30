//! Secret-bearing string types with deliberately narrow serialization rules.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const REDACTED: &str = "[REDACTED]";

/// An authentication secret that cannot be serialized through Serde.
///
/// This is suitable for API keys, access tokens, and workload credentials.
/// The contents are zeroized on drop by [`secrecy::SecretString`]. Use
/// [`Secret::with_exposed`] only at the narrow boundary that constructs an
/// authorization header.
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// Moves an owned secret string into zeroizing storage.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value.into())
    }

    /// Borrows the secret for the duration of one explicit operation.
    ///
    /// Callers should avoid returning, logging, or otherwise retaining the
    /// borrowed contents from the callback.
    pub fn with_exposed<R>(&self, operation: impl FnOnce(&str) -> R) -> R {
        operation(self.0.expose_secret())
    }

    /// Returns whether the secret is empty without exposing its contents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Secret").field(&REDACTED).finish()
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

/// A secret that the OpenAI wire protocol explicitly places in a JSON body.
///
/// Unlike [`Secret`], this wrapper implements [`Serialize`] for fields such as
/// protocol-defined MCP authorization values. Its `Debug` and `Display`
/// implementations remain redacted, and its contents are zeroized on drop.
#[derive(Clone)]
pub struct WireSecret(SecretString);

impl WireSecret {
    /// Moves an owned protocol secret into zeroizing storage.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value.into())
    }

    /// Borrows the secret for a narrowly scoped protocol operation.
    pub fn with_exposed<R>(&self, operation: impl FnOnce(&str) -> R) -> R {
        operation(self.0.expose_secret())
    }

    /// Returns whether the secret is empty without exposing its contents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }
}

impl From<String> for WireSecret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for WireSecret {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}

impl fmt::Debug for WireSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("WireSecret")
            .field(&REDACTED)
            .finish()
    }
}

impl fmt::Display for WireSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl Serialize for WireSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.expose_secret())
    }
}

impl<'de> Deserialize<'de> for WireSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::{Serialize, de::DeserializeOwned};
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::{Secret, WireSecret};

    assert_impl_all!(Secret: DeserializeOwned, Send, Sync);
    assert_not_impl_any!(Secret: Serialize);
    assert_impl_all!(WireSecret: Serialize, DeserializeOwned, Send, Sync);

    #[test]
    fn auth_secret_never_uses_serde_for_exfiltration() {
        let secret: Secret =
            serde_json::from_str(r#""sk-private-value""#).expect("load secret from config");

        assert_eq!(format!("{secret}"), "[REDACTED]");
        assert_eq!(format!("{secret:?}"), "Secret(\"[REDACTED]\")");
        assert!(!format!("{secret:?}").contains("sk-private-value"));
        assert_eq!(secret.with_exposed(str::len), "sk-private-value".len());
    }

    #[test]
    fn wire_secret_serializes_only_through_explicit_wrapper() {
        let secret = WireSecret::from("Bearer protocol-value");
        let encoded = serde_json::to_string(&secret).expect("encode wire secret");
        let decoded = serde_json::from_str::<WireSecret>(&encoded).expect("decode wire secret");

        assert_eq!(encoded, r#""Bearer protocol-value""#);
        assert_eq!(format!("{decoded}"), "[REDACTED]");
        assert_eq!(format!("{decoded:?}"), "WireSecret(\"[REDACTED]\")");
        assert_eq!(
            decoded.with_exposed(ToOwned::to_owned),
            "Bearer protocol-value"
        );
    }

    proptest! {
        #[test]
        fn redaction_is_input_independent(value in ".{1,256}") {
            let auth = Secret::from(value.as_str());
            let wire = WireSecret::from(value.as_str());
            let auth_debug = format!("{auth:?}");
            let wire_debug = format!("{wire:?}");

            prop_assert_eq!(auth_debug, "Secret(\"[REDACTED]\")");
            prop_assert_eq!(wire_debug, "WireSecret(\"[REDACTED]\")");
            prop_assert_eq!(format!("{auth}"), "[REDACTED]");
            prop_assert_eq!(format!("{wire}"), "[REDACTED]");
        }
    }
}
