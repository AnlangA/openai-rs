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
use http::{Method, StatusCode, header};
use openai_rs_types::{
    ResponseId,
    beta_responses::{
        BetaCompactResponseRequest, BetaCompactedResponse, BetaCountInputTokensRequest,
        BetaCreateResponseRequest, BetaCreateStreamingResponseRequest, BetaInputTokenCountResponse,
        BetaListInputItemsParams, BetaResponse, BetaResponseInjectEvent, BetaResponseItemList,
        BetaResponseStreamEvent, BetaResponsesClientEvent, BetaResponsesServerEvent,
        BetaRetrieveResponseParams, BetaRetrieveResponseStreamParams,
    },
    responses::DeletedResponse,
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
        connect_socket, is_unauthorized_websocket_error, map_websocket_error,
        retryable_connect_error, websocket_connector, websocket_request,
    },
    sse::{SseDispatch, SseEndpointPolicy, SseFrame, SseLimits, SseStreamDecoder, SseStreamState},
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];
const OK_OR_NO_CONTENT: &[StatusCode] = &[StatusCode::OK, StatusCode::NO_CONTENT];

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
        self.client
            .transport()
            .execute_json::<CreateBetaResponse, _>(
                &path,
                Some(&BetaOnlyQuery::VALUE),
                Some(&request),
            )
            .await
    }

    /// Creates a beta response and decodes its SSE events incrementally.
    pub async fn create_stream(
        &self,
        request: BetaCreateStreamingResponseRequest,
    ) -> Result<BetaResponseEventStream, Error> {
        let path = [PathSegment::literal("responses")];
        let response = self
            .client
            .transport()
            .send::<CreateStreamingBetaResponse, _>(
                &path,
                Some(&BetaOnlyQuery::VALUE),
                Some(&request),
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
        self.client
            .transport()
            .execute_json::<RetrieveBetaResponse, _>(&path, Some(&query), None)
            .await
    }

    /// Retrieves or resumes a stored beta response SSE stream.
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
            .send::<RetrieveStreamingBetaResponse, _>(&path, Some(&query), None)
            .await?;
        BetaResponseEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Deletes a stored beta response.
    pub async fn delete(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<DeleteResponseResult>, Error> {
        let path = response_path(response_id)?;
        let response = self
            .client
            .transport()
            .execute_optional_json::<DeleteBetaResponse, _>(
                &path,
                Some(&BetaOnlyQuery::VALUE),
                None,
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
        self.client
            .transport()
            .execute_json::<CancelBetaResponse, _>(&path, Some(&BetaOnlyQuery::VALUE), None)
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
        self.client
            .transport()
            .execute_json::<CompactBetaResponse, _>(
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
        self.client
            .transport()
            .execute_json::<ListBetaResponseInputItems, _>(&path, Some(&query), None)
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
        self.client
            .transport()
            .execute_json::<CountBetaResponseInputTokens, _>(
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
            let dispatches = match decoder.finish() {
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
                    Ok(Some(event)) => yield Ok(event),
                    Ok(None) => return,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BetaWebSocketReconnectPolicy {
    #[default]
    Never,
    InitialConnect {
        max_retries: u32,
        delay: Duration,
    },
}

/// Bounded resource configuration for a beta Responses WebSocket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BetaResponsesWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
    reconnect: BetaWebSocketReconnectPolicy,
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

    fn tungstenite(self) -> TungsteniteConfig {
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
            let request = websocket_request(
                &url,
                authorization.header,
                transport.organization(),
                transport.project(),
                transport.client_request_id(),
            )?;
            let connect = connect_socket(request, config.tungstenite(), connector.clone());
            match tokio::time::timeout(config.connect_timeout, connect).await {
                Ok(Ok((socket, response))) => {
                    return Ok(Self {
                        socket,
                        meta: ResponseMeta::from_headers(response.status(), response.headers()),
                        max_message_bytes: config.max_message_bytes,
                        closed: false,
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

    /// Atomically injects typed client-owned output items.
    pub async fn send_inject(&mut self, event: BetaResponseInjectEvent) -> Result<(), Error> {
        self.send_event(BetaResponsesClientEvent::inject(event))
            .await
    }

    /// Sends any typed beta client event with bounded buffering.
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
        self.socket
            .send(Message::text(encoded))
            .await
            .map_err(map_websocket_error)
    }

    /// Receives the next typed beta server event.
    pub async fn recv(&mut self) -> Result<Option<BetaResponsesServerEvent>, Error> {
        if self.closed {
            return Ok(None);
        }
        loop {
            let Some(message) = self.socket.next().await else {
                self.closed = true;
                return Ok(None);
            };
            match message.map_err(map_websocket_error)? {
                Message::Text(text) => {
                    if text.len() > self.max_message_bytes {
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
                Message::Ping(payload) => {
                    self.socket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(map_websocket_error)?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => {
                    self.closed = true;
                    return Ok(None);
                }
                Message::Binary(_) => {
                    return Err(Error::WebSocketProtocol(
                        "beta Responses WebSocket sent a binary data message",
                    ));
                }
                Message::Frame(_) => {
                    return Err(Error::WebSocketProtocol(
                        "beta Responses WebSocket exposed an unexpected raw frame",
                    ));
                }
            }
        }
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
    let mut url = base_url.clone();
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => {
            return Err(invalid_configuration(
                "beta Responses WebSocket requires an HTTP(S) API base URL",
            ));
        }
    };
    url.set_scheme(scheme).map_err(|()| {
        invalid_configuration("failed to derive the beta Responses WebSocket scheme")
    })?;
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            invalid_configuration("API base URL cannot contain WebSocket path segments")
        })?;
        segments.pop_if_empty().push("responses");
    }
    // The pinned beta Node oracle deliberately uses `/responses` without the
    // REST-only `beta=true` query parameter.
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn validate_stream_id(encoded: &str) -> Result<(), Error> {
    let value = serde_json::from_str::<serde_json::Value>(encoded).map_err(Error::Encode)?;
    let Some(stream_id) = value.get("stream_id") else {
        return Ok(());
    };
    let Some(stream_id) = stream_id.as_str() else {
        return Err(Error::WebSocketProtocol("stream_id must be a string"));
    };
    if stream_id.is_empty()
        || stream_id.len() > 256
        || !stream_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(Error::WebSocketProtocol(
            "stream_id must match ^[A-Za-z0-9_.-]{1,256}$",
        ));
    }
    Ok(())
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
        BetaAgentInputText, BetaAgentMessage, BetaMultiAgentAction, BetaMultiAgentCallOutput,
        BetaMultiAgentConfig, BetaMultiAgentOutputText, BetaResponseIncludable,
        BetaResponseInputItem, BetaResponseItemOrder,
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use tokio_tungstenite::{accept_hdr_async, tungstenite::handshake::server};

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
    async fn create_uses_beta_query_and_typed_multi_agent_body_without_beta_header() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            "application/json",
            response_json("completed"),
        )
        .await;
        let routed = BetaAgentMessage::new(
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
        assert_eq!(captured.beta_header, None);
        let body: Value = serde_json::from_slice(&captured.body).expect("create JSON");
        assert_eq!(body["multi_agent"]["enabled"], true);
        assert_eq!(body["input"][0]["type"], "agent_message");
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
            "/v1/responses/resp%2Fa%20b?beta=true&include=file_search_call.results"
        );

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
            "/v1/responses/resp_1/input_items?beta=true&after=item_0&include=reasoning.encrypted_content&limit=20&order=asc"
        );

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
        let body: Value = serde_json::from_slice(&request.body).expect("stream JSON");
        assert_eq!(body["stream"], true);
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
                [BetaResponseInputItem::from(BetaMultiAgentCallOutput::new(
                    BetaMultiAgentAction::WaitAgent,
                    "call_1",
                    [BetaMultiAgentOutputText::new("done")],
                ))],
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
        assert_eq!(handshake.beta_header, None);
        let sent = sent_event.await.expect("captured inject event");
        assert_eq!(sent["type"], "response.inject");
        assert_eq!(sent["input"][0]["type"], "multi_agent_call_output");

        socket.close().await.expect("close beta WebSocket");
    }

    #[test]
    fn websocket_url_matches_pinned_node_oracle_without_beta_query() {
        let base = Url::parse("https://api.openai.com/v1/").expect("official base URL");
        let url = beta_websocket_url(&base).expect("derived beta WebSocket URL");
        assert_eq!(url.as_str(), "wss://api.openai.com/v1/responses");
    }
}
