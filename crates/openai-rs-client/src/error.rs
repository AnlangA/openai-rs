use std::fmt;

use http::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::ResponseMeta;

const MAX_BODY_PREVIEW_BYTES: usize = 8 * 1024;

/// A bounded, sanitized representation of a response body.
#[derive(Clone, PartialEq, Eq)]
pub struct BodyPreview {
    text: Box<str>,
    truncated: bool,
}

impl BodyPreview {
    pub(crate) fn from_bytes(bytes: &[u8], truncated: bool) -> Self {
        let preview_bytes = &bytes[..bytes.len().min(MAX_BODY_PREVIEW_BYTES)];
        let mut truncated = truncated || preview_bytes.len() < bytes.len();
        let mut text = match serde_json::from_slice::<Value>(preview_bytes) {
            Ok(mut value) => {
                redact_json(&mut value, None);
                serde_json::to_string(&value)
                    .unwrap_or_else(|_| "<unavailable JSON body>".to_owned())
            }
            Err(_) => redact_inline(&String::from_utf8_lossy(preview_bytes)),
        };
        if text.len() > MAX_BODY_PREVIEW_BYTES {
            let mut boundary = MAX_BODY_PREVIEW_BYTES;
            while !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            text.truncate(boundary);
            truncated = true;
        }
        Self {
            text: text.into_boxed_str(),
            truncated,
        }
    }

    /// Returns the sanitized preview text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the wire body exceeded the configured preview limit.
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }
}

impl fmt::Debug for BodyPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BodyPreview")
            .field("text", &"[REDACTED]")
            .field("truncated", &self.truncated)
            .finish()
    }
}

fn redact_json(value: &mut Value, key: Option<&str>) {
    if key.is_some_and(is_sensitive_key) {
        *value = Value::String("[REDACTED]".to_owned());
        return;
    }

    match value {
        Value::Object(object) => {
            for (key, value) in object {
                redact_json(value, Some(key));
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json(value, key);
            }
        }
        Value::String(string) => *string = redact_inline(string),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "authorization",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "token",
        "secret",
        "password",
        "prompt",
        "input",
        "content",
        "output",
        "text",
        "delta",
        "arguments",
        "after",
        "before",
        "cursor",
        "url",
        "uri",
        "metadata",
        "user",
        "email",
    ]
    .iter()
    .any(|sensitive| normalized == *sensitive || normalized.ends_with(sensitive))
}

fn redact_inline(input: &str) -> String {
    const PREFIXES: [&str; 6] = ["Bearer ", "bearer ", "sk-", "sess-", "api_key=", "token="];
    let mut output = input.to_owned();
    for prefix in PREFIXES {
        let mut search_from = 0;
        while let Some(relative_start) = output[search_from..].find(prefix) {
            let start = search_from + relative_start;
            let secret_start = start + prefix.len();
            let secret_len = output[secret_start..]
                .char_indices()
                .find_map(|(offset, character)| {
                    (character.is_whitespace()
                        || matches!(character, '"' | '\'' | ',' | ';' | '<' | '>' | '&'))
                    .then_some(offset)
                })
                .unwrap_or(output.len() - secret_start);
            let secret_end = secret_start + secret_len;
            output.replace_range(start..secret_end, "[REDACTED]");
            search_from = start + "[REDACTED]".len();
        }
    }
    output
}

/// A typed error returned by the OpenAI API.
#[derive(Clone)]
pub struct ApiError {
    meta: ResponseMeta,
    message: Box<str>,
    kind: Option<Box<str>>,
    param: Option<Box<str>>,
    code: Option<Box<str>>,
    body: BodyPreview,
}

impl ApiError {
    pub(crate) fn from_body(meta: ResponseMeta, body: &[u8], truncated: bool) -> Self {
        let envelope = serde_json::from_slice::<ApiErrorEnvelope>(body).ok();
        let typed = envelope.map(|envelope| envelope.error);
        let body = BodyPreview::from_bytes(body, truncated);
        // Every envelope field is extracted independently: a field whose wire
        // type does not match is dropped on its own instead of invalidating
        // the whole envelope, matching the per-field `get` semantics of the
        // official clients.
        let message = typed
            .as_ref()
            .and_then(|error| value_string(error.message.as_ref()))
            .unwrap_or_else(|| {
                format!("OpenAI API returned HTTP {}", meta.status()).into_boxed_str()
            });
        Self {
            meta,
            message,
            kind: typed
                .as_ref()
                .and_then(|error| value_string(error.kind.as_ref())),
            param: typed
                .as_ref()
                .and_then(|error| value_string(error.param.as_ref())),
            code: typed
                .as_ref()
                .and_then(|error| value_string(error.code.as_ref())),
            body,
        }
    }

    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.meta.status()
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The raw server retry hint carried by the failing response.
    ///
    /// The value is the verbatim header text, preferring `retry-after-ms`
    /// when both hints are present. It is intentionally not interpreted
    /// here; delay gating for automatic retries stays owned by the
    /// transport.
    #[must_use]
    pub fn retry_after(&self) -> Option<&str> {
        self.meta.retry_after()
    }

    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    #[must_use]
    pub fn param(&self) -> Option<&str> {
        self.param.as_deref()
    }

    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    #[must_use]
    pub fn body_preview(&self) -> &BodyPreview {
        &self.body
    }

    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    #[must_use]
    pub const fn rate_limits(&self) -> &crate::RateLimitMetadata {
        self.meta.rate_limits()
    }

    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        self.status() == StatusCode::TOO_MANY_REQUESTS
    }

    #[must_use]
    pub fn is_server_error(&self) -> bool {
        self.status().is_server_error()
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "OpenAI API error ({})", self.status())?;
        if let Some(code) = self.code() {
            write!(formatter, ", code {code}")?;
        }
        if let Some(request_id) = self.request_id() {
            write!(formatter, ", request {request_id}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiError")
            .field("status", &self.status())
            .field("request_id", &self.request_id())
            .field("kind", &self.kind())
            .field("code", &self.code())
            .field("message", &"[REDACTED]")
            .field("body", &"[REDACTED]")
            .finish()
    }
}

impl std::error::Error for ApiError {}

/// A typed error delivered in-band inside an otherwise successful stream.
///
/// The same error shape is surfaced by every SSE channel in this crate —
/// chat completions, legacy completions, media (speech, transcription,
/// images), and Responses — so its fields are deliberately not
/// Responses-specific.
#[derive(Clone)]
pub struct StreamError {
    request_id: Option<Box<str>>,
    message: Box<str>,
    kind: Option<Box<str>>,
    code: Option<Box<str>>,
    param: Option<Box<str>>,
    body: BodyPreview,
}

impl StreamError {
    pub(crate) fn from_body(request_id: Option<&str>, body: &[u8]) -> Self {
        let typed = serde_json::from_slice::<StreamErrorBody>(body).ok();
        let nested = serde_json::from_slice::<ApiErrorEnvelope>(body).ok();
        // Flat payloads win per field; the nested `{"error":{..}}` envelope is
        // only consulted for fields the flat form did not carry.
        let message = typed
            .as_ref()
            .and_then(|error| value_string(error.message.as_ref()))
            .or_else(|| {
                nested
                    .as_ref()
                    .and_then(|envelope| value_string(envelope.error.message.as_ref()))
            })
            .unwrap_or_else(|| "OpenAI returned an in-band stream error".into());
        Self {
            request_id: request_id.map(Box::<str>::from),
            message,
            kind: typed
                .as_ref()
                .and_then(|error| value_string(error.kind.as_ref()))
                .or_else(|| {
                    nested
                        .as_ref()
                        .and_then(|envelope| value_string(envelope.error.kind.as_ref()))
                }),
            code: typed
                .as_ref()
                .and_then(|error| value_string(error.code.as_ref()))
                .or_else(|| {
                    nested
                        .as_ref()
                        .and_then(|envelope| value_string(envelope.error.code.as_ref()))
                }),
            param: typed
                .as_ref()
                .and_then(|error| value_string(error.param.as_ref()))
                .or_else(|| {
                    nested
                        .as_ref()
                        .and_then(|envelope| value_string(envelope.error.param.as_ref()))
                }),
            body: BodyPreview::from_bytes(body, false),
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The officially required `type` discriminator of the stream error.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    #[must_use]
    pub fn param(&self) -> Option<&str> {
        self.param.as_deref()
    }

    #[must_use]
    pub const fn body_preview(&self) -> &BodyPreview {
        &self.body
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenAI stream error")?;
        if let Some(code) = self.code() {
            write!(formatter, ", code {code}")?;
        }
        if let Some(request_id) = self.request_id() {
            write!(formatter, ", request {request_id}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for StreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamError")
            .field("request_id", &self.request_id())
            .field("kind", &self.kind())
            .field("code", &self.code())
            .field("message", &"[REDACTED]")
            .field("body", &"[REDACTED]")
            .finish()
    }
}

impl std::error::Error for StreamError {}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

/// Deserialized leniently: every field is an optional raw [`Value`] so a
/// single mistyped field cannot discard its siblings.
#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    message: Option<Value>,
    #[serde(default, rename = "type")]
    kind: Option<Value>,
    #[serde(default)]
    param: Option<Value>,
    #[serde(default)]
    code: Option<Value>,
}

/// Deserialized leniently, mirroring [`ApiErrorBody`].
#[derive(Debug, Deserialize)]
struct StreamErrorBody {
    #[serde(default)]
    message: Option<Value>,
    #[serde(default, rename = "type")]
    kind: Option<Value>,
    #[serde(default)]
    code: Option<Value>,
    #[serde(default)]
    param: Option<Value>,
}

/// Extracts one envelope field leniently: `null` means absent, strings keep
/// their text, and any other JSON value is stringified — in both cases after
/// inline redaction, since loose payloads can still carry token-looking text.
fn value_string(value: Option<&Value>) -> Option<Box<str>> {
    match value? {
        Value::Null => None,
        Value::String(value) => Some(redact_inline(value).into_boxed_str()),
        value => Some(redact_inline(&value.to_string()).into_boxed_str()),
    }
}

/// Errors produced by the Platform client.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Api(Box<ApiError>),

    #[error("HTTP transport failed: {0}")]
    Transport(#[source] reqwest::Error),

    /// The request timed out before any HTTP response arrived. A timeout on
    /// an already-received response (mid-body or mid-stream) surfaces as
    /// [`Error::ResponseBody`] instead; `source().is_timeout()` distinguishes
    /// the cause there.
    #[error("HTTP request timed out: {0}")]
    Timeout(#[source] reqwest::Error),

    #[error("the overall request deadline elapsed before a response was delivered")]
    DeadlineExceeded,

    /// Reading an already-received HTTP response body (or SSE stream) failed.
    /// Unlike [`Error::Timeout`], the status and request id of the response
    /// are preserved; a transport-level timeout underneath is detected via
    /// `source().is_timeout()`.
    #[error("failed while reading HTTP response body (status {status}): {source}")]
    ResponseBody {
        #[source]
        source: reqwest::Error,
        status: StatusCode,
        request_id: Option<Box<str>>,
    },

    #[error("failed to encode request JSON: {0}")]
    Encode(#[source] serde_json::Error),

    #[error("failed to encode request query: {0}")]
    EncodeQuery(Box<str>),

    #[error("failed to decode an SSE stream: {source}")]
    Sse {
        #[source]
        source: crate::sse::SseDecodeError,
        request_id: Option<Box<str>>,
    },

    #[error(transparent)]
    Stream(Box<StreamError>),

    #[error("invalid Responses stream protocol: {message}")]
    StreamProtocol {
        message: &'static str,
        request_id: Option<Box<str>>,
        body: BodyPreview,
    },

    #[error("unexpected response content type; expected {expected}, received {actual:?}")]
    UnexpectedContentType {
        expected: &'static str,
        actual: Option<Box<str>>,
        status: StatusCode,
        request_id: Option<Box<str>>,
    },

    #[error("failed to decode response JSON (status {meta_status}): {source}")]
    Decode {
        #[source]
        source: serde_json::Error,
        path: Option<Box<str>>,
        meta_status: StatusCode,
        request_id: Option<Box<str>>,
        body: BodyPreview,
    },

    #[error("response body exceeds the configured {limit}-byte limit")]
    BodyTooLarge {
        limit: usize,
        status: StatusCode,
        request_id: Option<Box<str>>,
    },

    #[error("response text is not valid UTF-8 (status {status})")]
    InvalidUtf8 {
        status: StatusCode,
        request_id: Option<Box<str>>,
        body: BodyPreview,
    },

    #[error("invalid client configuration: {0}")]
    InvalidConfiguration(Box<str>),

    #[error("request payload exceeds the {limit_bytes}-byte limit before transport")]
    RequestPayloadTooLarge { limit_bytes: usize },

    #[error("invalid {name} path parameter: {reason}")]
    InvalidPathParameter {
        name: &'static str,
        reason: &'static str,
    },

    #[error(transparent)]
    Accumulator(Box<openai_rs_types::responses::ResponseAccumulatorError>),

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    #[error("WebSocket handshake failed with HTTP {status}")]
    WebSocketHandshake {
        status: StatusCode,
        request_id: Option<Box<str>>,
        /// Sanitized, bounded preview of the handshake rejection body that
        /// tungstenite buffered alongside the response head (4-17). Kept out
        /// of `Display` per the L4-5 posture: rendered errors stay limited to
        /// status/request id, the body is only reachable through
        /// [`Error::handshake_body`].
        body: BodyPreview,
    },

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    #[error("WebSocket transport failed: {0}")]
    WebSocketTransport(Box<str>),

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    #[error("invalid WebSocket protocol state: {0}")]
    WebSocketProtocol(&'static str),

    #[cfg(feature = "workload-identity")]
    #[error(transparent)]
    WorkloadIdentity(std::sync::Arc<crate::WorkloadIdentityError>),
}

impl From<ApiError> for Error {
    fn from(error: ApiError) -> Self {
        Self::Api(Box::new(error))
    }
}

impl From<StreamError> for Error {
    fn from(error: StreamError) -> Self {
        Self::Stream(Box::new(error))
    }
}

impl From<openai_rs_types::responses::ResponseAccumulatorError> for Error {
    fn from(error: openai_rs_types::responses::ResponseAccumulatorError) -> Self {
        Self::Accumulator(Box::new(error))
    }
}

#[cfg(feature = "workload-identity")]
impl From<std::sync::Arc<crate::WorkloadIdentityError>> for Error {
    fn from(error: std::sync::Arc<crate::WorkloadIdentityError>) -> Self {
        Self::WorkloadIdentity(error)
    }
}

impl Error {
    pub(crate) fn from_reqwest(error: reqwest::Error) -> Self {
        let is_timeout = error.is_timeout();
        // reqwest errors can retain the complete request URL, including opaque
        // cursors or signed query values. They are never needed for this
        // public error because the typed operation already identifies the call.
        let error = error.without_url();
        if is_timeout {
            Self::Timeout(error)
        } else {
            Self::Transport(error)
        }
    }

    pub(crate) fn from_response_body(error: reqwest::Error, meta: &ResponseMeta) -> Self {
        Self::ResponseBody {
            source: error.without_url(),
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        }
    }

    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Api(error) => Some(error.status()),
            Self::Decode { meta_status, .. } => Some(*meta_status),
            Self::BodyTooLarge { status, .. } => Some(*status),
            Self::InvalidUtf8 { status, .. } => Some(*status),
            Self::ResponseBody { status, .. } => Some(*status),
            Self::UnexpectedContentType { status, .. } => Some(*status),
            #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
            Self::WebSocketHandshake { status, .. } => Some(*status),
            #[cfg(feature = "workload-identity")]
            Self::WorkloadIdentity(error) => error.status(),
            Self::Transport(_)
            | Self::Timeout(_)
            | Self::DeadlineExceeded
            | Self::Encode(_)
            | Self::EncodeQuery(_)
            | Self::Sse { .. }
            | Self::Stream(_)
            | Self::StreamProtocol { .. }
            | Self::Accumulator(_)
            | Self::InvalidConfiguration(_)
            | Self::InvalidPathParameter { .. }
            | Self::RequestPayloadTooLarge { .. } => None,
            #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
            Self::WebSocketTransport(_) | Self::WebSocketProtocol(_) => None,
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Api(error) => error.request_id(),
            Self::Decode { request_id, .. }
            | Self::BodyTooLarge { request_id, .. }
            | Self::InvalidUtf8 { request_id, .. }
            | Self::ResponseBody { request_id, .. }
            | Self::UnexpectedContentType { request_id, .. } => request_id.as_deref(),
            Self::Sse { request_id, .. } => request_id.as_deref(),
            Self::Stream(error) => error.request_id(),
            Self::StreamProtocol { request_id, .. } => request_id.as_deref(),
            #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
            Self::WebSocketHandshake { request_id, .. } => request_id.as_deref(),
            Self::Transport(_)
            | Self::Timeout(_)
            | Self::DeadlineExceeded
            | Self::Encode(_)
            | Self::EncodeQuery(_)
            | Self::Accumulator(_)
            | Self::InvalidConfiguration(_)
            | Self::InvalidPathParameter { .. }
            | Self::RequestPayloadTooLarge { .. } => None,
            #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
            Self::WebSocketTransport(_) | Self::WebSocketProtocol(_) => None,
            #[cfg(feature = "workload-identity")]
            Self::WorkloadIdentity(_) => None,
        }
    }

    /// JSON Pointer-like Serde path for typed decode failures.
    #[must_use]
    pub fn decode_path(&self) -> Option<&str> {
        match self {
            Self::Decode { path, .. } => path.as_deref(),
            _ => None,
        }
    }

    /// Bounded, sanitized preview of the HTTP body a failed WebSocket
    /// handshake received (401/403/429 rejections carry a JSON error body
    /// that was previously discarded). `None` for every other variant; the
    /// preview text is redacted and its `truncated` flag reflects a wire body
    /// longer than what tungstenite buffered beside the response head.
    #[must_use]
    pub fn handshake_body(&self) -> Option<&BodyPreview> {
        match self {
            #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
            Self::WebSocketHandshake { body, .. } => Some(body),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimitMetadata;
    use http::{HeaderMap, HeaderValue};
    use static_assertions::assert_impl_all;

    assert_impl_all!(ApiError: Send, Sync);
    assert_impl_all!(BodyPreview: Send, Sync);
    assert_impl_all!(Error: Send, Sync);
    assert_impl_all!(StreamError: Send, Sync);

    #[test]
    fn body_preview_redacts_json_and_inline_tokens() {
        let preview = BodyPreview::from_bytes(
            br#"{"token":"sk-sensitive","nested":{"input":"private"},"message":"Bearer abc123"}"#,
            false,
        );
        assert!(!preview.as_str().contains("sensitive"));
        assert!(!preview.as_str().contains("private"));
        assert!(!preview.as_str().contains("abc123"));
    }

    #[test]
    fn non_json_preview_is_bounded_before_utf8_conversion() {
        let input = vec![b'x'; MAX_BODY_PREVIEW_BYTES * 4];
        let preview = BodyPreview::from_bytes(&input, false);
        assert!(preview.is_truncated());
        assert_eq!(preview.as_str().len(), MAX_BODY_PREVIEW_BYTES);

        let invalid_utf8 = vec![0xff; MAX_BODY_PREVIEW_BYTES];
        let preview = BodyPreview::from_bytes(&invalid_utf8, false);
        assert!(preview.as_str().len() <= MAX_BODY_PREVIEW_BYTES);
        assert!(preview.is_truncated());
    }

    #[test]
    fn typed_api_error_preserves_metadata() {
        let meta = ResponseMeta::new(
            StatusCode::UNAUTHORIZED,
            Some("req_test".into()),
            RateLimitMetadata::default(),
        );
        let error = ApiError::from_body(
            meta,
            br#"{"error":{"message":"invalid key","type":"authentication_error","param":null,"code":"invalid_api_key"}}"#,
            false,
        );
        assert_eq!(error.request_id(), Some("req_test"));
        assert_eq!(error.code(), Some("invalid_api_key"));
        assert!(!format!("{error:?}").contains("invalid key"));
        assert!(!error.to_string().contains("invalid key"));
    }

    #[test]
    fn stream_error_display_is_channel_neutral() {
        let error = StreamError::from_body(
            Some("req_1"),
            br#"{"type":"error","code":"server_error","message":"boom"}"#,
        );
        assert_eq!(
            error.to_string(),
            "OpenAI stream error, code server_error, request req_1"
        );
        assert!(!error.to_string().contains("Responses"));
    }

    #[test]
    fn stream_error_exposes_type_from_flat_and_nested_bodies() {
        let flat = StreamError::from_body(None, br#"{"type":"error","code":"c","message":"m"}"#);
        assert_eq!(flat.kind(), Some("error"));

        let nested = StreamError::from_body(
            None,
            br#"{"error":{"message":"m","type":"server_error","code":"c"}}"#,
        );
        assert_eq!(nested.kind(), Some("server_error"));

        let both = StreamError::from_body(
            None,
            br#"{"type":"flat_type","error":{"type":"nested_type"}}"#,
        );
        assert_eq!(both.kind(), Some("flat_type"));

        let empty = StreamError::from_body(None, br#"{}"#);
        assert_eq!(empty.kind(), None);
    }

    #[test]
    fn stream_error_flat_fields_win_over_nested_envelope() {
        let error = StreamError::from_body(
            Some("req_flat"),
            br#"{"message":"flat message","code":"flat_code","param":"flat_param","error":{"message":"nested message","code":"nested_code","param":"nested_param"}}"#,
        );
        assert_eq!(error.message(), "flat message");
        assert_eq!(error.code(), Some("flat_code"));
        assert_eq!(error.param(), Some("flat_param"));
    }

    #[test]
    fn stream_error_nested_envelope_used_when_flat_fields_absent() {
        let error = StreamError::from_body(
            None,
            br#"{"error":{"message":"nested message","type":"server_error","code":"c","param":"p"}}"#,
        );
        assert_eq!(error.message(), "nested message");
        assert_eq!(error.kind(), Some("server_error"));
        assert_eq!(error.code(), Some("c"));
        assert_eq!(error.param(), Some("p"));
    }

    #[test]
    fn stream_error_empty_envelope_keeps_fallback_surface() {
        let error = StreamError::from_body(None, br#"{"error":{}}"#);
        assert_eq!(error.message(), "OpenAI returned an in-band stream error");
        assert_eq!(error.kind(), None);
        assert_eq!(error.code(), None);
        assert_eq!(error.param(), None);
        assert_eq!(error.to_string(), "OpenAI stream error");
    }

    #[test]
    fn api_error_string_error_key_falls_back_without_panicking() {
        let meta = ResponseMeta::new(StatusCode::NOT_FOUND, None, RateLimitMetadata::default());
        let error = ApiError::from_body(meta, br#"{"error":"gone"}"#, false);
        assert_eq!(error.message(), "OpenAI API returned HTTP 404 Not Found");
        assert_eq!(error.code(), None);
        assert_eq!(error.kind(), None);
        assert_eq!(error.param(), None);
        assert!(error.body_preview().as_str().contains("gone"));
    }

    #[test]
    fn api_error_empty_error_object_keeps_fallback_surface() {
        let meta = ResponseMeta::new(StatusCode::BAD_GATEWAY, None, RateLimitMetadata::default());
        let error = ApiError::from_body(meta, br#"{"error":{}}"#, false);
        assert_eq!(error.message(), "OpenAI API returned HTTP 502 Bad Gateway");
        assert_eq!(error.to_string(), "OpenAI API error (502 Bad Gateway)");
    }

    #[test]
    fn api_error_numeric_fields_are_stringified_per_field() {
        let meta = ResponseMeta::new(StatusCode::BAD_REQUEST, None, RateLimitMetadata::default());
        let error = ApiError::from_body(
            meta,
            br#"{"error":{"code":429,"type":400,"param":null}}"#,
            false,
        );
        assert_eq!(error.code(), Some("429"));
        assert_eq!(error.kind(), Some("400"));
        assert_eq!(error.param(), None);
        assert_eq!(error.message(), "OpenAI API returned HTTP 400 Bad Request");
    }

    #[test]
    fn api_error_malformed_field_does_not_discard_sibling_fields() {
        let meta = ResponseMeta::new(StatusCode::BAD_REQUEST, None, RateLimitMetadata::default());
        let error = ApiError::from_body(
            meta,
            br#"{"error":{"message":"bad input","type":{"unexpected":"shape"},"code":"invalid_request"}}"#,
            false,
        );
        assert_eq!(error.message(), "bad input");
        assert_eq!(error.kind(), Some(r#"{"unexpected":"shape"}"#));
        assert_eq!(error.code(), Some("invalid_request"));
    }

    #[test]
    fn api_error_non_string_message_is_stringified() {
        let meta = ResponseMeta::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            RateLimitMetadata::default(),
        );
        let error = ApiError::from_body(meta, br#"{"error":{"message":123}}"#, false);
        assert_eq!(error.message(), "123");
    }

    #[test]
    fn api_error_malformed_body_falls_back_without_secondary_failure() {
        let meta = ResponseMeta::new(
            StatusCode::BAD_GATEWAY,
            Some("req_bad".into()),
            RateLimitMetadata::default(),
        );
        for body in [&b"\xff\xfe not json"[..], b"{", b"", b"plain text"] {
            let error = ApiError::from_body(meta.clone(), body, false);
            assert_eq!(error.message(), "OpenAI API returned HTTP 502 Bad Gateway");
            assert_eq!(error.code(), None);
            assert_eq!(error.kind(), None);
            assert_eq!(error.request_id(), Some("req_bad"));
        }
    }

    #[test]
    fn api_error_exposes_retry_hints_from_response_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("250"));
        headers.insert("retry-after", HeaderValue::from_static("1"));
        headers.insert("x-should-retry", HeaderValue::from_static("true"));
        headers.insert("x-request-id", HeaderValue::from_static("req_limited"));
        let meta = ResponseMeta::from_headers(StatusCode::TOO_MANY_REQUESTS, &headers);
        let error = ApiError::from_body(
            meta,
            br#"{"error":{"message":"rate limited","type":"rate_limit_error","code":"429"}}"#,
            false,
        );
        assert!(error.is_rate_limited());
        assert_eq!(error.retry_after(), Some("250"));
        assert_eq!(error.meta().should_retry(), Some(true));
    }
}
