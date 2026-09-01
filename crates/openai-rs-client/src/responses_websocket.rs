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
use tokio_tungstenite::tungstenite::{
    Message, Utf8Bytes,
    protocol::frame::{CloseFrame, coding::CloseCode},
};
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
///
/// [`WebSocketReconnectPolicy::InitialConnect`] retries only handshake-time
/// failures: transport errors (I/O, TLS), handshake timeouts, and non-101
/// rejections whose HTTP status is retryable on the REST face too — 408, 429,
/// and every status >= 500 (7-08; the numeric bound, which also covers a
/// representable non-standard 6xx, is D0264). Any other rejection — a 401
/// (after the single credential refresh) or a 4xx such as 404 — surfaces from
/// the attempt that produced it, and the REST-only `x-should-retry` override
/// and 409 stay REST contract details: a WebSocket handshake replays no
/// conflicting mutation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg(feature = "realtime")]
pub enum WebSocketReconnectPolicy {
    #[default]
    Never,
    /// Retries a failed initial handshake before surfacing its error.
    InitialConnect {
        /// Additional attempts *after* the first one, so the handshake is
        /// tried at most `1 + max_retries` times in total (capped at 10 by
        /// [`ResponsesWebSocketConfig`]'s validation).
        max_retries: u32,
        /// Fixed pause between attempts with no backoff: every pause lasts
        /// exactly `delay`, which callers are expected to keep small (the
        /// validation caps it at 60s).
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
    ///
    /// A transport failure while *writing* the frame retires the socket
    /// (`is_closed` becomes `true`), extending the recv-side posture (4-19,
    /// D0212): a connection that cannot be written to is not usable again, so
    /// later `send`/`recv` calls report the closed state instead of polling a
    /// half-broken socket. Local validation failures — an event that fails to
    /// encode, carries an invalid `stream_id`, or exceeds the configured
    /// message limit — leave the connection open, because nothing reached the
    /// wire and the socket remains healthy.
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
        match self.socket.send(Message::text(encoded)).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.closed = true;
                Err(map_websocket_error(error))
            }
        }
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

    /// Initiates the WebSocket close handshake with the RFC 6455 normal
    /// closure code 1000 and an empty reason — the same explicit-code posture
    /// as the Realtime socket (14-E-2): openai-python's `close()` defaults to
    /// `code=1000` and openai-node's to `1000`/`"OK"`, while an unframed empty
    /// close body is observed by the peer as the abnormal 1005.
    pub async fn close(&mut self) -> Result<(), Error> {
        if !self.closed {
            self.socket
                .close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: Utf8Bytes::default(),
                }))
                .await
                .map_err(map_websocket_error)?;
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

/// Derives a WebSocket URL from the configured API base for all three
/// WebSocket faces (7-22): the scheme is mapped `https`→`wss` / `http`→`ws`,
/// one `segment` is appended after the base path, and any fragment is
/// dropped. Query parameters configured on the base survive untouched: the
/// Responses faces append none of their own, and the Realtime face rejects
/// (rather than clears) only the target keys it is about to set, so a gateway
/// base such as `https://proxy.example/v1/?api-version=...` keeps its query on
/// every WebSocket face.
pub(crate) fn derive_websocket_url(
    base: &Url,
    segment: &'static str,
    face: &'static str,
) -> Result<Url, Error> {
    let mut url = base.clone();
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => {
            return Err(invalid_configuration(format!(
                "{face} WebSocket requires an HTTP(S) base URL"
            )));
        }
    };
    url.set_scheme(scheme).map_err(|()| {
        invalid_configuration(format!("failed to derive the {face} WebSocket scheme"))
    })?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| invalid_configuration(format!("{face} base URL cannot encode a path")))?;
        segments.pop_if_empty().push(segment);
    }
    url.set_fragment(None);
    Ok(url)
}

#[cfg(feature = "realtime")]
fn websocket_url(base_url: &Url) -> Result<Url, Error> {
    derive_websocket_url(base_url, "responses", "Responses")
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

/// Local pre-send guard shared by the GA and beta Responses WebSocket faces
/// (10-03 deduplicated the two identical copies): every outbound event that
/// carries a `stream_id` must be a 1-256 byte `[A-Za-z0-9_.-]` lane key, so a
/// malformed lane is rejected before anything reaches the wire.
pub(crate) fn validate_stream_id(encoded: &str) -> Result<(), Error> {
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

/// Handshake failures a `WebSocketReconnectPolicy::InitialConnect` policy may
/// replay: transport errors (I/O, TLS) plus non-101 rejections whose HTTP
/// status is retryable on the REST face (408, 429, every status >= 500 — 7-08,
/// D0264). Everything else — 4xx rejections such as 401/404, protocol
/// violations, capacity errors — surfaces from the attempt that produced it.
pub(crate) fn retryable_connect_error(error: &tungstenite::Error) -> bool {
    match error {
        tungstenite::Error::Io(_) | tungstenite::Error::Tls(_) => true,
        tungstenite::Error::Http(response) => {
            let status = response.status().as_u16();
            // The >= 500 bound is numeric, not `is_server_error()` (exactly
            // 500..=599): the REST retry fallback compares the raw code, so a
            // representable non-standard 6xx handshake rejection retries here
            // too — openai-python (`_base_client.py:851-853`) and openai-node
            // (`client.ts:1606-1607`) both use `status >= 500` (D0264).
            matches!(status, 408 | 429) || status >= 500
        }
        _ => false,
    }
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
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::Full;
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::kernel::UnknownTaggedObject;
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
        // 7-22: base query parameters survive the derivation (the Realtime
        // semantics) and a base fragment is dropped — only the scheme, path,
        // and query pairing differ per face.
        let versioned = Url::parse("https://gateway.example/v1/?api-version=2026-01-01#anchor")
            .expect("versioned base URL");
        let url = websocket_url(&versioned).expect("derived versioned URL");
        assert_eq!(
            url.as_str(),
            "wss://gateway.example/v1/responses?api-version=2026-01-01"
        );
    }

    #[test]
    fn retryable_connect_error_covers_the_rest_retry_statuses() {
        // 7-08: a non-101 rejection carrying a REST-retryable status (408/429
        // and every status >= 500) joins I/O and TLS as replayable handshake
        // failures; the remaining 4xx surface from the attempt that produced
        // them. The >= 500 bound is numeric (D0264): a representable
        // non-standard 6xx such as 600 retries exactly like the REST fallback,
        // while 499 — one below the bound — still surfaces.
        let rejection = |status: u16| {
            let response = http::Response::builder()
                .status(StatusCode::from_u16(status).expect("raw rejection status"))
                .body(None)
                .expect("handshake rejection response");
            tungstenite::Error::Http(Box::new(response))
        };
        for status in [408_u16, 429, 500, 502, 503, 599, 600] {
            assert!(
                retryable_connect_error(&rejection(status)),
                "{status} must be retryable"
            );
        }
        for status in [400_u16, 401, 403, 404, 409, 422, 499] {
            assert!(
                !retryable_connect_error(&rejection(status)),
                "{status} must surface instead of retrying"
            );
        }
        assert!(retryable_connect_error(&tungstenite::Error::Io(
            std::io::Error::other("connection reset")
        )));
        assert!(!retryable_connect_error(&tungstenite::Error::Capacity(
            tungstenite::error::CapacityError::TooManyHeaders
        )));
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

    /// Serves one raw HTTP rejection per listed status — one TCP connection
    /// each — and then accepts a single WebSocket, recording how many TCP
    /// connections reached the listener so retry counts stay assertable.
    async fn rejecting_then_accepting_handshake_server(
        rejections: &[u16],
    ) -> (Client, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retrying handshake server");
        let address = listener
            .local_addr()
            .expect("retrying handshake server address");
        let connections = Arc::new(Mutex::new(0_usize));
        let server_connections = Arc::clone(&connections);
        let rejections = rejections.to_vec();
        tokio::spawn(async move {
            for status in rejections {
                let (mut stream, _) = listener.accept().await.expect("accept rejected handshake");
                *server_connections
                    .lock()
                    .expect("retry connection counter lock") += 1;
                let mut request = vec![0_u8; 4096];
                let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                    .await
                    .expect("timely rejected handshake read");
                let status = StatusCode::from_u16(status).expect("raw rejection status code");
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: 0\r\n\r\n"
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write raw retry rejection");
            }
            let (stream, _) = listener.accept().await.expect("accept retrying WebSocket");
            *server_connections
                .lock()
                .expect("retry connection counter lock") += 1;
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("retrying WebSocket handshake");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("retrying handshake client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("retrying handshake client");
        (client, connections)
    }

    #[tokio::test]
    async fn initial_connect_retries_a_503_handshake_rejection() {
        // 7-08: a 503 handshake rejection carries a REST-retryable status, so
        // an InitialConnect policy replays the handshake instead of surfacing
        // the rejection from the first attempt.
        let (client, connections) = rejecting_then_accepting_handshake_server(&[503]).await;
        let mut socket = client
            .responses()
            .connect_with(ResponsesWebSocketConfig::new().reconnect_policy(
                WebSocketReconnectPolicy::InitialConnect {
                    max_retries: 2,
                    delay: Duration::from_millis(10),
                },
            ))
            .await
            .expect("connect after the 503 rejection");
        assert!(!socket.is_closed());
        socket.close().await.expect("close the retried WebSocket");
        assert_eq!(
            *connections.lock().expect("retry connection counter lock"),
            2,
            "the 503 must be retried exactly once before the success"
        );
    }

    #[tokio::test]
    async fn initial_connect_retries_429_rejections_until_the_budget_is_spent() {
        // 7-08: a 429 rejection is retryable too, and the budget is exactly
        // `1 + max_retries` attempts — the second 429 surfaces with no third
        // connection.
        let (client, connections) = rejecting_then_accepting_handshake_server(&[429, 429]).await;
        let error = client
            .responses()
            .connect_with(ResponsesWebSocketConfig::new().reconnect_policy(
                WebSocketReconnectPolicy::InitialConnect {
                    max_retries: 1,
                    delay: Duration::from_millis(10),
                },
            ))
            .await
            .expect_err("the second 429 exhausts the retry budget");
        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(
            *connections.lock().expect("retry connection counter lock"),
            2,
            "total attempts must equal 1 + max_retries"
        );
    }

    #[tokio::test]
    async fn send_write_failure_retires_the_responses_socket() {
        // 7-22: a failed send write leaves the socket unusable in both
        // directions, so it is retired like every recv-side failure (4-19,
        // D0212) instead of staying half-open. The write fails
        // deterministically against a `max_write_buffer_size` smaller than one
        // text frame (a 12+-byte frame against an 8-byte cap).
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind send-failure server");
        let address = listener.local_addr().expect("send-failure server address");
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept send-failure socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("send-failure server handshake");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("send-failure client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("send-failure client");
        let mut socket = client
            .responses()
            .connect_with(
                ResponsesWebSocketConfig::new()
                    .write_buffer_bytes(1)
                    .max_queued_write_bytes(8),
            )
            .await
            .expect("connect Responses WebSocket with a tiny write buffer");
        match socket
            .send_create(CreateResponseRequest::new("test-model", "hello"))
            .await
        {
            Err(Error::WebSocketTransport(reason)) => {
                assert!(
                    reason.to_lowercase().contains("buffer"),
                    "expected a write-buffer failure, got {reason}"
                );
            }
            unexpected => panic!("expected a send write failure, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "a failed send must retire the Responses socket"
        );
        assert!(
            socket
                .recv()
                .await
                .expect("recv after the send failure")
                .is_none(),
            "a retired socket reports EOF on every later recv"
        );
        assert!(
            matches!(
                socket
                    .send_create(CreateResponseRequest::new("test-model", "again"))
                    .await,
                Err(Error::WebSocketProtocol(_))
            ),
            "a later send must report the closed socket"
        );
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

    /// Accepts every TCP connection, reads the handshake request, and then
    /// parks holding the stream open — no response, no EOF — so the only way
    /// a client can finish is through its connect timeout. Connections are
    /// parked in their own tasks so retries are accepted (and counted)
    /// promptly.
    async fn hanging_handshake_server() -> (Client, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind hanging handshake server");
        let address = listener
            .local_addr()
            .expect("hanging handshake server address");
        let connections = Arc::new(Mutex::new(0_usize));
        let server_connections = Arc::clone(&connections);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                *server_connections
                    .lock()
                    .expect("hanging connection counter lock") += 1;
                tokio::spawn(async move {
                    let mut request = vec![0_u8; 4096];
                    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                        .await;
                    // Park far beyond any test-side timeout without writing.
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("hanging handshake client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("hanging handshake client");
        (client, connections)
    }

    #[tokio::test]
    async fn a_hanging_handshake_times_out_with_a_transport_error() {
        // 8-11: the handshake-timeout branch. A server that accepts TCP but
        // never answers the upgrade surfaces the dedicated timeout error
        // rather than hanging for the caller's lifetime.
        let (client, connections) = hanging_handshake_server().await;
        let error = client
            .responses()
            .connect_with(
                ResponsesWebSocketConfig::new().connect_timeout(Duration::from_millis(50)),
            )
            .await
            .expect_err("a silent handshake must time out");
        match &error {
            Error::WebSocketTransport(reason) => assert!(
                reason.contains("handshake timed out"),
                "expected the handshake-timeout error, got {reason}"
            ),
            unexpected => panic!("expected a transport error, got {unexpected:?}"),
        }
        assert_eq!(
            *connections.lock().expect("hanging connection counter lock"),
            1,
            "the default policy must not replay a timed-out handshake"
        );
    }

    #[tokio::test]
    async fn a_hanging_handshake_replays_within_the_initial_connect_budget() {
        // 8-11: an `InitialConnect` policy counts a handshake timeout as a
        // retryable failure, so the budget is exactly `1 + max_retries`
        // attempts before the timeout error surfaces.
        let (client, connections) = hanging_handshake_server().await;
        let error = client
            .responses()
            .connect_with(
                ResponsesWebSocketConfig::new()
                    .connect_timeout(Duration::from_millis(50))
                    .reconnect_policy(WebSocketReconnectPolicy::InitialConnect {
                        max_retries: 2,
                        delay: Duration::from_millis(10),
                    }),
            )
            .await
            .expect_err("the retry budget must eventually run out");
        assert!(
            matches!(&error, Error::WebSocketTransport(reason) if reason.contains("handshake timed out")),
            "expected the handshake-timeout error, got {error:?}"
        );
        assert_eq!(
            *connections.lock().expect("hanging connection counter lock"),
            3,
            "total attempts must equal 1 + max_retries"
        );
    }

    #[tokio::test]
    async fn an_oversized_send_is_a_local_error_and_keeps_the_socket_open() {
        // 8-11: the send-side half of `max_message_bytes`. The oversized event
        // is rejected before anything reaches the wire, so — unlike a write
        // failure — the socket is not retired and a later, smaller event
        // still sends.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind oversized-send server");
        let address = listener
            .local_addr()
            .expect("oversized-send server address");
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
                    .expect("oversized-send server handshake");
                    while socket.next().await.is_some() {}
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("oversized-send client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("oversized-send client");
        let mut socket = client
            .responses()
            .connect_with(ResponsesWebSocketConfig::new().max_message_bytes(64))
            .await
            .expect("connect with a tiny message limit");

        let oversized = ResponsesClientEvent::Unknown(
            UnknownTaggedObject::from_value(json!({
                "type": "future.client.event",
                "payload": "x".repeat(200),
            }))
            .expect("oversized unknown client event"),
        );
        match socket.send_event(oversized).await {
            Err(Error::WebSocketProtocol(reason)) => assert_eq!(
                reason,
                "outgoing Responses event exceeds the configured message limit"
            ),
            unexpected => panic!("expected a local message-limit error, got {unexpected:?}"),
        }
        assert!(
            !socket.is_closed(),
            "a local send rejection must not retire the socket"
        );

        let small = ResponsesClientEvent::Unknown(
            UnknownTaggedObject::from_value(json!({"type": "future.client.event"}))
                .expect("small unknown client event"),
        );
        socket
            .send_event(small)
            .await
            .expect("a smaller event still sends after the local rejection");
        assert!(!socket.is_closed());
        socket.close().await.expect("close the healthy socket");
    }

    /// Accepts every TCP connection as a WebSocket and parks it open — the
    /// fixture for send-side rejections, which never reach the peer.
    async fn accepting_websocket_server() -> Client {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind accepting WebSocket server");
        let address = listener
            .local_addr()
            .expect("accepting WebSocket server address");
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
                    .expect("accepting WebSocket server handshake");
                    while socket.next().await.is_some() {}
                });
            }
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("accepting WebSocket client base");
        Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("accepting WebSocket client")
    }

    /// Sends one `Unknown` client event carrying the given `stream_id` and
    /// asserts it is rejected locally with `expected` while the healthy socket
    /// stays open (nothing reached the wire).
    async fn assert_stream_id_rejection(stream_id: Value, expected: &'static str) {
        let client = accepting_websocket_server().await;
        let mut socket = client
            .responses()
            .connect()
            .await
            .expect("connect the accepting WebSocket");
        let event = ResponsesClientEvent::Unknown(
            UnknownTaggedObject::from_value(json!({
                "type": "future.client.event",
                "stream_id": stream_id,
            }))
            .expect("unknown client event"),
        );
        match socket.send_event(event).await {
            Err(Error::WebSocketProtocol(reason)) => assert_eq!(reason, expected),
            unexpected => panic!("expected a stream_id rejection, got {unexpected:?}"),
        }
        assert!(
            !socket.is_closed(),
            "a local stream_id rejection must not retire the socket"
        );
        socket.close().await.expect("close the healthy socket");
    }

    /// 10-03: the empty-lane rejection branch of the shared pre-send guard,
    /// exercised end-to-end through a verbatim `Unknown` event.
    #[tokio::test]
    async fn an_empty_stream_id_unknown_event_is_rejected_locally() {
        assert_stream_id_rejection(json!(""), "stream_id must match ^[A-Za-z0-9_.-]{1,256}$").await;
    }

    /// 10-03: the over-long-lane rejection branch (a 257-byte key).
    #[tokio::test]
    async fn an_overlong_stream_id_unknown_event_is_rejected_locally() {
        assert_stream_id_rejection(
            json!("a".repeat(257)),
            "stream_id must match ^[A-Za-z0-9_.-]{1,256}$",
        )
        .await;
    }

    /// 10-03: the illegal-character rejection branch (`/` is outside the
    /// lane alphabet).
    #[tokio::test]
    async fn an_illegal_character_stream_id_unknown_event_is_rejected_locally() {
        assert_stream_id_rejection(
            json!("lane/1"),
            "stream_id must match ^[A-Za-z0-9_.-]{1,256}$",
        )
        .await;
    }

    /// 10-03: the non-string rejection branch of the shared guard.
    #[tokio::test]
    async fn a_non_string_stream_id_unknown_event_is_rejected_locally() {
        assert_stream_id_rejection(json!(7), "stream_id must be a string").await;
    }

    #[tokio::test]
    async fn an_oversized_inbound_event_retires_the_responses_socket() {
        // 8-11: the inbound half of `max_message_bytes`. A peer text frame
        // larger than the configured limit retires the socket — tungstenite's
        // capacity guard fires first with the identical predicate, and the
        // local check is the defense-in-depth twin — and a retired socket
        // reports EOF on every later recv.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind oversized-inbound server");
        let address = listener
            .local_addr()
            .expect("oversized-inbound server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept oversized-inbound socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("oversized-inbound server handshake");
            socket
                .send(Message::text("x".repeat(200)))
                .await
                .expect("send oversized inbound event");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("oversized-inbound client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("oversized-inbound client");
        let mut socket = client
            .responses()
            .connect_with(ResponsesWebSocketConfig::new().max_message_bytes(64))
            .await
            .expect("connect with a tiny inbound limit");

        match socket.recv().await {
            Err(Error::WebSocketTransport(reason)) => assert!(
                reason.contains("capacity"),
                "expected the capacity limit, got {reason}"
            ),
            Err(Error::WebSocketProtocol(reason)) => assert_eq!(
                reason,
                "incoming Responses event exceeds the configured message limit"
            ),
            unexpected => panic!("expected an inbound-limit error, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "an oversized inbound event must retire the socket"
        );
        assert!(
            socket
                .recv()
                .await
                .expect("recv after retirement")
                .is_none(),
            "a retired socket reports EOF on every later recv"
        );
    }

    /// Serves two workload token exchanges: `access_one` then `access_two`.
    async fn two_token_exchange_server() -> (Url, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind workload exchange server");
        let address = listener
            .local_addr()
            .expect("workload exchange server address");
        let exchanges = Arc::new(Mutex::new(0_usize));
        let server_exchanges = Arc::clone(&exchanges);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let exchanges = Arc::clone(&server_exchanges);
                tokio::spawn(async move {
                    let service = service_fn(|_request: Request<Incoming>| {
                        let exchanges = Arc::clone(&exchanges);
                        async move {
                            let attempt = {
                                let mut exchanges =
                                    exchanges.lock().expect("exchange counter lock");
                                let attempt = *exchanges;
                                *exchanges += 1;
                                attempt
                            };
                            let token = if attempt == 0 {
                                "access_one"
                            } else {
                                "access_two"
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(StatusCode::OK)
                                    .header(header::CONTENT_TYPE, "application/json")
                                    .body(Full::new(Bytes::from(format!(
                                        r#"{{"access_token":"{token}","expires_in":3600}}"#
                                    ))))
                                    .expect("workload exchange response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (
            Url::parse(&format!("http://{address}/oauth/token")).expect("workload exchange URL"),
            exchanges,
        )
    }

    /// Rejects the first WebSocket handshake with a raw 401, then accepts the
    /// replay and reports every authorization header it saw, in order.
    async fn rejecting_then_accepting_websocket_server() -> (Url, oneshot::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind 401-then-accept server");
        let address = listener
            .local_addr()
            .expect("401-then-accept server address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let mut authorizations = Vec::new();

            let (mut stream, _) = listener
                .accept()
                .await
                .expect("accept the rejected handshake");
            let mut request = vec![0_u8; 4096];
            let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut request))
                .await
                .expect("timely rejected handshake read")
                .unwrap_or_default();
            let head = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
            if let Some(value) = head
                .lines()
                .find_map(|line| line.strip_prefix("authorization:"))
            {
                authorizations.push(value.trim().to_owned());
            }
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 0\r\n\r\n",
                )
                .await
                .expect("write the raw 401 rejection");

            let (stream, _) = listener
                .accept()
                .await
                .expect("accept the replayed handshake");
            let authorizations = Arc::new(Mutex::new(Some(authorizations)));
            let sender = Arc::new(Mutex::new(Some(sender)));
            let callback = move |request: &server::Request, response: server::Response| {
                let authorization = request
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                if let Some(authorization) = authorization
                    && let Some(mut authorizations) =
                        authorizations.lock().expect("authorization lock").take()
                {
                    authorizations.push(authorization);
                    if let Some(sender) = sender.lock().expect("sender lock").take() {
                        let _ = sender.send(authorizations);
                    }
                }
                Ok::<_, server::ErrorResponse>(response)
            };
            let mut socket = accept_hdr_async(stream, callback)
                .await
                .expect("replayed WebSocket handshake");
            while socket.next().await.is_some() {}
        });
        (
            Url::parse(&format!("http://{address}/v1/")).expect("401-then-accept client base"),
            receiver,
        )
    }

    #[cfg(feature = "workload-identity")]
    #[tokio::test]
    async fn a_401_handshake_refreshes_the_workload_credential_and_replays_once() {
        // 8-11: the WebSocket-side twin of the REST 401 refresh lane. A
        // workload-identity client whose first handshake is rejected with 401
        // invalidates the cached token generation, exchanges a fresh token,
        // and replays the handshake exactly once — the REST face of the same
        // refresh is pinned in `workload_identity.rs`
        // (`api_401_invalidates_generation_and_replays_once`).
        use crate::workload_identity::{
            SubjectToken, SubjectTokenProviderError, SubjectTokenProviderFn, SubjectTokenType,
            WorkloadIdentityConfig,
        };

        let (exchange_url, exchanges) = two_token_exchange_server().await;
        let (api_url, authorizations) = rejecting_then_accepting_websocket_server().await;
        let provider = SubjectTokenProviderFn::new(SubjectTokenType::Jwt, || async {
            SubjectToken::new("subject.jwt.token").map_err(|_| SubjectTokenProviderError::new())
        });
        let config = WorkloadIdentityConfig::new("idp_test", "svc_test", provider)
            .expect("workload config")
            .with_token_exchange_url(exchange_url);
        let client = Client::workload_identity_builder(config)
            .base_url(api_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("workload WebSocket client");

        let mut socket = client
            .responses()
            .connect()
            .await
            .expect("the refreshed credential must complete the replayed handshake");
        assert!(!socket.is_closed());
        socket.close().await.expect("close the refreshed socket");

        assert_eq!(
            *exchanges.lock().expect("exchange counter lock"),
            2,
            "the 401 must invalidate the cached generation and force one new exchange"
        );
        let authorizations = authorizations.await.expect("captured authorizations");
        // The first header was scraped from the lowercased raw rejection head,
        // the second from the accepted handshake, so compare case-blind.
        assert_eq!(authorizations.len(), 2);
        assert!(
            authorizations[0].eq_ignore_ascii_case("bearer access_one"),
            "the rejected handshake must carry the first token, got {}",
            authorizations[0]
        );
        assert_eq!(
            authorizations[1], "Bearer access_two",
            "the replayed handshake must carry the refreshed bearer token"
        );
    }
}
