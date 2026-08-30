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
}
