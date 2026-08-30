//! Private, experimental ChatGPT Codex Responses backend.
//!
//! This module is deliberately sealed to one origin and one operation family.
//! It is not an OpenAI-compatible proxy and exposes no raw URL request API.

mod auth;
mod jwt;
#[cfg(feature = "experimental-direct-keyring")]
mod keyring_store;
mod sse;
mod transport;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use auth::{
    BrowserLogin, CredentialStore, DeviceCodeLogin, DirectAuthClient, EphemeralStore,
    StoredCodexSession, TokenManager,
};
pub use jwt::ChatGptAccountId;
#[cfg(feature = "experimental-direct-keyring")]
pub use keyring_store::KeyringStore;
pub use transport::{DirectCodexResponsesClient, DirectResponseStream};

/// The only model endpoint reachable by the direct backend.
pub const CODEX_RESPONSES_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";

/// Errors from the private experimental direct backend.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DirectError {
    #[error("invalid direct Codex configuration: {0}")]
    Configuration(String),
    #[error("secure randomness failed")]
    Random,
    #[error("direct Codex HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("direct Codex JSON codec failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("OIDC token validation failed: {0}")]
    Jwt(String),
    #[error("OAuth protocol failed: {0}")]
    OAuth(String),
    #[error("credential store failed: {0}")]
    Store(String),
    #[error("operation was cancelled")]
    Cancelled,
    #[error("operation timed out")]
    Timeout,
    #[error("HTTP redirect was rejected")]
    RedirectRejected,
    #[error("direct Codex returned HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },
    #[error("response body exceeded the configured limit")]
    BodyTooLarge,
    #[error("invalid SSE stream: {0}")]
    Sse(String),
    #[error("request field {0} is not supported by the sealed Codex backend")]
    UnsupportedRequestField(&'static str),
    #[error("authentication is required")]
    ReauthenticationRequired,
}

/// Cloneable cooperative cancellation signal used by browser/device flows.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationInner>,
}

#[derive(Debug, Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

pub(crate) fn secure_equal(first: &[u8], second: &[u8]) -> bool {
    let key = ring::hmac::Key::new(
        ring::hmac::HMAC_SHA256,
        b"openai-rs/direct-auth/equality/v1",
    );
    let expected = ring::hmac::sign(&key, first);
    ring::hmac::verify(&key, second, expected.as_ref()).is_ok()
}
