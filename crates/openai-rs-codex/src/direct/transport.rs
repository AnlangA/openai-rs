use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_core::Stream;
use futures_util::StreamExt;
use openai_rs_types::responses::{
    CreateResponseRequest, CreateStreamingResponseRequest, Response, ResponseStreamEvent,
};
use serde_json::Value;
use tokio::sync::mpsc;
use url::Url;
use zeroize::Zeroizing;

use tracing::Instrument;

use super::auth::{CredentialStore, StoredCodexSession, TokenManager};
use super::sse::{SseDecoder, SseItem};
use super::{CODEX_RESPONSES_ENDPOINT, DirectError};

const MAX_JSON_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
/// Default maximum size of one physical SSE line, matching the platform
/// decoder's `DEFAULT_MAX_SSE_LINE_BYTES` (D0144): Codex Responses frames are
/// single physical `data:` lines, so the line cap must live in the same
/// magnitude class as the event cap that actually bounds memory.
const DEFAULT_MAX_SSE_LINE_BYTES: usize = 32 * 1024 * 1024;
/// Default maximum size of the joined `data:` value of one SSE event (D0144:
/// 32 MiB, up from the earlier 4 MiB hardcode).
const DEFAULT_MAX_SSE_EVENT_BYTES: usize = 32 * 1024 * 1024;
const MAX_ERROR_MESSAGE_CHARS: usize = 256;
const STREAM_QUEUE_CAPACITY: usize = 64;

/// Default total budget for a non-streaming request.
///
/// 600s mirrors the platform client's `DEFAULT_REQUEST_TIMEOUT` (D0199
/// total-budget semantics); the previous 120s hardcode was sized for a
/// transport that also truncated streaming turns, which no longer applies.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// Connection-establishment budget shared by both lanes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Host-locked executor for the private Codex Responses operation.
///
/// # Timeout lanes
///
/// The two operations run under deliberately different time budgets:
///
/// - [`create`](Self::create) runs under one total request budget
///   ([`request_timeout`](Self::request_timeout), default 600s), applied per
///   request and configurable through
///   [`with_request_timeout`](Self::with_request_timeout).
/// - [`stream`](Self::stream) applies **no** total budget. A long streaming
///   turn is bounded by the SSE protocol itself — the `[DONE]`/lifecycle
///   terminal, EOF, or a decoder limit — and by the caller dropping the
///   [`DirectResponseStream`]. Connection establishment is still bounded by
///   the shared 10s connect timeout.
///
/// This split is why the inner `reqwest::Client` carries no client-level
/// total timeout: reqwest 0.12 offers no per-request "no timeout" override
/// (`RequestBuilder::timeout` only replaces a duration with another), so the
/// total budget is attached to the non-streaming request alone. The knob on
/// this type therefore never widens or narrows the streaming lane.
pub struct DirectCodexResponsesClient<S: CredentialStore> {
    http: reqwest::Client,
    tokens: Arc<TokenManager<S>>,
    endpoint: Url,
    request_timeout: Duration,
    max_sse_line_bytes: usize,
    max_sse_event_bytes: usize,
}

impl<S: CredentialStore> std::fmt::Debug for DirectCodexResponsesClient<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectCodexResponsesClient")
            .field("endpoint", &CODEX_RESPONSES_ENDPOINT)
            .field("request_timeout", &self.request_timeout)
            .field("max_sse_line_bytes", &self.max_sse_line_bytes)
            .field("max_sse_event_bytes", &self.max_sse_event_bytes)
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl<S: CredentialStore> DirectCodexResponsesClient<S> {
    pub fn new(tokens: Arc<TokenManager<S>>) -> Result<Self, DirectError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            // No client-level total timeout: the streaming lane must not be
            // truncated by one (see the type-level "Timeout lanes" docs).
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("openai-rs/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let endpoint = Url::parse(CODEX_RESPONSES_ENDPOINT)
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        Ok(Self {
            http,
            tokens,
            endpoint,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_sse_line_bytes: DEFAULT_MAX_SSE_LINE_BYTES,
            max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
        })
    }

    /// Override the total budget for [`create`](Self::create) requests.
    ///
    /// This is the escape hatch for deliberately slow non-streaming turns (or
    /// tight callers wanting a narrower budget). It does not affect
    /// [`stream`](Self::stream), which runs without a total budget by design.
    /// A zero duration mirrors the platform `Client::with_request_timeout`
    /// posture: it cannot be rejected on this infallible surface and fails
    /// every non-streaming request immediately with a timeout error.
    #[must_use]
    pub fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    /// Override the SSE decoder resource limits.
    ///
    /// `max_line_bytes` bounds one physical line and `max_event_bytes` bounds
    /// the joined `data:` value of one event; both must be non-zero and both
    /// fail a stream only when a completed length strictly exceeds the limit.
    /// Defaults are 32 MiB / 32 MiB, matching the platform decoder (D0144).
    pub fn with_sse_limits(
        mut self,
        max_line_bytes: usize,
        max_event_bytes: usize,
    ) -> Result<Self, DirectError> {
        if max_line_bytes == 0 {
            return Err(DirectError::Configuration(
                "max_line_bytes must be non-zero".to_owned(),
            ));
        }
        if max_event_bytes == 0 {
            return Err(DirectError::Configuration(
                "max_event_bytes must be non-zero".to_owned(),
            ));
        }
        self.max_sse_line_bytes = max_line_bytes;
        self.max_sse_event_bytes = max_event_bytes;
        Ok(self)
    }

    /// The total budget applied to [`create`](Self::create) requests.
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Maximum size of one physical SSE line accepted by
    /// [`stream`](Self::stream).
    pub const fn max_sse_line_bytes(&self) -> usize {
        self.max_sse_line_bytes
    }

    /// Maximum joined `data:` size of one SSE event accepted by
    /// [`stream`](Self::stream).
    pub const fn max_sse_event_bytes(&self) -> usize {
        self.max_sse_event_bytes
    }

    /// Execute the only supported non-streaming operation.
    ///
    /// Runs under the [`request_timeout`](Self::request_timeout) total budget
    /// (default 600s): connection establishment, the request, and the full
    /// JSON response body must all fit inside it. If the request fails with
    /// 401 and is retried once with a refreshed token, the retry gets its own
    /// budget, so the worst-case wall time is up to ~2× the configured budget
    /// plus one token-refresh round trip.
    pub async fn create(&self, request: &CreateResponseRequest) -> Result<Response, DirectError> {
        let body = serde_json::to_value(request)?;
        validate_body(&body, false)?;
        let session = self.tokens.session().await?;
        let generation = session.generation();
        let mut response = self.send(&body, &session, false).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            tracing::debug!("401 received, invalidating cached authentication and retrying");
            let refreshed = self.tokens.refresh_after_unauthorized(generation).await?;
            response = self.send(&body, &refreshed, false).await?;
        }
        decode_json_response(response).await
    }

    /// Execute the only supported streaming operation.
    ///
    /// Runs with **no** total time budget: connection establishment is bounded
    /// by the shared 10s connect timeout, but the streamed body may take as
    /// long as the turn needs. The stream ends on the `[DONE]`/lifecycle
    /// terminal, on EOF, on a decoder or decoding error (fail-stop), or when
    /// the caller drops the returned stream. Resource bounds are spatial, not
    /// temporal: see [`with_sse_limits`](Self::with_sse_limits).
    pub async fn stream(
        &self,
        request: &CreateStreamingResponseRequest,
    ) -> Result<DirectResponseStream, DirectError> {
        let body = serde_json::to_value(request)?;
        validate_body(&body, true)?;
        let session = self.tokens.session().await?;
        let generation = session.generation();
        let mut response = self.send(&body, &session, true).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            tracing::debug!("401 received, invalidating cached authentication and retrying");
            let refreshed = self.tokens.refresh_after_unauthorized(generation).await?;
            response = self.send(&body, &refreshed, true).await?;
        }
        if response.status().is_redirection() {
            return Err(DirectError::RedirectRejected);
        }
        if !response.status().is_success() {
            return Err(status_error(response).await);
        }
        // The sealed ChatGPT Codex endpoint can omit Content-Type on a valid
        // SSE response. When it is absent, defer validation to the bounded,
        // fail-stop decoder; an explicitly incompatible type still fails the
        // handshake.
        let content_type_missing = !response
            .headers()
            .contains_key(reqwest::header::CONTENT_TYPE);
        for value in response
            .headers()
            .get_all(reqwest::header::CONTENT_TYPE)
            .iter()
        {
            let content_type = value.to_str().map_err(|_| {
                DirectError::Sse("response content type was not valid ASCII".to_owned())
            })?;
            let is_event_stream = content_type
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
            if !is_event_stream {
                let actual =
                    sanitize_error_message(content_type).unwrap_or_else(|| "<empty>".to_owned());
                return Err(DirectError::Sse(format!(
                    "response content type was {actual:?}, expected text/event-stream"
                )));
            }
        }

        let max_sse_line_bytes = self.max_sse_line_bytes;
        let max_sse_event_bytes = self.max_sse_event_bytes;
        let (sender, receiver) = mpsc::channel(STREAM_QUEUE_CAPACITY);
        tokio::spawn(
            async move {
                let mut body = response.bytes_stream();
                let mut decoder = SseDecoder::new(max_sse_line_bytes, max_sse_event_bytes);
                let mut saw_sse_item = false;
                while let Some(chunk) = body.next().await {
                    let items = match chunk {
                        Ok(chunk) => decoder.feed(&chunk),
                        Err(error) => Err(DirectError::Http(error)),
                    };
                    let items = match items {
                        Ok(items) => items,
                        Err(error) => {
                            let _ = sender.send(Err(error)).await;
                            return;
                        }
                    };
                    saw_sse_item |= !items.is_empty();
                    if dispatch_sse_items(&sender, items).await {
                        return;
                    }
                }
                match decoder.finish() {
                    Ok(items) => {
                        saw_sse_item |= !items.is_empty();
                        if dispatch_sse_items(&sender, items).await {
                            return;
                        }
                        if content_type_missing && !saw_sse_item {
                            let _ = sender
                                .send(Err(DirectError::Sse(
                                    "response body did not contain SSE events".to_owned(),
                                )))
                                .await;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                    }
                }
            }
            .instrument(tracing::debug_span!("codex.direct.sse")),
        );
        Ok(DirectResponseStream { receiver })
    }

    async fn send(
        &self,
        body: &Value,
        session: &StoredCodexSession,
        streaming: bool,
    ) -> Result<reqwest::Response, DirectError> {
        // No redirect policy, base URL, raw URL, arbitrary path, or caller
        // headers are accepted by this sealed transport.
        let mut session_id_bytes = [0_u8; 24];
        getrandom::fill(&mut session_id_bytes).map_err(|_| DirectError::Random)?;
        let session_id = URL_SAFE_NO_PAD.encode(session_id_bytes);
        let accept = if streaming {
            "text/event-stream"
        } else {
            "application/json"
        };
        let bearer = Zeroizing::new(format!("Bearer {}", session.access_token()));
        let mut authorization = reqwest::header::HeaderValue::from_str(&bearer).map_err(|_| {
            DirectError::Configuration("access token could not be encoded as a header".to_owned())
        })?;
        authorization.set_sensitive(true);
        let mut account_id = reqwest::header::HeaderValue::from_str(session.account_id().as_str())
            .map_err(|_| {
                DirectError::Configuration(
                    "account identifier could not be encoded as a header".to_owned(),
                )
            })?;
        account_id.set_sensitive(true);
        let span = tracing::debug_span!(
            "openai.http_request",
            operation.id = "codex.direct.responses",
            http.request.method = "POST",
            http.route = "/backend-api/codex/responses",
            http.response.status_code = tracing::field::Empty,
            openai.request_id = tracing::field::Empty,
            retry.count = tracing::field::Empty,
        );
        async move {
            let mut request = self.http.post(self.endpoint.clone());
            if !streaming {
                // Total budget for the non-streaming lane only. The streaming
                // lane deliberately gets no total timeout: attaching one here
                // (or on the client) would truncate long turns mid-body.
                request = request.timeout(self.request_timeout);
            }
            let response = request
                .header(reqwest::header::AUTHORIZATION, authorization)
                .header("ChatGPT-Account-Id", account_id)
                .header("originator", super::CODEX_ORIGINATOR)
                .header("session_id", session_id)
                .header(reqwest::header::ACCEPT, accept)
                .json(body)
                .send()
                .await?;
            tracing::Span::current().record("retry.count", 0_u32);
            tracing::Span::current()
                .record("http.response.status_code", response.status().as_u16());
            if let Some(request_id) = response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
            {
                tracing::Span::current().record("openai.request_id", request_id);
            }
            Ok(response)
        }
        .instrument(span)
        .await
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoint(
        tokens: Arc<TokenManager<S>>,
        endpoint: Url,
    ) -> Result<Self, DirectError> {
        let mut client = Self::new(tokens)?;
        client.endpoint = endpoint;
        Ok(client)
    }
}

/// Bounded typed SSE stream.
pub struct DirectResponseStream {
    receiver: mpsc::Receiver<Result<ResponseStreamEvent, DirectError>>,
}

impl DirectResponseStream {
    pub async fn next_event(&mut self) -> Option<Result<ResponseStreamEvent, DirectError>> {
        self.receiver.recv().await
    }
}

impl Stream for DirectResponseStream {
    type Item = Result<ResponseStreamEvent, DirectError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}

/// Dispatch decoded SSE items to the bounded stream channel.
///
/// Returns `true` when dispatching must stop: the `[DONE]` terminal was seen,
/// the receiver was dropped, or — fail-stop, matching the platform decoder's
/// D0194 posture — a `data` frame did not decode into a typed event, in which
/// case the codec error is surfaced once and everything after it (including
/// later valid frames) is discarded.
async fn dispatch_sse_items(
    sender: &mpsc::Sender<Result<ResponseStreamEvent, DirectError>>,
    items: Vec<SseItem>,
) -> bool {
    for item in items {
        match item {
            SseItem::Done => return true,
            SseItem::Data(data) => {
                let event = match serde_json::from_str(&data) {
                    Ok(event) => event,
                    Err(error) => {
                        let _ = sender.send(Err(DirectError::Json(error))).await;
                        return true;
                    }
                };
                if sender.send(Ok(event)).await.is_err() {
                    return true;
                }
            }
        }
    }
    false
}

fn validate_body(body: &Value, streaming: bool) -> Result<(), DirectError> {
    let object = body.as_object().ok_or_else(|| {
        DirectError::Configuration("Responses request did not serialize to an object".to_owned())
    })?;
    if object.contains_key("max_output_tokens") {
        return Err(DirectError::UnsupportedRequestField("max_output_tokens"));
    }
    if object.get("background") == Some(&Value::Bool(true)) {
        return Err(DirectError::UnsupportedRequestField("background"));
    }
    match (streaming, object.get("stream")) {
        (true, Some(Value::Bool(true))) | (false, None | Some(Value::Bool(false))) => Ok(()),
        _ => Err(DirectError::Configuration(
            "Responses stream typestate did not match serialized body".to_owned(),
        )),
    }
}

async fn decode_json_response(response: reqwest::Response) -> Result<Response, DirectError> {
    if response.status().is_redirection() {
        return Err(DirectError::RedirectRejected);
    }
    if !response.status().is_success() {
        return Err(status_error(response).await);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_JSON_RESPONSE_BYTES as u64)
    {
        return Err(DirectError::BodyTooLarge);
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > MAX_JSON_RESPONSE_BYTES {
            return Err(DirectError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(DirectError::Json)
}

/// Keep a server-controlled `error.code` only when it is a short, inert
/// token: the string ends up in [`DirectError::HttpStatus`]'s display, so it
/// must not smuggle control bytes or unbounded prose.
fn sanitize_error_code(code: &str) -> Option<String> {
    if !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Some(code.to_owned())
    } else {
        None
    }
}

/// The same sanitizing discipline applied to the `error.message` fallback:
/// control characters (including newlines) are neutralized, surrounding
/// whitespace is trimmed, and the prose is truncated to a bounded length.
fn sanitize_error_message(message: &str) -> Option<String> {
    let flattened: String = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = flattened.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut sanitized: String = trimmed.chars().take(MAX_ERROR_MESSAGE_CHARS).collect();
    if sanitized.chars().count() < trimmed.chars().count() {
        sanitized.push_str("...[truncated]");
    }
    Some(sanitized)
}

async fn status_error(response: reqwest::Response) -> DirectError {
    let status = response.status().as_u16();
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) if body.len().saturating_add(chunk.len()) <= MAX_ERROR_BYTES => {
                body.extend_from_slice(&chunk);
            }
            _ => break,
        }
    }
    // Prefer the machine-readable `error.code`; when it is absent or not an
    // inert token, fall back to the sanitized `error.message`. The ChatGPT
    // Codex backend often returns FastAPI-style `{"detail":"..."}` instead.
    let parsed = serde_json::from_slice::<Value>(&body).ok();
    let from_error = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| {
            error
                .get("code")
                .and_then(Value::as_str)
                .and_then(sanitize_error_code)
                .or_else(|| {
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(sanitize_error_message)
                })
        });
    let from_detail = parsed.as_ref().and_then(|value| match value.get("detail") {
        Some(Value::String(detail)) => sanitize_error_message(detail),
        Some(Value::Array(items)) => items.iter().find_map(|item| {
            item.as_str()
                .or_else(|| item.get("msg").and_then(Value::as_str))
                .and_then(sanitize_error_message)
        }),
        _ => None,
    });
    let message = from_error
        .or(from_detail)
        .unwrap_or_else(|| "request failed".to_owned());
    DirectError::HttpStatus { status, message }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use openai_rs_types::responses::{CreateResponseRequest, ResponseInput, ResponseStreamEvent};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    use super::DirectCodexResponsesClient;
    use crate::direct::DirectError;
    use crate::direct::auth::{
        CredentialStore, DirectAuthClient, EphemeralStore, StoredCodexSession, TokenManager,
    };
    use crate::direct::jwt::ChatGptAccountId;
    use crate::direct::sse::SseItem;

    /// Serve exactly one HTTP response carrying `status`/`body` and return the
    /// `DirectError` the sealed transport produced for it.
    async fn status_error_round(
        status: u16,
        body: &'static str,
    ) -> Result<DirectError, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let headers = format!(
                "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body.as_bytes()).await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        let error = client
            .create(&request)
            .await
            .err()
            .ok_or("expected an HTTP status error")?;
        server.await??;
        Ok(error)
    }

    /// 4-40: `error.code` stays preferred, a missing code falls back to the
    /// sanitized `error.message`, and neither field keeps the neutral
    /// placeholder instead of the server's explanation.
    #[tokio::test]
    async fn status_error_prefers_code_and_falls_back_to_sanitized_message()
    -> Result<(), Box<dyn std::error::Error>> {
        match status_error_round(
            500,
            r#"{"error":{"code":"usage_limit","message":"ignored prose"}}"#,
        )
        .await?
        {
            DirectError::HttpStatus { status, message } => {
                assert_eq!(status, 500);
                assert_eq!(message, "usage_limit");
            }
            other => return Err(format!("unexpected error: {other:?}").into()),
        }
        match status_error_round(429, r#"{"error":{"message":"quota exceeded for team"}}"#).await? {
            DirectError::HttpStatus { status, message } => {
                assert_eq!(status, 429);
                assert_eq!(message, "quota exceeded for team");
            }
            other => return Err(format!("unexpected error: {other:?}").into()),
        }
        match status_error_round(502, r#"{"error":{"message":"line1\nline2\u0007"}}"#).await? {
            DirectError::HttpStatus { status, message } => {
                assert_eq!(status, 502);
                assert_eq!(message, "line1 line2");
            }
            other => return Err(format!("unexpected error: {other:?}").into()),
        }
        match status_error_round(403, r#"{"error":{}}"#).await? {
            DirectError::HttpStatus { status, message } => {
                assert_eq!(status, 403);
                assert_eq!(message, "request failed");
            }
            other => return Err(format!("unexpected error: {other:?}").into()),
        }
        match status_error_round(400, r#"{"detail":"Instructions are not valid"}"#).await? {
            DirectError::HttpStatus { status, message } => {
                assert_eq!(status, 400);
                assert_eq!(message, "Instructions are not valid");
            }
            other => return Err(format!("unexpected error: {other:?}").into()),
        }
        Ok(())
    }

    #[tokio::test]
    async fn sealed_transport_sets_headers_and_rejects_redirects()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut buffer = vec![0_u8; 16 * 1024];
            let read = stream.read(&mut buffer).await?;
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            stream
                .write_all(
                    b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://example.com/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            Ok::<_, std::io::Error>(request)
        });
        let store = Arc::new(EphemeralStore::default());
        let session = StoredCodexSession::fixture(
            "access-secret",
            "refresh-secret",
            u64::MAX,
            ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&session).await?;
        let manager = Arc::new(TokenManager::new(store, DirectAuthClient::new()?));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        assert!(client.create(&request).await.is_err());
        let captured = server.await??;
        let lower = captured.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer access-secret"));
        assert!(lower.contains("chatgpt-account-id: acct-123"));
        assert!(lower.contains(&format!("originator: {}", super::super::CODEX_ORIGINATOR)));
        assert!(!captured.contains("refresh-secret"));
        Ok(())
    }

    /// 7-03: the request-timeout knob defaults to the documented two-lane
    /// posture (600s total budget, 32 MiB decoder limits) and validates its
    /// inputs.
    #[test]
    fn timeout_and_sse_limit_knobs_have_defaults_and_validation()
    -> Result<(), Box<dyn std::error::Error>> {
        let manager = Arc::new(TokenManager::new(
            Arc::new(EphemeralStore::default()),
            DirectAuthClient::new()?,
        ));
        let endpoint = url::Url::parse("http://127.0.0.1:1/backend-api/codex/responses")?;
        let client =
            DirectCodexResponsesClient::with_test_endpoint(manager.clone(), endpoint.clone())?;
        assert_eq!(client.request_timeout(), Duration::from_secs(600));
        assert_eq!(client.max_sse_line_bytes(), 32 * 1024 * 1024);
        assert_eq!(client.max_sse_event_bytes(), 32 * 1024 * 1024);

        let tuned = client
            .with_request_timeout(Duration::from_secs(30))
            .with_sse_limits(1024, 2048)?;
        assert_eq!(tuned.request_timeout(), Duration::from_secs(30));
        assert_eq!(tuned.max_sse_line_bytes(), 1024);
        assert_eq!(tuned.max_sse_event_bytes(), 2048);

        assert!(matches!(
            DirectCodexResponsesClient::with_test_endpoint(manager.clone(), endpoint.clone())?
                .with_sse_limits(0, 2048),
            Err(DirectError::Configuration(_))
        ));
        assert!(matches!(
            DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?
                .with_sse_limits(1024, 0),
            Err(DirectError::Configuration(_))
        ));
        Ok(())
    }

    /// 7-03: the total budget applies to the non-streaming lane — a server
    /// slower than `request_timeout` fails with a timeout error instead of
    /// hanging until the platform-wide ceiling.
    #[tokio::test]
    async fn non_streaming_request_times_out_under_its_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            tokio::time::sleep(Duration::from_millis(750)).await;
            // The client has already timed out; a failed write to the closed
            // socket is expected and irrelevant here.
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )
            .await;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint)
            .await?
            .with_request_timeout(Duration::from_millis(150));
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        let error = client
            .create(&request)
            .await
            .err()
            .ok_or("expected a timeout error")?;
        assert!(
            matches!(&error, DirectError::Http(error) if error.is_timeout()),
            "unexpected error: {error:?}"
        );
        server.await??;
        Ok(())
    }

    /// 7-03: the streaming lane carries no total budget — events keep
    /// arriving well past the point where the non-streaming budget would
    /// have expired, and the stream still terminates cleanly.
    #[tokio::test]
    async fn streaming_body_outlives_the_request_timeout_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        const EVENT_GAP: Duration = Duration::from_millis(100);
        const EVENTS: usize = 4;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await?;
            for index in 0..EVENTS {
                let frame = format!(
                    "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"{index}\",\"sequence_number\":{index},\"logprobs\":[]}}\n\n"
                );
                stream.write_all(frame.as_bytes()).await?;
                tokio::time::sleep(EVENT_GAP).await;
            }
            stream.write_all(b"data: [DONE]\n\n").await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        // Tighter than the total streaming duration: the last event lands at
        // ~4x the budget, so a lane that still applied the budget would fail
        // the stream mid-body.
        let client = test_client(endpoint)
            .await?
            .with_request_timeout(EVENT_GAP + Duration::from_millis(50));
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        let mut deltas = Vec::new();
        while let Some(event) = stream.next_event().await {
            match event? {
                ResponseStreamEvent::OutputTextDelta(delta) => {
                    deltas.push(delta.delta().to_owned())
                }
                other => return Err(format!("unexpected SSE event: {other:?}").into()),
            }
        }
        let expected = (0..EVENTS)
            .map(|index| index.to_string())
            .collect::<Vec<_>>();
        assert_eq!(deltas, expected);
        server.await??;
        Ok(())
    }

    /// 7-07(d): dispatch is fail-stop — an undecodable `data` frame surfaces
    /// its codec error once and discards every later item, including valid
    /// ones.
    #[tokio::test]
    async fn dispatch_stops_after_an_undecodable_frame() {
        let (sender, mut receiver) = mpsc::channel(8);
        let valid = "{\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\",\"sequence_number\":1,\"logprobs\":[]}";
        let items = vec![
            SseItem::Data("not json".to_owned()),
            SseItem::Data(valid.to_owned()),
            SseItem::Done,
        ];
        assert!(super::dispatch_sse_items(&sender, items).await);
        drop(sender);
        assert!(matches!(
            receiver.recv().await,
            Some(Err(DirectError::Json(_)))
        ));
        assert!(receiver.recv().await.is_none());
    }

    /// 7-07(d): the same fail-stop posture end to end — after an invalid
    /// frame the stream yields exactly one error and then ends, even though
    /// later frames and `[DONE]` were well-formed.
    #[tokio::test]
    async fn stream_fails_stop_after_an_invalid_frame() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let body = b"data: not-json\n\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\",\"sequence_number\":1,\"logprobs\":[]}\n\ndata: [DONE]\n\n";
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body).await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        assert!(matches!(
            stream.next_event().await,
            Some(Err(DirectError::Json(_)))
        ));
        assert!(stream.next_event().await.is_none());
        server.await??;
        Ok(())
    }

    async fn test_client(
        endpoint: url::Url,
    ) -> Result<DirectCodexResponsesClient<EphemeralStore>, Box<dyn std::error::Error>> {
        let store = Arc::new(EphemeralStore::default());
        let session = StoredCodexSession::fixture(
            "access-secret",
            "refresh-secret",
            u64::MAX,
            ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&session).await?;
        let manager = Arc::new(TokenManager::new(store, DirectAuthClient::new()?));
        Ok(DirectCodexResponsesClient::with_test_endpoint(
            manager, endpoint,
        )?)
    }

    #[tokio::test]
    async fn typed_create_decodes_response() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let body = br#"{"id":"resp_1","created_at":1,"error":null,"incomplete_details":null,"instructions":null,"metadata":null,"model":"gpt-test","object":"response","output":[],"parallel_tool_calls":true,"temperature":null,"tool_choice":"auto","tools":[],"top_p":null}"#;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body).await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        assert_eq!(client.create(&request).await?.id(), "resp_1");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn typed_stream_decodes_sse() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let body = b"data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\",\"sequence_number\":1,\"logprobs\":[]}\n\ndata: {\"type\":\"response.completed\",\"sequence_number\":2,\"response\":{\"id\":\"resp_stream\",\"created_at\":1,\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":null,\"model\":\"gpt-test\",\"object\":\"response\",\"output\":[],\"parallel_tool_calls\":true,\"temperature\":null,\"tool_choice\":\"auto\",\"tools\":[],\"top_p\":null}}\n\ndata: [DONE]\n\n";
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: Text/Event-Stream; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body).await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        let event = stream.next_event().await.ok_or("missing SSE event")??;
        match event {
            ResponseStreamEvent::OutputTextDelta(delta) => assert_eq!(delta.delta(), "Hi"),
            other => return Err(format!("unexpected SSE event: {other:?}").into()),
        }
        // 17-J-4: a terminal-shape lifecycle event reuses the platform
        // codec's `Response` payload verbatim.
        let terminal = stream
            .next_event()
            .await
            .ok_or("missing terminal event")??;
        match terminal {
            ResponseStreamEvent::Completed(completed) => {
                assert_eq!(completed.sequence_number(), 2);
                assert_eq!(completed.response().id(), "resp_stream");
            }
            other => return Err(format!("unexpected terminal event: {other:?}").into()),
        }
        assert!(stream.next_event().await.is_none());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn typed_stream_decodes_sse_without_content_type()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let body = b"data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\",\"sequence_number\":1,\"logprobs\":[]}\n\ndata: [DONE]\n\n";
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body).await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        let event = stream.next_event().await.ok_or("missing SSE event")??;
        match event {
            ResponseStreamEvent::OutputTextDelta(delta) => assert_eq!(delta.delta(), "Hi"),
            other => return Err(format!("unexpected SSE event: {other:?}").into()),
        }
        assert!(stream.next_event().await.is_none());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn headerless_non_sse_body_fails_in_stream() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        let error = stream
            .next_event()
            .await
            .ok_or("missing stream error")?
            .err()
            .ok_or("expected an SSE error")?;
        assert!(matches!(
            error,
            DirectError::Sse(message) if message.contains("did not contain SSE events")
        ));
        assert!(stream.next_event().await.is_none());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn typed_stream_rejects_explicit_non_sse_content_type()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .await?;
            Ok::<_, std::io::Error>(())
        });
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = test_client(endpoint).await?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let error = client
            .stream(&request)
            .await
            .err()
            .ok_or("expected a content type error")?;
        assert!(matches!(
            error,
            DirectError::Sse(message)
                if message.contains("application/json")
                    && message.contains("text/event-stream")
        ));
        server.await??;
        Ok(())
    }

    /// 8-22: the sealed-backend body guard rejects `max_output_tokens` and a
    /// `background: true` request before any network I/O (the backend
    /// supports neither), keeps an explicit `background: false` legal, and
    /// polices the streaming typestate against the serialized `stream` key.
    #[test]
    fn validate_body_rejects_unsupported_fields_and_mismatched_stream_state() {
        use serde_json::json;

        assert!(matches!(
            super::validate_body(&json!({"model":"gpt-test","max_output_tokens":128}), false),
            Err(DirectError::UnsupportedRequestField("max_output_tokens"))
        ));
        assert!(matches!(
            super::validate_body(&json!({"model":"gpt-test","background":true}), false),
            Err(DirectError::UnsupportedRequestField("background"))
        ));
        assert!(
            super::validate_body(&json!({"model":"gpt-test","background":false}), false).is_ok(),
            "an explicit background: false stays legal"
        );

        assert!(matches!(
            super::validate_body(&serde_json::Value::Null, false),
            Err(DirectError::Configuration(_))
        ));

        assert!(matches!(
            super::validate_body(&json!({"model":"gpt-test","stream":true}), false),
            Err(DirectError::Configuration(_))
        ));
        assert!(matches!(
            super::validate_body(&json!({"model":"gpt-test"}), true),
            Err(DirectError::Configuration(_))
        ));
        assert!(
            super::validate_body(&json!({"model":"gpt-test","stream":true}), true).is_ok(),
            "the streaming body matches the streaming typestate"
        );
        for absent_or_false in [
            json!({"model":"gpt-test"}),
            json!({"model":"gpt-test","stream":false}),
        ] {
            assert!(super::validate_body(&absent_or_false, false).is_ok());
        }
    }

    /// 17-J-1: the streaming-lane 401-recovery — same scripted loopback shape
    /// as the create-lane test, but the retried request is a `stream` call and
    /// the success is a `text/event-stream` body whose events must all arrive
    /// through the refreshed token's connection.
    #[tokio::test]
    async fn stream_retries_once_with_a_refreshed_token_after_a_401()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut backend_authorizations = Vec::new();
            for round in 0..3 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = vec![0_u8; 16 * 1024];
                let _ = stream.readable().await;
                let read = stream.try_read(&mut request).unwrap_or(0);
                let captured = String::from_utf8_lossy(&request[..read]).to_string();
                let authorization = captured
                    .to_ascii_lowercase()
                    .lines()
                    .find(|line| line.starts_with("authorization:"))
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                if captured.starts_with("POST /oauth/token") {
                    let body = br#"{"access_token":"access-refreshed","expires_in":3600}"#;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(headers.as_bytes()).await?;
                    stream.write_all(body).await?;
                } else if round == 0 {
                    backend_authorizations.push(authorization);
                    stream
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 42\r\nConnection: close\r\n\r\n{\"error\":{\"code\":\"token_expired\",\"message\":\"x\"}}",
                        )
                        .await?;
                } else {
                    backend_authorizations.push(authorization);
                    let accept_stream = captured.contains("accept: text/event-stream");
                    let body = b"data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"refreshed\",\"sequence_number\":1,\"logprobs\":[]}\n\ndata: [DONE]\n\n";
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    assert!(accept_stream, "the retry must stay a streaming request");
                    stream.write_all(headers.as_bytes()).await?;
                    stream.write_all(body).await?;
                }
            }
            Ok::<_, std::io::Error>(backend_authorizations)
        });

        let store = Arc::new(EphemeralStore::default());
        store
            .save(&StoredCodexSession::fixture(
                "access-secret",
                "refresh-secret",
                u64::MAX,
                ChatGptAccountId::fixture("acct-123")?,
            ))
            .await?;
        let auth = DirectAuthClient::with_test_token_endpoint(url::Url::parse(&format!(
            "http://{address}/oauth/token"
        ))?)?;
        let manager = Arc::new(TokenManager::new(store, auth));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let mut stream = client.stream(&request).await?;
        let event = stream.next_event().await.ok_or("missing SSE event")??;
        match event {
            ResponseStreamEvent::OutputTextDelta(delta) => {
                assert_eq!(delta.delta(), "refreshed")
            }
            other => return Err(format!("unexpected SSE event: {other:?}").into()),
        }
        assert!(stream.next_event().await.is_none());

        let authorizations = server.await??;
        assert_eq!(authorizations.len(), 2, "exactly one retry after the 401");
        assert!(
            authorizations[0].contains("bearer access-secret"),
            "the failed request carried the cached token: {}",
            authorizations[0]
        );
        assert!(
            authorizations[1].contains("bearer access-refreshed"),
            "the retry carried the refreshed token: {}",
            authorizations[1]
        );
        Ok(())
    }

    /// 17-J-2: a second 401 after the refresh is terminal — the client made
    /// exactly two backend attempts (cached token, refreshed token) and no
    /// third refresh/retry cycle, surfacing the 401 as the final error.
    #[tokio::test]
    async fn a_second_401_after_the_refresh_surfaces_as_a_terminal_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut backend_authorizations = Vec::new();
            let mut refreshes = 0_usize;
            // Three connections total: 401, token refresh, 401 again. A
            // fourth (a third backend attempt) would never be accepted, so
            // the loop simply stops — the counts below prove it never came.
            for round in 0..3 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = vec![0_u8; 16 * 1024];
                let _ = stream.readable().await;
                let read = stream.try_read(&mut request).unwrap_or(0);
                let captured = String::from_utf8_lossy(&request[..read]).to_string();
                let authorization = captured
                    .to_ascii_lowercase()
                    .lines()
                    .find(|line| line.starts_with("authorization:"))
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                if captured.starts_with("POST /oauth/token") {
                    refreshes += 1;
                    let body = br#"{"access_token":"access-refreshed","expires_in":3600}"#;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(headers.as_bytes()).await?;
                    stream.write_all(body).await?;
                } else {
                    backend_authorizations.push(authorization);
                    stream
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 42\r\nConnection: close\r\n\r\n{\"error\":{\"code\":\"token_expired\",\"message\":\"x\"}}",
                        )
                        .await?;
                    let _ = round;
                }
            }
            Ok::<_, std::io::Error>((backend_authorizations, refreshes))
        });

        let store = Arc::new(EphemeralStore::default());
        store
            .save(&StoredCodexSession::fixture(
                "access-secret",
                "refresh-secret",
                u64::MAX,
                ChatGptAccountId::fixture("acct-123")?,
            ))
            .await?;
        let auth = DirectAuthClient::with_test_token_endpoint(url::Url::parse(&format!(
            "http://{address}/oauth/token"
        ))?)?;
        let manager = Arc::new(TokenManager::new(store, auth));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()))
            .into_streaming();
        let error = client
            .stream(&request)
            .await
            .err()
            .ok_or("the second 401 must surface as a terminal error")?;
        assert!(
            matches!(&error, DirectError::HttpStatus { status: 401, .. }),
            "unexpected error: {error:?}"
        );

        let (authorizations, refreshes) = server.await??;
        assert_eq!(refreshes, 1, "exactly one refresh cycle");
        assert_eq!(
            authorizations.len(),
            2,
            "exactly two backend attempts, no third try"
        );
        assert!(
            authorizations[1].contains("bearer access-refreshed"),
            "the second attempt carried the refreshed token: {}",
            authorizations[1]
        );
        Ok(())
    }

    /// 8-22: the 401-recovery lane end to end — the first request fails with
    /// 401 carrying the cached access token, the client refreshes against
    /// the token endpoint, and the retry succeeds with the new token, all on
    /// one scripted loopback origin.
    #[tokio::test]
    async fn create_retries_once_with_a_refreshed_token_after_a_401()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut backend_authorizations = Vec::new();
            let mut refresh_body = String::new();
            for round in 0..3 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = vec![0_u8; 16 * 1024];
                let _ = stream.readable().await;
                let read = stream.try_read(&mut request).unwrap_or(0);
                let captured = String::from_utf8_lossy(&request[..read]).to_string();
                let authorization = captured
                    .to_ascii_lowercase()
                    .lines()
                    .find(|line| line.starts_with("authorization:"))
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                let body_start = captured.find("\r\n\r\n").map(|index| index + 4);
                if captured.starts_with("POST /oauth/token") {
                    refresh_body = body_start
                        .map(|start| captured[start..].to_owned())
                        .unwrap_or_default();
                    let body = br#"{"access_token":"access-refreshed","expires_in":3600}"#;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(headers.as_bytes()).await?;
                    stream.write_all(body).await?;
                } else if round == 0 {
                    backend_authorizations.push(authorization);
                    stream
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 42\r\nConnection: close\r\n\r\n{\"error\":{\"code\":\"token_expired\",\"message\":\"x\"}}",
                        )
                        .await?;
                } else {
                    backend_authorizations.push(authorization);
                    let body = br#"{"id":"resp_after_refresh","created_at":1,"error":null,"incomplete_details":null,"instructions":null,"metadata":null,"model":"gpt-test","object":"response","output":[],"parallel_tool_calls":true,"temperature":null,"tool_choice":"auto","tools":[],"top_p":null}"#;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(headers.as_bytes()).await?;
                    stream.write_all(body).await?;
                }
            }
            Ok::<_, std::io::Error>((backend_authorizations, refresh_body))
        });

        let store = Arc::new(EphemeralStore::default());
        store
            .save(&StoredCodexSession::fixture(
                "access-secret",
                "refresh-secret",
                u64::MAX,
                ChatGptAccountId::fixture("acct-123")?,
            ))
            .await?;
        let auth = DirectAuthClient::with_test_token_endpoint(url::Url::parse(&format!(
            "http://{address}/oauth/token"
        ))?)?;
        let manager = Arc::new(TokenManager::new(store, auth));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        assert_eq!(client.create(&request).await?.id(), "resp_after_refresh");

        let (authorizations, refresh_body) = server.await??;
        assert_eq!(authorizations.len(), 2, "exactly one retry after the 401");
        assert!(
            authorizations[0].contains("bearer access-secret"),
            "the failed request carried the cached token: {}",
            authorizations[0]
        );
        assert!(
            authorizations[1].contains("bearer access-refreshed"),
            "the retry carried the refreshed token: {}",
            authorizations[1]
        );
        assert!(refresh_body.contains("grant_type=refresh_token"));
        assert!(
            refresh_body.contains("refresh_token=refresh-secret"),
            "the refresh posted the stored refresh token"
        );
        Ok(())
    }
}
