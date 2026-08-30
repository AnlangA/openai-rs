//! Verification and typed decoding for OpenAI webhook deliveries.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use http::HeaderMap;
use openai_rs_types::{
    Secret,
    webhooks::{VerifiedWebhook, WebhookEvent},
};
use sha2::Sha256;
use thiserror::Error;

const DEFAULT_TOLERANCE: Duration = Duration::from_secs(5 * 60);
const DEFAULT_MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SIGNATURE_CANDIDATES: usize = 32;
const MAX_SIGNATURE_HEADER_BYTES: usize = 8 * 1024;
const MAX_WEBHOOK_ID_BYTES: usize = 512;
const MAX_TIMESTAMP_BYTES: usize = 32;
const HMAC_SHA256_BYTES: usize = 32;

/// A secret and replay policy used to authenticate webhook deliveries.
#[derive(Clone)]
pub struct WebhookVerifier {
    secret: Secret,
    tolerance: Duration,
    max_payload_bytes: usize,
}

impl WebhookVerifier {
    /// Creates a verifier with a five-minute replay window.
    pub fn new(secret: impl Into<Secret>) -> Result<Self, WebhookVerificationError> {
        let secret = secret.into();
        if secret.is_empty() {
            return Err(WebhookVerificationError::InvalidSecret);
        }
        Ok(Self {
            secret,
            tolerance: DEFAULT_TOLERANCE,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
        })
    }

    /// Replaces the accepted clock-skew/replay window.
    pub fn with_tolerance(mut self, tolerance: Duration) -> Result<Self, WebhookVerificationError> {
        if tolerance.is_zero() {
            return Err(WebhookVerificationError::InvalidTolerance);
        }
        self.tolerance = tolerance;
        Ok(self)
    }

    /// Replaces the maximum body size accepted before HMAC work or decoding.
    pub fn with_max_payload_bytes(
        mut self,
        max_payload_bytes: usize,
    ) -> Result<Self, WebhookVerificationError> {
        if max_payload_bytes == 0 {
            return Err(WebhookVerificationError::InvalidPayloadLimit);
        }
        self.max_payload_bytes = max_payload_bytes;
        Ok(self)
    }

    /// Verifies the timestamp and HMAC before decoding a typed event.
    pub fn verify(
        &self,
        payload: &[u8],
        headers: &HeaderMap,
    ) -> Result<VerifiedWebhook<WebhookEvent>, WebhookVerificationError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| WebhookVerificationError::Clock)?
            .as_secs();
        self.verify_at(payload, headers, now)
    }

    /// Deterministic variant of [`Self::verify`] for controlled clocks/tests.
    pub fn verify_at(
        &self,
        payload: &[u8],
        headers: &HeaderMap,
        now_epoch_seconds: u64,
    ) -> Result<VerifiedWebhook<WebhookEvent>, WebhookVerificationError> {
        if payload.len() > self.max_payload_bytes {
            return Err(WebhookVerificationError::PayloadTooLarge {
                limit: self.max_payload_bytes,
            });
        }

        let timestamp_text = required_header(headers, "webhook-timestamp", MAX_TIMESTAMP_BYTES)?;
        if timestamp_text.is_empty() || !timestamp_text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(WebhookVerificationError::InvalidTimestamp);
        }
        let timestamp = timestamp_text
            .parse::<u64>()
            .map_err(|_| WebhookVerificationError::InvalidTimestamp)?;
        let tolerance = self.tolerance.as_secs();
        if now_epoch_seconds.saturating_sub(timestamp) > tolerance {
            return Err(WebhookVerificationError::TimestampTooOld);
        }
        if timestamp.saturating_sub(now_epoch_seconds) > tolerance {
            return Err(WebhookVerificationError::TimestampTooNew);
        }

        let webhook_id = required_header(headers, "webhook-id", MAX_WEBHOOK_ID_BYTES)?;
        let signature_header =
            joined_header_values(headers, "webhook-signature", MAX_SIGNATURE_HEADER_BYTES)?;
        let signatures = decode_signatures(&signature_header)?;
        let signing_key = self.secret.with_exposed(decode_secret)?;

        let mut signed = Vec::with_capacity(
            webhook_id
                .len()
                .saturating_add(timestamp_text.len())
                .saturating_add(payload.len())
                .saturating_add(2),
        );
        signed.extend_from_slice(webhook_id.as_bytes());
        signed.push(b'.');
        signed.extend_from_slice(timestamp_text.as_bytes());
        signed.push(b'.');
        signed.extend_from_slice(payload);

        let mut matched = false;
        for signature in signatures {
            let mut mac = Hmac::<Sha256>::new_from_slice(&signing_key)
                .map_err(|_| WebhookVerificationError::InvalidSecret)?;
            mac.update(&signed);
            // Evaluate every bounded candidate rather than returning on the
            // first match. This keeps work independent of the matching index.
            matched |= mac.verify_slice(&signature).is_ok();
        }
        if !matched {
            return Err(WebhookVerificationError::SignatureMismatch);
        }

        let event = serde_json::from_slice(payload).map_err(WebhookVerificationError::Decode)?;
        Ok(VerifiedWebhook::from_verified(event))
    }
}

impl fmt::Debug for WebhookVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebhookVerifier")
            .field("secret", &"[REDACTED]")
            .field("tolerance", &self.tolerance)
            .field("max_payload_bytes", &self.max_payload_bytes)
            .finish()
    }
}

/// A webhook could not be authenticated or decoded.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WebhookVerificationError {
    /// A required delivery header was absent or invalid.
    #[error("missing or invalid required webhook header `{0}`")]
    InvalidHeader(&'static str),
    /// The signature header contained too many or malformed candidates.
    #[error("invalid webhook signature header")]
    InvalidSignatureHeader,
    /// The delivery timestamp was not a strict unsigned integer.
    #[error("invalid webhook timestamp")]
    InvalidTimestamp,
    /// The delivery fell outside the replay window in the past.
    #[error("webhook timestamp is too old")]
    TimestampTooOld,
    /// The delivery fell outside the replay window in the future.
    #[error("webhook timestamp is too new")]
    TimestampTooNew,
    /// None of the bounded signatures matched.
    #[error("webhook signature does not match")]
    SignatureMismatch,
    /// The configured signing secret was empty or malformed.
    #[error("invalid webhook secret")]
    InvalidSecret,
    /// A zero replay window was requested.
    #[error("webhook tolerance must be non-zero")]
    InvalidTolerance,
    /// A zero body limit was requested.
    #[error("webhook payload limit must be non-zero")]
    InvalidPayloadLimit,
    /// The body was larger than the configured limit.
    #[error("webhook payload exceeds the {limit}-byte limit")]
    PayloadTooLarge {
        /// Configured body limit.
        limit: usize,
    },
    /// The verified body was not a valid typed event.
    #[error("verified webhook body is invalid JSON or wire data")]
    Decode(#[source] serde_json::Error),
    /// The system clock was before the Unix epoch.
    #[error("system clock is before the Unix epoch")]
    Clock,
}

fn required_header(
    headers: &HeaderMap,
    name: &'static str,
    max_bytes: usize,
) -> Result<String, WebhookVerificationError> {
    let values = headers.get_all(name);
    let mut iter = values.iter();
    let value = iter
        .next()
        .ok_or(WebhookVerificationError::InvalidHeader(name))?;
    if iter.next().is_some() {
        return Err(WebhookVerificationError::InvalidHeader(name));
    }
    let value = value
        .to_str()
        .map_err(|_| WebhookVerificationError::InvalidHeader(name))?;
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(WebhookVerificationError::InvalidHeader(name));
    }
    Ok(value.to_owned())
}

fn joined_header_values(
    headers: &HeaderMap,
    name: &'static str,
    max_bytes: usize,
) -> Result<String, WebhookVerificationError> {
    let mut joined = String::new();
    for value in headers.get_all(name) {
        let value = value
            .to_str()
            .map_err(|_| WebhookVerificationError::InvalidHeader(name))?;
        if !joined.is_empty() {
            joined.push(' ');
        }
        if joined.len().saturating_add(value.len()) > max_bytes {
            return Err(WebhookVerificationError::InvalidSignatureHeader);
        }
        joined.push_str(value);
    }
    if joined.is_empty() {
        Err(WebhookVerificationError::InvalidHeader(name))
    } else {
        Ok(joined)
    }
}

fn decode_signatures(
    header: &str,
) -> Result<Vec<[u8; HMAC_SHA256_BYTES]>, WebhookVerificationError> {
    let mut signatures = Vec::new();
    for candidate in header.split_ascii_whitespace() {
        if signatures.len() == MAX_SIGNATURE_CANDIDATES {
            return Err(WebhookVerificationError::InvalidSignatureHeader);
        }
        let encoded = candidate.strip_prefix("v1,").unwrap_or(candidate);
        let decoded = match STANDARD.decode(encoded) {
            Ok(decoded) if decoded.len() == HMAC_SHA256_BYTES => decoded,
            Ok(_) | Err(_) => continue,
        };
        let mut signature = [0_u8; HMAC_SHA256_BYTES];
        signature.copy_from_slice(&decoded);
        signatures.push(signature);
    }
    if signatures.is_empty() {
        Err(WebhookVerificationError::InvalidSignatureHeader)
    } else {
        Ok(signatures)
    }
}

fn decode_secret(secret: &str) -> Result<Vec<u8>, WebhookVerificationError> {
    if secret.is_empty() {
        return Err(WebhookVerificationError::InvalidSecret);
    }
    match secret.strip_prefix("whsec_") {
        Some(encoded) => STANDARD
            .decode(encoded)
            .map_err(|_| WebhookVerificationError::InvalidSecret),
        None => Ok(secret.as_bytes().to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac};
    use http::{HeaderMap, HeaderValue};
    use sha2::Sha256;

    use super::{MAX_SIGNATURE_CANDIDATES, WebhookVerificationError, WebhookVerifier};

    const NOW: u64 = 1_800_000_000;
    const ID: &str = "evt_delivery_123";
    const PAYLOAD: &[u8] = br#"{"type":"future.event","id":"evt_1","private":"do-not-log"}"#;

    fn headers(secret: &[u8], timestamp: u64, payload: &[u8]) -> HeaderMap {
        let timestamp = timestamp.to_string();
        let signed = [ID.as_bytes(), b".", timestamp.as_bytes(), b".", payload].concat();
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("test HMAC key");
        mac.update(&signed);
        let signature = STANDARD.encode(mac.finalize().into_bytes());
        let mut headers = HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_static(ID));
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&timestamp).expect("timestamp header"),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&format!("v1,invalid v1,{signature}")).expect("signature header"),
        );
        headers
    }

    #[test]
    fn verifies_before_decoding_and_accepts_any_matching_rotation_signature() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new(String::from_utf8(secret.to_vec()).expect("UTF-8"))
            .expect("verifier");
        let verified = verifier
            .verify_at(PAYLOAD, &headers(secret, NOW, PAYLOAD), NOW)
            .expect("valid delivery");
        assert_eq!(verified.as_ref().event_type(), "future.event");
        assert!(!format!("{verified:?}").contains("do-not-log"));
    }

    #[test]
    fn rejects_replay_future_tamper_and_unbounded_signature_lists() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new(String::from_utf8(secret.to_vec()).expect("UTF-8"))
            .expect("verifier");
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &headers(secret, NOW - 301, PAYLOAD), NOW),
            Err(WebhookVerificationError::TimestampTooOld)
        ));
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &headers(secret, NOW + 301, PAYLOAD), NOW),
            Err(WebhookVerificationError::TimestampTooNew)
        ));
        assert!(matches!(
            verifier.verify_at(b"{}", &headers(secret, NOW, PAYLOAD), NOW),
            Err(WebhookVerificationError::SignatureMismatch)
        ));

        let mut too_many = headers(secret, NOW, PAYLOAD);
        let candidates = std::iter::repeat_n("v1,AAAA", MAX_SIGNATURE_CANDIDATES + 1)
            .collect::<Vec<_>>()
            .join(" ");
        too_many.insert(
            "webhook-signature",
            HeaderValue::from_str(&candidates).expect("bounded test header"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &too_many, NOW),
            Err(WebhookVerificationError::InvalidSignatureHeader)
        ));
    }

    #[test]
    fn supports_prefixed_base64_secret_and_redacts_debug() {
        let raw = b"prefixed-secret";
        let secret = format!("whsec_{}", STANDARD.encode(raw));
        let verifier = WebhookVerifier::new(secret.clone()).expect("verifier");
        verifier
            .verify_at(PAYLOAD, &headers(raw, NOW, PAYLOAD), NOW)
            .expect("valid prefixed secret");
        let debug = format!("{verifier:?}");
        assert!(!debug.contains(&secret));
        assert!(debug.contains("REDACTED"));
    }
}
