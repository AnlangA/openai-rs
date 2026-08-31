use std::{
    fmt,
    time::{Duration, Instant, SystemTime},
};

use futures_util::StreamExt;
use http::{HeaderValue, header};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{
    ApiError, ApiResponse, BodyPreview, Error, ResponseMeta, RetryPolicy, TlsBackend,
    auth::AuthProvider,
    operation::{AuthScope, Operation, RequestEncoding, RetryClass},
    sse::SseLimits,
    trace::{self, RetryReason},
};

const JSON_MIME: &str = "application/json";
const SSE_MIME: &str = "text/event-stream";
const DECODE_PREVIEW_BYTES: usize = 8 * 1024;

/// One safely encoded component in an operation route.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PathSegment<'a> {
    Literal(&'static str),
    Parameter { name: &'static str, value: &'a str },
}

impl<'a> PathSegment<'a> {
    pub(crate) const fn literal(value: &'static str) -> Self {
        Self::Literal(value)
    }

    pub(crate) fn parameter(name: &'static str, value: &'a str) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not be empty",
            });
        }
        if value == "." || value == ".." {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not be a dot segment",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not contain control characters",
            });
        }
        Ok(Self::Parameter { name, value })
    }
}

/// The shared authenticated JSON transport.
pub(crate) struct Transport {
    http: reqwest::Client,
    base_url: Url,
    auth: AuthProvider,
    organization: Option<HeaderValue>,
    project: Option<HeaderValue>,
    client_request_id: Option<HeaderValue>,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    retry_policy: RetryPolicy,
    overall_timeout: Duration,
    sse_limits: SseLimits,
    tls_backend: Option<TlsBackend>,
}

impl Transport {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        http: reqwest::Client,
        base_url: Url,
        auth: AuthProvider,
        organization: Option<HeaderValue>,
        project: Option<HeaderValue>,
        client_request_id: Option<HeaderValue>,
        max_json_body_bytes: usize,
        max_error_body_bytes: usize,
        retry_policy: RetryPolicy,
        overall_timeout: Duration,
        sse_limits: SseLimits,
        tls_backend: Option<TlsBackend>,
    ) -> Self {
        Self {
            http,
            base_url,
            auth,
            organization,
            project,
            client_request_id,
            max_json_body_bytes,
            max_error_body_bytes,
            retry_policy,
            overall_timeout,
            sse_limits,
            tls_backend,
        }
    }

    pub(crate) const fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub(crate) const fn sse_limits(&self) -> SseLimits {
        self.sse_limits
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) async fn authorization(&self) -> Result<crate::auth::AuthLease, Error> {
        self.auth.authorization().await
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) async fn invalidate_authorization(&self, generation: Option<u64>) -> bool {
        self.auth.invalidate_if_generation(generation).await
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) fn organization(&self) -> Option<HeaderValue> {
        self.organization.clone()
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) fn project(&self) -> Option<HeaderValue> {
        self.project.clone()
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) fn client_request_id(&self) -> Option<HeaderValue> {
        self.client_request_id.clone()
    }

    #[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
    pub(crate) const fn tls_backend(&self) -> Option<TlsBackend> {
        self.tls_backend
    }

    pub(crate) async fn execute_json<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<ApiResponse<O::Response>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::Json {
            return Err(Error::InvalidConfiguration(
                "JSON decoder used for a non-JSON operation".into(),
            ));
        }
        let response = self.send::<O, Q>(path, query, body).await?;
        self.decode_json(response).await
    }

    /// Executes a JSON operation with one operation-owned static header.
    /// Callers cannot use this to override authentication or codec headers.
    pub(crate) async fn execute_json_with_static_header<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
        name: &'static str,
        value: &'static str,
    ) -> Result<ApiResponse<O::Response>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::Json {
            return Err(Error::InvalidConfiguration(
                "JSON decoder used for a non-JSON operation".into(),
            ));
        }
        let response = self
            .send_with_static_header::<O, Q>(path, query, body, Some((name, value)))
            .await?;
        self.decode_json(response).await
    }

    pub(crate) async fn execute_optional_json<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<ApiResponse<Option<O::Response>>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::EmptyOrJson {
            return Err(Error::InvalidConfiguration(
                "empty-or-JSON decoder used for an incompatible operation".into(),
            ));
        }
        let response = self.send::<O, Q>(path, query, body).await?;
        self.decode_optional_json(response).await
    }

    #[cfg(feature = "realtime")]
    pub(crate) async fn execute_empty<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<ApiResponse<()>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::Empty {
            return Err(Error::InvalidConfiguration(
                "empty decoder used for an incompatible operation".into(),
            ));
        }
        let response = self.send::<O, Q>(path, query, body).await?;
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let _body = read_success(response, self.max_json_body_bytes, &meta).await?;
        Ok(ApiResponse::new((), meta))
    }

    /// Sends an operation after validating its static contract. Kept separate
    /// from decoding so the streaming layer can reuse authentication, safe URL
    /// construction, status handling, and metadata extraction.
    pub(crate) async fn send<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<reqwest::Response, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        self.send_with_static_header::<O, Q>(path, query, body, None)
            .await
    }

    #[tracing::instrument(
        level = "debug",
        name = "openai.http_request",
        skip_all,
        fields(
            operation.id = O::META.id,
            http.request.method = %O::META.method,
            http.route = O::META.route,
            http.response.status_code = tracing::field::Empty,
            openai.request_id = tracing::field::Empty,
            retry.count = tracing::field::Empty,
        )
    )]
    async fn send_with_static_header<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
        static_header: Option<(&'static str, &'static str)>,
    ) -> Result<reqwest::Response, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        let meta = &O::META;
        if meta.id.is_empty() || !meta.route.starts_with('/') {
            return Err(Error::InvalidConfiguration(
                "operation metadata has an invalid identifier or route template".into(),
            ));
        }
        validate_operation_route(meta.route, path)?;
        if meta.auth != AuthScope::Platform {
            return Err(Error::InvalidConfiguration(
                "operation is not authorized for Platform credentials".into(),
            ));
        }
        match (meta.request_encoding, body) {
            (RequestEncoding::Json, None) => {
                return Err(Error::InvalidConfiguration(
                    "JSON operation is missing its request body".into(),
                ));
            }
            (RequestEncoding::None, Some(_)) => {
                return Err(Error::InvalidConfiguration(
                    "bodyless operation unexpectedly received a request body".into(),
                ));
            }
            (RequestEncoding::None, None) | (RequestEncoding::Json, Some(_)) => {}
        }

        let mut url = self.operation_url(path)?;
        if let Some(query) = query {
            append_query(&mut url, query)?;
        }
        let accept = match meta.response_mode {
            crate::operation::ResponseMode::Json | crate::operation::ResponseMode::EmptyOrJson => {
                JSON_MIME
            }
            #[cfg(feature = "realtime")]
            crate::operation::ResponseMode::Empty => JSON_MIME,
            crate::operation::ResponseMode::Sse => SSE_MIME,
        };
        let encoded_body = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(Error::Encode)?;
        let static_header = static_header
            .map(validate_static_operation_header)
            .transpose()?;
        let started = Instant::now();
        let mut retries = 0;
        let mut auth_refreshed = false;

        loop {
            let remaining = self
                .overall_timeout
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    trace::emit_deadline_exceeded();
                    trace::record_retry_count(retries);
                    Error::DeadlineExceeded
                })?;
            let authorization = self.auth.authorization().await?;
            let mut request = self
                .http
                .request(meta.method.clone(), url.clone())
                .timeout(remaining)
                .header(header::AUTHORIZATION, authorization.header.clone())
                .header(header::ACCEPT, accept);
            if let Some(organization) = &self.organization {
                request = request.header("OpenAI-Organization", organization.clone());
            }
            if let Some(project) = &self.project {
                request = request.header("OpenAI-Project", project.clone());
            }
            if let Some(client_request_id) = &self.client_request_id {
                request = request.header("X-Client-Request-Id", client_request_id.clone());
            }
            if let Some((name, value)) = &static_header {
                request = request.header(name.clone(), value.clone());
            }
            if let Some(encoded) = &encoded_body {
                request = request
                    .header(header::CONTENT_TYPE, JSON_MIME)
                    .body(encoded.clone());
            }

            let request = request.build().map_err(Error::from_reqwest)?;
            if !same_origin(request.url(), &self.base_url) {
                return Err(Error::InvalidConfiguration(
                    "operation URL escaped the configured authentication origin".into(),
                ));
            }
            let response = match self.http.execute(request).await {
                Ok(response) => response,
                Err(error)
                    if retryable_operation(meta.retry, self.retry_policy)
                        && retries < self.retry_policy.max_retries
                        && (error.is_connect() || error.is_timeout()) =>
                {
                    let delay = local_retry_delay(retries);
                    if !can_wait(started, delay, self.overall_timeout) {
                        trace::record_retry_count(retries);
                        return Err(Error::from_reqwest(error));
                    }
                    retries += 1;
                    trace::emit_retry(retries, delay, RetryReason::from_reqwest(&error));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(error) => {
                    trace::record_retry_count(retries);
                    return Err(Error::from_reqwest(error));
                }
            };

            if response.status() == http::StatusCode::UNAUTHORIZED
                && authorization.generation.is_some()
                && !auth_refreshed
            {
                let _ = self
                    .auth
                    .invalidate_if_generation(authorization.generation)
                    .await;
                auth_refreshed = true;
                trace::emit_auth_refresh();
                drop(response);
                continue;
            }

            if meta.success_statuses.contains(&response.status()) {
                trace::record_http_outcome(retries, &response);
                return Ok(response);
            }

            if retryable_operation(meta.retry, self.retry_policy)
                && retries < self.retry_policy.max_retries
                && should_retry_response(&response)
            {
                let delay = match server_retry_delay(
                    response.headers(),
                    self.retry_policy.max_server_delay,
                ) {
                    ServerDelay::Valid(delay) => delay,
                    ServerDelay::Absent => local_retry_delay(retries),
                    ServerDelay::TooLong => {
                        trace::record_http_outcome(retries, &response);
                        return self.api_error(response).await;
                    }
                };
                if can_wait(started, delay, self.overall_timeout) {
                    retries += 1;
                    trace::emit_retry(retries, delay, RetryReason::HttpStatus);
                    drop(response);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
            trace::record_http_outcome(retries, &response);
            return self.api_error(response).await;
        }
    }

    async fn api_error(&self, response: reqwest::Response) -> Result<reqwest::Response, Error> {
        Err(self.error_from_response(response).await)
    }

    pub(crate) async fn error_from_response(&self, response: reqwest::Response) -> Error {
        let response_meta = ResponseMeta::from_headers(response.status(), response.headers());
        match read_up_to(response, self.max_error_body_bytes).await {
            Ok((body, truncated)) => ApiError::from_body(response_meta, &body, truncated).into(),
            Err(error) => Error::from_response_body(error, &response_meta),
        }
    }

    pub(crate) async fn decode_json<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<ApiResponse<T>, Error>
    where
        T: DeserializeOwned,
    {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let decoded = deserialize_json(&body).map_err(|error| Error::Decode {
            source: error.source,
            path: error.path,
            meta_status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(
                &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                body.len() > DECODE_PREVIEW_BYTES,
            ),
        })?;
        Ok(ApiResponse::new(decoded, meta))
    }

    pub(crate) async fn decode_optional_json<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<ApiResponse<Option<T>>, Error>
    where
        T: DeserializeOwned,
    {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let decoded = if body.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            Some(deserialize_json(&body).map_err(|error| Error::Decode {
                source: error.source,
                path: error.path,
                meta_status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
                body: BodyPreview::from_bytes(
                    &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                    body.len() > DECODE_PREVIEW_BYTES,
                ),
            })?)
        };
        Ok(ApiResponse::new(decoded, meta))
    }

    #[cfg(feature = "realtime")]
    pub(crate) async fn decode_text(
        &self,
        response: reqwest::Response,
        expected_mime: &'static str,
    ) -> Result<ApiResponse<String>, Error> {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(expected_mime))
        }) {
            return Err(Error::UnexpectedContentType {
                expected: expected_mime,
                actual: content_type.map(Box::<str>::from),
                status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
            });
        }
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let text = String::from_utf8(body).map_err(|error| Error::InvalidUtf8 {
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(error.as_bytes(), false),
        })?;
        Ok(ApiResponse::new(text, meta))
    }

    #[cfg(feature = "realtime")]
    pub(crate) const fn http(&self) -> &reqwest::Client {
        &self.http
    }

    #[cfg(feature = "realtime")]
    pub(crate) const fn overall_timeout(&self) -> Duration {
        self.overall_timeout
    }

    #[cfg(feature = "realtime")]
    pub(crate) fn request_builder(
        &self,
        method: reqwest::Method,
        url: Url,
        accept: &'static str,
        authorization: HeaderValue,
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .request(method, url)
            .header(header::AUTHORIZATION, authorization)
            .header(header::ACCEPT, accept);
        if let Some(organization) = &self.organization {
            request = request.header("OpenAI-Organization", organization.clone());
        }
        if let Some(project) = &self.project {
            request = request.header("OpenAI-Project", project.clone());
        }
        if let Some(client_request_id) = &self.client_request_id {
            request = request.header("X-Client-Request-Id", client_request_id.clone());
        }
        request
    }

    #[cfg(feature = "realtime")]
    pub(crate) fn ensure_same_origin(&self, url: &Url) -> Result<(), Error> {
        if same_origin(url, &self.base_url) {
            Ok(())
        } else {
            Err(Error::InvalidConfiguration(
                "request URL escaped the configured authentication origin".into(),
            ))
        }
    }

    pub(crate) fn operation_url(&self, path: &[PathSegment<'_>]) -> Result<Url, Error> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|()| {
                Error::InvalidConfiguration("base URL cannot contain path segments".into())
            })?;
            segments.pop_if_empty();
            for segment in path {
                match segment {
                    PathSegment::Literal(value) => segments.push(value),
                    PathSegment::Parameter { name: _, value } => segments.push(value),
                };
            }
        }
        if !same_origin(&url, &self.base_url) {
            return Err(Error::InvalidConfiguration(
                "operation path escaped the configured authentication origin".into(),
            ));
        }
        Ok(url)
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base_origin = self.base_url.origin().ascii_serialization();
        formatter
            .debug_struct("Transport")
            .field("base_origin", &base_origin)
            .field("auth", &self.auth)
            .field(
                "organization",
                &self.organization.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project.as_ref().map(|_| "[REDACTED]"))
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .field("retry_policy", &self.retry_policy)
            .field("overall_timeout", &self.overall_timeout)
            .field("sse_limits", &self.sse_limits)
            .field("tls_backend", &self.tls_backend)
            .finish_non_exhaustive()
    }
}

async fn read_success(
    response: reqwest::Response,
    limit: usize,
    meta: &ResponseMeta,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        });
    }
    let (body, truncated) = read_up_to(response, limit)
        .await
        .map_err(|error| Error::from_response_body(error, meta))?;
    if truncated {
        Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        })
    } else {
        Ok(body)
    }
}

async fn read_up_to(
    response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn validate_static_operation_header(
    (name, value): (&'static str, &'static str),
) -> Result<(http::header::HeaderName, HeaderValue), Error> {
    let name = http::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
        Error::InvalidConfiguration("operation static header has an invalid name".into())
    })?;
    if matches!(
        name,
        header::AUTHORIZATION | header::ACCEPT | header::CONTENT_TYPE | header::HOST
    ) || name.as_str().eq_ignore_ascii_case("openai-organization")
        || name.as_str().eq_ignore_ascii_case("openai-project")
        || name.as_str().eq_ignore_ascii_case("x-client-request-id")
    {
        return Err(Error::InvalidConfiguration(
            "operation static header cannot override a protected header".into(),
        ));
    }
    let value = HeaderValue::from_str(value).map_err(|_| {
        Error::InvalidConfiguration("operation static header has an invalid value".into())
    })?;
    Ok((name, value))
}

pub(crate) struct JsonDecodeFailure {
    pub source: serde_json::Error,
    pub path: Option<Box<str>>,
}

pub(crate) fn deserialize_json<T>(bytes: &[u8]) -> Result<T, JsonDecodeFailure>
where
    T: DeserializeOwned,
{
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = error.path().to_string();
        JsonDecodeFailure {
            source: error.into_inner(),
            path: (!path.is_empty()).then(|| path.into_boxed_str()),
        }
    })?;
    deserializer
        .end()
        .map_err(|source| JsonDecodeFailure { source, path: None })?;
    Ok(value)
}

fn retryable_operation(class: RetryClass, policy: RetryPolicy) -> bool {
    match class {
        RetryClass::Safe => true,
        RetryClass::Replayable => policy.retry_replayable_mutations,
        #[cfg(any(feature = "realtime", feature = "legacy-realtime"))]
        RetryClass::Never => false,
    }
}

fn should_retry_response(response: &reqwest::Response) -> bool {
    match response
        .headers()
        .get("x-should-retry")
        .and_then(|value| value.to_str().ok())
    {
        Some("true") => true,
        Some("false") => false,
        Some(_) | None => {
            matches!(response.status().as_u16(), 408 | 409 | 429)
                || response.status().is_server_error()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerDelay {
    Absent,
    Valid(Duration),
    TooLong,
}

fn server_retry_delay(headers: &http::HeaderMap, maximum: Duration) -> ServerDelay {
    if let Some(value) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        && let Ok(milliseconds) = value.parse::<f64>()
        && milliseconds.is_finite()
        && milliseconds >= 0.0
    {
        return bounded_delay(milliseconds / 1000.0, maximum);
    }

    let Some(value) = headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
    else {
        return ServerDelay::Absent;
    };
    if let Ok(seconds) = value.parse::<f64>()
        && seconds.is_finite()
        && seconds >= 0.0
    {
        return bounded_delay(seconds, maximum);
    }
    match httpdate::parse_http_date(value) {
        Ok(time) => {
            let delay = match time.duration_since(SystemTime::now()) {
                Ok(delay) => delay,
                Err(_) => Duration::ZERO,
            };
            if delay <= maximum {
                ServerDelay::Valid(delay)
            } else {
                ServerDelay::TooLong
            }
        }
        Err(_) => ServerDelay::Absent,
    }
}

fn bounded_delay(seconds: f64, maximum: Duration) -> ServerDelay {
    if seconds > maximum.as_secs_f64() {
        ServerDelay::TooLong
    } else {
        match Duration::try_from_secs_f64(seconds) {
            Ok(delay) => ServerDelay::Valid(delay),
            Err(_) => ServerDelay::TooLong,
        }
    }
}

fn local_retry_delay(retries: u32) -> Duration {
    let exponent = retries.min(4) as i32;
    let base_seconds = (0.5_f64 * 2_f64.powi(exponent)).min(8.0);
    let fraction = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => f64::from(duration.subsec_nanos()) / 1_000_000_000.0,
        Err(_) => 0.5,
    };
    Duration::from_secs_f64(base_seconds * (0.75 + fraction * 0.25))
}

fn can_wait(started: Instant, delay: Duration, overall_timeout: Duration) -> bool {
    started
        .elapsed()
        .checked_add(delay)
        .is_some_and(|elapsed| elapsed < overall_timeout)
}

fn validate_operation_route(route: &str, path: &[PathSegment<'_>]) -> Result<(), Error> {
    let route_segments = route
        .strip_prefix('/')
        .ok_or_else(|| {
            Error::InvalidConfiguration("operation route must start with a slash".into())
        })?
        .split('/')
        .collect::<Vec<_>>();
    if route_segments.len() != path.len() {
        return Err(Error::InvalidConfiguration(
            "operation route metadata does not match its encoded path".into(),
        ));
    }
    for (template, segment) in route_segments.into_iter().zip(path) {
        let matches = match segment {
            PathSegment::Literal(value) => template == *value,
            PathSegment::Parameter { name, value: _ } => {
                template
                    .strip_prefix('{')
                    .and_then(|template| template.strip_suffix('}'))
                    == Some(*name)
            }
        };
        if !matches {
            return Err(Error::InvalidConfiguration(
                "operation route metadata does not match its encoded path".into(),
            ));
        }
    }
    Ok(())
}

fn append_query<T>(url: &mut Url, query: &T) -> Result<(), Error>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(query)
        .map_err(|error| Error::EncodeQuery(error.to_string().into()))?;
    let serde_json::Value::Object(fields) = value else {
        return Err(Error::EncodeQuery(
            "operation query must serialize as an object".into(),
        ));
    };
    if fields.is_empty() {
        return Ok(());
    }
    let mut serializer = url.query_pairs_mut();
    for (name, value) in fields {
        match value {
            serde_json::Value::Null => {
                serializer.append_pair(&name, "");
            }
            serde_json::Value::Bool(value) => {
                serializer.append_pair(&name, if value { "true" } else { "false" });
            }
            serde_json::Value::Number(value) => {
                serializer.append_pair(&name, &value.to_string());
            }
            serde_json::Value::String(value) => {
                serializer.append_pair(&name, &value);
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    let value = query_scalar(&name, value)?;
                    serializer.append_pair(&name, &value);
                }
            }
            serde_json::Value::Object(_) => {
                return Err(Error::EncodeQuery(
                    format!("query field `{name}` requires an unsupported object encoding").into(),
                ));
            }
        }
    }
    Ok(())
}

fn query_scalar(name: &str, value: serde_json::Value) -> Result<String, Error> {
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(Error::EncodeQuery(
            format!("query array field `{name}` contains a non-scalar value").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parameters_are_single_percent_encoded_segments() {
        let base = Url::parse("https://api.openai.com/v1/").expect("test URL");
        let transport = Transport::new(
            reqwest::Client::new(),
            base,
            AuthProvider::api_key(crate::ApiKey::new("test-placeholder-key").expect("test key")),
            None,
            None,
            None,
            1024,
            1024,
            RetryPolicy::disabled(),
            Duration::from_secs(1),
            SseLimits::default(),
            None,
        );
        let path = [
            PathSegment::literal("responses"),
            PathSegment::parameter("response_id", "resp/a b").expect("valid ID"),
        ];
        let url = transport.operation_url(&path).expect("operation URL");
        assert_eq!(
            url.as_str(),
            "https://api.openai.com/v1/responses/resp%2Fa%20b"
        );
    }

    #[test]
    fn dot_segments_are_rejected() {
        assert!(PathSegment::parameter("response_id", "..").is_err());
    }

    #[test]
    fn empty_query_does_not_add_a_trailing_question_mark() {
        let mut url =
            Url::parse("https://api.openai.com/v1/responses/resp_1").expect("test operation URL");
        append_query(&mut url, &serde_json::json!({})).expect("empty query");
        assert_eq!(url.as_str(), "https://api.openai.com/v1/responses/resp_1");
        assert!(url.query().is_none());
    }

    #[test]
    fn operation_static_headers_cannot_override_security_or_codec_headers() {
        assert!(validate_static_operation_header(("OpenAI-Beta", "chatkit_beta=v1")).is_ok());
        for name in [
            "Authorization",
            "Accept",
            "Content-Type",
            "Host",
            "OpenAI-Organization",
            "OpenAI-Project",
            "X-Client-Request-Id",
        ] {
            assert!(validate_static_operation_header((name, "forbidden")).is_err());
        }
    }

    #[test]
    fn retry_headers_are_strict_and_bounded() {
        let mut headers = http::HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("250"));
        assert_eq!(
            server_retry_delay(&headers, Duration::from_secs(1)),
            ServerDelay::Valid(Duration::from_millis(250))
        );

        headers.insert("retry-after-ms", HeaderValue::from_static("2000"));
        assert_eq!(
            server_retry_delay(&headers, Duration::from_secs(1)),
            ServerDelay::TooLong
        );
    }

    #[test]
    fn conservative_policy_only_retries_safe_operations() {
        assert!(retryable_operation(
            RetryClass::Safe,
            RetryPolicy::conservative()
        ));
        assert!(!retryable_operation(
            RetryClass::Replayable,
            RetryPolicy::conservative()
        ));
        assert!(retryable_operation(
            RetryClass::Replayable,
            RetryPolicy::openai_compatible()
        ));
    }
}
