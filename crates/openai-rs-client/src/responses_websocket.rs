#[cfg(feature = "realtime")]
use std::{fmt, time::Duration};

#[cfg(feature = "rustls-tls")]
use std::sync::Arc;

#[cfg(feature = "realtime")]
use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, header};
#[cfg(feature = "realtime")]
use openai_rs_types::responses::{
    CreateResponseRequest, ResponseAccumulator, ResponsesClientEvent, ResponsesServerEvent,
};
use tokio::net::TcpStream;
#[cfg(feature = "realtime")]
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream,
    tungstenite::{
        self, client::IntoClientRequest, protocol::WebSocketConfig as TungsteniteConfig,
    },
};
use url::Url;

#[cfg(feature = "realtime")]
use crate::{Client, ResponseMeta, transport::deserialize_json};
use crate::{Error, TlsBackend};

#[cfg(feature = "realtime")]
const DEFAULT_MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
#[cfg(feature = "realtime")]
const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
#[cfg(feature = "realtime")]
const DEFAULT_WRITE_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(feature = "realtime")]
const DEFAULT_MAX_QUEUED_WRITE_BYTES: usize = 1024 * 1024;
#[cfg(feature = "realtime")]
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(feature = "realtime")]
const MAX_INITIAL_RECONNECTS: u32 = 10;
#[cfg(feature = "realtime")]
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Explicit policy for retrying only the initial WebSocket handshake.
///
/// An established connection is never automatically reconnected because
/// replaying a `response.create` event could duplicate model work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg(feature = "realtime")]
pub enum WebSocketReconnectPolicy {
    #[default]
    Never,
    InitialConnect {
        max_retries: u32,
        delay: Duration,
    },
}

/// Resource and reconnect limits for a Responses WebSocket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(feature = "realtime")]
pub struct ResponsesWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
    reconnect: WebSocketReconnectPolicy,
}

#[cfg(feature = "realtime")]
impl ResponsesWebSocketConfig {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
            max_queued_write_bytes: DEFAULT_MAX_QUEUED_WRITE_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            reconnect: WebSocketReconnectPolicy::Never,
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

    /// Bounds tungstenite's buffered writes. Calls to `send_*` still apply
    /// backpressure directly; there is no unbounded SDK-owned channel.
    #[must_use]
    pub const fn max_queued_write_bytes(mut self, limit: usize) -> Self {
        self.max_queued_write_bytes = limit;
        self
    }

    #[must_use]
    pub const fn write_buffer_bytes(mut self, size: usize) -> Self {
        self.write_buffer_bytes = size;
        self
    }

    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn reconnect_policy(mut self, policy: WebSocketReconnectPolicy) -> Self {
        self.reconnect = policy;
        self
    }

    fn validate(self) -> Result<Self, Error> {
        if self.max_message_bytes == 0 || self.max_frame_bytes == 0 {
            return Err(invalid_configuration(
                "WebSocket message and frame limits must be non-zero",
            ));
        }
        if self.max_queued_write_bytes <= self.write_buffer_bytes {
            return Err(invalid_configuration(
                "WebSocket queued-write limit must exceed its write-buffer size",
            ));
        }
        if self.connect_timeout.is_zero() {
            return Err(invalid_configuration(
                "WebSocket connect timeout must be non-zero",
            ));
        }
        if let WebSocketReconnectPolicy::InitialConnect { max_retries, delay } = self.reconnect {
            if max_retries > MAX_INITIAL_RECONNECTS {
                return Err(invalid_configuration(
                    "WebSocket initial reconnect count exceeds the supported limit",
                ));
            }
            if delay > MAX_RECONNECT_DELAY {
                return Err(invalid_configuration(
                    "WebSocket initial reconnect delay exceeds 60 seconds",
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

#[cfg(feature = "realtime")]
impl Default for ResponsesWebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A bounded, typed persistent connection to the Responses API.
#[cfg(feature = "realtime")]
pub struct ResponsesWebSocket {
    socket: Socket,
    meta: ResponseMeta,
    max_message_bytes: usize,
    closed: bool,
    last_close: Option<(u16, String)>,
}

#[cfg(feature = "realtime")]
impl ResponsesWebSocket {
    pub(crate) async fn connect(
        client: &Client,
        config: ResponsesWebSocketConfig,
    ) -> Result<Self, Error> {
        let config = config.validate()?;
        let transport = client.transport();
        let url = websocket_url(client.base_url())?;
        let connector = websocket_connector(url.scheme(), transport.tls_backend())?;
        let (max_retries, retry_delay) = match config.reconnect {
            WebSocketReconnectPolicy::Never => (0, Duration::ZERO),
            WebSocketReconnectPolicy::InitialConnect { max_retries, delay } => (max_retries, delay),
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
                    let meta = ResponseMeta::from_headers(response.status(), response.headers());
                    return Ok(Self {
                        socket,
                        meta,
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
                        "initial WebSocket handshake timed out".into(),
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

    /// Sends a typed `response.create` event.
    pub async fn send_create(&mut self, request: CreateResponseRequest) -> Result<(), Error> {
        self.send_event(ResponsesClientEvent::create(request)).await
    }

    /// Sends a typed `response.create` event on a FIFO WebSocket lane.
    pub async fn send_create_on_stream(
        &mut self,
        stream_id: impl Into<String>,
        request: CreateResponseRequest,
    ) -> Result<(), Error> {
        self.send_event(ResponsesClientEvent::create_on_stream(stream_id, request))
            .await
    }

    /// Sends one typed client event with bounded buffering.
    pub async fn send_event(&mut self, event: ResponsesClientEvent) -> Result<(), Error> {
        if self.closed {
            return Err(Error::WebSocketProtocol(
                "cannot send on a closed Responses WebSocket",
            ));
        }
        let encoded = serde_json::to_string(&event).map_err(Error::Encode)?;
        validate_stream_id(&encoded)?;
        if encoded.len() > self.max_message_bytes {
            return Err(Error::WebSocketProtocol(
                "outgoing Responses event exceeds the configured message limit",
            ));
        }
        self.socket
            .send(Message::text(encoded))
            .await
            .map_err(map_websocket_error)
    }

    /// Receives the next typed server event. Ping/pong control frames are
    /// handled internally. `None` means the peer completed the close handshake.
    ///
    /// Failure posture (4-19, unified by 5-08 with the Realtime socket):
    /// every transport or protocol failure — a broken connection, an oversized
    /// event, or a frame that violates the Responses event-transport contract —
    /// retires the socket (`is_closed` becomes `true`, matching openai-node,
    /// which destroys the WebSocket on any error). A failed event *decode* is
    /// the one recoverable path: the connection stays open so a malformed
    /// event need not take down an otherwise healthy session.
    pub async fn recv(&mut self) -> Result<Option<ResponsesServerEvent>, Error> {
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
                            "incoming Responses event exceeds the configured message limit",
                        ));
                    }
                    let event =
                        deserialize_json(text.as_bytes()).map_err(|error| Error::Decode {
                            source: error.source,
                            path: error.path,
                            meta_status: self.meta.status(),
                            request_id: self.meta.request_id().map(Box::<str>::from),
                            body: crate::BodyPreview::from_bytes(text.as_bytes(), false),
                        })?;
                    return Ok(Some(event));
                }
                // tungstenite queues the RFC 6455 Pong while reading the Ping
                // and flushes it on the next poll; an explicit reply here only
                // adds a redundant write path.
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
                    // A frame that violates the Responses event-transport
                    // contract retires the socket; the stream is only usable
                    // for well-formed Responses frames.
                    self.closed = true;
                    return Err(Error::WebSocketProtocol(
                        "Responses WebSocket sent a binary data message",
                    ));
                }
                Message::Frame(_) => {
                    self.closed = true;
                    return Err(Error::WebSocketProtocol(
                        "Responses WebSocket exposed an unexpected raw frame",
                    ));
                }
            }
        }
    }

    /// Receives one event and also feeds a caller-owned single-lane
    /// accumulator. Multiplexed callers should use [`Self::recv`], route by
    /// [`ResponsesServerEvent::stream_id`], then push into the matching lane's
    /// accumulator.
    pub async fn recv_into(
        &mut self,
        accumulator: &mut ResponseAccumulator,
    ) -> Result<Option<ResponsesServerEvent>, Error> {
        let event = self.recv().await?;
        if let Some(event) = &event
            && let Some(stream_event) = event.event()
        {
            accumulator.push(stream_event.clone())?;
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

#[cfg(feature = "realtime")]
impl fmt::Debug for ResponsesWebSocket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesWebSocket")
            .field("meta", &self.meta)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "realtime")]
fn websocket_url(base_url: &Url) -> Result<Url, Error> {
    let mut url = base_url.clone();
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => {
            return Err(invalid_configuration(
                "Responses WebSocket requires an HTTP(S) API base URL",
            ));
        }
    };
    url.set_scheme(websocket_scheme)
        .map_err(|()| invalid_configuration("failed to derive the Responses WebSocket scheme"))?;
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            invalid_configuration("API base URL cannot contain WebSocket path segments")
        })?;
        segments.pop_if_empty().push("responses");
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn websocket_request(
    url: &Url,
    authorization: HeaderValue,
    organization: Option<HeaderValue>,
    project: Option<HeaderValue>,
    client_request_id: Option<HeaderValue>,
) -> Result<tungstenite::http::Request<()>, Error> {
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(map_websocket_error)?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    request.headers_mut().insert(
        header::USER_AGENT,
        HeaderValue::from_static(concat!("openai-rs/", env!("CARGO_PKG_VERSION"))),
    );
    if let Some(organization) = organization {
        request
            .headers_mut()
            .insert("OpenAI-Organization", organization);
    }
    if let Some(project) = project {
        request.headers_mut().insert("OpenAI-Project", project);
    }
    if let Some(client_request_id) = client_request_id {
        request
            .headers_mut()
            .insert("X-Client-Request-Id", client_request_id);
    }
    Ok(request)
}

#[cfg(feature = "realtime")]
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

#[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
pub(crate) async fn connect_socket(
    request: tungstenite::http::Request<()>,
    config: TungsteniteConfig,
    connector: Connector,
) -> Result<(Socket, tungstenite::handshake::client::Response), tungstenite::Error> {
    tokio_tungstenite::connect_async_tls_with_config(request, Some(config), false, Some(connector))
        .await
}

#[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
pub(crate) async fn connect_socket(
    request: tungstenite::http::Request<()>,
    config: TungsteniteConfig,
    _connector: Connector,
) -> Result<(Socket, tungstenite::handshake::client::Response), tungstenite::Error> {
    tokio_tungstenite::connect_async_with_config(request, Some(config), false).await
}

pub(crate) fn websocket_connector(
    scheme: &str,
    backend: Option<TlsBackend>,
) -> Result<Connector, Error> {
    if scheme == "ws" {
        return Ok(Connector::Plain);
    }
    if scheme != "wss" {
        return Err(invalid_configuration(
            "derived Responses WebSocket URL has an unsupported scheme",
        ));
    }
    match backend {
        #[cfg(feature = "rustls-tls")]
        Some(TlsBackend::Rustls) => {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            Ok(Connector::Rustls(Arc::new(config)))
        }
        #[cfg(feature = "native-tls")]
        Some(TlsBackend::Native) => native_tls::TlsConnector::new()
            .map(Connector::NativeTls)
            .map_err(|_| Error::WebSocketTransport("failed to initialize native TLS".into())),
        None => Err(invalid_configuration(
            "secure Responses WebSocket requires a compiled TLS backend",
        )),
    }
}

pub(crate) fn retryable_connect_error(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(_) | tungstenite::Error::Tls(_)
    )
}

pub(crate) fn is_unauthorized_websocket_error(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Http(response)
            if response.status() == http::StatusCode::UNAUTHORIZED
    )
}

pub(crate) fn map_websocket_error(error: tungstenite::Error) -> Error {
    match error {
        tungstenite::Error::Http(response) => {
            let status = response.status();
            let headers = response.headers().clone();
            // tungstenite only exposes the bytes that arrived in the same read
            // as the response head ("tail"), so compare the buffered length
            // against the declared `Content-Length` to flag the preview as
            // truncated honestly instead of presenting a partial body as the
            // whole rejection payload (4-17).
            let tail = response.into_body().unwrap_or_default();
            let truncated = headers
                .get(header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|declared| tail.len() < declared);
            Error::WebSocketHandshake {
                status,
                request_id: headers
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok())
                    .map(Box::<str>::from),
                body: crate::BodyPreview::from_bytes(&tail, truncated),
            }
        }
        tungstenite::Error::Capacity(_) => {
            Error::WebSocketTransport("WebSocket capacity limit exceeded".into())
        }
        tungstenite::Error::Url(_) => {
            Error::WebSocketTransport("invalid derived WebSocket URL".into())
        }
        error => Error::WebSocketTransport(error.to_string().into()),
    }
}

fn invalid_configuration(message: impl Into<Box<str>>) -> Error {
    Error::InvalidConfiguration(message.into())
}

#[cfg(all(test, feature = "realtime"))]
mod tests {
    use std::sync::{Arc, Mutex};

    use bytes::Bytes;
    use http::StatusCode;
    use openai_rs_types::responses::ResponseStreamEvent;
    use serde_json::{Value, json};
    use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener, sync::oneshot};
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
    struct Handshake {
        path: String,
        authorization: Option<String>,
    }

    async fn websocket_server() -> (
        Client,
        oneshot::Receiver<Handshake>,
        oneshot::Receiver<Value>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind WebSocket loopback");
        let address = listener.local_addr().expect("WebSocket address");
        let (handshake_sender, handshake_receiver) = oneshot::channel();
        let (event_sender, event_receiver) = oneshot::channel();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept WebSocket");
            let handshake_sender = Arc::new(Mutex::new(Some(handshake_sender)));
            let callback = move |request: &server::Request, mut response: server::Response| {
                let authorization = request
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                if let Some(sender) = handshake_sender
                    .lock()
                    .expect("handshake sender lock")
                    .take()
                {
                    let _ = sender.send(Handshake {
                        path: request.uri().path().to_owned(),
                        authorization,
                    });
                }
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_static("req_websocket"));
                Ok::<_, server::ErrorResponse>(response)
            };
            let mut socket = accept_hdr_async(stream, callback)
                .await
                .expect("WebSocket handshake");
            let message = socket
                .next()
                .await
                .expect("client event")
                .expect("valid client message");
            let value = match message {
                Message::Text(text) => {
                    serde_json::from_slice(text.as_bytes()).expect("client event JSON")
                }
                other => panic!("unexpected client message: {other:?}"),
            };
            let _ = event_sender.send(value);

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
                .expect("send server event");
            let _ = socket.next().await;
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("WebSocket loopback base");
        let key = ApiKey::new("test-placeholder-key").expect("test key");
        let client = Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("WebSocket loopback client");
        (client, handshake_receiver, event_receiver)
    }

    #[tokio::test]
    async fn persistent_connection_sends_and_receives_typed_events() {
        let (client, handshake, client_event) = websocket_server().await;
        let mut socket = client
            .responses()
            .connect()
            .await
            .expect("connect Responses WebSocket");
        assert_eq!(socket.request_id(), Some("req_websocket"));

        socket
            .send_create_on_stream("lane_1", CreateResponseRequest::new("test-model", "hello"))
            .await
            .expect("send response.create");
        let mut accumulator = ResponseAccumulator::new();
        let event = socket
            .recv_into(&mut accumulator)
            .await
            .expect("receive event")
            .expect("one event");
        assert_eq!(event.stream_id(), Some("lane_1"));
        match event.event() {
            Some(ResponseStreamEvent::OutputTextDelta(delta)) => {
                assert_eq!(delta.delta(), "hello")
            }
            other => panic!("unexpected server event: {other:?}"),
        }
        assert_eq!(accumulator.output_text(), "hello");

        let handshake = handshake.await.expect("captured handshake");
        assert_eq!(handshake.path, "/v1/responses");
        assert_eq!(
            handshake.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let sent = client_event.await.expect("captured client event");
        assert_eq!(sent["type"], "response.create");
        assert_eq!(sent["stream_id"], "lane_1");
        assert_eq!(sent["stream"], Value::Null);

        socket.close().await.expect("close WebSocket");
        assert!(socket.is_closed());
    }

    #[test]
    fn websocket_url_is_derived_without_raw_input() {
        let base = Url::parse("https://api.openai.com/v1/").expect("official base URL");
        let url = websocket_url(&base).expect("derived URL");
        assert_eq!(url.as_str(), "wss://api.openai.com/v1/responses");
    }

    #[test]
    fn websocket_limits_reject_unbounded_or_panicking_config() {
        assert!(
            ResponsesWebSocketConfig::new()
                .max_message_bytes(0)
                .validate()
                .is_err()
        );
        assert!(
            ResponsesWebSocketConfig::new()
                .write_buffer_bytes(1024)
                .max_queued_write_bytes(1024)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn handshake_errors_carry_a_sanitized_body_preview() {
        // 4-17: the buffered rejection tail becomes a BodyPreview whose
        // truncation flag honestly reflects a wire body longer than the tail.
        let body = br#"{"error":{"message":"no such lane","type":"invalid_request_error","code":"lane_not_found"}}"#;
        let response = http::Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("x-request-id", "req_ws_401")
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Some(body.to_vec()))
            .expect("handshake rejection response");
        let error = map_websocket_error(tungstenite::Error::Http(Box::new(response)));
        let Error::WebSocketHandshake {
            status,
            request_id,
            body: preview,
        } = &error
        else {
            panic!("expected a handshake error, got {error:?}");
        };
        assert_eq!(*status, StatusCode::UNAUTHORIZED);
        assert_eq!(request_id.as_deref(), Some("req_ws_401"));
        assert!(preview.as_str().contains("no such lane"));
        assert!(!preview.is_truncated());

        // A declared Content-Length longer than the buffered tail is flagged.
        let response = http::Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::CONTENT_LENGTH, (body.len() * 2).to_string())
            .body(Some(body.to_vec()))
            .expect("truncated handshake rejection");
        let error = map_websocket_error(tungstenite::Error::Http(Box::new(response)));
        assert!(
            error
                .handshake_body()
                .expect("handshake body preview")
                .is_truncated()
        );

        // An absent body degrades to an empty, untruncated preview.
        let response = http::Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(None)
            .expect("empty handshake rejection");
        let error = map_websocket_error(tungstenite::Error::Http(Box::new(response)));
        let preview = error.handshake_body().expect("handshake body preview");
        assert_eq!(preview.as_str(), "");
        assert!(!preview.is_truncated());
    }

    /// Serves one raw HTTP rejection — head and body in a single TCP write —
    /// so the handshake fails with the JSON body buffered beside the head.
    async fn raw_handshake_rejection_server(body: &'static str) -> Client {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind raw rejection server");
        let address = listener.local_addr().expect("raw rejection address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept raw rejection");
            let mut request = vec![0_u8; 4096];
            let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                .await
                .expect("timely handshake request read");
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\nx-request-id: req_ws_401\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write raw rejection");
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("raw rejection base");
        Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("raw rejection client")
    }

    #[tokio::test]
    async fn handshake_rejection_preserves_the_json_error_body() {
        // 4-17: the shared handshake mapping covers the Responses client; the
        // Realtime and Beta sockets route their failures through the same
        // `map_websocket_error`.
        let body = r#"{"error":{"message":"workspace is suspended","type":"invalid_request_error","code":"workspace_suspended"}}"#;
        let client = raw_handshake_rejection_server(body).await;
        let error = client
            .responses()
            .connect()
            .await
            .expect_err("401 handshake rejection");
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.request_id(), Some("req_ws_401"));
        let preview = error.handshake_body().expect("handshake body preview");
        assert!(
            preview.as_str().contains("workspace is suspended"),
            "the rejection body must survive the handshake failure, got {}",
            preview.as_str()
        );
        assert!(!preview.is_truncated());
    }

    #[tokio::test]
    async fn peer_close_code_and_reason_stay_readable() {
        // 4-18: an abnormal coded close must remain distinguishable from a
        // clean frameless EOF on the Responses WebSocket too.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind coded-close server");
        let address = listener.local_addr().expect("coded-close server address");
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept coded-close socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("coded-close server handshake");
            socket
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: Utf8Bytes::from_static("response failed"),
                })))
                .await
                .expect("send coded close frame");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("coded-close client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("coded-close client");
        let mut socket = client
            .responses()
            .connect()
            .await
            .expect("connect Responses WebSocket");
        assert_eq!(
            socket.close_code(),
            None,
            "no close frame has been seen yet"
        );
        assert!(
            socket.recv().await.expect("coded close").is_none(),
            "a peer close ends the stream"
        );
        assert!(socket.is_closed());
        assert_eq!(socket.close_code(), Some(1011));
        assert_eq!(socket.close_reason(), Some("response failed"));
    }

    #[tokio::test]
    async fn binary_frame_retires_the_responses_socket() {
        // 5-08: a frame that violates the Responses event-transport contract
        // must retire the socket instead of leaving it half-alive, matching
        // the Realtime recv posture.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind binary-frame server");
        let address = listener.local_addr().expect("binary-frame server address");
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept binary-frame socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("binary-frame server handshake");
            socket
                .send(Message::Binary(Bytes::from_static(b"[1,2,3]")))
                .await
                .expect("send binary frame");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("binary-frame client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("binary-frame client");
        let mut socket = client
            .responses()
            .connect()
            .await
            .expect("connect Responses WebSocket");

        match socket.recv().await {
            Err(Error::WebSocketProtocol(reason)) => {
                assert_eq!(reason, "Responses WebSocket sent a binary data message");
            }
            unexpected => panic!("expected a protocol rejection, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "a rejected frame must retire the Responses socket"
        );
        assert!(
            socket.recv().await.expect("recv after rejection").is_none(),
            "a retired socket reports EOF on every later recv"
        );
    }
}
