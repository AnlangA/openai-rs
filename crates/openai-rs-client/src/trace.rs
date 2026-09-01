//! Shared, crate-private tracing helpers for outbound HTTP.
//!
//! Field names stay low-cardinality. Callers must never pass credentials,
//! URLs, query strings, or request/response bodies into these helpers.

use std::fmt;
use std::time::Duration;

use crate::ResponseMeta;
use crate::transport::PathSegment;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetryReason {
    Connect,
    Timeout,
    HttpStatus,
}

impl RetryReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::HttpStatus => "http_status",
        }
    }

    pub(crate) fn from_reqwest(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Connect
        }
    }
}

/// Creates the shared outbound HTTP span for lanes whose route template is a
/// static literal (no computation to defer).
pub(crate) fn http_request_span(operation_id: &str, method: &str, route: &str) -> tracing::Span {
    http_request_span_lazy(operation_id, method, || route)
}

/// Creates the shared outbound HTTP span, deferring the route template.
///
/// `tracing` only evaluates span field expressions when the callsite is
/// enabled, so passing the (allocating) [`route_template`] computation as a
/// closure keeps disabled debug spans allocation-free (6-17). The three
/// hand-copied span declarations in `transport.rs`, `admin.rs`, and the codex
/// direct transport must keep the same six-field shape.
pub(crate) fn http_request_span_lazy<F, S>(
    operation_id: &str,
    method: &str,
    route: F,
) -> tracing::Span
where
    F: FnOnce() -> S,
    S: fmt::Display,
{
    tracing::debug_span!(
        "openai.http_request",
        operation.id = operation_id,
        http.request.method = method,
        http.route = %route(),
        http.response.status_code = tracing::field::Empty,
        openai.request_id = tracing::field::Empty,
        retry.count = tracing::field::Empty,
    )
}

pub(crate) fn route_template(path: &[PathSegment<'_>]) -> String {
    let mut route = String::new();
    for segment in path {
        route.push('/');
        match segment {
            PathSegment::Literal(value) => route.push_str(value),
            PathSegment::Parameter { name, .. } => {
                route.push('{');
                route.push_str(name);
                route.push('}');
            }
        }
    }
    route
}

pub(crate) fn record_retry_count(retries: u32) {
    tracing::Span::current().record("retry.count", retries);
}

pub(crate) fn record_response(meta: &ResponseMeta) {
    let span = tracing::Span::current();
    span.record("http.response.status_code", meta.status().as_u16());
    if let Some(request_id) = meta.request_id() {
        span.record("openai.request_id", request_id);
    }
}

pub(crate) fn record_http_outcome(retries: u32, response: &reqwest::Response) {
    record_retry_count(retries);
    record_response(&ResponseMeta::from_headers(
        response.status(),
        response.headers(),
    ));
}

pub(crate) fn emit_retry(attempt: u32, delay: Duration, reason: RetryReason) {
    let delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
    // The event reuses the span's `retry.count` name (6-17): one concept, one
    // field name, instead of a `retry.attempt`/`retry.count` pair that forced
    // consumers to correlate two spellings for the same counter.
    tracing::warn!(
        retry.count = attempt,
        retry.delay_ms = delay_ms,
        retry.reason = reason.as_str(),
        "retrying OpenAI request"
    );
}

/// 401 accounting for lanes that invalidate the cached credential *and replay
/// the request* under the same span.
pub(crate) fn emit_auth_refresh() {
    tracing::debug!("401 received, invalidating cached authentication and retrying");
}

/// 401 accounting for single-shot lanes: the cached credential is invalidated,
/// but the request is never replayed, so the message must not claim a retry
/// (6-17; used by the multipart one-shot form lane).
pub(crate) fn emit_auth_refresh_no_retry() {
    tracing::debug!("401 received, invalidating cached authentication");
}

pub(crate) fn emit_deadline_exceeded() {
    tracing::warn!("request deadline exceeded");
}

#[cfg(test)]
pub(crate) mod capture {
    use std::collections::HashMap;
    use std::fmt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use std::cell::RefCell;

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};
    use tracing_core::span::Current;

    thread_local! {
        static CURRENT_SPANS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Debug, Default)]
    pub(crate) struct CapturedSpan {
        pub name: String,
        pub fields: Vec<(String, String)>,
    }

    impl CapturedSpan {
        pub(crate) fn field(&self, name: &str) -> Option<&str> {
            self.fields
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
    }

    #[derive(Clone, Debug, Default)]
    pub(crate) struct CapturedEvent {
        pub level: String,
        pub fields: Vec<(String, String)>,
    }

    impl CapturedEvent {
        pub(crate) fn field(&self, name: &str) -> Option<&str> {
            self.fields
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }

        pub(crate) fn message(&self) -> Option<&str> {
            self.field("message")
        }
    }

    #[derive(Default)]
    struct Inner {
        spans: Vec<CapturedSpan>,
        events: Vec<CapturedEvent>,
        by_id: HashMap<u64, usize>,
        metadata: HashMap<u64, &'static Metadata<'static>>,
    }

    #[derive(Clone)]
    pub(crate) struct Capture {
        inner: Arc<Mutex<Inner>>,
        next_id: Arc<AtomicU64>,
    }

    impl Capture {
        pub(crate) fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(Inner::default())),
                next_id: Arc::new(AtomicU64::new(1)),
            }
        }

        fn lock(&self) -> MutexGuard<'_, Inner> {
            self.inner.lock().unwrap_or_else(PoisonError::into_inner)
        }

        pub(crate) fn spans(&self) -> Vec<CapturedSpan> {
            self.lock().spans.clone()
        }

        pub(crate) fn events(&self) -> Vec<CapturedEvent> {
            self.lock().events.clone()
        }

        pub(crate) fn contains_text(&self, needle: &str) -> bool {
            let inner = self.lock();
            inner.spans.iter().any(|span| {
                span.name.contains(needle)
                    || span
                        .fields
                        .iter()
                        .any(|(key, value)| key.contains(needle) || value.contains(needle))
            }) || inner.events.iter().any(|event| {
                event
                    .fields
                    .iter()
                    .any(|(key, value)| key.contains(needle) || value.contains(needle))
            })
        }
    }

    struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

    impl Visit for FieldCollector<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push((field.name().to_owned(), value.to_owned()));
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }

        fn record_i64(&mut self, field: &Field, value: i64) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }

        fn record_bool(&mut self, field: &Field, value: bool) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::TRACE)
        }

        fn new_span(&self, attributes: &Attributes<'_>) -> Id {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let mut fields = Vec::new();
            attributes.record(&mut FieldCollector(&mut fields));
            let mut inner = self.lock();
            let index = inner.spans.len();
            inner.spans.push(CapturedSpan {
                name: attributes.metadata().name().to_owned(),
                fields,
            });
            inner.by_id.insert(id, index);
            inner.metadata.insert(id, attributes.metadata());
            Id::from_u64(id)
        }

        fn record(&self, span: &Id, values: &Record<'_>) {
            let mut fields = Vec::new();
            values.record(&mut FieldCollector(&mut fields));
            let mut inner = self.lock();
            if let Some(index) = inner.by_id.get(&span.into_u64()).copied()
                && let Some(captured) = inner.spans.get_mut(index)
            {
                captured.fields.extend(fields);
            }
        }

        fn event(&self, event: &Event<'_>) {
            let mut fields = Vec::new();
            event.record(&mut FieldCollector(&mut fields));
            self.lock().events.push(CapturedEvent {
                level: event.metadata().level().to_string(),
                fields,
            });
        }

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn enter(&self, span: &Id) {
            CURRENT_SPANS.with(|stack| stack.borrow_mut().push(span.into_u64()));
        }

        fn exit(&self, span: &Id) {
            CURRENT_SPANS.with(|stack| {
                let mut stack = stack.borrow_mut();
                if let Some(index) = stack.iter().rposition(|id| *id == span.into_u64()) {
                    stack.remove(index);
                }
            });
        }

        fn current_span(&self) -> Current {
            CURRENT_SPANS.with(|stack| {
                let Some(id) = stack.borrow().last().copied() else {
                    return Current::none();
                };
                match self.lock().metadata.get(&id).copied() {
                    Some(metadata) => Current::new(Id::from_u64(id), metadata),
                    None => Current::none(),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::Full;
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::CreateEmbeddingRequest;
    use openai_rs_types::responses::CreateResponseRequest;
    use tokio::net::TcpListener;
    use url::Url;

    use super::RetryReason;
    use super::capture::Capture;
    use crate::transport::PathSegment;
    use crate::{ApiKey, Client, RetryPolicy};

    const SECRET_KEY: &str = "sk-test-secret-12345";
    const SECRET_PROMPT: &str = "SUPER_SECRET_PROMPT_XYZ";
    const MODEL_LIST: &str = r#"{"object":"list","data":[]}"#;
    const EMBEDDING_BODY: &str = r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1],"index":0}],"model":"text-embedding-3-small","usage":{"prompt_tokens":1,"total_tokens":1}}"#;

    async fn serve_sequence(responses: Vec<(StatusCode, &'static str, &'static str)>) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("loopback address");
        let queue = Arc::new(Mutex::new(VecDeque::from(responses)));

        tokio::spawn(async move {
            loop {
                if queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_empty()
                {
                    break;
                }
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let queue = Arc::clone(&queue);
                let service = service_fn(move |_request: Request<Incoming>| {
                    let queue = Arc::clone(&queue);
                    async move {
                        let next = queue
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .pop_front();
                        let (status, request_id, body) =
                            next.unwrap_or((StatusCode::OK, "req_missing", "{}"));
                        let response = hyper::Response::builder()
                            .status(status)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", request_id)
                            .header("retry-after-ms", "1")
                            .body(Full::new(Bytes::from_static(body.as_bytes())))
                            .expect("build loopback response");
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            }
        });

        Url::parse(&format!("http://{address}/v1/")).expect("loopback base URL")
    }

    fn client(base_url: Url, key: &str) -> Client {
        let key = ApiKey::new(key).expect("valid test key");
        Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(
                RetryPolicy::openai_compatible()
                    .max_retries(1)
                    .max_server_delay(Duration::from_secs(1)),
            )
            .build()
            .expect("loopback client")
    }

    #[test]
    fn route_templates_use_parameter_names() {
        let path = [
            PathSegment::literal("files"),
            PathSegment::parameter("file_id", "file-secret").expect("valid id"),
            PathSegment::literal("content"),
        ];
        assert_eq!(super::route_template(&path), "/files/{file_id}/content");
    }

    #[test]
    fn retry_reason_distinguishes_timeout() {
        assert_eq!(RetryReason::Connect.as_str(), "connect");
        assert_eq!(RetryReason::Timeout.as_str(), "timeout");
        assert_eq!(RetryReason::HttpStatus.as_str(), "http_status");
    }

    #[tokio::test]
    async fn successful_request_records_operation_status_and_request_id() {
        let base = serve_sequence(vec![(StatusCode::OK, "req_loopback", MODEL_LIST)]).await;
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        let response = client(base, "test-placeholder-key")
            .models()
            .list()
            .await
            .expect("models list");

        assert_eq!(response.request_id(), Some("req_loopback"));
        let span = capture
            .spans()
            .into_iter()
            .find(|span| span.name == "openai.http_request")
            .expect("http request span");
        assert_eq!(span.field("operation.id"), Some("ListModels"));
        assert_eq!(span.field("http.request.method"), Some("GET"));
        assert_eq!(span.field("http.route"), Some("/models"));
        assert_eq!(span.field("http.response.status_code"), Some("200"));
        assert_eq!(span.field("openai.request_id"), Some("req_loopback"));
    }

    #[tokio::test]
    async fn http_retry_emits_warn_with_retry_count() {
        let base = serve_sequence(vec![
            (StatusCode::TOO_MANY_REQUESTS, "req_retry", "{\"error\":{}}"),
            (StatusCode::OK, "req_ok", MODEL_LIST),
        ])
        .await;
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        client(base, "test-placeholder-key")
            .models()
            .list()
            .await
            .expect("retried models list");

        let retry = capture
            .events()
            .into_iter()
            .find(|event| event.message() == Some("retrying OpenAI request"))
            .expect("retry event");
        assert_eq!(retry.level, "WARN");
        // The event and the span share the `retry.count` field name (6-17);
        // no `retry.attempt` synonym may reappear.
        assert_eq!(retry.field("retry.count"), Some("1"));
        assert!(retry.field("retry.attempt").is_none());
        assert_eq!(retry.field("retry.reason"), Some("http_status"));
        let span = capture
            .spans()
            .into_iter()
            .find(|span| span.name == "openai.http_request")
            .expect("http request span");
        assert_eq!(span.field("retry.count"), Some("1"));
    }

    #[test]
    fn auth_refresh_messages_match_their_lanes_retry_behavior() {
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        super::emit_auth_refresh();
        super::emit_auth_refresh_no_retry();

        let messages = capture
            .events()
            .into_iter()
            .filter_map(|event| event.message().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            vec![
                "401 received, invalidating cached authentication and retrying",
                "401 received, invalidating cached authentication",
            ],
            "the single-shot variant must not claim a retry that never happens"
        );
    }

    /// A subscriber that, like a production default logger, keeps DEBUG
    /// output off.
    struct WarnOnlySubscriber;

    impl tracing::Subscriber for WarnOnlySubscriber {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= tracing::Level::WARN
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::WARN)
        }

        fn new_span(&self, _attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn event(&self, _event: &tracing::Event<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    #[test]
    fn lazy_route_template_is_skipped_when_debug_spans_are_disabled() {
        // 6-17: under a WARN-level subscriber (the production posture for a
        // default logger), the route-template closure must not run at all —
        // skipping that allocation is the point of the lazy variant. The
        // assertions are insensitive to the process-wide callsite cache:
        // the macro consults the current subscriber's `enabled` before any
        // field expression is evaluated, so a stale cache can only skip the
        // closure, never run it behind the subscriber's back.
        let _guard = tracing::subscriber::set_default(WarnOnlySubscriber);
        let path = [
            PathSegment::literal("files"),
            PathSegment::parameter("file_id", "file-secret").expect("valid id"),
            PathSegment::literal("content"),
        ];
        let mut evaluated = false;
        let span = super::http_request_span_lazy("test.disabled_lane", "GET", || {
            evaluated = true;
            super::route_template(&path)
        });
        assert!(span.is_none());
        assert!(
            !evaluated,
            "route template was computed although the span is disabled"
        );
    }

    #[tokio::test]
    async fn captured_fields_do_not_include_secrets_or_prompts() {
        let base = serve_sequence(vec![(StatusCode::OK, "req_embed", EMBEDDING_BODY)]).await;
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        let request = CreateEmbeddingRequest::new("text-embedding-3-small", SECRET_PROMPT);
        client(base, SECRET_KEY)
            .embeddings()
            .create(request)
            .await
            .expect("embeddings create");

        assert!(
            !capture.contains_text(SECRET_KEY),
            "API key leaked into tracing fields"
        );
        assert!(
            !capture.contains_text("Bearer "),
            "authorization header leaked into tracing fields"
        );
        assert!(
            !capture.contains_text(SECRET_PROMPT),
            "prompt leaked into tracing fields"
        );
    }

    /// Serves exactly one SSE response body, for the streaming no-leak test.
    async fn serve_sse_once(body: &'static str) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback SSE server");
        let address = listener.local_addr().expect("loopback SSE address");
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let service = service_fn(move |_request: Request<Incoming>| {
                let response = hyper::Response::builder()
                    .status(StatusCode::OK)
                    .header(http::header::CONTENT_TYPE, "text/event-stream")
                    .header("x-request-id", "req_sse")
                    .body(Full::new(Bytes::from_static(body.as_bytes())))
                    .expect("build loopback SSE response");
                async move { Ok::<_, Infallible>(response) }
            });
            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
        Url::parse(&format!("http://{address}/v1/")).expect("loopback base URL")
    }

    #[tokio::test]
    async fn sse_stream_deltas_never_enter_tracing() {
        const SECRET_DELTA: &str = "SUPER_SECRET_DELTA_XYZ";
        let body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"SUPER_SECRET_DELTA_XYZ\",\"sequence_number\":1,\"logprobs\":[]}\n\n",
            "data: [DONE]\n\n",
        );
        let base = serve_sse_once(body).await;
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        let request = CreateResponseRequest::new("test-model", SECRET_PROMPT).into_streaming();
        let mut stream = client(base, "test-placeholder-key")
            .responses()
            .create_stream(request)
            .await
            .expect("stream handshake");
        while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
            assert!(event.is_ok(), "typed SSE event must decode");
        }

        // The handshake span keeps the six-field shape while the decoded
        // deltas stay out of spans and events entirely (6-18: stream content
        // is never traced; consumption happens below any span).
        let span = capture
            .spans()
            .into_iter()
            .find(|span| span.name == "openai.http_request")
            .expect("stream handshake span");
        assert_eq!(span.field("operation.id"), Some("CreateStreamingResponse"));
        assert_eq!(span.field("http.request.method"), Some("POST"));
        assert_eq!(span.field("http.route"), Some("/responses"));
        assert_eq!(span.field("http.response.status_code"), Some("200"));
        assert_eq!(span.field("openai.request_id"), Some("req_sse"));
        assert_eq!(span.field("retry.count"), Some("0"));
        assert!(
            !capture.contains_text(SECRET_DELTA),
            "SSE delta content leaked into tracing fields"
        );
        assert!(
            !capture.contains_text(SECRET_PROMPT),
            "streamed prompt leaked into tracing fields"
        );
        assert!(
            !capture.contains_text("Bearer "),
            "authorization header leaked into tracing fields"
        );
    }

    /// 6-18: the Administration lane keeps the same six-field span shape as
    /// the platform transport while neither the admin key nor its Bearer
    /// header ever reaches a span or event.
    #[cfg(feature = "admin")]
    #[tokio::test]
    async fn admin_lane_span_records_six_fields_without_credentials() {
        const ADMIN_SECRET_KEY: &str = "admin-secret-key-8f3a";
        let base = serve_sequence(vec![(
            StatusCode::OK,
            "req_admin",
            r#"{"object":"list","data":[],"has_more":false}"#,
        )])
        .await;
        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        let key = crate::AdminApiKey::new(ADMIN_SECRET_KEY).expect("valid admin key");
        let client = crate::AdminClient::builder(key)
            .base_url(base)
            .allow_insecure_loopback(true)
            .build()
            .expect("loopback admin client");
        let users = client
            .users()
            .list(&openai_rs_types::admin::AdminListParams::default())
            .await
            .expect("admin user list");

        assert!(users.data.is_empty());
        let span = capture
            .spans()
            .into_iter()
            .find(|span| span.name == "openai.http_request")
            .expect("admin http request span");
        assert_eq!(span.field("operation.id"), Some("list-users"));
        assert_eq!(span.field("http.request.method"), Some("GET"));
        assert_eq!(span.field("http.route"), Some("/organization/users"));
        assert_eq!(span.field("http.response.status_code"), Some("200"));
        assert_eq!(span.field("openai.request_id"), Some("req_admin"));
        assert_eq!(span.field("retry.count"), Some("0"));
        assert!(
            !capture.contains_text(ADMIN_SECRET_KEY),
            "admin key leaked into tracing fields"
        );
        assert!(
            !capture.contains_text("Bearer "),
            "authorization header leaked into tracing fields"
        );
    }
}
