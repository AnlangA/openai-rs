use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::DirectError;
use super::auth::{CredentialStore, StoredCodexSession};
use super::jwt::ChatGptAccountId;

const SERVICE: &str = "openai-rs-codex";
const FORMAT_VERSION: u32 = 1;

/// Native secure-store implementation for one explicitly named local account.
pub struct KeyringStore {
    entry_name: String,
}

impl KeyringStore {
    pub fn new(entry_name: impl Into<String>) -> Result<Self, DirectError> {
        let entry_name = entry_name.into();
        if entry_name.trim().is_empty() || entry_name.len() > 128 {
            return Err(DirectError::Configuration(
                "keyring entry name must be 1..=128 characters".to_owned(),
            ));
        }
        Ok(Self { entry_name })
    }
}

impl std::fmt::Debug for KeyringStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyringStore")
            .field("service", &SERVICE)
            .field("entry_name", &"<redacted>")
            .finish()
    }
}

#[async_trait]
impl CredentialStore for KeyringStore {
    async fn load(&self) -> Result<Option<StoredCodexSession>, DirectError> {
        let entry_name = self.entry_name.clone();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(SERVICE, &entry_name)
                .map_err(|error| store_error("open", error))?;
            match entry.get_password() {
                Ok(password) => {
                    let password = Zeroizing::new(password);
                    decode_session(&password).map(Some)
                }
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(store_error("load", error)),
            }
        })
        .await
        .map_err(|error| DirectError::Store(format!("keyring load task failed: {error}")))?
    }

    async fn save(&self, session: &StoredCodexSession) -> Result<(), DirectError> {
        let entry_name = self.entry_name.clone();
        let encoded = encode_session(session)?;
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(SERVICE, &entry_name)
                .map_err(|error| store_error("open", error))?;
            entry
                .set_password(&encoded)
                .map_err(|error| store_error("save", error))
        })
        .await
        .map_err(|error| DirectError::Store(format!("keyring save task failed: {error}")))?
    }

    async fn delete(&self) -> Result<(), DirectError> {
        let entry_name = self.entry_name.clone();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(SERVICE, &entry_name)
                .map_err(|error| store_error("open", error))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(store_error("delete", error)),
            }
        })
        .await
        .map_err(|error| DirectError::Store(format!("keyring delete task failed: {error}")))?
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedSession {
    format_version: u32,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    account_id: String,
    generation: u64,
}

impl Drop for PersistedSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.account_id.zeroize();
    }
}

fn encode_session(session: &StoredCodexSession) -> Result<Zeroizing<String>, DirectError> {
    let persisted = PersistedSession {
        format_version: FORMAT_VERSION,
        access_token: session.access_token().to_owned(),
        refresh_token: session.refresh_token().to_owned(),
        expires_at: session.expires_at,
        account_id: session.account_id.as_str().to_owned(),
        generation: session.generation,
    };
    serde_json::to_string(&persisted)
        .map(Zeroizing::new)
        .map_err(DirectError::Json)
}

fn decode_session(encoded: &str) -> Result<StoredCodexSession, DirectError> {
    let mut persisted: PersistedSession = serde_json::from_str(encoded)?;
    if persisted.format_version != FORMAT_VERSION {
        return Err(DirectError::Store(
            "unsupported keyring session format".to_owned(),
        ));
    }
    let access_token = Arc::new(SecretString::from(std::mem::take(
        &mut persisted.access_token,
    )));
    let refresh_token = Arc::new(SecretString::from(std::mem::take(
        &mut persisted.refresh_token,
    )));
    Ok(StoredCodexSession {
        access_token,
        refresh_token,
        expires_at: persisted.expires_at,
        account_id: ChatGptAccountId::parse(std::mem::take(&mut persisted.account_id))?,
        generation: persisted.generation,
    })
}

fn store_error(operation: &'static str, error: keyring::Error) -> DirectError {
    DirectError::Store(format!("keyring {operation} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{KeyringStore, decode_session, encode_session};
    use crate::direct::auth::StoredCodexSession;
    use crate::direct::jwt::ChatGptAccountId;

    #[test]
    fn private_codec_round_trips_and_public_debug_is_redacted()
    -> Result<(), Box<dyn std::error::Error>> {
        let session = StoredCodexSession::fixture(
            "access-secret",
            "refresh-secret",
            1234,
            ChatGptAccountId::fixture("acct-123")?,
        );
        let encoded = encode_session(&session)?;
        let decoded = decode_session(&encoded)?;
        assert_eq!(decoded.access_token(), "access-secret");
        assert_eq!(decoded.refresh_token(), "refresh-secret");
        let debug = format!("{session:?} {:?}", KeyringStore::new("local-account")?);
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert!(!debug.contains("local-account"));
        Ok(())
    }

    #[test]
    fn private_codec_rejects_unknown_version() {
        let encoded = r#"{"format_version":99,"access_token":"a","refresh_token":"r","expires_at":1,"account_id":"acct-1","generation":0}"#;
        assert!(decode_session(encoded).is_err());
    }

    /// 8-22: the entry-name guard rejects empty/whitespace and 129-character
    /// names while accepting the documented 128-character boundary.
    #[test]
    fn entry_name_validation_covers_empty_and_length_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        for empty in ["", "   ", "\t"] {
            assert!(
                matches!(
                    KeyringStore::new(empty),
                    Err(crate::direct::DirectError::Configuration(ref message))
                        if message.contains("1..=128")
                ),
                "an empty entry name must be rejected: {empty:?}"
            );
        }
        assert!(KeyringStore::new("x".repeat(129)).is_err());
        let boundary = KeyringStore::new("x".repeat(128))?;
        assert!(
            !format!("{boundary:?}").contains("x".repeat(128).as_str()),
            "the entry name must stay redacted from Debug"
        );
        Ok(())
    }

    /// 8-22: a persisted session with an account identifier the JWT parser
    /// would refuse (illegal characters or over-length) fails decode instead
    /// of materializing an unvalidated account id.
    #[test]
    fn decode_session_rejects_an_illegal_account_id() {
        let illegal = ["acct/1", "acct@openai", "", "a b c"];
        let mut account_ids: Vec<String> = illegal.iter().map(ToString::to_string).collect();
        account_ids.push("a".repeat(257));
        for account_id in account_ids {
            let encoded = format!(
                r#"{{"format_version":1,"access_token":"a","refresh_token":"r","expires_at":1,"account_id":"{account_id}","generation":0}}"#
            );
            assert!(
                decode_session(&encoded).is_err(),
                "account_id {account_id:?} must fail decode"
            );
        }
        // The legal shape still decodes.
        let encoded = r#"{"format_version":1,"access_token":"a","refresh_token":"r","expires_at":1,"account_id":"acct-123_456","generation":2}"#;
        let session = decode_session(encoded).expect("legal account id decodes");
        assert_eq!(session.account_id().as_str(), "acct-123_456");
    }
}
