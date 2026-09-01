//! Client resources for the preview Responses multi-agent API.

use std::{
    collections::HashSet,
    fmt,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;
use futures_util::{SinkExt, StreamExt};
use http::{HeaderName, HeaderValue, Method, StatusCode, header};
use openai_rs_types::{
    ResponseId,
    beta_responses::{
        BetaCompactResponseRequest, BetaCompactedResponse, BetaCountInputTokensRequest,
        BetaCreateResponseRequest, BetaCreateStreamingResponseRequest, BetaInputTokenCountResponse,
        BetaListInputItemsParams, BetaResponse, BetaResponseInjectEvent, BetaResponseItemList,
        BetaResponseStreamEvent, BetaResponsesClientEvent, BetaResponsesCreateEvent,
        BetaResponsesServerEvent, BetaRetrieveResponseParams, BetaRetrieveResponseStreamParams,
    },
    responses::{DeletedResponse, ResponseAccumulator},
};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig as TungsteniteConfig},
};
use url::Url;

use crate::{
    ApiResponse, BodyPreview, Client, Error, PollError, PollOptions, ResponseMeta, StreamError,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    responses::DeleteResponseResult,
    responses_websocket::{
        connect_socket, derive_websocket_url, is_unauthorized_websocket_error, map_websocket_error,
        retryable_connect_error, validate_stream_id, websocket_connector, websocket_request,
    },
    sse::{SseDispatch, SseEndpointPolicy, SseFrame, SseLimits, SseStreamDecoder, SseStreamState},
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];
const OK_OR_NO_CONTENT: &[StatusCode] = &[StatusCode::OK, StatusCode::NO_CONTENT];
const BETA_HEADER: &str = "OpenAI-Beta";
const BETA_VALUE: &str = "responses_multi_agent=v1";

/// Sends the static `OpenAI-Beta: responses_multi_agent=v1` header with a
/// JSON-decoded beta operation.
///
/// The pinned beta routes declare the optional `openai-beta` header (a single
/// enum value, `responses_multi_agent=v1`) and openai-python/openai-node send
/// it whenever the multi-agent preview is addressed, so every JSON face of
/// the beta Responses REST surface attaches it here (the D0087 Vector Store
/// and ChatKit pattern). The empty-or-JSON delete lane and both SSE lanes
/// carry the header through their own static-header transport entries; the
/// WebSocket face stays header-free (the preview is REST-only in the
/// official SDKs).
async fn execute_beta_json<O, Q>(
    client: &Client,
    path: &[PathSegment<'_>],
    query: Option<&Q>,
    body: Option<&O::Request>,
) -> Result<ApiResponse<O::Response>, Error>
where
    O: Operation,
    Q: Serialize + ?Sized,
{
    client
        .transport()
        .execute_json_with_static_header::<O, Q>(path, query, body, BETA_HEADER, BETA_VALUE)
        .await
}

/// A stream of bounded beta Response input item collection pages.
pub type BetaResponseInputItemPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<BetaResponseItemList>, Error>> + Send + 'static>>;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WRITE_BUFFER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_QUEUED_WRITE_BYTES: usize = 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_INITIAL_RECONNECTS: u32 = 10;
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

type BetaEventStream =
    Pin<Box<dyn Stream<Item = Result<BetaResponseStreamEvent, Error>> + Send + 'static>>;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Copy, Serialize)]
struct BetaOnlyQuery {
    beta: bool,
}

impl BetaOnlyQuery {
    const VALUE: Self = Self { beta: true };
}

#[derive(Debug, Serialize)]
struct BetaQuery<'a, Q> {
    beta: bool,
    #[serde(flatten)]
    query: &'a Q,
}

impl<'a, Q> BetaQuery<'a, Q> {
    const fn new(query: &'a Q) -> Self {
        Self { beta: true, query }
    }
}

/// Preview Responses resource methods.
#[derive(Clone, Debug)]
pub struct BetaResponses {
    client: Client,
}

impl BetaResponses {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a non-streaming beta response.
    pub async fn create(
        &self,
        request: BetaCreateResponseRequest,
    ) -> Result<ApiResponse<BetaResponse>, Error> {
        let path = [PathSegment::literal("responses")];
        execute_beta_json::<CreateBetaResponse, _>(
            &self.client,
            &path,
            Some(&BetaOnlyQuery::VALUE),
            Some(&request),
        )
        .await
    }

    /// Creates a beta response and decodes its SSE events incrementally.
    ///
    /// The SSE lane carries the static beta header alongside the `beta=true`
    /// query (see `execute_beta_json`).
    ///
    /// Once the SSE handshake succeeds, transport errors (including
    /// mid-stream timeouts) are terminal: the stream yields the error and
    /// ends, and no automatic retry happens. Re-issue the request to
    /// recover (D0244).
    pub async fn create_stream(
        &self,
        request: BetaCreateStreamingResponseRequest,
    ) -> Result<BetaResponseEventStream, Error> {
        let path = [PathSegment::literal("responses")];
        let response = self
            .client
            .transport()
            .send_with_static_header::<CreateStreamingBetaResponse, _>(
                &path,
                Some(&BetaOnlyQuery::VALUE),
                Some(&request),
                Some((BETA_HEADER, BETA_VALUE)),
            )
            .await?;
        BetaResponseEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Retrieves a stored beta response.
    pub async fn retrieve(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<BetaResponse>, Error> {
        self.retrieve_with(response_id, BetaRetrieveResponseParams::new())
            .await
    }

    /// Retrieves a stored beta response with explicit expansions.
    pub async fn retrieve_with(
        &self,
        response_id: &ResponseId,
        params: BetaRetrieveResponseParams,
    ) -> Result<ApiResponse<BetaResponse>, Error> {
        let path = response_path(response_id)?;
        let query = BetaQuery::new(&params);
        execute_beta_json::<RetrieveBetaResponse, _>(&self.client, &path, Some(&query), None).await
    }

    /// Retrieves or resumes a stored beta response SSE stream.
    ///
    /// The SSE lane carries the static beta header alongside the `beta=true`
    /// query (see `execute_beta_json`).
    ///
    /// Once the SSE handshake succeeds, transport errors (including
    /// mid-stream timeouts) are terminal: the stream yields the error and
    /// ends, and no automatic retry happens. Re-issue the request to
    /// recover (D0244).
    pub async fn retrieve_stream(
        &self,
        response_id: &ResponseId,
        params: BetaRetrieveResponseStreamParams,
    ) -> Result<BetaResponseEventStream, Error> {
        let path = response_path(response_id)?;
        let query = BetaQuery::new(&params);
        let response = self
            .client
            .transport()
            .send_with_static_header::<RetrieveStreamingBetaResponse, _>(
                &path,
                Some(&query),
                None,
                Some((BETA_HEADER, BETA_VALUE)),
            )
            .await?;
        BetaResponseEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Deletes a stored beta response.
    ///
    /// The empty-or-JSON lane also carries the static beta header (see
    /// `execute_beta_json`).
    pub async fn delete(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<DeleteResponseResult>, Error> {
        let path = response_path(response_id)?;
        let response = self
            .client
            .transport()
            .execute_optional_json_with_static_header::<DeleteBetaResponse, _>(
                &path,
                Some(&BetaOnlyQuery::VALUE),
                None,
                BETA_HEADER,
                BETA_VALUE,
            )
            .await?;
        let (body, meta) = response.into_parts();
        Ok(ApiResponse::new(
            body.map_or(DeleteResponseResult::Empty, DeleteResponseResult::Deleted),
            meta,
        ))
    }

    /// Cancels a background beta response.
    pub async fn cancel(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<BetaResponse>, Error> {
        let path = [
            PathSegment::literal("responses"),
            response_id_segment(response_id)?,
            PathSegment::literal("cancel"),
        ];
        execute_beta_json::<CancelBetaResponse, _>(
            &self.client,
            &path,
            Some(&BetaOnlyQuery::VALUE),
            None,
        )
        .await
    }

    /// Compacts beta response context.
    pub async fn compact(
        &self,
        request: BetaCompactResponseRequest,
    ) -> Result<ApiResponse<BetaCompactedResponse>, Error> {
        let path = [
            PathSegment::literal("responses"),
            PathSegment::literal("compact"),
        ];
        execute_beta_json::<CompactBetaResponse, _>(
            &self.client,
            &path,
            Some(&BetaOnlyQuery::VALUE),
            Some(&request),
        )
        .await
    }

    /// Returns the beta input-items subresource.
    #[must_use]
    pub fn input_items(&self) -> BetaResponseInputItems {
        BetaResponseInputItems {
            client: self.client.clone(),
        }
    }

    /// Returns the beta input-token counting subresource.
    #[must_use]
    pub fn input_tokens(&self) -> BetaResponseInputTokens {
        BetaResponseInputTokens {
            client: self.client.clone(),
        }
    }

    /// Convenience alias for `beta_responses().input_items().list_pages(...)`.
    #[must_use]
    pub fn list_input_item_pages(
        &self,
        response_id: &ResponseId,
        params: BetaListInputItemsParams,
    ) -> BetaResponseInputItemPageStream {
        self.input_items().list_pages(response_id, params)
    }

    /// Polls a background beta response until it reaches a terminal status.
    pub async fn poll(
        &self,
        response_id: &ResponseId,
        options: PollOptions,
    ) -> Result<ApiResponse<BetaResponse>, PollError> {
        crate::poll::poll_resource_with_status(
            || self.retrieve(response_id),
            |response| {
                matches!(
                    response.status(),
                    Some(
                        openai_rs_types::responses::ResponseStatus::Completed
                            | openai_rs_types::responses::ResponseStatus::Failed
                            | openai_rs_types::responses::ResponseStatus::Incomplete
                            | openai_rs_types::responses::ResponseStatus::Cancelled
                    )
                )
            },
            |response| {
                response
                    .status()
                    .map(|s| s.as_str().to_owned())
                    .unwrap_or_else(|| "unknown".into())
            },
            options,
        )
        .await
    }

    /// Opens a typed beta Responses WebSocket at the pinned `/responses` path.
    pub async fn connect(&self) -> Result<BetaResponsesWebSocket, Error> {
        self.connect_with(BetaResponsesWebSocketConfig::new()).await
    }

    /// Opens a typed beta Responses WebSocket with explicit limits.
    pub async fn connect_with(
        &self,
        config: BetaResponsesWebSocketConfig,
    ) -> Result<BetaResponsesWebSocket, Error> {
        BetaResponsesWebSocket::connect(&self.client, config).await
    }
}

/// Beta response input-item methods.
#[derive(Clone, Debug)]
pub struct BetaResponseInputItems {
    client: Client,
}

impl BetaResponseInputItems {
    /// Lists input items for a beta response.
    pub async fn list(
        &self,
        response_id: &ResponseId,
        params: BetaListInputItemsParams,
    ) -> Result<ApiResponse<BetaResponseItemList>, Error> {
        let path = [
            PathSegment::literal("responses"),
            response_id_segment(response_id)?,
            PathSegment::literal("input_items"),
        ];
        let query = BetaQuery::new(&params);
        execute_beta_json::<ListBetaResponseInputItems, _>(&self.client, &path, Some(&query), None)
            .await
    }

    /// Streams input item pages while rejecting a repeated or missing cursor.
    #[must_use]
    pub fn list_pages(
        &self,
        response_id: &ResponseId,
        params: BetaListInputItemsParams,
    ) -> BetaResponseInputItemPageStream {
        let items = self.clone();
        let response_id = response_id.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Some(cursor) = params.after_ref() {
                crate::pagination::seed_seen(&mut seen, Some(cursor));
            }
            loop {
                let page = items.list(&response_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id()),
                    // Beta response input items are a tagged union without a
                    // shared id accessor, so no per-item fallback cursor is
                    // available.
                    None,
                    &mut seen,
                    "beta response input item",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(cursor),
                    None => break,
                }
            }
        })
    }
}

/// Beta response input-token methods.
#[derive(Clone, Debug)]
pub struct BetaResponseInputTokens {
    client: Client,
}

impl BetaResponseInputTokens {
    /// Counts tokens for a typed beta response input.
    pub async fn count(
        &self,
        request: BetaCountInputTokensRequest,
    ) -> Result<ApiResponse<BetaInputTokenCountResponse>, Error> {
        let path = [
            PathSegment::literal("responses"),
            PathSegment::literal("input_tokens"),
        ];
        execute_beta_json::<CountBetaResponseInputTokens, _>(
            &self.client,
            &path,
            Some(&BetaOnlyQuery::VALUE),
            Some(&request),
        )
        .await
    }
}

/// Beta Responses SSE stream with HTTP metadata.
pub struct BetaResponseEventStream {
    meta: ResponseMeta,
    inner: BetaEventStream,
}

impl BetaResponseEventStream {
    fn from_response(response: reqwest::Response, limits: SseLimits) -> Result<Self, Error> {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        }) {
            return Err(Error::UnexpectedContentType {
                expected: "text/event-stream",
                actual: content_type.map(Box::<str>::from),
                status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
            });
        }

        let stream_meta = meta.clone();
        let inner = async_stream::stream! {
            let mut chunks = Box::pin(response.bytes_stream());
            let mut decoder = SseStreamDecoder::new(limits, SseEndpointPolicy::responses());
            while let Some(chunk) = chunks.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(Error::from_response_body(error, &stream_meta));
                        return;
                    }
                };
                let dispatches = match decoder.push(&chunk) {
                    Ok(dispatches) => dispatches,
                    Err(source) => {
                        yield Err(Error::Sse {
                            source,
                            request_id: stream_meta.request_id().map(Box::<str>::from),
                        });
                        return;
                    }
                };
                for dispatch in dispatches {
                    match decode_beta_dispatch(dispatch, &stream_meta) {
                        Ok(Some(event)) if event.is_terminal() => {
                            yield Ok(event);
                            return;
                        }
                        Ok(Some(event)) => yield Ok(event),
                        Ok(None) => return,
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    }
                }
                if decoder.state() != SseStreamState::Active {
                    return;
                }
            }
            // 14-G-1: EOF-flushed dispatches survive a failing EOF. A
            // data-only terminal frame (no `event:` line, so the policy's
            // terminal table cannot match it) flushes at EOF as a plain
            // event; yield it before the UnexpectedEof instead of losing the
            // terminal payload under the error.
            let (dispatches, eof_error) = match decoder.finish_with_flushed() {
                Ok(dispatches) => (dispatches, None),
                Err((source, flushed)) => (
                    flushed,
                    Some(Error::Sse {
                        source,
                        request_id: stream_meta.request_id().map(Box::<str>::from),
                    }),
                ),
            };
            for dispatch in dispatches {
                match decode_beta_dispatch(dispatch, &stream_meta) {
                    Ok(Some(event)) if event.is_terminal() => {
                        yield Ok(event);
                        return;
                    }
                    Ok(Some(event)) => yield Ok(event),
                    Ok(None) => return,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }
            }
            if let Some(error) = eof_error {
                yield Err(error);
            }
        };
        Ok(Self {
            meta,
            inner: Box::pin(inner),
        })
    }

    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    /// Drains the stream and returns the last terminal response snapshot.
    pub async fn collect_final(mut self) -> Result<BetaResponse, Error> {
        let mut terminal = None;
        while let Some(event) = self.next().await {
            let event = event?;
            if event.is_terminal() {
                terminal = event.response().cloned();
            }
        }
        terminal.ok_or_else(|| Error::StreamProtocol {
            message: "beta Responses stream ended without a terminal response snapshot",
            request_id: self.meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(&[], false),
        })
    }

    /// Continues reduction with a caller-supplied accumulator (14-F-6),
    /// mirroring the GA `ResponseEventStream::collect_with`, which is useful
    /// after explicitly validated stream resumption.
    ///
    /// The accumulator consumes the stable core codec
    /// ([`BetaResponseStreamEvent::core`]), so the reduced value is the GA
    /// [`Response`](openai_rs_types::responses::Response); the beta-only
    /// overlays (agent routing, lane ids, and the beta response snapshot
    /// behind [`Self::collect_final`]) are not folded into it.
    pub async fn collect_with(
        mut self,
        mut accumulator: ResponseAccumulator,
    ) -> Result<openai_rs_types::responses::Response, Error> {
        while let Some(event) = self.next().await {
            accumulator.push(event?.core().clone())?;
        }
        accumulator.finish().map_err(Error::from)
    }
}

impl Stream for BetaResponseEventStream {
    type Item = Result<BetaResponseStreamEvent, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for BetaResponseEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BetaResponseEventStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// Explicit retry policy for only the initial beta WebSocket handshake.
///
/// An established connection is never automatically reconnected because
/// replaying a beta `response.create` event could duplicate model work.
///
/// [`BetaWebSocketReconnectPolicy::InitialConnect`] mirrors the GA
/// `WebSocketReconnectPolicy`: only handshake-time failures are replayed —
/// transport errors (I/O, TLS), handshake timeouts, and non-101 rejections
/// whose HTTP status is retryable on the REST face too (408, 429, and every
/// 5xx — 7-08). Any other rejection, such as a 401 after the single
/// credential refresh or a 404, surfaces from the attempt that produced it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BetaWebSocketReconnectPolicy {
    #[default]
    Never,
    /// Retries a failed initial handshake before surfacing its error.
    InitialConnect {
        /// Additional attempts *after* the first one, so the handshake is
        /// tried at most `1 + max_retries` times in total (capped at 10 by
        /// [`BetaResponsesWebSocketConfig`]'s validation).
        max_retries: u32,
        /// Fixed pause between attempts with no backoff: every pause lasts
        /// exactly `delay`, which callers are expected to keep small (the
        /// validation caps it at 60s).
        delay: Duration,
    },
}

/// Header names the beta WebSocket handshake always sets from authenticated
/// client state, so caller-supplied static headers may not override them
/// (14-F-2).
const PROTECTED_HANDSHAKE_HEADERS: [&str; 4] = [
    "authorization",
    "openai-organization",
    "openai-project",
    "x-client-request-id",
];

/// Bounded resource configuration for a beta Responses WebSocket.
///
/// [`Clone`]-but-not-`Copy` since 14-F-2: an opt-in static-header slot is
/// owned data, unlike the numeric limits around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BetaResponsesWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
    reconnect: BetaWebSocketReconnectPolicy,
    extra_static_headers: Vec<(HeaderName, HeaderValue)>,
}

impl BetaResponsesWebSocketConfig {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
            max_queued_write_bytes: DEFAULT_MAX_QUEUED_WRITE_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            reconnect: BetaWebSocketReconnectPolicy::Never,
            extra_static_headers: Vec::new(),
        }
    }

    #[must_use]
    pub const fn max_message_bytes(mut self, limit: usize) -> Self {
        self.max_message_bytes = limit;
        self
    }

    #[must_use]
    pub const fn max_frame_bytes(mut self, limit: usize) -> Self {
        self.max_frame_bytes = limit;
        self
    }

    #[must_use]
    pub const fn write_buffer_bytes(mut self, size: usize) -> Self {
        self.write_buffer_bytes = size;
        self
    }

    #[must_use]
    pub const fn max_queued_write_bytes(mut self, limit: usize) -> Self {
        self.max_queued_write_bytes = limit;
        self
    }

    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn reconnect_policy(mut self, policy: BetaWebSocketReconnectPolicy) -> Self {
        self.reconnect = policy;
        self
    }

    /// Sends `OpenAI-Beta: responses_multi_agent=v1` on the handshake
    /// (14-F-2), the preview-gate escape hatch the official examples attach
    /// manually because their WebSocket clients expose no typed slot for it
    /// (openai-python `examples/responses/multi_agent_websocket.py`,
    /// openai-node `examples/responses/multi-agent-websocket.ts`).
    ///
    /// Opt-in and off by default: the pinned beta WebSocket face is reachable
    /// without the header (D0210), so the default handshake stays
    /// header-free.
    #[must_use]
    pub fn with_beta_header(self) -> Self {
        self.extra_static_header(BETA_HEADER, BETA_VALUE)
            .expect("the pinned beta header name and value are valid")
    }

    /// Adds one static header to the WebSocket handshake (14-F-2), refusing
    /// the headers the handshake already sets from authenticated client
    /// state: `Authorization`, `OpenAI-Organization`, `OpenAI-Project`, and
    /// `X-Client-Request-Id`.
    ///
    /// The header is sent on every (re)connect attempt of this config, so it
    /// must be static per connection; per-request state belongs on the event
    /// stream instead.
    pub fn extra_static_header(
        mut self,
        name: &'static str,
        value: &'static str,
    ) -> Result<Self, Error> {
        let name = HeaderName::try_from(name).map_err(|error| {
            invalid_configuration(format!(
                "beta WebSocket static header name {name:?} is invalid: {error}"
            ))
        })?;
        let value = HeaderValue::try_from(value).map_err(|error| {
            invalid_configuration(format!(
                "beta WebSocket static header value for {name:?} is invalid: {error}"
            ))
        })?;
        if PROTECTED_HANDSHAKE_HEADERS
            .iter()
            .any(|protected| name.as_str() == *protected)
        {
            return Err(invalid_configuration(format!(
                "beta WebSocket static header {name:?} is managed by the client and cannot be overridden"
            )));
        }
        self.extra_static_headers.push((name, value));
        Ok(self)
    }

    fn validate(self) -> Result<Self, Error> {
        if self.max_message_bytes == 0 || self.max_frame_bytes == 0 {
            return Err(invalid_configuration(
                "beta WebSocket message and frame limits must be non-zero",
            ));
        }
        if self.max_queued_write_bytes <= self.write_buffer_bytes {
            return Err(invalid_configuration(
                "beta WebSocket queued-write limit must exceed write-buffer size",
            ));
        }
        if self.connect_timeout.is_zero() {
            return Err(invalid_configuration(
                "beta WebSocket connect timeout must be non-zero",
            ));
        }
        if let BetaWebSocketReconnectPolicy::InitialConnect { max_retries, delay } = self.reconnect
        {
            if max_retries > MAX_INITIAL_RECONNECTS {
                return Err(invalid_configuration(
                    "beta WebSocket initial reconnect count exceeds the supported limit",
                ));
            }
            if delay > MAX_RECONNECT_DELAY {
                return Err(invalid_configuration(
                    "beta WebSocket initial reconnect delay exceeds 60 seconds",
                ));
            }
        }
        Ok(self)
    }

    fn tungstenite(&self) -> TungsteniteConfig {
        TungsteniteConfig::default()
            .write_buffer_size(self.write_buffer_bytes)
            .max_write_buffer_size(self.max_queued_write_bytes)
            .max_message_size(Some(self.max_message_bytes))
            .max_frame_size(Some(self.max_frame_bytes))
    }
}

impl Default for BetaResponsesWebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Typed persistent connection to the beta Responses multi-agent protocol.
pub struct BetaResponsesWebSocket {
    socket: Socket,
    meta: ResponseMeta,
    max_message_bytes: usize,
    closed: bool,
    last_close: Option<(u16, String)>,
}

impl BetaResponsesWebSocket {
    async fn connect(client: &Client, config: BetaResponsesWebSocketConfig) -> Result<Self, Error> {
        let config = config.validate()?;
        let transport = client.transport();
        let url = beta_websocket_url(client.base_url())?;
        let connector = websocket_connector(url.scheme(), transport.tls_backend())?;
        let (max_retries, retry_delay) = match config.reconnect {
            BetaWebSocketReconnectPolicy::Never => (0, Duration::ZERO),
            BetaWebSocketReconnectPolicy::InitialConnect { max_retries, delay } => {
                (max_retries, delay)
            }
        };
        let mut retries = 0;
        let mut auth_refreshed = false;
        loop {
            let authorization = transport.authorization().await?;
            let generation = authorization.generation;
            let mut request = websocket_request(
                &url,
                authorization.header,
                transport.organization(),
                transport.project(),
                transport.client_request_id(),
            )?;
            // 14-F-2: opt-in static headers ride the shared handshake builder's
            // request without touching it; the protected set was already
            // refused by `extra_static_header`, so the client-managed headers
            // above cannot be overridden here.
            for (name, value) in &config.extra_static_headers {
                request.headers_mut().insert(name.clone(), value.clone());
            }
            let connect = connect_socket(request, config.tungstenite(), connector.clone());
            match tokio::time::timeout(config.connect_timeout, connect).await {
                Ok(Ok((socket, response))) => {
                    return Ok(Self {
                        socket,
                        meta: ResponseMeta::from_headers(response.status(), response.headers()),
                        max_message_bytes: config.max_message_bytes,
                        closed: false,
                        last_close: None,
                    });
                }
                Ok(Err(error))
                    if generation.is_some()
                        && !auth_refreshed
                        && is_unauthorized_websocket_error(&error) =>
                {
                    let _ = transport.invalidate_authorization(generation).await;
                    auth_refreshed = true;
                }
                Ok(Err(error)) if retries < max_retries && retryable_connect_error(&error) => {
                    retries += 1;
                    tokio::time::sleep(retry_delay).await;
                }
                Ok(Err(error)) => return Err(map_websocket_error(error)),
                Err(_) if retries < max_retries => {
                    retries += 1;
                    tokio::time::sleep(retry_delay).await;
                }
                Err(_) => {
                    return Err(Error::WebSocketTransport(
                        "initial beta Responses WebSocket handshake timed out".into(),
                    ));
                }
            }
        }
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
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Close status code carried by the peer's close frame, if one was
    /// received (4-18). `None` after a frameless EOF (or before any close):
    /// a code such as 1011 distinguishes a server-side failure close from a
    /// clean `1000`/`1001` shutdown.
    #[must_use]
    pub const fn close_code(&self) -> Option<u16> {
        match &self.last_close {
            Some((code, _)) => Some(*code),
            None => None,
        }
    }

    /// Close reason text carried by the peer's close frame, if one was
    /// received with a close frame (the reason may be empty — an unframed
    /// EOF stays `None`).
    #[must_use]
    pub fn close_reason(&self) -> Option<&str> {
        match &self.last_close {
            Some((_, reason)) => Some(reason.as_str()),
            None => None,
        }
    }

    /// Sends a typed beta `response.create` event.
    pub async fn send_create(&mut self, request: BetaCreateResponseRequest) -> Result<(), Error> {
        self.send_event(BetaResponsesClientEvent::create(request))
            .await
    }

    /// Sends a lane-routed beta `response.create` event.
    pub async fn send_create_on_stream(
        &mut self,
        stream_id: impl Into<String>,
        request: BetaCreateResponseRequest,
    ) -> Result<(), Error> {
        self.send_event(BetaResponsesClientEvent::create_on_stream(
            stream_id, request,
        ))
        .await
    }

    /// Sends a beta `response.create` event built from a streaming request,
    /// preserving its `stream_options` (14-F-1).
    ///
    /// The pinned WebSocket create schema embeds the same `stream_options`
    /// member the SSE lane uses and openai-python forwards it on the socket,
    /// so an SSE-shaped request can move to the WebSocket face without
    /// losing its obfuscation tuning; only the HTTP `stream` flag is dropped
    /// (it is implicit over the WebSocket).
    pub async fn send_create_streaming(
        &mut self,
        request: BetaCreateStreamingResponseRequest,
    ) -> Result<(), Error> {
        self.send_event(BetaResponsesClientEvent::Create(Box::new(
            BetaResponsesCreateEvent::from_streaming(request),
        )))
        .await
    }

    /// Atomically injects typed client-owned output items.
    pub async fn send_inject(&mut self, event: BetaResponseInjectEvent) -> Result<(), Error> {
        self.send_event(BetaResponsesClientEvent::inject(event))
            .await
    }

    /// Sends any typed beta client event with bounded buffering.
    ///
    /// A transport failure while *writing* the frame retires the socket
    /// (`is_closed` becomes `true`), extending the recv-side posture (4-19,
    /// D0212): a connection that cannot be written to is not usable again, so
    /// later `send`/`recv` calls report the closed state instead of polling a
    /// half-broken socket. Local validation failures — an event that fails to
    /// encode, carries an invalid `stream_id`, or exceeds the configured
    /// message limit — leave the connection open, because nothing reached the
    /// wire and the socket remains healthy.
    pub async fn send_event(&mut self, event: BetaResponsesClientEvent) -> Result<(), Error> {
        if self.closed {
            return Err(Error::WebSocketProtocol(
                "cannot send on a closed beta Responses WebSocket",
            ));
        }
        let encoded = serde_json::to_string(&event).map_err(Error::Encode)?;
        validate_stream_id(&encoded)?;
        if encoded.len() > self.max_message_bytes {
            return Err(Error::WebSocketProtocol(
                "outgoing beta Responses event exceeds the configured message limit",
            ));
        }
        match self.socket.send(Message::text(encoded)).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.closed = true;
                Err(map_websocket_error(error))
            }
        }
    }

    /// Receives the next typed beta server event.
    ///
    /// Failure posture (4-19, synced by 5-04 to match the GA and Realtime
    /// sockets): every transport or protocol failure — a broken connection, an
    /// oversized event, or a frame that violates the beta event-transport
    /// contract — retires the socket (`is_closed` becomes `true`, matching
    /// openai-node, which destroys the WebSocket on any error). A failed event
    /// *decode* is the one recoverable path: the connection stays open so a
    /// malformed event need not take down an otherwise healthy session.
    pub async fn recv(&mut self) -> Result<Option<BetaResponsesServerEvent>, Error> {
        if self.closed {
            return Ok(None);
        }
        loop {
            let Some(message) = self.socket.next().await else {
                self.closed = true;
                return Ok(None);
            };
            // A read failure leaves the underlying connection unusable, so it
            // retires the socket like every other non-decode error path.
            let message = match message {
                Ok(message) => message,
                Err(error) => {
                    self.closed = true;
                    return Err(map_websocket_error(error));
                }
            };
            match message {
                Message::Text(text) => {
                    if text.len() > self.max_message_bytes {
                        // An oversized event means the peer (or an
                        // intermediary) already sent bytes this socket cannot
                        // frame, so the socket retires instead of being polled
                        // again.
                        self.closed = true;
                        return Err(Error::WebSocketProtocol(
                            "incoming beta Responses event exceeds the configured message limit",
                        ));
                    }
                    let event =
                        deserialize_json(text.as_bytes()).map_err(|error| Error::Decode {
                            source: error.source,
                            path: error.path,
                            meta_status: self.meta.status(),
                            request_id: self.meta.request_id().map(Box::<str>::from),
                            body: BodyPreview::from_bytes(text.as_bytes(), false),
                        })?;
                    return Ok(Some(event));
                }
                // tungstenite queues the RFC 6455 Pong while reading the Ping
                // and flushes it on the next poll; an explicit reply here only
                // adds a redundant write path (D0148).
                Message::Ping(_) => {}
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    self.closed = true;
                    if let Some(frame) = frame {
                        self.last_close =
                            Some((u16::from(frame.code), frame.reason.as_str().to_owned()));
                    }
                    return Ok(None);
                }
                Message::Binary(_) => {
                    // A frame that violates the beta event-transport contract
                    // retires the socket; the stream is only usable for
                    // well-formed beta Responses frames.
                    self.closed = true;
                    return Err(Error::WebSocketProtocol(
                        "beta Responses WebSocket sent a binary data message",
                    ));
                }
                Message::Frame(_) => {
                    self.closed = true;
                    return Err(Error::WebSocketProtocol(
                        "beta Responses WebSocket exposed an unexpected raw frame",
                    ));
                }
            }
        }
    }

    /// Receives one event and also feeds a caller-owned single-lane
    /// accumulator (14-F-6), mirroring the GA `ResponsesWebSocket::recv_into`.
    ///
    /// Multiplexed callers should use [`Self::recv`], route by
    /// [`BetaResponsesServerEvent::stream_id`], then push the stable core of
    /// the matching lane's event into its accumulator.
    pub async fn recv_into(
        &mut self,
        accumulator: &mut ResponseAccumulator,
    ) -> Result<Option<BetaResponsesServerEvent>, Error> {
        let event = self.recv().await?;
        if let Some(event) = &event
            && let Some(stream_event) = event.event()
        {
            accumulator.push(stream_event.core().clone())?;
        }
        Ok(event)
    }

    /// Initiates the WebSocket close handshake.
    pub async fn close(&mut self) -> Result<(), Error> {
        if !self.closed {
            self.socket.close(None).await.map_err(map_websocket_error)?;
            self.closed = true;
        }
        Ok(())
    }
}

impl fmt::Debug for BetaResponsesWebSocket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BetaResponsesWebSocket")
            .field("meta", &self.meta)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

fn decode_beta_dispatch(
    dispatch: SseDispatch,
    meta: &ResponseMeta,
) -> Result<Option<BetaResponseStreamEvent>, Error> {
    match dispatch {
        SseDispatch::Event(frame) => decode_beta_event(&frame, meta).map(Some),
        SseDispatch::Terminal(frame) => decode_beta_event(&frame, meta).map(Some),
        SseDispatch::RemoteError(frame) => {
            Err(StreamError::from_body(meta.request_id(), frame.data.as_bytes()).into())
        }
    }
}

fn decode_beta_event(
    frame: &SseFrame,
    meta: &ResponseMeta,
) -> Result<BetaResponseStreamEvent, Error> {
    if frame.event.is_none() {
        return Err(Error::StreamProtocol {
            message: "a beta Responses event is missing its SSE event field",
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        });
    }
    let value = deserialize_json::<serde_json::Value>(frame.data.as_bytes()).map_err(|error| {
        Error::Decode {
            source: error.source,
            path: error.path,
            meta_status: StatusCode::OK,
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        }
    })?;
    if value.get("type").and_then(serde_json::Value::as_str) != frame.event.as_deref() {
        return Err(Error::StreamProtocol {
            message: "the beta SSE event field and JSON type discriminator differ",
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        });
    }
    let event: BetaResponseStreamEvent =
        deserialize_json(frame.data.as_bytes()).map_err(|error| Error::Decode {
            source: error.source,
            path: error.path,
            meta_status: StatusCode::OK,
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        })?;
    if matches!(
        event.core(),
        openai_rs_types::responses::ResponseStreamEvent::Error(_)
    ) {
        return Err(StreamError::from_body(meta.request_id(), frame.data.as_bytes()).into());
    }
    Ok(event)
}

fn beta_websocket_url(base_url: &Url) -> Result<Url, Error> {
    // 7-22: the shared derivation maps the scheme, appends `responses`, drops
    // the fragment, and preserves query parameters configured on the base —
    // the Realtime semantics. The derivation still never *adds* the REST-only
    // `beta=true` query: the pinned beta Node oracle reaches `/responses`
    // without it.
    derive_websocket_url(base_url, "responses", "beta Responses")
}

fn response_path(response_id: &ResponseId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("responses"),
        response_id_segment(response_id)?,
    ])
}

fn response_id_segment(response_id: &ResponseId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("response_id", response_id.as_str())
}

fn invalid_configuration(message: impl Into<Box<str>>) -> Error {
    Error::InvalidConfiguration(message.into())
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr,
        response_mode = $response_mode:expr,
        retry = $retry:expr,
        success = $success:expr $(,)?
    ) => {
        struct $name;

        impl Sealed for $name {}

        impl Operation for $name {
            type Request = $request;
            type Response = $response;

            const META: OperationMeta = OperationMeta {
                id: stringify!($name),
                method: $method,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: $request_encoding,
                response_mode: $response_mode,
                retry: $retry,
                success_statuses: $success,
            };
        }
    };
}

operation!(
    CreateBetaResponse,
    request = BetaCreateResponseRequest,
    response = BetaResponse,
    method = Method::POST,
    route = "/responses",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    CreateStreamingBetaResponse,
    request = BetaCreateStreamingResponseRequest,
    response = BetaResponseStreamEvent,
    method = Method::POST,
    route = "/responses",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Sse,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    RetrieveBetaResponse,
    request = (),
    response = BetaResponse,
    method = Method::GET,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    RetrieveStreamingBetaResponse,
    request = (),
    response = BetaResponseStreamEvent,
    method = Method::GET,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Sse,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    DeleteBetaResponse,
    request = (),
    response = DeletedResponse,
    method = Method::DELETE,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::EmptyOrJson,
    retry = RetryClass::Replayable,
    success = OK_OR_NO_CONTENT
);
operation!(
    CancelBetaResponse,
    request = (),
    response = BetaResponse,
    method = Method::POST,
    route = "/responses/{response_id}/cancel",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    CompactBetaResponse,
    request = BetaCompactResponseRequest,
    response = BetaCompactedResponse,
    method = Method::POST,
    route = "/responses/compact",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    ListBetaResponseInputItems,
    request = (),
    response = BetaResponseItemList,
    method = Method::GET,
    route = "/responses/{response_id}/input_items",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    CountBetaResponseInputTokens,
    request = BetaCountInputTokensRequest,
    response = BetaInputTokenCountResponse,
    method = Method::POST,
    route = "/responses/input_tokens",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK
);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::{SinkExt, StreamExt};
    use http::HeaderValue;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::beta_responses::{
        BetaAgentInputText, BetaAgentMessageParam, BetaMultiAgentAction,
        BetaMultiAgentCallOutputParam, BetaMultiAgentConfig, BetaMultiAgentOutputTextParam,
        BetaResponseIncludable, BetaResponseInputItem, BetaResponseItemOrder,
    };
    use openai_rs_types::kernel::UnknownTaggedObject;
    use openai_rs_types::responses::ResponseStreamOptions;
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            Utf8Bytes,
            handshake::server,
            protocol::frame::{CloseFrame, coding::CloseCode},
        },
    };

    use super::*;
    use crate::ApiKey;

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        beta_header: Option<String>,
        body: Vec<u8>,
    }

    fn response_json(status: &str) -> String {
        json!({
            "id": "resp_beta_1",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-test",
            "object": "response",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null,
            "status": status
        })
        .to_string()
    }

    async fn serve_once(
        status: StatusCode,
        content_type: &'static str,
        body: String,
    ) -> (Url, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta loopback server");
        let address = listener.local_addr().expect("loopback address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept beta request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                let body = body.clone();
                async move {
                    let method = request.method().clone();
                    let path_and_query = request
                        .uri()
                        .path_and_query()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let authorization = header_string(request.headers(), header::AUTHORIZATION);
                    let beta_header = header_string(request.headers(), "openai-beta");
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read beta request body")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("capture lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path_and_query,
                            authorization,
                            beta_header,
                            body: request_body,
                        });
                    }
                    let response = hyper::Response::builder()
                        .status(status)
                        .header(header::CONTENT_TYPE, content_type)
                        .header("x-request-id", "req_beta")
                        .body(Full::new(Bytes::from(body)))
                        .expect("build beta response");
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve beta request");
        });
        (
            Url::parse(&format!("http://{address}/v1/")).expect("beta base URL"),
            receiver,
        )
    }

    fn client(base_url: Url) -> Client {
        let key = ApiKey::new("test-placeholder-key").expect("valid test key");
        Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("beta loopback client")
    }

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Url, tokio::sync::mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta sequence server");
        let address = listener.local_addr().expect("beta sequence address");
        let responses = Arc::new(Mutex::new(std::collections::VecDeque::from(responses)));
        let (sender, receiver) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            loop {
                if responses
                    .lock()
                    .expect("beta sequence queue lock")
                    .is_empty()
                {
                    break;
                }
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => break,
                };
                let responses = Arc::clone(&responses);
                let sender = sender.clone();
                let service = service_fn(move |request: Request<Incoming>| {
                    let responses = Arc::clone(&responses);
                    let sender = sender.clone();
                    async move {
                        let method = request.method().clone();
                        let path_and_query = request
                            .uri()
                            .path_and_query()
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        let authorization = header_string(request.headers(), header::AUTHORIZATION);
                        let beta_header = header_string(request.headers(), "openai-beta");
                        let body = request
                            .into_body()
                            .collect()
                            .await
                            .expect("read beta sequence body")
                            .to_bytes()
                            .to_vec();
                        let _ = sender
                            .send(CapturedRequest {
                                method,
                                path_and_query,
                                authorization,
                                beta_header,
                                body,
                            })
                            .await;

                        let next = responses
                            .lock()
                            .expect("beta sequence queue lock")
                            .pop_front()
                            .unwrap_or((StatusCode::OK, "{}".into()));
                        let response = hyper::Response::builder()
                            .status(next.0)
                            .header(header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_beta_seq")
                            .body(Full::new(Bytes::from(next.1)))
                            .expect("build beta sequence response");
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            }
        });

        let base = Url::parse(&format!("http://{address}/v1/")).expect("beta sequence base URL");
        (base, receiver)
    }

    fn header_string(
        headers: &http::HeaderMap,
        name: impl http::header::AsHeaderName,
    ) -> Option<String> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }

    #[tokio::test]
    async fn create_uses_beta_query_multi_agent_body_and_beta_header() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "application/json",
            response_json("completed"),
        )
        .await;
        let routed = BetaAgentMessageParam::new(
            "root",
            "root/research",
            [BetaAgentInputText::new("inspect")],
        );
        let request =
            BetaCreateResponseRequest::new("gpt-test", vec![BetaResponseInputItem::from(routed)])
                .multi_agent(BetaMultiAgentConfig::new(true).max_concurrent_subagents(3));
        let response = client(base_url)
            .beta_responses()
            .create(request)
            .await
            .expect("create beta response");
        assert_eq!(response.request_id(), Some("req_beta"));
        assert_eq!(response.id(), "resp_beta_1");

        let captured = captured.await.expect("captured create");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses?beta=true");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(captured.beta_header.as_deref(), Some(BETA_VALUE));
        let body: Value = serde_json::from_slice(&captured.body).expect("create JSON");
        assert_eq!(body["multi_agent"]["enabled"], true);
        assert_eq!(body["input"][0]["type"], "agent_message");
    }

    #[tokio::test]
    async fn beta_background_poll_stops_at_terminal_status() {
        // 8-18: mirror of the GA `response_poll_stops_at_terminal_state` —
        // the background poll keeps hitting the beta retrieve route (query
        // flag plus static beta header) until a terminal status arrives.
        let (base_url, mut captured) = serve_sequence(vec![
            (StatusCode::OK, response_json("in_progress")),
            (StatusCode::OK, response_json("completed")),
        ])
        .await;

        let response = client(base_url)
            .beta_responses()
            .poll(
                &ResponseId::new("resp_beta_1"),
                PollOptions::new()
                    .with_interval(std::time::Duration::from_millis(1))
                    .with_timeout(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("poll beta background response");
        assert_eq!(
            response.status(),
            Some(&openai_rs_types::responses::ResponseStatus::Completed)
        );

        for expected_path in [
            "/v1/responses/resp_beta_1?beta=true",
            "/v1/responses/resp_beta_1?beta=true",
        ] {
            let request = captured.recv().await.expect("poll request");
            assert_eq!(request.method, Method::GET);
            assert_eq!(request.path_and_query, expected_path);
            assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));
        }
        assert!(captured.recv().await.is_none());
    }

    #[tokio::test]
    async fn retrieve_delete_cancel_and_compact_use_pinned_routes() {
        let response_id = ResponseId::new("resp/a b");

        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "application/json",
            response_json("completed"),
        )
        .await;
        client(base_url)
            .beta_responses()
            .retrieve_with(
                &response_id,
                BetaRetrieveResponseParams::new()
                    .include(BetaResponseIncludable::FileSearchResults),
            )
            .await
            .expect("retrieve beta response");
        let request = captured.await.expect("captured retrieve");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            // `include[]` percent-encodes its brackets on the wire
            // (`include%5B%5D=`, same as the Administration channel's
            // bracketed filters).
            "/v1/responses/resp%2Fa%20b?beta=true&include%5B%5D=file_search_call.results"
        );
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));

        let (base_url, captured) =
            serve_once(StatusCode::NO_CONTENT, "application/json", String::new()).await;
        let deleted = client(base_url)
            .beta_responses()
            .delete(&response_id)
            .await
            .expect("delete beta response");
        assert!(matches!(deleted.body(), DeleteResponseResult::Empty));
        let request = captured.await.expect("captured delete");
        assert_eq!(request.method, Method::DELETE);
        assert_eq!(
            request.path_and_query,
            "/v1/responses/resp%2Fa%20b?beta=true"
        );
        // The empty-or-JSON delete lane carries the static beta header like
        // every other beta REST operation.
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));

        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "application/json",
            response_json("cancelled"),
        )
        .await;
        client(base_url)
            .beta_responses()
            .cancel(&response_id)
            .await
            .expect("cancel beta response");
        let request = captured.await.expect("captured cancel");
        assert_eq!(request.method, Method::POST);
        assert_eq!(
            request.path_and_query,
            "/v1/responses/resp%2Fa%20b/cancel?beta=true"
        );
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));

        let compacted = json!({
            "id": "resp_compact_1",
            "created_at": 2,
            "object": "response.compaction",
            "output": [],
            "usage": {
                "input_tokens": 3,
                "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
                "output_tokens": 1,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": 4
            }
        })
        .to_string();
        let (base_url, captured) = serve_once(StatusCode::OK, "application/json", compacted).await;
        client(base_url)
            .beta_responses()
            .compact(BetaCompactResponseRequest::new("gpt-test").input("hello"))
            .await
            .expect("compact beta response");
        let request = captured.await.expect("captured compact");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/responses/compact?beta=true");
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));
        let body: Value = serde_json::from_slice(&request.body).expect("compact JSON");
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["input"], "hello");
    }

    #[tokio::test]
    async fn input_items_and_input_tokens_use_operation_specific_contracts() {
        let item_page = json!({
            "object": "list",
            "data": [{
                "type": "multi_agent_call",
                "action": "wait_agent",
                "arguments": "{}",
                "call_id": "call_1"
            }],
            "first_id": "item_1",
            "last_id": "item_1",
            "has_more": false
        })
        .to_string();
        let (base_url, captured) = serve_once(StatusCode::OK, "application/json", item_page).await;
        let page = client(base_url)
            .beta_responses()
            .input_items()
            .list(
                &ResponseId::new("resp_1"),
                BetaListInputItemsParams::new()
                    .after("item_0")
                    .include(BetaResponseIncludable::EncryptedReasoning)
                    .limit(20)
                    .expect("valid limit")
                    .order(BetaResponseItemOrder::Asc),
            )
            .await
            .expect("list beta input items");
        assert!(matches!(
            page.data(),
            [BetaResponseInputItem::MultiAgentCall(_)]
        ));
        let request = captured.await.expect("captured input-items list");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            "/v1/responses/resp_1/input_items?beta=true&after=item_0&include%5B%5D=reasoning.encrypted_content&limit=20&order=asc"
        );
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));

        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "application/json",
            r#"{"object":"response.input_tokens","input_tokens":11}"#.to_owned(),
        )
        .await;
        let count = client(base_url)
            .beta_responses()
            .input_tokens()
            .count(BetaCountInputTokensRequest::new("gpt-test", "hello").personality("friendly"))
            .await
            .expect("count beta input tokens");
        assert_eq!(count.input_tokens(), 11);
        let request = captured.await.expect("captured token count");
        assert_eq!(request.method, Method::POST);
        assert_eq!(
            request.path_and_query,
            "/v1/responses/input_tokens?beta=true"
        );
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));
        let body: Value = serde_json::from_slice(&request.body).expect("count JSON");
        assert_eq!(body["personality"], "friendly");
    }

    #[tokio::test]
    async fn beta_sse_decodes_agent_metadata_and_terminal_snapshot() {
        let created = json!({
            "type": "response.created",
            "sequence_number": 1,
            "agent": {"agent_name": "root"},
            "response": serde_json::from_str::<Value>(&response_json("in_progress"))
                .expect("created response JSON")
        });
        let completed = json!({
            "type": "response.completed",
            "sequence_number": 2,
            "agent": {"agent_name": "root"},
            "response": serde_json::from_str::<Value>(&response_json("completed"))
                .expect("completed response JSON")
        });
        let body = format!(
            "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {completed}\n\n"
        );
        let (base_url, captured) = serve_once(StatusCode::OK, "text/event-stream", body).await;
        let mut stream = client(base_url)
            .beta_responses()
            .create_stream(BetaCreateStreamingResponseRequest::new("gpt-test", "hello"))
            .await
            .expect("open beta SSE stream");
        let first = stream
            .next()
            .await
            .expect("created event")
            .expect("valid created event");
        assert_eq!(first.agent().map(|agent| agent.agent_name()), Some("root"));
        let second = stream
            .next()
            .await
            .expect("completed event")
            .expect("valid completed event");
        assert!(second.is_terminal());
        assert_eq!(second.response().map(BetaResponse::id), Some("resp_beta_1"));
        assert!(stream.next().await.is_none());

        let request = captured.await.expect("captured SSE create");
        assert_eq!(request.path_and_query, "/v1/responses?beta=true");
        // The SSE lane carries the static beta header alongside the
        // `beta=true` query.
        assert_eq!(request.beta_header.as_deref(), Some(BETA_VALUE));
        let body: Value = serde_json::from_slice(&request.body).expect("stream JSON");
        assert_eq!(body["stream"], true);
    }

    #[tokio::test]
    async fn eof_flushed_data_only_terminal_is_delivered_before_unexpected_eof() {
        // 14-G-1: the final `response.completed` frame carries no `event:`
        // line and no trailing blank line, so it only surfaces through the
        // EOF flush as a plain event; the policy's terminal table matches
        // event names and cannot classify it. Unlike the media lane, the beta
        // typed codec requires the SSE event field, so a data-only frame can
        // never decode to a terminal event; the corner cannot deliver a
        // terminal payload. Adopting `finish_with_flushed` still surfaces the
        // flushed frame to the typed codec, whose StreamProtocol error (with
        // the body preview) is strictly more diagnostic than the generic
        // UnexpectedEof that plain `finish()` would raise instead.
        let created = json!({
            "type": "response.created",
            "sequence_number": 1,
            "agent": {"agent_name": "root"},
            "response": serde_json::from_str::<Value>(&response_json("in_progress"))
                .expect("created response JSON")
        });
        let completed = json!({
            "type": "response.completed",
            "sequence_number": 2,
            "agent": {"agent_name": "root"},
            "response": serde_json::from_str::<Value>(&response_json("completed"))
                .expect("completed response JSON")
        });
        let body = format!("event: response.created\ndata: {created}\n\ndata: {completed}");
        let (base_url, _captured) = serve_once(StatusCode::OK, "text/event-stream", body).await;
        let mut stream = client(base_url)
            .beta_responses()
            .create_stream(BetaCreateStreamingResponseRequest::new("gpt-test", "hello"))
            .await
            .expect("open beta SSE stream");
        assert!(
            stream
                .next()
                .await
                .expect("created event")
                .expect("valid created event")
                .response()
                .is_some()
        );
        let error = stream
            .next()
            .await
            .expect("EOF-flushed frame surfaces through the codec")
            .expect_err("data-only frame cannot decode without its event field");
        match error {
            Error::StreamProtocol { message, .. } => {
                assert!(message.contains("missing its SSE event field"));
            }
            other => panic!("expected stream protocol error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn beta_retrieve_stream_encodes_resume_query() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "text/event-stream",
            "data: [DONE]\n\n".to_owned(),
        )
        .await;
        let response_id = ResponseId::new("resp_resume");
        let params = BetaRetrieveResponseStreamParams::new()
            .include(BetaResponseIncludable::EncryptedReasoning)
            .starting_after(41)
            .include_obfuscation(false);

        let mut stream = client(base_url)
            .beta_responses()
            .retrieve_stream(&response_id, params)
            .await
            .expect("beta retrieve stream handshake");
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured beta resume request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(
            captured.path_and_query,
            "/v1/responses/resp_resume?beta=true&include%5B%5D=reasoning.encrypted_content&stream=true&include_obfuscation=false&starting_after=41"
        );
        // The SSE lane carries the static beta header (see
        // `execute_beta_json`).
        assert_eq!(captured.beta_header.as_deref(), Some(BETA_VALUE));
    }

    #[tokio::test]
    async fn beta_create_stream_sends_stream_options_on_the_sse_lane() {
        // 14-F-1: the SSE lane keeps writing `stream: true` next to its
        // `stream_options` and stays untouched by the WebSocket fix.
        let body = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({
                "type": "response.completed",
                "sequence_number": 1,
                "response": serde_json::from_str::<Value>(&response_json("completed"))
                    .expect("completed response JSON")
            })
        );
        let (base_url, captured) = serve_once(StatusCode::OK, "text/event-stream", body).await;
        let mut stream = client(base_url)
            .beta_responses()
            .create_stream(
                BetaCreateStreamingResponseRequest::new("gpt-test", "hello")
                    .stream_options(ResponseStreamOptions::default().include_obfuscation(false)),
            )
            .await
            .expect("open beta SSE stream");
        assert!(stream.next().await.expect("terminal event").is_ok());
        assert!(stream.next().await.is_none());

        let request = captured.await.expect("captured SSE create");
        let body: Value = serde_json::from_slice(&request.body).expect("stream JSON");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_obfuscation"], false);
    }

    #[tokio::test]
    async fn beta_stream_collect_with_reduces_through_the_stable_core() {
        // 14-F-6: the GA `collect_with` parity. The caller-owned accumulator
        // consumes the stable core codec, so the reduction yields the GA
        // `Response` even though the stream surfaces beta events.
        let upper = concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_beta\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]},\"sequence_number\":1}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_beta\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hello \",\"sequence_number\":2,\"logprobs\":[]}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_beta\",\"output_index\":0,\"content_index\":0,\"delta\":\"beta\",\"sequence_number\":3,\"logprobs\":[]}\n\n",
        );
        let (base_url, _captured) =
            serve_once(StatusCode::OK, "text/event-stream", upper.to_owned()).await;
        let mut accumulator = ResponseAccumulator::new();
        let mut upper_stream = client(base_url)
            .beta_responses()
            .create_stream(BetaCreateStreamingResponseRequest::new("gpt-test", "hello"))
            .await
            .expect("upper-half handshake");
        let mut interrupted = false;
        while let Some(item) = upper_stream.next().await {
            match item {
                Ok(event) => {
                    accumulator
                        .push(event.core().clone())
                        .expect("accept upper-half event");
                }
                Err(error) => {
                    assert!(
                        matches!(error, Error::Sse { .. }),
                        "interruption must be an SSE error, got {error:?}"
                    );
                    interrupted = true;
                }
            }
        }
        assert!(interrupted, "the upper half must end interrupted");
        assert_eq!(accumulator.last_sequence_number(), Some(3));

        let lower = concat!(
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_beta\",\"output_index\":0,\"content_index\":0,\"text\":\"Hello beta\",\"sequence_number\":4,\"logprobs\":[]}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"sequence_number\":5,\"response\":{\"id\":\"resp_beta_resume\",\"created_at\":1,\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":null,\"model\":\"gpt-test\",\"object\":\"response\",\"output\":[{\"type\":\"message\",\"id\":\"msg_beta\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello beta\",\"annotations\":[],\"logprobs\":[]}]}],\"parallel_tool_calls\":true,\"temperature\":1.0,\"tool_choice\":\"auto\",\"tools\":[],\"top_p\":1.0,\"status\":\"completed\"}}\n\n",
        );
        let (base_url, _captured) =
            serve_once(StatusCode::OK, "text/event-stream", lower.to_owned()).await;
        let response = client(base_url)
            .beta_responses()
            .retrieve_stream(
                &ResponseId::new("resp_beta_resume"),
                BetaRetrieveResponseStreamParams::new().starting_after(3),
            )
            .await
            .expect("resume handshake")
            .collect_with(accumulator)
            .await
            .expect("resumed terminal response");
        assert_eq!(response.id(), "resp_beta_resume");
        assert_eq!(response.output_text(), "Hello beta");
    }

    #[tokio::test]
    async fn beta_create_stream_rejects_non_sse_content_type() {
        // 8-05(b): the beta SSE handshake fails closed on a JSON body, the
        // same guard the GA lane pins.
        let (base_url, _captured) =
            serve_once(StatusCode::OK, "application/json", "{}".to_owned()).await;
        let error = client(base_url)
            .beta_responses()
            .create_stream(BetaCreateStreamingResponseRequest::new("gpt-test", "hello"))
            .await
            .expect_err("a JSON content type must fail the beta SSE handshake");
        match &error {
            Error::UnexpectedContentType {
                expected,
                actual,
                status,
                ..
            } => {
                assert_eq!(*expected, "text/event-stream");
                assert_eq!(actual.as_deref(), Some("application/json"));
                assert_eq!(*status, StatusCode::OK);
            }
            other => panic!("expected UnexpectedContentType, got {other:?}"),
        }
        assert_eq!(error.request_id(), Some("req_beta"));
        assert_eq!(error.status(), Some(StatusCode::OK));
    }

    #[tokio::test]
    async fn beta_list_input_item_pages_fails_closed_on_repeated_cursor() {
        let page = json!({
            "object": "list",
            "data": [],
            "first_id": "item_1",
            "last_id": "item_1",
            "has_more": true
        });
        let (base_url, mut captured) = serve_sequence(vec![
            (StatusCode::OK, page.to_string()),
            (StatusCode::OK, page.to_string()),
        ])
        .await;

        let mut stream = client(base_url)
            .beta_responses()
            .list_input_item_pages(&ResponseId::new("resp_1"), BetaListInputItemsParams::new());
        let first = stream.next().await.expect("first beta page").expect("ok");
        assert_eq!(first.last_id(), "item_1");
        // The server repeats the same cursor: pagination must fail closed
        // instead of silently re-fetching the page forever.
        let error = stream
            .next()
            .await
            .expect("repeated cursor surfaces")
            .expect_err("repeated cursor fails closed");
        assert!(matches!(
            error,
            Error::Pagination {
                reason: crate::error::PaginationFault::RepeatedCursor,
                ..
            }
        ));
        assert!(stream.next().await.is_none());
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
        assert!(
            captured.try_recv().is_err(),
            "no third request after the repeated cursor"
        );
    }

    #[tokio::test]
    async fn beta_list_input_item_pages_fails_closed_when_has_more_lacks_last_id() {
        // An empty `last_id` decodes but cannot advance: beta input items are
        // a tagged union without a shared id accessor, so no fallback cursor
        // exists (D0147) and the stream fails closed.
        let empty_last_id = json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "",
            "has_more": true
        });
        let (base_url, mut captured) =
            serve_sequence(vec![(StatusCode::OK, empty_last_id.to_string())]).await;
        let mut stream = client(base_url)
            .beta_responses()
            .list_input_item_pages(&ResponseId::new("resp_1"), BetaListInputItemsParams::new());
        let error = stream
            .next()
            .await
            .expect("empty cursor surfaces")
            .expect_err("empty last_id fails closed");
        assert!(matches!(
            error,
            Error::Pagination {
                reason: crate::error::PaginationFault::MissingCursor,
                ..
            }
        ));
        assert!(stream.next().await.is_none());
        assert!(captured.recv().await.is_some());
        assert!(
            captured.try_recv().is_err(),
            "no follow-up request without a cursor"
        );

        // A page that omits `last_id` entirely cannot even decode as a list
        // envelope, which still fails the stream closed before any retry.
        let missing_last_id = json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "has_more": true
        });
        let (base_url, mut captured) =
            serve_sequence(vec![(StatusCode::OK, missing_last_id.to_string())]).await;
        let mut stream = client(base_url)
            .beta_responses()
            .list_input_item_pages(&ResponseId::new("resp_1"), BetaListInputItemsParams::new());
        assert!(
            stream
                .next()
                .await
                .expect("undecodable page surfaces")
                .is_err(),
            "a page without last_id errors instead of advancing"
        );
        assert!(stream.next().await.is_none());
        assert!(captured.recv().await.is_some());
    }

    #[derive(Debug)]
    struct WebSocketHandshake {
        path_and_query: String,
        authorization: Option<String>,
        beta_header: Option<String>,
    }

    async fn websocket_server() -> (
        Client,
        oneshot::Receiver<WebSocketHandshake>,
        oneshot::Receiver<Value>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta WebSocket");
        let address = listener.local_addr().expect("WebSocket address");
        let (handshake_sender, handshake_receiver) = oneshot::channel();
        let (event_sender, event_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept beta WebSocket");
            let handshake_sender = Arc::new(Mutex::new(Some(handshake_sender)));
            let callback = move |request: &server::Request, mut response: server::Response| {
                if let Some(sender) = handshake_sender.lock().expect("handshake lock").take() {
                    let _ = sender.send(WebSocketHandshake {
                        path_and_query: request.uri().to_string(),
                        authorization: header_string(request.headers(), header::AUTHORIZATION),
                        beta_header: header_string(request.headers(), "openai-beta"),
                    });
                }
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_static("req_beta_ws"));
                Ok::<_, server::ErrorResponse>(response)
            };
            let mut socket = accept_hdr_async(stream, callback)
                .await
                .expect("beta WebSocket handshake");
            let message = socket
                .next()
                .await
                .expect("beta client event")
                .expect("valid beta client event");
            let value = match message {
                Message::Text(text) => {
                    serde_json::from_slice(text.as_bytes()).expect("beta event JSON")
                }
                other => panic!("unexpected beta client message: {other:?}"),
            };
            let _ = event_sender.send(value);
            socket
                .send(Message::text(
                    json!({
                        "type": "response.inject.created",
                        "response_id": "resp_beta_1",
                        "sequence_number": 7,
                        "stream_id": "lane_1"
                    })
                    .to_string(),
                ))
                .await
                .expect("send inject confirmation");
            let _ = socket.next().await;
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta WebSocket base URL");
        (client(base_url), handshake_receiver, event_receiver)
    }

    #[tokio::test]
    async fn beta_websocket_uses_pinned_path_without_query_and_supports_inject() {
        let (client, handshake, sent_event) = websocket_server().await;
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");
        assert_eq!(socket.request_id(), Some("req_beta_ws"));
        socket
            .send_inject(BetaResponseInjectEvent::new(
                "resp_beta_1",
                [BetaResponseInputItem::from(
                    BetaMultiAgentCallOutputParam::new(
                        BetaMultiAgentAction::WaitAgent,
                        "call_1",
                        [BetaMultiAgentOutputTextParam::new("done")],
                    ),
                )],
            ))
            .await
            .expect("send inject event");
        let event = socket
            .recv()
            .await
            .expect("receive inject confirmation")
            .expect("one server event");
        assert!(matches!(event, BetaResponsesServerEvent::InjectCreated(_)));
        assert_eq!(event.stream_id(), Some("lane_1"));

        let handshake = handshake.await.expect("captured beta handshake");
        assert_eq!(handshake.path_and_query, "/v1/responses");
        assert_eq!(
            handshake.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        // The WebSocket face stays header-free: openai-python exposes the
        // multi-agent preview REST-only, and the pinned Node oracle reaches
        // `/responses` without the REST `beta=true` query.
        assert_eq!(handshake.beta_header, None);
        let sent = sent_event.await.expect("captured inject event");
        assert_eq!(sent["type"], "response.inject");
        assert_eq!(sent["input"][0]["type"], "multi_agent_call_output");

        socket.close().await.expect("close beta WebSocket");
    }

    /// 10-03: the beta face now delegates its pre-send lane guard to the GA
    /// copy in `responses_websocket` (this module's duplicate validator was
    /// removed), so every rejection branch must still fire before anything
    /// reaches the wire — exercised through verbatim `Unknown` events so
    /// arbitrary `stream_id` shapes reach the encoder.
    #[tokio::test]
    async fn beta_websocket_rejects_malformed_stream_id_unknown_events() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta accepting server");
        let address = listener
            .local_addr()
            .expect("beta accepting server address");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut socket = accept_hdr_async(
                        stream,
                        |_request: &server::Request, response: server::Response| {
                            Ok::<_, server::ErrorResponse>(response)
                        },
                    )
                    .await
                    .expect("beta accepting server handshake");
                    while socket.next().await.is_some() {}
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta accepting client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("beta accepting client");
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect the accepting beta WebSocket");

        let lane_pattern = "stream_id must match ^[A-Za-z0-9_.-]{1,256}$";
        let cases: [(Value, &str); 4] = [
            (json!(""), lane_pattern),
            (json!("a".repeat(257)), lane_pattern),
            (json!("lane/1"), lane_pattern),
            (json!(7), "stream_id must be a string"),
        ];
        for (stream_id, expected) in cases {
            let event = BetaResponsesClientEvent::Unknown(
                UnknownTaggedObject::from_value(json!({
                    "type": "future.client.event",
                    "stream_id": stream_id,
                }))
                .expect("unknown beta client event"),
            );
            match socket.send_event(event).await {
                Err(Error::WebSocketProtocol(reason)) => assert_eq!(reason, expected),
                unexpected => panic!("expected a beta stream_id rejection, got {unexpected:?}"),
            }
            assert!(
                !socket.is_closed(),
                "a local beta stream_id rejection must not retire the socket"
            );
        }
        socket.close().await.expect("close the healthy beta socket");
    }

    /// 8-19: the lane-routed `response.create` face mirrors the GA persistent
    /// connection wire shape — a flattened create body tagged
    /// `response.create` with the `stream_id` lane, no REST `beta=true` query,
    /// and no `stream` key (the WebSocket transport is the stream).
    #[tokio::test]
    async fn send_create_on_stream_routes_the_lane_on_the_wire() {
        let (client, handshake, sent_event) = websocket_server().await;
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");
        socket
            .send_create_on_stream(
                "lane_1",
                BetaCreateResponseRequest::new("gpt-test", "hello"),
            )
            .await
            .expect("send lane-routed response.create");
        let event = socket
            .recv()
            .await
            .expect("receive lane event")
            .expect("one server event");
        assert_eq!(event.stream_id(), Some("lane_1"));

        let handshake = handshake.await.expect("captured beta handshake");
        assert_eq!(handshake.path_and_query, "/v1/responses");
        assert_eq!(handshake.beta_header, None);
        let sent = sent_event.await.expect("captured create event");
        assert_eq!(sent["type"], "response.create");
        assert_eq!(sent["stream_id"], "lane_1");
        assert_eq!(sent["model"], "gpt-test");
        // A plain-string input is the easy-input form and stays verbatim.
        assert_eq!(sent["input"], "hello");
        assert!(
            sent.get("stream").is_none(),
            "the create event must drop the REST-only stream key"
        );

        socket.close().await.expect("close beta WebSocket");
    }

    /// 14-F-1: the WS create face forwards `stream_options` from a streaming
    /// request (openai-python's socket `response.create` does the same) while
    /// still dropping the REST-only `stream` key.
    #[tokio::test]
    async fn beta_websocket_create_streaming_preserves_stream_options() {
        let (client, handshake, sent_event) = websocket_server().await;
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");
        socket
            .send_create_streaming(
                BetaCreateStreamingResponseRequest::new("gpt-test", "hello")
                    .stream_options(ResponseStreamOptions::default().include_obfuscation(false)),
            )
            .await
            .expect("send streaming response.create");
        let event = socket
            .recv()
            .await
            .expect("receive lane event")
            .expect("one server event");
        assert_eq!(event.stream_id(), Some("lane_1"));

        let handshake = handshake.await.expect("captured beta handshake");
        // D0210 default: the handshake stays header-free without the opt-in.
        assert_eq!(handshake.beta_header, None);
        let sent = sent_event.await.expect("captured create event");
        assert_eq!(sent["type"], "response.create");
        assert_eq!(sent["model"], "gpt-test");
        assert_eq!(sent["input"], "hello");
        assert_eq!(sent["stream_options"]["include_obfuscation"], false);
        assert!(
            sent.get("stream").is_none(),
            "the create event must drop the REST-only stream key"
        );

        socket.close().await.expect("close beta WebSocket");
    }

    /// 14-F-2: the multi-agent preview gate the official examples attach by
    /// hand (openai-python `examples/responses/multi_agent_websocket.py`,
    /// openai-node `examples/responses/multi-agent-websocket.ts`) is available
    /// as an opt-in static handshake header; the default handshake carries
    /// neither it nor any other extra header (D0210).
    #[tokio::test]
    async fn beta_websocket_handshake_carries_opt_in_beta_header() {
        let (client, handshake, sent_event) = websocket_server().await;
        let mut socket = client
            .beta_responses()
            .connect_with(BetaResponsesWebSocketConfig::new().with_beta_header())
            .await
            .expect("connect beta WebSocket with the preview gate header");
        socket
            .send_create(BetaCreateResponseRequest::new("gpt-test", "hello"))
            .await
            .expect("send response.create");
        let _ = socket
            .recv()
            .await
            .expect("receive lane event")
            .expect("one server event");

        let handshake = handshake.await.expect("captured beta handshake");
        assert_eq!(handshake.path_and_query, "/v1/responses");
        assert_eq!(handshake.beta_header.as_deref(), Some(BETA_VALUE));
        assert_eq!(
            handshake.authorization.as_deref(),
            Some("Bearer test-placeholder-key"),
            "the client-managed Authorization header stays intact"
        );
        let sent = sent_event.await.expect("captured create event");
        assert_eq!(sent["type"], "response.create");

        socket.close().await.expect("close beta WebSocket");
    }

    #[test]
    fn beta_websocket_config_refuses_protected_static_headers() {
        // 14-F-2: only headers the handshake does not already manage may be
        // attached; the authenticated set is refused in every casing.
        for name in [
            "Authorization",
            "authorization",
            "OpenAI-Organization",
            "OpenAI-Project",
            "X-Client-Request-Id",
        ] {
            match BetaResponsesWebSocketConfig::new().extra_static_header(name, "value") {
                Err(Error::InvalidConfiguration(reason)) => {
                    assert!(
                        reason.contains("cannot be overridden"),
                        "unexpected refusal reason for {name}: {reason}"
                    );
                }
                unexpected => panic!("expected {name} to be refused, got {unexpected:?}"),
            }
        }
        let customized = BetaResponsesWebSocketConfig::new()
            .extra_static_header("X-Custom-Gateway", "eu-west")
            .expect("an unmanaged header is accepted");
        assert_ne!(
            customized,
            BetaResponsesWebSocketConfig::new(),
            "the accepted static header must change the config"
        );
        assert!(
            BetaResponsesWebSocketConfig::new()
                .extra_static_header("Bad Header Name", "value")
                .is_err()
        );
        assert_eq!(
            BetaResponsesWebSocketConfig::new().with_beta_header(),
            BetaResponsesWebSocketConfig::new()
                .extra_static_header(BETA_HEADER, BETA_VALUE)
                .expect("the pinned beta header is valid"),
            "with_beta_header is the validated generic path"
        );
    }

    /// 14-F-6: the beta socket gains the GA `recv_into` convenience — one
    /// receive that also feeds a caller-owned single-lane accumulator through
    /// the stable core codec.
    #[tokio::test]
    async fn beta_websocket_recv_into_feeds_the_lane_accumulator() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta recv_into server");
        let address = listener
            .local_addr()
            .expect("beta recv_into server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta recv_into socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta recv_into handshake");
            let _ = socket.next().await.expect("one client event");
            socket
                .send(Message::text(
                    json!({
                        "type": "response.output_text.delta",
                        "stream_id": "lane_1",
                        "item_id": "msg_1",
                        "output_index": 0,
                        "content_index": 0,
                        "delta": "hello",
                        "sequence_number": 1,
                        "logprobs": []
                    })
                    .to_string(),
                ))
                .await
                .expect("send beta delta event");
            let _ = socket.next().await;
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta recv_into client base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");
        socket
            .send_create(BetaCreateResponseRequest::new("gpt-test", "hello"))
            .await
            .expect("send response.create");
        let mut accumulator = ResponseAccumulator::new();
        let event = socket
            .recv_into(&mut accumulator)
            .await
            .expect("receive into accumulator")
            .expect("one server event");
        assert_eq!(event.stream_id(), Some("lane_1"));
        assert!(
            event.event().is_some(),
            "the delta routes through the Response variant"
        );
        assert_eq!(accumulator.output_text(), "hello");

        socket.close().await.expect("close beta WebSocket");
    }

    #[test]
    fn websocket_url_matches_pinned_node_oracle_without_beta_query() {
        let base = Url::parse("https://api.openai.com/v1/").expect("official base URL");
        let url = beta_websocket_url(&base).expect("derived beta WebSocket URL");
        assert_eq!(url.as_str(), "wss://api.openai.com/v1/responses");
        // 7-22: the derivation invents no `beta=true` (the pinned Node oracle
        // reaches `/responses` without the REST-only query), but query
        // parameters configured on the base survive — the Realtime
        // semantics — and a base fragment is dropped.
        let versioned = Url::parse("https://gateway.example/v1/?api-version=2026-01-01#anchor")
            .expect("versioned base URL");
        let url = beta_websocket_url(&versioned).expect("derived versioned beta WebSocket URL");
        assert_eq!(
            url.as_str(),
            "wss://gateway.example/v1/responses?api-version=2026-01-01"
        );
        assert!(
            !url.query_pairs().any(|(key, _)| key == "beta"),
            "the WebSocket face never adds the REST-only beta query"
        );
    }

    /// Serves one raw 503 handshake rejection and then accepts a single
    /// WebSocket, so the beta initial-connect policy can be exercised against
    /// a REST-retryable status (the shared 7-08 `retryable_connect_error`).
    async fn rejecting_then_accepting_handshake_server() -> Client {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta retrying handshake server");
        let address = listener
            .local_addr()
            .expect("beta retrying handshake server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("accept beta rejected handshake");
            let mut request = vec![0_u8; 4096];
            let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                .await
                .expect("timely beta rejected handshake read");
            let response = "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: 0\r\n\r\n";
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write beta raw rejection");
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta retrying WebSocket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta retrying WebSocket handshake");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta retrying client base");
        client(base_url)
    }

    #[tokio::test]
    async fn beta_initial_connect_retries_a_503_handshake_rejection() {
        // 7-08: a 503 handshake rejection is REST-retryable, so the beta
        // InitialConnect policy — which shares the GA `retryable_connect_error`
        // — replays the handshake instead of surfacing the rejection.
        let client = rejecting_then_accepting_handshake_server().await;
        let mut socket = client
            .beta_responses()
            .connect_with(BetaResponsesWebSocketConfig::new().reconnect_policy(
                BetaWebSocketReconnectPolicy::InitialConnect {
                    max_retries: 1,
                    delay: Duration::from_millis(10),
                },
            ))
            .await
            .expect("connect the beta WebSocket after the 503");
        assert!(!socket.is_closed());
        socket.close().await.expect("close the retried beta socket");
    }

    #[tokio::test]
    async fn send_write_failure_retires_the_beta_socket() {
        // 7-22: a failed send write leaves the socket unusable in both
        // directions, so it is retired like every recv-side failure (4-19,
        // D0212) instead of staying half-open. The write fails
        // deterministically against a `max_write_buffer_size` smaller than one
        // text frame (a 12+-byte frame against an 8-byte cap).
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta send-failure server");
        let address = listener
            .local_addr()
            .expect("beta send-failure server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta send-failure socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta send-failure server handshake");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta send-failure client base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect_with(
                BetaResponsesWebSocketConfig::new()
                    .write_buffer_bytes(1)
                    .max_queued_write_bytes(8),
            )
            .await
            .expect("connect beta WebSocket with a tiny write buffer");
        match socket
            .send_create(BetaCreateResponseRequest::new("gpt-test", "hello"))
            .await
        {
            Err(Error::WebSocketTransport(reason)) => {
                assert!(
                    reason.to_lowercase().contains("buffer"),
                    "expected a write-buffer failure, got {reason}"
                );
            }
            unexpected => panic!("expected a beta send write failure, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "a failed send must retire the beta socket"
        );
        assert!(
            socket
                .recv()
                .await
                .expect("recv after the beta send failure")
                .is_none(),
            "a retired beta socket reports EOF on every later recv"
        );
        assert!(
            matches!(
                socket
                    .send_create(BetaCreateResponseRequest::new("gpt-test", "again"))
                    .await,
                Err(Error::WebSocketProtocol(_))
            ),
            "a later beta send must report the closed socket"
        );
    }

    /// Accepts one beta WebSocket and immediately closes it with a coded
    /// close frame, so the client's recv observes a coded close.
    async fn coded_close_websocket_server() -> (Client, oneshot::Receiver<Option<(u16, String)>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta coded-close server");
        let address = listener
            .local_addr()
            .expect("beta coded-close server address");
        let (close_sender, close_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta coded-close socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta coded-close server handshake");
            socket
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: Utf8Bytes::from_static("beta lane failed"),
                })))
                .await
                .expect("send beta coded close frame");
            // Report before draining: the drain loop only ends when the client
            // drops its side, so awaiting it from the test would deadlock.
            let _ = close_sender.send(Some((1011_u16, "beta lane failed".to_owned())));
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta coded-close client base");
        (client(base_url), close_receiver)
    }

    #[tokio::test]
    async fn beta_websocket_close_code_and_reason_survive_the_close_handshake() {
        // 5-04 mirror (4-18): an abnormal coded close on the beta socket must
        // stay distinguishable from a clean frameless EOF.
        let (client, server_close) = coded_close_websocket_server().await;
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");
        assert_eq!(
            socket.close_code(),
            None,
            "no close frame has been seen yet"
        );
        assert!(
            socket.recv().await.expect("coded close").is_none(),
            "a peer close ends the beta stream"
        );
        assert!(socket.is_closed());
        assert_eq!(socket.close_code(), Some(1011));
        assert_eq!(socket.close_reason(), Some("beta lane failed"));
        drop(socket);
        drop(client);
        let observed = tokio::time::timeout(Duration::from_secs(5), server_close)
            .await
            .expect("timely beta server drain")
            .expect("beta server completed its side");
        assert_eq!(
            observed,
            Some((1011_u16, "beta lane failed".to_owned())),
            "the beta server saw its coded close accepted"
        );
    }

    #[tokio::test]
    async fn rejected_frame_retires_the_beta_socket() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta binary-frame server");
        let address = listener
            .local_addr()
            .expect("beta binary-frame server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta binary-frame socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta binary-frame server handshake");
            socket
                .send(Message::Binary(Bytes::from_static(b"[1,2,3]")))
                .await
                .expect("send beta binary frame");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta binary-frame client base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");

        // 5-04 mirror (4-19): a frame that violates the beta event-transport
        // contract retires the socket instead of leaving it half-alive.
        match socket.recv().await {
            Err(Error::WebSocketProtocol(reason)) => {
                assert_eq!(
                    reason,
                    "beta Responses WebSocket sent a binary data message"
                );
            }
            unexpected => panic!("expected a beta protocol rejection, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "a rejected frame must retire the beta socket"
        );
        assert!(
            socket
                .recv()
                .await
                .expect("recv after beta rejection")
                .is_none(),
            "a retired beta socket reports EOF on every later recv"
        );
    }

    #[tokio::test]
    async fn beta_event_decode_failure_keeps_the_socket_open() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta decode-failure server");
        let address = listener
            .local_addr()
            .expect("beta decode-failure server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta decode-failure socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta decode-failure server handshake");
            socket
                .send(Message::text("{not json"))
                .await
                .expect("send malformed beta event");
            socket
                .send(Message::text(
                    json!({
                        "type": "response.inject.created",
                        "response_id": "resp_beta_1",
                        "sequence_number": 8,
                        "stream_id": "lane_2"
                    })
                    .to_string(),
                ))
                .await
                .expect("send well-formed beta event");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta decode-failure client base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta WebSocket");

        // 5-04 mirror (4-19): only event decoding is recoverable; the beta
        // socket stays open for the next frame.
        assert!(
            socket.recv().await.is_err(),
            "a malformed beta event must surface as a decode error"
        );
        assert!(
            !socket.is_closed(),
            "a decode failure must not retire the beta socket"
        );
        let event = socket
            .recv()
            .await
            .expect("the beta connection survives a decode failure")
            .expect("the following beta event still decodes");
        assert!(matches!(event, BetaResponsesServerEvent::InjectCreated(_)));
        assert_eq!(event.stream_id(), Some("lane_2"));
    }

    /// Accepts every TCP connection, reads the handshake request, and then
    /// parks holding the stream open so only a client-side connect timeout
    /// can finish the attempt. Each connection parks in its own task so the
    /// counter stays assertable.
    async fn hanging_handshake_server() -> (Client, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta hanging handshake server");
        let address = listener
            .local_addr()
            .expect("beta hanging handshake address");
        let connections = Arc::new(Mutex::new(0_usize));
        let server_connections = Arc::clone(&connections);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                *server_connections
                    .lock()
                    .expect("beta hanging connection lock") += 1;
                tokio::spawn(async move {
                    let mut request = vec![0_u8; 4096];
                    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                        .await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta hanging handshake base");
        (client(base_url), connections)
    }

    #[tokio::test]
    async fn a_hanging_beta_handshake_times_out_and_replays_within_the_budget() {
        // 8-11: the beta handshake-timeout branch. A timeout counts as a
        // retryable handshake failure for `BetaWebSocketReconnectPolicy::
        // InitialConnect`, so the budget is exactly `1 + max_retries`
        // attempts before the timeout error surfaces.
        let (client, connections) = hanging_handshake_server().await;
        let error = client
            .beta_responses()
            .connect_with(
                BetaResponsesWebSocketConfig::new()
                    .connect_timeout(Duration::from_millis(50))
                    .reconnect_policy(BetaWebSocketReconnectPolicy::InitialConnect {
                        max_retries: 1,
                        delay: Duration::from_millis(10),
                    }),
            )
            .await
            .expect_err("the beta retry budget must eventually run out");
        assert!(
            matches!(&error, Error::WebSocketTransport(reason) if reason.contains("handshake timed out")),
            "expected the beta handshake-timeout error, got {error:?}"
        );
        assert_eq!(
            *connections.lock().expect("beta hanging connection lock"),
            2,
            "total attempts must equal 1 + max_retries"
        );
    }

    #[tokio::test]
    async fn an_oversized_beta_send_is_local_and_keeps_the_socket_open() {
        // 8-11: the send-side half of `max_message_bytes` on the beta face.
        // The oversized event is rejected before anything reaches the wire,
        // so the socket is not retired and a later, smaller event still sends.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta oversized-send server");
        let address = listener.local_addr().expect("beta oversized-send address");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut socket = accept_hdr_async(
                        stream,
                        |_request: &server::Request, response: server::Response| {
                            Ok::<_, server::ErrorResponse>(response)
                        },
                    )
                    .await
                    .expect("beta oversized-send handshake");
                    while socket.next().await.is_some() {}
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta oversized-send base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect_with(BetaResponsesWebSocketConfig::new().max_message_bytes(64))
            .await
            .expect("connect the beta WebSocket with a tiny message limit");

        let oversized = BetaResponsesClientEvent::Unknown(
            openai_rs_types::kernel::UnknownTaggedObject::from_value(json!({
                "type": "future.beta.client.event",
                "payload": "x".repeat(200),
            }))
            .expect("oversized unknown beta client event"),
        );
        match socket.send_event(oversized).await {
            Err(Error::WebSocketProtocol(reason)) => assert_eq!(
                reason,
                "outgoing beta Responses event exceeds the configured message limit"
            ),
            unexpected => panic!("expected a local message-limit error, got {unexpected:?}"),
        }
        assert!(
            !socket.is_closed(),
            "a local send rejection must not retire the beta socket"
        );

        let small = BetaResponsesClientEvent::Unknown(
            openai_rs_types::kernel::UnknownTaggedObject::from_value(
                json!({"type": "future.beta.client.event"}),
            )
            .expect("small unknown beta client event"),
        );
        socket
            .send_event(small)
            .await
            .expect("a smaller beta event still sends after the local rejection");
        assert!(!socket.is_closed());
        socket.close().await.expect("close the healthy beta socket");
    }

    #[tokio::test]
    async fn an_oversized_beta_inbound_event_retires_the_socket() {
        // 8-11: the inbound half of `max_message_bytes` on the beta face. A
        // peer text frame past the limit retires the socket — tungstenite's
        // capacity guard fires first with the identical predicate, the local
        // check is its defense-in-depth twin.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta oversized-inbound server");
        let address = listener
            .local_addr()
            .expect("beta oversized-inbound address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta oversized-inbound socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta oversized-inbound handshake");
            socket
                .send(Message::text("x".repeat(200)))
                .await
                .expect("send oversized beta event");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta oversized-inbound base");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect_with(BetaResponsesWebSocketConfig::new().max_message_bytes(64))
            .await
            .expect("connect the beta WebSocket with a tiny inbound limit");

        match socket.recv().await {
            Err(Error::WebSocketTransport(reason)) => assert!(
                reason.contains("capacity"),
                "expected the capacity limit, got {reason}"
            ),
            Err(Error::WebSocketProtocol(reason)) => assert_eq!(
                reason,
                "incoming beta Responses event exceeds the configured message limit"
            ),
            unexpected => panic!("expected an inbound-limit error, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "an oversized inbound event must retire the beta socket"
        );
        assert!(
            socket
                .recv()
                .await
                .expect("recv after the beta retirement")
                .is_none(),
            "a retired beta socket reports EOF on every later recv"
        );
    }

    #[tokio::test]
    async fn beta_error_envelope_is_lane_scoped_and_keeps_sibling_lanes_flowing() {
        // 17-C-2(a): on one socket, lane A receives an `error` envelope
        // (nested error object) while lane B still receives its
        // `response.created`. The envelope is delivered as `Ok` — it failed
        // one request, not the connection — so the socket stays open and the
        // sibling lane's event still arrives.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind beta lane-error server");
        let address = listener
            .local_addr()
            .expect("beta lane-error server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept beta lane-error socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("beta lane-error handshake");
            socket
                .send(Message::text(
                    json!({
                        "type": "error",
                        "error": {
                            "code": "lane_failed",
                            "message": "the lane request failed",
                            "param": null,
                            "type": "server_error"
                        },
                        "stream_id": "lane_a",
                        "sequence_number": 1
                    })
                    .to_string(),
                ))
                .await
                .expect("send lane-error envelope");
            socket
                .send(Message::text(
                    json!({
                        "type": "response.created",
                        "sequence_number": 2,
                        "stream_id": "lane_b",
                        "response": serde_json::from_str::<Value>(&response_json("in_progress"))
                            .expect("created response JSON")
                    })
                    .to_string(),
                ))
                .await
                .expect("send sibling-lane created event");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("beta lane-error base URL");
        let client = client(base_url);
        let mut socket = client
            .beta_responses()
            .connect()
            .await
            .expect("connect beta lane-error WebSocket");

        let error_event = socket
            .recv()
            .await
            .expect("receive the lane-error envelope")
            .expect("a lane-scoped error envelope is an Ok delivery");
        assert!(error_event.is_error());
        match &error_event {
            BetaResponsesServerEvent::WebSocketError(event) => {
                assert_eq!(event.error().code(), Some("lane_failed"));
                assert_eq!(event.error().error_type(), "server_error");
            }
            other => panic!("expected a WebSocketError envelope, got {other:?}"),
        }
        assert_eq!(error_event.stream_id(), Some("lane_a"));
        assert!(
            !socket.is_closed(),
            "a lane-scoped error envelope must not retire the beta socket"
        );

        let sibling = socket
            .recv()
            .await
            .expect("the sibling lane still drains")
            .expect("the sibling-lane created event decodes");
        assert_eq!(sibling.stream_id(), Some("lane_b"));
        assert!(matches!(
            sibling,
            BetaResponsesServerEvent::Response(_) | BetaResponsesServerEvent::InjectCreated(_)
        ));
        socket.close().await.expect("close beta WebSocket");
    }
}
