//! GA Realtime WebSocket, WebRTC signaling, and SIP call-control clients.
//!
//! This module establishes signaling and event transports. It intentionally
//! does not capture devices, encode media, implement RTP, or manage a WebRTC
//! peer connection.

use std::{fmt, time::Duration};

use futures_util::{SinkExt, StreamExt};
use http::{Method, StatusCode, header};
use openai_rs_types::realtime::{
    RealtimeCallAcceptRequest, RealtimeCallCreateRequest, RealtimeCallReferRequest,
    RealtimeCallRejectRequest, RealtimeClientEvent, RealtimeClientEventInputAudioBufferAppend,
    RealtimeClientEventResponseCancel, RealtimeCreateClientSecretRequest,
    RealtimeCreateClientSecretResponse, RealtimeSdp, RealtimeServerEvent,
};
use openai_rs_types::{ModelId, Omittable};
use reqwest::multipart::{Form, Part};
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig as TungsteniteConfig};
use url::Url;

use crate::{
    ApiResponse, BodyPreview, Client, Error, ResponseMeta,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    responses_websocket::{
        Socket, connect_socket, is_unauthorized_websocket_error, map_websocket_error,
        websocket_connector, websocket_request,
    },
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];
const CREATED: StatusCode = StatusCode::CREATED;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WRITE_BUFFER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_QUEUED_WRITE_BYTES: usize = 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// GA Realtime API resource facade.
#[derive(Clone, Debug)]
pub struct Realtime {
    client: Client,
}

impl Realtime {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Opens a GA Realtime WebSocket for one model. The URL is derived from the
    /// configured Platform base and cannot be supplied by the caller.
    pub async fn connect(&self, model: impl Into<ModelId>) -> Result<RealtimeWebSocket, Error> {
        self.connect_with(model, RealtimeWebSocketConfig::default())
            .await
    }

    pub async fn connect_with(
        &self,
        model: impl Into<ModelId>,
        config: RealtimeWebSocketConfig,
    ) -> Result<RealtimeWebSocket, Error> {
        RealtimeWebSocket::connect(&self.client, model.into(), config).await
    }

    /// Creates a short-lived client secret. The returned secret remains in a
    /// redacting wire-secret type.
    pub async fn create_client_secret(
        &self,
        request: RealtimeCreateClientSecretRequest,
    ) -> Result<ApiResponse<RealtimeCreateClientSecretResponse>, Error> {
        let path = [
            PathSegment::literal("realtime"),
            PathSegment::literal("client_secrets"),
        ];
        self.client
            .transport()
            .execute_json::<CreateClientSecret, ()>(&path, None, Some(&request))
            .await
    }

    /// Exchanges an SDP offer and optional typed session configuration for an
    /// SDP answer. Media capture, codecs, ICE, DTLS, RTP, and peer-connection
    /// management remain the caller's responsibility.
    pub async fn create_call(
        &self,
        request: RealtimeCallCreateRequest,
    ) -> Result<ApiResponse<RealtimeCallCreated>, Error> {
        let transport = self.client.transport();
        let url = transport.operation_url(&[
            PathSegment::literal("realtime"),
            PathSegment::literal("calls"),
        ])?;
        let RealtimeCallCreateRequest { sdp, session, .. } = request;
        let sdp_part = Part::text(sdp.0)
            .mime_str("application/sdp")
            .map_err(Error::from_reqwest)?;
        let mut form = Form::new().part("sdp", sdp_part);
        match session {
            Omittable::Value(session) => {
                let session = serde_json::to_string(&session).map_err(Error::Encode)?;
                let part = Part::text(session)
                    .mime_str("application/json")
                    .map_err(Error::from_reqwest)?;
                form = form.part("session", part);
            }
            Omittable::Omitted => {}
            _ => {
                return Err(Error::InvalidConfiguration(
                    "unsupported Realtime call session state".into(),
                ));
            }
        }
        let authorization = transport.authorization().await?;
        let request = transport
            .request_builder(
                reqwest::Method::POST,
                url,
                "application/sdp",
                authorization.header.clone(),
            )
            .timeout(transport.overall_timeout())
            .multipart(form)
            .build()
            .map_err(Error::from_reqwest)?;
        transport.ensure_same_origin(request.url())?;
        let response = transport
            .http()
            .execute(request)
            .await
            .map_err(Error::from_reqwest)?;
        if response.status() != CREATED {
            if response.status() == StatusCode::UNAUTHORIZED {
                let _ = transport
                    .invalidate_authorization(authorization.generation)
                    .await;
            }
            return Err(transport.error_from_response(response).await);
        }
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                Error::InvalidConfiguration(
                    "Realtime call response is missing a valid Location header".into(),
                )
            })?
            .to_owned();
        let call_id = validate_call_location(self.client.base_url(), &location)?;
        let response = transport.decode_text(response, "application/sdp").await?;
        let (answer, meta) = response.into_parts();
        Ok(ApiResponse::new(
            RealtimeCallCreated {
                sdp: RealtimeSdp(answer),
                location: location.into_boxed_str(),
                call_id,
            },
            meta,
        ))
    }

    /// Accepts one incoming SIP call. This side effect is never retried.
    pub async fn accept_call(
        &self,
        call_id: &str,
        request: RealtimeCallAcceptRequest,
    ) -> Result<ApiResponse<()>, Error> {
        let path = call_action_path(call_id, "accept")?;
        self.client
            .transport()
            .execute_empty::<AcceptCall, ()>(&path, None, Some(&request))
            .await
    }

    /// Rejects one incoming SIP call with an optional status override.
    pub async fn reject_call(
        &self,
        call_id: &str,
        request: RealtimeCallRejectRequest,
    ) -> Result<ApiResponse<()>, Error> {
        let path = call_action_path(call_id, "reject")?;
        self.client
            .transport()
            .execute_empty::<RejectCall, ()>(&path, None, Some(&request))
            .await
    }

    /// Rejects a call using the service-default 603 status.
    pub async fn reject_call_default(&self, call_id: &str) -> Result<ApiResponse<()>, Error> {
        let path = call_action_path(call_id, "reject")?;
        self.client
            .transport()
            .execute_empty::<RejectCallDefault, ()>(&path, None, None)
            .await
    }

    /// Hangs up an active SIP or WebRTC call without automatic replay.
    pub async fn hangup_call(&self, call_id: &str) -> Result<ApiResponse<()>, Error> {
        let path = call_action_path(call_id, "hangup")?;
        self.client
            .transport()
            .execute_empty::<HangupCall, ()>(&path, None, None)
            .await
    }

    /// Transfers an active SIP call without automatic replay.
    pub async fn refer_call(
        &self,
        call_id: &str,
        request: RealtimeCallReferRequest,
    ) -> Result<ApiResponse<()>, Error> {
        let path = call_action_path(call_id, "refer")?;
        self.client
            .transport()
            .execute_empty::<ReferCall, ()>(&path, None, Some(&request))
            .await
    }
}

/// SDP answer and validated follow-up call identifier.
pub struct RealtimeCallCreated {
    sdp: RealtimeSdp,
    location: Box<str>,
    call_id: Box<str>,
}

impl RealtimeCallCreated {
    #[must_use]
    pub const fn sdp(&self) -> &RealtimeSdp {
        &self.sdp
    }

    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }
}

impl fmt::Debug for RealtimeCallCreated {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeCallCreated")
            .field("sdp", &"[REDACTED]")
            .field("location", &"[REDACTED]")
            .field("call_id", &self.call_id)
            .finish()
    }
}

/// Bounds for a GA Realtime WebSocket. There is deliberately no automatic
/// reconnect policy because client events may have side effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
}

impl RealtimeWebSocketConfig {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
            max_queued_write_bytes: DEFAULT_MAX_QUEUED_WRITE_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    #[must_use]
    pub const fn max_message_bytes(mut self, value: usize) -> Self {
        self.max_message_bytes = value;
        self
    }

    #[must_use]
    pub const fn max_frame_bytes(mut self, value: usize) -> Self {
        self.max_frame_bytes = value;
        self
    }

    #[must_use]
    pub const fn max_queued_write_bytes(mut self, value: usize) -> Self {
        self.max_queued_write_bytes = value;
        self
    }

    #[must_use]
    pub const fn connect_timeout(mut self, value: Duration) -> Self {
        self.connect_timeout = value;
        self
    }

    fn validate(self) -> Result<Self, Error> {
        if self.max_message_bytes == 0 || self.max_frame_bytes == 0 {
            return Err(Error::InvalidConfiguration(
                "Realtime WebSocket limits must be non-zero".into(),
            ));
        }
        if self.max_queued_write_bytes <= self.write_buffer_bytes {
            return Err(Error::InvalidConfiguration(
                "Realtime queued-write limit must exceed its write-buffer size".into(),
            ));
        }
        if self.connect_timeout.is_zero() {
            return Err(Error::InvalidConfiguration(
                "Realtime connect timeout must be non-zero".into(),
            ));
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

impl Default for RealtimeWebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A typed GA Realtime WebSocket.
pub struct RealtimeWebSocket {
    socket: Socket,
    meta: ResponseMeta,
    max_message_bytes: usize,
    closed: bool,
}

impl RealtimeWebSocket {
    async fn connect(
        client: &Client,
        model: ModelId,
        config: RealtimeWebSocketConfig,
    ) -> Result<Self, Error> {
        let config = config.validate()?;
        let url = realtime_websocket_url(client.base_url(), &model)?;
        let transport = client.transport();
        let connector = websocket_connector(url.scheme(), transport.tls_backend())?;
        let mut auth_refreshed = false;
        let (socket, response) = loop {
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
                Ok(Ok(connection)) => break connection,
                Ok(Err(error))
                    if generation.is_some()
                        && !auth_refreshed
                        && is_unauthorized_websocket_error(&error) =>
                {
                    let _ = transport.invalidate_authorization(generation).await;
                    auth_refreshed = true;
                }
                Ok(Err(error)) => return Err(map_websocket_error(error)),
                Err(_) => {
                    return Err(Error::WebSocketTransport(
                        "Realtime handshake timed out".into(),
                    ));
                }
            }
        };
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        Ok(Self {
            socket,
            meta,
            max_message_bytes: config.max_message_bytes,
            closed: false,
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

    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    pub async fn send(&mut self, event: RealtimeClientEvent) -> Result<(), Error> {
        if self.closed {
            return Err(Error::WebSocketProtocol(
                "cannot send on a closed Realtime WebSocket",
            ));
        }
        let encoded = serde_json::to_string(&event).map_err(Error::Encode)?;
        if encoded.len() > self.max_message_bytes {
            return Err(Error::WebSocketProtocol(
                "outgoing Realtime event exceeds the configured message limit",
            ));
        }
        self.socket
            .send(Message::text(encoded))
            .await
            .map_err(map_websocket_error)
    }

    /// Appends raw audio bytes; the typed event performs base64 encoding.
    pub async fn append_audio(&mut self, audio: impl Into<Vec<u8>>) -> Result<(), Error> {
        self.send(RealtimeClientEventInputAudioBufferAppend::new(audio).into())
            .await
    }

    /// Cancels a specific or current response without reconnect/replay.
    pub async fn cancel_response(&mut self, response_id: Option<&str>) -> Result<(), Error> {
        let event = match response_id {
            Some(response_id) => RealtimeClientEventResponseCancel::for_response(response_id),
            None => RealtimeClientEventResponseCancel::default(),
        };
        self.send(event.into()).await
    }

    pub async fn recv(&mut self) -> Result<Option<RealtimeServerEvent>, Error> {
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
                            "incoming Realtime event exceeds the configured message limit",
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
                Message::Ping(payload) => self
                    .socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(map_websocket_error)?,
                Message::Pong(_) => {}
                Message::Close(_) => {
                    self.closed = true;
                    return Ok(None);
                }
                Message::Binary(_) => {
                    return Err(Error::WebSocketProtocol(
                        "Realtime WebSocket sent a binary data message",
                    ));
                }
                Message::Frame(_) => {
                    return Err(Error::WebSocketProtocol(
                        "Realtime WebSocket exposed an unexpected raw frame",
                    ));
                }
            }
        }
    }

    pub async fn close(&mut self) -> Result<(), Error> {
        if !self.closed {
            self.socket.close(None).await.map_err(map_websocket_error)?;
            self.closed = true;
        }
        Ok(())
    }
}

impl fmt::Debug for RealtimeWebSocket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeWebSocket")
            .field("meta", &self.meta)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

fn realtime_websocket_url(base: &Url, model: &ModelId) -> Result<Url, Error> {
    let mut url = base.clone();
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => {
            return Err(Error::InvalidConfiguration(
                "Realtime WebSocket requires an HTTP(S) base URL".into(),
            ));
        }
    };
    url.set_scheme(scheme).map_err(|()| {
        Error::InvalidConfiguration("failed to derive Realtime WebSocket scheme".into())
    })?;
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            Error::InvalidConfiguration("Realtime base URL cannot encode a path".into())
        })?;
        segments.pop_if_empty().push("realtime");
    }
    url.query_pairs_mut().append_pair("model", model.as_str());
    Ok(url)
}

fn call_action_path<'a>(
    call_id: &'a str,
    action: &'static str,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("realtime"),
        PathSegment::literal("calls"),
        PathSegment::parameter("call_id", call_id)?,
        PathSegment::literal(action),
    ])
}

fn validate_call_location(base: &Url, location: &str) -> Result<Box<str>, Error> {
    if Url::parse(location).is_ok() {
        return Err(Error::InvalidConfiguration(
            "Realtime call Location must be relative".into(),
        ));
    }
    let resolved = base.join(location).map_err(|_| {
        Error::InvalidConfiguration("Realtime call Location is not a valid relative URL".into())
    })?;
    if resolved.scheme() != base.scheme()
        || resolved.host_str() != base.host_str()
        || resolved.port_or_known_default() != base.port_or_known_default()
    {
        return Err(Error::InvalidConfiguration(
            "Realtime call Location escaped the configured origin".into(),
        ));
    }
    if resolved.query().is_some() || resolved.fragment().is_some() {
        return Err(Error::InvalidConfiguration(
            "Realtime call Location must not contain query or fragment data".into(),
        ));
    }
    let mut prefix = base.path().to_owned();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    prefix.push_str("realtime/calls/");
    let call_id = resolved
        .path()
        .strip_prefix(&prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| {
            Error::InvalidConfiguration("Realtime call Location does not identify one call".into())
        })?;
    Ok(call_id.into())
}

macro_rules! operation {
    ($name:ident, $request:ty, $response:ty, $route:literal, $encoding:expr, $mode:expr) => {
        struct $name;
        impl Sealed for $name {}
        impl Operation for $name {
            type Request = $request;
            type Response = $response;
            const META: OperationMeta = OperationMeta {
                id: stringify!($name),
                method: Method::POST,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: $encoding,
                response_mode: $mode,
                retry: RetryClass::Never,
                success_statuses: OK,
            };
        }
    };
}

operation!(
    CreateClientSecret,
    RealtimeCreateClientSecretRequest,
    RealtimeCreateClientSecretResponse,
    "/realtime/client_secrets",
    RequestEncoding::Json,
    ResponseMode::Json
);
operation!(
    AcceptCall,
    RealtimeCallAcceptRequest,
    (),
    "/realtime/calls/{call_id}/accept",
    RequestEncoding::Json,
    ResponseMode::Empty
);
operation!(
    RejectCall,
    RealtimeCallRejectRequest,
    (),
    "/realtime/calls/{call_id}/reject",
    RequestEncoding::Json,
    ResponseMode::Empty
);
operation!(
    RejectCallDefault,
    (),
    (),
    "/realtime/calls/{call_id}/reject",
    RequestEncoding::None,
    ResponseMode::Empty
);
operation!(
    HangupCall,
    (),
    (),
    "/realtime/calls/{call_id}/hangup",
    RequestEncoding::None,
    ResponseMode::Empty
);
operation!(
    ReferCall,
    RealtimeCallReferRequest,
    (),
    "/realtime/calls/{call_id}/refer",
    RequestEncoding::Json,
    ResponseMode::Empty
);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::realtime::RealtimeSessionCreateRequest;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use tokio_tungstenite::{accept_hdr_async, tungstenite::handshake::server};

    use super::*;
    use crate::ApiKey;

    #[derive(Debug)]
    struct WebSocketHandshake {
        path_and_query: String,
        authorization: Option<String>,
    }

    async fn websocket_server() -> (
        Client,
        oneshot::Receiver<WebSocketHandshake>,
        oneshot::Receiver<Vec<Value>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Realtime WebSocket");
        let address = listener.local_addr().expect("Realtime WebSocket address");
        let (handshake_sender, handshake_receiver) = oneshot::channel();
        let (events_sender, events_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept Realtime socket");
            let handshake_sender = Arc::new(Mutex::new(Some(handshake_sender)));
            let callback = move |request: &server::Request, mut response: server::Response| {
                let authorization = request
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                if let Some(sender) = handshake_sender
                    .lock()
                    .expect("Realtime handshake lock")
                    .take()
                {
                    let _ = sender.send(WebSocketHandshake {
                        path_and_query: request
                            .uri()
                            .path_and_query()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                        authorization,
                    });
                }
                response.headers_mut().insert(
                    "x-request-id",
                    http::HeaderValue::from_static("req_realtime_ws"),
                );
                Ok::<_, server::ErrorResponse>(response)
            };
            let mut socket = accept_hdr_async(stream, callback)
                .await
                .expect("Realtime WebSocket handshake");
            let mut events = Vec::new();
            let audio = socket
                .next()
                .await
                .expect("audio event")
                .expect("valid audio event");
            if let Message::Text(text) = audio {
                events.push(serde_json::from_slice(text.as_bytes()).expect("audio JSON"));
            }
            socket
                .send(Message::text(
                    json!({
                        "type": "future.server.event",
                        "event_id": "evt_future",
                        "payload": {"ok": true}
                    })
                    .to_string(),
                ))
                .await
                .expect("send unknown Realtime event");
            let cancel = socket
                .next()
                .await
                .expect("cancel event")
                .expect("valid cancel event");
            if let Message::Text(text) = cancel {
                events.push(serde_json::from_slice(text.as_bytes()).expect("cancel JSON"));
            }
            let close = socket
                .next()
                .await
                .expect("close frame")
                .expect("valid close");
            events.push(json!({"closed": matches!(close, Message::Close(_))}));
            let _ = events_sender.send(events);
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("Realtime WebSocket base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("Realtime WebSocket client");
        (client, handshake_receiver, events_receiver)
    }

    #[derive(Debug)]
    struct CapturedHttp {
        method: reqwest::Method,
        path: String,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn http_server(
        status: StatusCode,
        content_type: &'static str,
        body: &'static str,
        location: Option<&'static str>,
    ) -> (Client, oneshot::Receiver<CapturedHttp>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Realtime HTTP");
        let address = listener.local_addr().expect("Realtime HTTP address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept Realtime HTTP");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let request_content_type = request
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read Realtime HTTP body")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("Realtime HTTP lock").take() {
                        let _ = sender.send(CapturedHttp {
                            method,
                            path,
                            content_type: request_content_type,
                            body: request_body,
                        });
                    }
                    let mut response = hyper::Response::builder()
                        .status(status)
                        .header(header::CONTENT_TYPE, content_type)
                        .header("x-request-id", "req_realtime_http");
                    if let Some(location) = location {
                        response = response.header(header::LOCATION, location);
                    }
                    Ok::<_, Infallible>(
                        response
                            .body(Full::new(Bytes::from_static(body.as_bytes())))
                            .expect("build Realtime HTTP response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve Realtime HTTP");
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("Realtime HTTP base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("Realtime HTTP client");
        (client, receiver)
    }

    #[tokio::test]
    async fn websocket_preserves_unknown_events_and_base64_audio() {
        let (client, handshake, events) = websocket_server().await;
        let mut socket = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect Realtime WebSocket");
        assert_eq!(socket.request_id(), Some("req_realtime_ws"));
        socket
            .append_audio(vec![0, 1, 2, 255])
            .await
            .expect("append typed audio");
        let unknown = socket
            .recv()
            .await
            .expect("receive future event")
            .expect("one future event");
        assert_eq!(unknown.event_type(), "future.server.event");
        socket
            .cancel_response(Some("resp_1"))
            .await
            .expect("cancel response");
        socket.close().await.expect("close Realtime socket");

        let handshake = handshake.await.expect("captured Realtime handshake");
        let url = Url::parse(&format!("http://loopback{}", handshake.path_and_query))
            .expect("captured Realtime URL");
        assert_eq!(url.path(), "/v1/realtime");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "model")
                .map(|(_, value)| value),
            Some("gpt-realtime/test".into())
        );
        assert_eq!(
            handshake.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let events = events.await.expect("captured Realtime events");
        assert_eq!(events[0]["type"], "input_audio_buffer.append");
        assert_eq!(events[0]["audio"], "AAEC/w==");
        assert_eq!(events[1]["type"], "response.cancel");
        assert_eq!(events[1]["response_id"], "resp_1");
        assert_eq!(events[2]["closed"], true);
    }

    #[tokio::test]
    async fn client_secret_route_is_typed_and_secret_debug_is_redacted() {
        let (client, captured) = http_server(
            StatusCode::OK,
            "application/json",
            r#"{"value":"ek_private","expires_at":123,"session":{"type":"realtime","id":"sess_1","object":"realtime.session"}}"#,
            None,
        )
        .await;
        let response = client
            .realtime()
            .create_client_secret(RealtimeCreateClientSecretRequest::default())
            .await
            .expect("Realtime client secret");
        assert!(!format!("{:?}", response.body()).contains("ek_private"));
        assert_eq!(response.request_id(), Some("req_realtime_http"));
        let captured = captured.await.expect("captured client-secret request");
        assert_eq!(captured.method, reqwest::Method::POST);
        assert_eq!(captured.path, "/v1/realtime/client_secrets");
        assert_eq!(captured.body, b"{}");
    }

    #[tokio::test]
    async fn create_call_sends_multipart_and_returns_sdp_location() {
        let (client, captured) = http_server(
            StatusCode::CREATED,
            "application/sdp",
            "v=0\r\na=answer\r\n",
            Some("/v1/realtime/calls/call_123"),
        )
        .await;
        let request: RealtimeCallCreateRequest = serde_json::from_value(json!({
            "sdp": "v=0\r\na=offer\r\n",
            "session": {"type": "realtime", "model": "gpt-realtime"}
        }))
        .expect("typed call request");
        let response = client
            .realtime()
            .create_call(request)
            .await
            .expect("created Realtime call");
        assert_eq!(response.call_id(), "call_123");
        assert_eq!(response.sdp().as_str(), "v=0\r\na=answer\r\n");
        assert_eq!(response.request_id(), Some("req_realtime_http"));
        assert!(!format!("{:?}", response.body()).contains("a=answer"));

        let captured = captured.await.expect("captured call request");
        assert_eq!(captured.path, "/v1/realtime/calls");
        assert!(
            captured
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let body = String::from_utf8_lossy(&captured.body);
        assert!(body.contains("application/sdp"));
        assert!(body.contains("a=offer"));
        assert!(body.contains("application/json"));
        assert!(body.contains("gpt-realtime"));
    }

    #[tokio::test]
    async fn sip_actions_use_typed_routes_and_never_need_response_json() {
        let (client, accept) = http_server(StatusCode::OK, "application/json", "", None).await;
        client
            .realtime()
            .accept_call("call/a", RealtimeSessionCreateRequest::default())
            .await
            .expect("accept SIP call");
        let captured = accept.await.expect("captured accept");
        assert_eq!(captured.path, "/v1/realtime/calls/call%2Fa/accept");
        assert!(String::from_utf8_lossy(&captured.body).contains("\"type\":\"realtime\""));

        let (client, reject) = http_server(StatusCode::OK, "application/json", "", None).await;
        client
            .realtime()
            .reject_call_default("call_1")
            .await
            .expect("reject SIP call");
        let captured = reject.await.expect("captured reject");
        assert_eq!(captured.path, "/v1/realtime/calls/call_1/reject");
        assert!(captured.body.is_empty());

        let (client, refer) = http_server(StatusCode::OK, "application/json", "", None).await;
        let request: RealtimeCallReferRequest =
            serde_json::from_value(json!({"target_uri":"tel:+14155550123"}))
                .expect("typed REFER request");
        client
            .realtime()
            .refer_call("call_1", request)
            .await
            .expect("refer SIP call");
        let captured = refer.await.expect("captured refer");
        assert_eq!(captured.path, "/v1/realtime/calls/call_1/refer");
        assert!(String::from_utf8_lossy(&captured.body).contains("tel:+14155550123"));

        let (client, hangup) = http_server(StatusCode::OK, "application/json", "", None).await;
        client
            .realtime()
            .hangup_call("call_1")
            .await
            .expect("hang up call");
        let captured = hangup.await.expect("captured hangup");
        assert_eq!(captured.path, "/v1/realtime/calls/call_1/hangup");
        assert!(captured.body.is_empty());
    }
}
