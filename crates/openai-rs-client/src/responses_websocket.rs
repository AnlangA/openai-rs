use std::{fmt, time::Duration};

#[cfg(feature = "rustls-tls")]
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use openai_rs_types::responses::{
    CreateResponseRequest, ResponseAccumulator, ResponsesClientEvent, ResponsesServerEvent,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream,
    tungstenite::{
        self, Message, client::IntoClientRequest, protocol::WebSocketConfig as TungsteniteConfig,
    },
};
use url::Url;

use crate::{Client, Error, ResponseMeta, TlsBackend, transport::deserialize_json};

const DEFAULT_MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WRITE_BUFFER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_QUEUED_WRITE_BYTES: usize = 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_INITIAL_RECONNECTS: u32 = 10;
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Explicit policy for retrying only the initial WebSocket handshake.
///
/// An established connection is never automatically reconnected because
/// replaying a `response.create` event could duplicate model work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
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
pub struct ResponsesWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
    reconnect: WebSocketReconnectPolicy,
}

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

impl Default for ResponsesWebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A bounded, typed persistent connection to the Responses API.
pub struct ResponsesWebSocket {
    socket: Socket,
    meta: ResponseMeta,
    max_message_bytes: usize,
    closed: bool,
}

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
        loop {
            let authorization = transport.authorization().await?;
            let request = websocket_request(
                &url,
                authorization.header,
                transport.organization(),
                transport.project(),
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
                    });
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
    pub async fn recv(&mut self) -> Result<Option<ResponsesServerEvent>, Error> {
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
                        "Responses WebSocket sent a binary data message",
                    ));
                }
                Message::Frame(_) => {
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
        if let Some(event) = &event {
            accumulator.push(event.event().clone())?;
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
    Ok(request)
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

pub(crate) fn map_websocket_error(error: tungstenite::Error) -> Error {
    match error {
        tungstenite::Error::Http(response) => Error::WebSocketHandshake {
            status: response.status(),
            request_id: response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .map(Box::<str>::from),
        },
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use openai_rs_types::responses::ResponseStreamEvent;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use tokio_tungstenite::{accept_hdr_async, tungstenite::handshake::server};

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
            ResponseStreamEvent::OutputTextDelta(delta) => assert_eq!(delta.delta(), "hello"),
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
}
