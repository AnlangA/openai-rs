//! Client resources for the preview Responses multi-agent API.

use std::{
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
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig as TungsteniteConfig},
};
use url::Url;

use crate::{
    ApiResponse, BodyPreview, Client, Error, ResponseMeta, StreamError,
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
    PathSegment::encoded(response_id.as_str()).map_err(Error::InvalidPathSegment)
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
