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
    #[cfg(feature = "realtime")]
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
}

impl ResponseMeta {
    pub(crate) fn from_headers(status: StatusCode, headers: &HeaderMap) -> Self {
        let header = |name: &'static str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(Box::<str>::from)
        };
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
