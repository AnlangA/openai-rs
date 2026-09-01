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
    /// Neutral display (the D0206 app-server stance, synced to the direct
    /// backend by 10-01): the wrapped `serde_json::Error` message can quote
    /// payload fragments — a streamed SSE `data` frame, a keyring session, or
    /// an auth response body — so only the category is shown; the source stays
    /// reachable for handlers that want line/column diagnostics.
    #[error("direct Codex JSON codec failed")]
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

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::DirectError;

    /// 10-01: the four `Json` construction points (streamed SSE `data` frames,
    /// the Responses body decode, the auth body decode, and the keyring
    /// session codec) all wrap the raw `serde_json::Error`, whose message
    /// quotes payload fragments. The display must stay neutral — the D0206
    /// app-server stance — while the source remains reachable for diagnostics.
    #[test]
    fn json_display_never_quotes_the_payload() {
        let codec = serde_json::from_str::<u32>("\"direct-payload-literal\"")
            .expect_err("serde_json must reject a string where a u32 is expected");
        assert!(
            codec.to_string().contains("direct-payload-literal"),
            "the fixture must quote the payload so the leak assertion is meaningful"
        );

        let error = DirectError::Json(codec);
        assert_eq!(error.to_string(), "direct Codex JSON codec failed");
        assert!(
            !error.to_string().contains("direct-payload-literal"),
            "the payload literal must never reach the Display"
        );
        assert!(
            error.source().is_some(),
            "the serde_json source must stay reachable for diagnostics"
        );
    }
}
