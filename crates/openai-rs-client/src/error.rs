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
        let truncated = truncated || preview_bytes.len() < bytes.len();
        let text = match serde_json::from_slice::<Value>(preview_bytes) {
            Ok(mut value) => {
                redact_json(&mut value, None);
                serde_json::to_string(&value)
                    .unwrap_or_else(|_| "<unavailable JSON body>".to_owned())
            }
            Err(_) => redact_inline(&String::from_utf8_lossy(bytes)),
        };
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
            .field("text", &self.text)
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
#[derive(Clone, Debug)]
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
        let message = typed
            .as_ref()
            .and_then(|error| error.message.as_deref())
            .map(redact_inline)
            .unwrap_or_else(|| format!("OpenAI API returned HTTP {}", meta.status()))
            .into_boxed_str();
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
        write!(
            formatter,
            "OpenAI API error ({}): {}",
            self.status(),
            self.message
        )
    }
}

impl std::error::Error for ApiError {}

/// A typed error delivered inside an otherwise successful Responses stream.
#[derive(Clone, Debug)]
pub struct StreamError {
    request_id: Option<Box<str>>,
    message: Box<str>,
    code: Option<Box<str>>,
    param: Option<Box<str>>,
    body: BodyPreview,
}

impl StreamError {
    pub(crate) fn from_body(request_id: Option<&str>, body: &[u8]) -> Self {
        let typed = serde_json::from_slice::<StreamErrorBody>(body).ok();
        let message = typed
            .as_ref()
            .and_then(|error| error.message.as_deref())
            .map(redact_inline)
            .unwrap_or_else(|| "OpenAI returned an in-band stream error".to_owned())
            .into_boxed_str();
        Self {
            request_id: request_id.map(Box::<str>::from),
            message,
            code: typed
                .as_ref()
                .and_then(|error| value_string(error.code.as_ref())),
            param: typed
                .as_ref()
                .and_then(|error| value_string(error.param.as_ref())),
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
        write!(formatter, "OpenAI Responses stream error: {}", self.message)
    }
}

impl std::error::Error for StreamError {}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<Value>,
    #[serde(default)]
    param: Option<Value>,
    #[serde(default)]
    code: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct StreamErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<Value>,
    #[serde(default)]
    param: Option<Value>,
}

fn value_string(value: Option<&Value>) -> Option<Box<str>> {
    match value? {
        Value::Null => None,
        Value::String(value) => Some(redact_inline(value).into_boxed_str()),
        value => Some(value.to_string().into_boxed_str()),
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

    #[error("HTTP request timed out: {0}")]
    Timeout(#[source] reqwest::Error),

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

    #[error("invalid client configuration: {0}")]
    InvalidConfiguration(Box<str>),

    #[error("invalid {name} path parameter: {reason}")]
    InvalidPathParameter {
        name: &'static str,
        reason: &'static str,
    },
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

    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Api(error) => Some(error.status()),
            Self::Decode { meta_status, .. } => Some(*meta_status),
            Self::BodyTooLarge { status, .. } => Some(*status),
            Self::UnexpectedContentType { status, .. } => Some(*status),
            Self::Transport(_)
            | Self::Timeout(_)
            | Self::Encode(_)
            | Self::EncodeQuery(_)
            | Self::Sse { .. }
            | Self::Stream(_)
            | Self::InvalidConfiguration(_)
            | Self::InvalidPathParameter { .. } => None,
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Api(error) => error.request_id(),
            Self::Decode { request_id, .. }
            | Self::BodyTooLarge { request_id, .. }
            | Self::UnexpectedContentType { request_id, .. } => request_id.as_deref(),
            Self::Sse { request_id, .. } => request_id.as_deref(),
            Self::Stream(error) => error.request_id(),
            Self::Transport(_)
            | Self::Timeout(_)
            | Self::Encode(_)
            | Self::EncodeQuery(_)
            | Self::InvalidConfiguration(_)
            | Self::InvalidPathParameter { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimitMetadata;

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
    }
}
