//! Verification and typed decoding for OpenAI webhook deliveries.
//!
//! # Delivery semantics
//!
//! OpenAI treats a delivery as failed unless the endpoint answers with a
//! 2xx status within a few seconds, and retries failed deliveries with
//! exponential backoff for up to 72 hours. Redirect responses (3xx) are
//! not followed and count as failures. A delivery that times out after
//! the handler already did its work is still retried, so the same event
//! may be delivered more than once: use the `webhook-id` header —
//! preserved as [`VerifiedWebhook::webhook_id`] on the verification
//! result — as the idempotency key and skip deliveries already processed.
//! Respond with 2xx as quickly as possible and process the event in the
//! background, mirroring the official webhook guidance in openai-node's
//! `docs/webhooks.md`.

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
// The joined `webhook-signature` header bound is the only limiter on the
// candidate count (a full 8 KiB header of `v1,<44-char tag>` candidates
// admits about 170 slots): node's `webhook-signature-amplification` test
// pins that every slot inside the bound is evaluated, with no per-slot
// count rejection.
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
    ///
    /// The secret is validated eagerly: an empty secret, or a `whsec_`
    /// prefix whose base64 payload is empty or malformed, is rejected with
    /// [`WebhookVerificationError::InvalidSecret`] here instead of failing
    /// at first verification. An empty decoded key must not construct a
    /// usable verifier: HMAC accepts an empty key, so anyone could forge
    /// signatures for a `whsec_` secret.
    pub fn new(secret: impl Into<Secret>) -> Result<Self, WebhookVerificationError> {
        let secret = secret.into();
        if secret.is_empty() {
            return Err(WebhookVerificationError::InvalidSecret);
        }
        secret.with_exposed(|value| decode_secret(value).map(drop))?;
        Ok(Self {
            secret,
            tolerance: DEFAULT_TOLERANCE,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
        })
    }

    /// Replaces the accepted clock-skew/replay window.
    ///
    /// The window bounds replay: a captured delivery only verifies while
    /// its timestamp stays inside the window, so a valid signature alone
    /// never proves freshness. The window is applied symmetrically to the
    /// past and the future — deliveries dated further ahead than the
    /// tolerance are rejected with
    /// [`WebhookVerificationError::TimestampTooNew`], because a far-future
    /// timestamp would otherwise still count as recent long after the
    /// capture. Timestamp validation therefore rejects events that are
    /// too far in the past or future, so keep the receiving server's
    /// clock synchronized, mirroring the official webhook guidance in
    /// openai-node's `docs/webhooks.md`.
    ///
    /// The window is compared in whole seconds against the delivery
    /// timestamp, so sub-second durations are rejected: `500ms` would
    /// otherwise truncate to a zero window and reject valid deliveries.
    pub fn with_tolerance(mut self, tolerance: Duration) -> Result<Self, WebhookVerificationError> {
        if tolerance < Duration::from_secs(1) {
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
    ///
    /// # Raw-body requirement
    ///
    /// `payload` must be the original request bytes exactly as they
    /// arrived on the wire. The signature covers those bytes, so parsing
    /// the body as JSON and re-serializing it — even without editing any
    /// field — can change what was signed (object key order, whitespace,
    /// number formatting) and make verification fail. Capture the raw body
    /// before any JSON middleware parses it, the same way openai-node's
    /// `docs/webhooks.md` instructs node users to register a raw-body
    /// middleware ahead of the JSON body parser.
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
    ///
    /// Like [`Self::verify`], this requires the original, un-re-serialized
    /// request bytes; see the raw-body requirement on [`Self::verify`].
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
            // Evaluate every candidate inside the 8 KiB header bound rather
            // than returning on the first match, mirroring node's bounded
            // verification path: the HMAC work stays independent of the
            // matching index and of how many candidates precede it.
            matched |= mac.verify_slice(&signature).is_ok();
        }
        if !matched {
            return Err(WebhookVerificationError::SignatureMismatch);
        }

        let event = serde_json::from_slice(payload)
            .map_err(|error| sanitized_decode_failure(payload, error))?;
        Ok(VerifiedWebhook::from_verified(webhook_id, event))
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
    /// The signature header exceeded the header-size bound, or none of
    /// its candidates decoded to a 32-byte HMAC-SHA-256 tag.
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
    /// The configured signing secret was empty, or carried a `whsec_`
    /// prefix with an empty or malformed base64 payload.
    #[error("invalid webhook secret")]
    InvalidSecret,
    /// A sub-second replay window was requested.
    #[error("webhook tolerance must be at least one second")]
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
    ///
    /// Only a coarse failure class and the serde-reported position are
    /// retained. The underlying `serde_json::Error` is deliberately
    /// dropped: its `Display` and `Debug` embed literals from the body it
    /// rejected (for example the value inside `invalid type: string
    /// "..."`), and that body has already passed signature verification,
    /// so keeping it in a `source` chain would leak verified payload
    /// content into logs.
    #[error(
        "verified webhook body is invalid JSON or wire data ({kind} error, line {line}, column {column})"
    )]
    Decode {
        /// Failure class: `syntax` (malformed or truncated JSON),
        /// `discriminator` (not a JSON object carrying a string `type`
        /// field), or `type` (a well-formed envelope that does not fit
        /// the typed event shape).
        kind: &'static str,
        /// One-based line reported by serde, or zero when the failing
        /// decode stage has no text position.
        line: usize,
        /// One-based column reported by serde, or zero when unknown.
        column: usize,
    },
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

/// Decodes every candidate in the joined `webhook-signature` header.
///
/// There is deliberately no per-slot count rejection: node's
/// `webhook-signature-amplification` test pins that a valid signature in
/// any slot — the 33rd, or after a flood of unusable ones — still
/// verifies, so the 8 KiB bound on the joined header is the only
/// amplifier limiter. Candidates that do not decode to the 32-byte tag
/// length are skipped, and a header carrying no usable candidate at all
/// is rejected.
fn decode_signatures(
    header: &str,
) -> Result<Vec<[u8; HMAC_SHA256_BYTES]>, WebhookVerificationError> {
    let mut signatures = Vec::new();
    for candidate in header.split_ascii_whitespace() {
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
        Some(encoded) => {
            let decoded = STANDARD
                .decode(encoded)
                .map_err(|_| WebhookVerificationError::InvalidSecret)?;
            if decoded.is_empty() {
                // HMAC accepts an empty key, so an empty decoded secret
                // would let anyone forge valid signatures.
                return Err(WebhookVerificationError::InvalidSecret);
            }
            Ok(decoded)
        }
        None => Ok(secret.as_bytes().to_vec()),
    }
}

/// Reduces a post-verification decode failure to a sanitized class and
/// position.
///
/// The class is re-derived from the payload envelope rather than from the
/// error message because the message embeds payload literals, and because
/// serde's `Data` category conflates envelope failures (no JSON object
/// with a string `type`) with typed-shape failures. The envelope contract
/// mirrors the types-side discriminator check: any JSON object with a
/// string `type` decodes into either a pinned variant or the retained
/// `Unknown` variant, so only the typed shape can fail beyond it.
fn sanitized_decode_failure(payload: &[u8], error: serde_json::Error) -> WebhookVerificationError {
    let kind = match serde_json::from_slice::<serde_json::Value>(payload) {
        Err(_) => "syntax",
        Ok(value) => {
            if value
                .as_object()
                .is_some_and(|object| object.get("type").is_some_and(serde_json::Value::is_string))
            {
                "type"
            } else {
                "discriminator"
            }
        }
    };
    WebhookVerificationError::Decode {
        kind,
        line: error.line(),
        column: error.column(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac};
    use http::{HeaderMap, HeaderValue};
    use sha2::Sha256;

    use super::{
        DEFAULT_MAX_PAYLOAD_BYTES, MAX_SIGNATURE_HEADER_BYTES, WebhookVerificationError,
        WebhookVerifier,
    };

    const NOW: u64 = 1_800_000_000;
    const ID: &str = "evt_delivery_123";
    const PAYLOAD: &[u8] = br#"{"type":"future.event","id":"evt_1","private":"do-not-log"}"#;

    fn signature(secret: &[u8], timestamp: u64, payload: &[u8]) -> String {
        let timestamp = timestamp.to_string();
        let signed = [ID.as_bytes(), b".", timestamp.as_bytes(), b".", payload].concat();
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("test HMAC key");
        mac.update(&signed);
        STANDARD.encode(mac.finalize().into_bytes())
    }

    fn headers(secret: &[u8], timestamp: u64, payload: &[u8]) -> HeaderMap {
        let signature = signature(secret, timestamp, payload);
        let timestamp = timestamp.to_string();
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
    fn verified_delivery_exposes_the_webhook_id_as_the_deduplication_key() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        let verified = verifier
            .verify_at(PAYLOAD, &headers(secret, NOW, PAYLOAD), NOW)
            .expect("valid delivery");
        // The `webhook-id` header the signature was computed over is the
        // recommended idempotency key for retried deliveries, so it must
        // survive onto the verification result and across mapping.
        assert_eq!(verified.webhook_id(), ID);
        let mapped = verified.map(|event| event.event_type().to_owned());
        assert_eq!(mapped.webhook_id(), ID);
        assert_eq!(mapped.into_body(), "future.event");
    }

    #[test]
    fn rejects_replay_future_tamper_and_unusable_signature_lists() {
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

        // A header whose every candidate fails to decode to a 32-byte tag
        // carries nothing to verify against and is rejected as a header
        // failure, before any HMAC work.
        let mut none_valid = headers(secret, NOW, PAYLOAD);
        none_valid.insert(
            "webhook-signature",
            HeaderValue::from_static("v1,AAAA v1,BBBB"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &none_valid, NOW),
            Err(WebhookVerificationError::InvalidSignatureHeader)
        ));
    }

    #[test]
    fn a_valid_signature_in_slot_thirty_three_still_verifies() {
        // Node's `webhook-signature-amplification` test pins that slot 33
        // is not special: bounded verification evaluates every candidate
        // the header cap admits, and a delivery signed with the 33rd
        // candidate alone must verify. 33 `v1,<tag>` candidates occupy
        // 1583 bytes, well inside the 8 KiB bound.
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        let valid = signature(secret, NOW, PAYLOAD);
        let mut capped = headers(secret, NOW, PAYLOAD);
        let candidates = std::iter::repeat_n(format!("v1,{valid}"), 33)
            .collect::<Vec<_>>()
            .join(" ");
        capped.insert(
            "webhook-signature",
            HeaderValue::from_str(&candidates).expect("bounded test header"),
        );
        let verified = verifier
            .verify_at(PAYLOAD, &capped, NOW)
            .expect("valid signature in slot 33 verifies");
        assert_eq!(verified.webhook_id(), ID);
    }

    #[test]
    fn a_valid_signature_after_1600_invalid_candidates_still_verifies() {
        // The other half of node's amplification baseline: a flood of
        // unusable candidates must not push the real signature out of the
        // window. 1600 single-character tokens plus the valid candidate
        // stay inside the 8 KiB header bound.
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        let valid = signature(secret, NOW, PAYLOAD);
        let mut flooded = headers(secret, NOW, PAYLOAD);
        let candidates = format!(
            "{} v1,{valid}",
            std::iter::repeat_n("A", 1600).collect::<Vec<_>>().join(" ")
        );
        flooded.insert(
            "webhook-signature",
            HeaderValue::from_str(&candidates).expect("bounded test header"),
        );
        assert!(
            candidates.len() <= MAX_SIGNATURE_HEADER_BYTES,
            "test header must stay inside the joined-header bound"
        );
        verifier
            .verify_at(PAYLOAD, &flooded, NOW)
            .expect("trailing valid signature after 1600 invalid candidates verifies");
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

    #[test]
    fn empty_or_malformed_prefixed_secrets_are_rejected_at_construction() {
        // `whsec_` decodes to an empty key that anyone could sign with,
        // and `!!!` is not base64: both must fail in `new`, not at first
        // verification.
        for secret in ["whsec_", "whsec_!!!"] {
            assert!(matches!(
                WebhookVerifier::new(secret),
                Err(WebhookVerificationError::InvalidSecret)
            ));
        }
        assert!(matches!(
            WebhookVerifier::new(""),
            Err(WebhookVerificationError::InvalidSecret)
        ));
        assert!(WebhookVerifier::new(format!("whsec_{}", STANDARD.encode(b"raw-secret"))).is_ok());
    }

    fn assert_sanitized(error: &WebhookVerificationError, marker: &str) {
        let debug = format!("{error:?}");
        assert!(
            !debug.contains(marker),
            "Debug leaked payload content: {debug}"
        );
        let display = format!("{error}");
        assert!(
            !display.contains(marker),
            "Display leaked payload content: {display}"
        );
        let mut source = std::error::Error::source(error);
        while let Some(current) = source {
            let text = format!("{current}");
            assert!(
                !text.contains(marker),
                "source Display leaked payload content: {text}"
            );
            source = current.source();
        }
    }

    #[test]
    fn decode_failures_report_sanitized_class_and_position_without_payload_content() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");

        // Typed-shape failure: the raw serde message would embed the
        // literal of the rejected `created_at` value ("invalid type:
        // string \"do-not-log\", expected i64").
        let shape = br#"{"type":"batch.completed","id":"evt_1","created_at":"do-not-log"}"#;
        let error = verifier
            .verify_at(shape, &headers(secret, NOW, shape), NOW)
            .expect_err("typed shape mismatch");
        assert!(matches!(
            &error,
            WebhookVerificationError::Decode { kind: "type", .. }
        ));
        assert_sanitized(&error, "do-not-log");

        // Envelope failure: a JSON object without a string `type`.
        let envelope = br#"{"id":"evt_1","private":"do-not-log"}"#;
        let error = verifier
            .verify_at(envelope, &headers(secret, NOW, envelope), NOW)
            .expect_err("missing discriminator");
        assert!(matches!(
            &error,
            WebhookVerificationError::Decode {
                kind: "discriminator",
                ..
            }
        ));
        assert_sanitized(&error, "do-not-log");

        // Syntax failure: truncated JSON, which still carries a real
        // serde position.
        let syntax = b"{\"type\":\"future.event\",\"private\":\"do-not-log\"";
        let error = verifier
            .verify_at(syntax, &headers(secret, NOW, syntax), NOW)
            .expect_err("truncated body");
        match &error {
            WebhookVerificationError::Decode {
                kind: "syntax",
                line,
                column,
            } => {
                assert!(*line >= 1, "syntax failures carry a line");
                assert!(*column >= 1, "syntax failures carry a column");
            }
            other => panic!("expected a syntax decode failure, got {other:?}"),
        }
        assert_sanitized(&error, "do-not-log");
    }

    #[test]
    fn sub_second_tolerances_are_rejected_instead_of_truncating_to_zero() {
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        assert!(matches!(
            verifier.clone().with_tolerance(Duration::from_millis(500)),
            Err(WebhookVerificationError::InvalidTolerance)
        ));
        assert!(matches!(
            verifier.clone().with_tolerance(Duration::ZERO),
            Err(WebhookVerificationError::InvalidTolerance)
        ));

        let one_second = verifier
            .with_tolerance(Duration::from_secs(1))
            .expect("one second is the smallest accepted window");
        one_second
            .verify_at(
                PAYLOAD,
                &headers(b"webhook-test-secret", NOW - 1, PAYLOAD),
                NOW,
            )
            .expect("delivery one second old fits the one-second window");
        assert!(matches!(
            one_second.verify_at(
                PAYLOAD,
                &headers(b"webhook-test-secret", NOW - 2, PAYLOAD),
                NOW
            ),
            Err(WebhookVerificationError::TimestampTooOld)
        ));

        // The window is symmetric: a delivery dated exactly `now +
        // tolerance` is the future-side boundary and must pass, while one
        // second further ahead is rejected.
        one_second
            .verify_at(
                PAYLOAD,
                &headers(b"webhook-test-secret", NOW + 1, PAYLOAD),
                NOW,
            )
            .expect("delivery one second ahead fits the one-second window");
        assert!(matches!(
            one_second.verify_at(
                PAYLOAD,
                &headers(b"webhook-test-secret", NOW + 2, PAYLOAD),
                NOW,
            ),
            Err(WebhookVerificationError::TimestampTooNew)
        ));
    }

    #[test]
    fn payload_limits_fail_closed_at_the_default_and_configured_bounds() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");

        // A zero limit would reject every delivery, including valid ones.
        assert!(matches!(
            verifier.clone().with_max_payload_bytes(0),
            Err(WebhookVerificationError::InvalidPayloadLimit)
        ));

        // The default bound is 16 MiB, enforced before any header parsing
        // or HMAC work; exactly at the bound the size gate passes (this
        // delivery has no headers, so it stops at the first header check).
        let oversized = vec![b'x'; DEFAULT_MAX_PAYLOAD_BYTES + 1];
        assert!(matches!(
            verifier.verify_at(&oversized, &HeaderMap::new(), NOW),
            Err(WebhookVerificationError::PayloadTooLarge { limit })
                if limit == DEFAULT_MAX_PAYLOAD_BYTES
        ));
        let at_limit = vec![b'x'; DEFAULT_MAX_PAYLOAD_BYTES];
        assert!(matches!(
            verifier.verify_at(&at_limit, &HeaderMap::new(), NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-timestamp"))
        ));

        // A configured tighter bound rejects one byte over it while still
        // accepting a valid small delivery.
        let limited = verifier
            .clone()
            .with_max_payload_bytes(1024)
            .expect("non-zero payload limit");
        let oversized_for_limit = vec![b'x'; 1025];
        assert!(matches!(
            limited.verify_at(
                &oversized_for_limit,
                &headers(secret, NOW, &oversized_for_limit),
                NOW,
            ),
            Err(WebhookVerificationError::PayloadTooLarge { limit }) if limit == 1024
        ));
        limited
            .verify_at(PAYLOAD, &headers(secret, NOW, PAYLOAD), NOW)
            .expect("delivery inside the configured limit still verifies");
    }

    #[test]
    fn delivery_headers_fail_closed_on_size_duplicates_and_missing_or_opaque_values() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");

        // A `webhook-id` longer than the 512-byte bound is rejected before
        // the signature is evaluated.
        let mut long_id = headers(secret, NOW, PAYLOAD);
        long_id.insert(
            "webhook-id",
            HeaderValue::from_str(&"e".repeat(513)).expect("long webhook-id header"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &long_id, NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-id"))
        ));

        // Repeated single-value headers are ambiguous (an intermediary may
        // have appended a second value), so they are rejected rather than
        // trusted to the first one.
        let mut duplicated = headers(secret, NOW, PAYLOAD);
        duplicated.append("webhook-id", HeaderValue::from_static(ID));
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &duplicated, NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-id"))
        ));

        // Non-UTF-8 header bytes cannot become part of the signed string.
        let mut opaque = headers(secret, NOW, PAYLOAD);
        opaque.insert(
            "webhook-id",
            HeaderValue::from_bytes(&[0xC3, 0x28]).expect("opaque header bytes"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &opaque, NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-id"))
        ));

        // Absent headers fail closed, starting with the first one checked.
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &HeaderMap::new(), NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-timestamp"))
        ));
        let mut no_signature = headers(secret, NOW, PAYLOAD);
        no_signature.remove("webhook-signature");
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &no_signature, NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-signature"))
        ));
    }

    #[test]
    fn timestamps_fail_closed_on_length_sign_and_overflow() {
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");

        // 33 digits exceed the 32-byte header bound.
        let mut too_long = headers(secret, NOW, PAYLOAD);
        too_long.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&"1".repeat(33)).expect("long timestamp header"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &too_long, NOW),
            Err(WebhookVerificationError::InvalidHeader("webhook-timestamp"))
        ));

        // `+123` is not a strict unsigned integer.
        let mut signed = headers(secret, NOW, PAYLOAD);
        signed.insert("webhook-timestamp", HeaderValue::from_static("+123"));
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &signed, NOW),
            Err(WebhookVerificationError::InvalidTimestamp)
        ));

        // 24 digits fit the header bound but overflow `u64` seconds.
        let mut overflow = headers(secret, NOW, PAYLOAD);
        overflow.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&"1".repeat(24)).expect("overflowing timestamp header"),
        );
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &overflow, NOW),
            Err(WebhookVerificationError::InvalidTimestamp)
        ));
    }

    #[test]
    fn multi_valued_signature_headers_are_space_joined_within_the_size_bound() {
        // Repeated `webhook-signature` headers are joined with spaces
        // before candidates are split, so an unusable candidate in the
        // first value must not stop the valid one in the second from
        // verifying (mirroring node's header joining).
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        let valid = signature(secret, NOW, PAYLOAD);
        let mut split = headers(secret, NOW, PAYLOAD);
        split.insert("webhook-signature", HeaderValue::from_static("v1,AAAA"));
        split.append(
            "webhook-signature",
            HeaderValue::from_str(&format!("v1,{valid}")).expect("valid candidate header"),
        );
        verifier
            .verify_at(PAYLOAD, &split, NOW)
            .expect("valid candidate in the second header value verifies");

        // The joined bound is 8 KiB: two values totalling more are rejected
        // before any candidate is decoded.
        let mut oversized = headers(secret, NOW, PAYLOAD);
        oversized.insert(
            "webhook-signature",
            HeaderValue::from_str(&"A".repeat(MAX_SIGNATURE_HEADER_BYTES - 1))
                .expect("large signature header"),
        );
        oversized.append("webhook-signature", HeaderValue::from_static("BB"));
        assert!(matches!(
            verifier.verify_at(PAYLOAD, &oversized, NOW),
            Err(WebhookVerificationError::InvalidSignatureHeader)
        ));
    }

    #[test]
    fn verify_accepts_a_fresh_delivery_against_the_wall_clock() {
        // Smoke test for the production entry point: `verify` reads the
        // wall clock itself, and a delivery signed for "now" stays inside
        // the default five-minute tolerance for the duration of the call.
        let secret = b"webhook-test-secret";
        let verifier = WebhookVerifier::new("webhook-test-secret").expect("verifier");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock after the Unix epoch")
            .as_secs();
        let verified = verifier
            .verify(PAYLOAD, &headers(secret, now, PAYLOAD))
            .expect("fresh delivery verifies against the wall clock");
        assert_eq!(verified.webhook_id(), ID);
        assert_eq!(verified.as_ref().event_type(), "future.event");
    }
}
