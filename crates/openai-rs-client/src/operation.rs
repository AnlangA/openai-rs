use std::ops::Deref;

use http::{HeaderMap, Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};

/// Authentication scopes understood by the Platform transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthScope {
    Platform,
}

/// How a request body is encoded on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestEncoding {
    None,
    Json,
}

/// How a successful response body is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseMode {
    Json,
    EmptyOrJson,
    #[cfg(feature = "realtime")]
    Empty,
    Sse,
}

/// Whether a fully buffered request can be retried before response delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetryClass {
    Safe,
    Replayable,
    #[cfg(any(feature = "realtime", feature = "legacy-realtime"))]
    Never,
}

/// Static wire contract for one generated or handwritten operation.
#[derive(Clone, Debug)]
pub(crate) struct OperationMeta {
    pub id: &'static str,
    pub method: Method,
    pub route: &'static str,
    pub auth: AuthScope,
    pub request_encoding: RequestEncoding,
    pub response_mode: ResponseMode,
    pub retry: RetryClass,
    pub success_statuses: &'static [StatusCode],
}

/// A sealed operation contract used by the transport.
pub(crate) trait Operation: private::Sealed + Send + Sync + 'static {
    type Request: Serialize + Sync;
    type Response: DeserializeOwned + Send + 'static;
    const META: OperationMeta;
}

pub(crate) mod private {
    pub trait Sealed {}
}

/// Metadata returned alongside a decoded response.
#[derive(Clone, Debug)]
pub struct ResponseMeta {
    status: StatusCode,
    request_id: Option<Box<str>>,
    rate_limits: RateLimitMetadata,
    retry_after: Option<Box<str>>,
    x_should_retry: Option<bool>,
}

impl ResponseMeta {
    pub(crate) fn from_headers(status: StatusCode, headers: &HeaderMap) -> Self {
        let header = |name: &'static str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(Box::<str>::from)
        };
        // The retry hint is preserved verbatim (`retry-after-ms` wins over a
        // stale `Retry-After`); interpreting it into a delay stays owned by
        // the transport's retry loop, which has its own bounds and fallbacks.
        let retry_after = header("retry-after-ms").or_else(|| header("retry-after"));
        // Only the literal `true`/`false` spellings are kept, so any other
        // value leaves callers on status-code based classification.
        let x_should_retry = headers
            .get("x-should-retry")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| match value {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            });
        Self {
            status,
            request_id: header("x-request-id"),
            rate_limits: RateLimitMetadata {
                limit_requests: header("x-ratelimit-limit-requests"),
                limit_tokens: header("x-ratelimit-limit-tokens"),
                remaining_requests: header("x-ratelimit-remaining-requests"),
                remaining_tokens: header("x-ratelimit-remaining-tokens"),
                reset_requests: header("x-ratelimit-reset-requests"),
                reset_tokens: header("x-ratelimit-reset-tokens"),
            },
            retry_after,
            x_should_retry,
        }
    }

    #[cfg(test)]
    pub(crate) const fn new(
        status: StatusCode,
        request_id: Option<Box<str>>,
        rate_limits: RateLimitMetadata,
    ) -> Self {
        Self {
            status,
            request_id,
            rate_limits,
            retry_after: None,
            x_should_retry: None,
        }
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// The verbatim server retry hint, preferring `retry-after-ms`.
    ///
    /// Returns `None` when the response carried neither `retry-after-ms` nor
    /// `Retry-After`. The text is uninterpreted: callers that need a duration
    /// own the parsing, exactly like the raw headers exposed by the official
    /// clients.
    #[must_use]
    pub fn retry_after(&self) -> Option<&str> {
        self.retry_after.as_deref()
    }

    /// The literal `x-should-retry` header, when it was exactly `true` or
    /// `false`.
    #[must_use]
    pub const fn should_retry(&self) -> Option<bool> {
        self.x_should_retry
    }

    #[must_use]
    pub const fn rate_limits(&self) -> &RateLimitMetadata {
        &self.rate_limits
    }
}

/// OpenAI rate-limit headers preserved as opaque protocol strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RateLimitMetadata {
    pub limit_requests: Option<Box<str>>,
    pub limit_tokens: Option<Box<str>>,
    pub remaining_requests: Option<Box<str>>,
    pub remaining_tokens: Option<Box<str>>,
    pub reset_requests: Option<Box<str>>,
    pub reset_tokens: Option<Box<str>>,
}

/// A decoded body together with HTTP response metadata.
#[derive(Clone, Debug)]
pub struct ApiResponse<T> {
    body: T,
    meta: ResponseMeta,
}

impl<T> ApiResponse<T> {
    pub(crate) const fn new(body: T, meta: ResponseMeta) -> Self {
        Self { body, meta }
    }

    #[must_use]
    pub const fn body(&self) -> &T {
        &self.body
    }

    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    #[must_use]
    pub fn into_inner(self) -> T {
        self.body
    }

    #[must_use]
    pub fn into_parts(self) -> (T, ResponseMeta) {
        (self.body, self.meta)
    }
}

impl<T> Deref for ApiResponse<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue};

    fn rate_limited_meta(headers: &[(&'static str, &'static str)]) -> ResponseMeta {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(*name, HeaderValue::from_static(value));
        }
        ResponseMeta::from_headers(StatusCode::TOO_MANY_REQUESTS, &map)
    }

    #[test]
    fn retry_after_prefers_retry_after_ms_over_retry_after() {
        let meta = rate_limited_meta(&[("retry-after-ms", "250"), ("retry-after", "60")]);
        assert_eq!(meta.retry_after(), Some("250"));
    }

    #[test]
    fn retry_after_falls_back_to_retry_after_header() {
        let meta = rate_limited_meta(&[("retry-after", "60")]);
        assert_eq!(meta.retry_after(), Some("60"));
    }

    #[test]
    fn retry_after_absent_when_neither_header_is_present() {
        let meta = rate_limited_meta(&[]);
        assert_eq!(meta.retry_after(), None);
    }

    #[test]
    fn retry_after_preserves_non_numeric_values_verbatim() {
        let meta = rate_limited_meta(&[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")]);
        assert_eq!(meta.retry_after(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
    }

    #[test]
    fn should_retry_keeps_only_literal_booleans() {
        assert_eq!(
            rate_limited_meta(&[("x-should-retry", "true")]).should_retry(),
            Some(true)
        );
        assert_eq!(
            rate_limited_meta(&[("x-should-retry", "false")]).should_retry(),
            Some(false)
        );
        assert_eq!(
            rate_limited_meta(&[("x-should-retry", "maybe")]).should_retry(),
            None
        );
        assert_eq!(rate_limited_meta(&[]).should_retry(), None);
    }
}
