//! GA Realtime WebSocket, WebRTC signaling, and SIP call-control clients.
//!
//! This module establishes signaling and event transports. It intentionally
//! does not capture devices, encode media, implement RTP, or manage a WebRTC
//! peer connection.

use std::{fmt, time::Duration};

use futures_util::{SinkExt, StreamExt};
use http::{Method, StatusCode, header};
use openai_rs_types::{ModelId, Omittable};
use openai_rs_types::realtime::{
    RealtimeCallAcceptRequest, RealtimeCallCreateRequest, RealtimeCallHangupRequest,
    RealtimeCallReferRequest, RealtimeCallRejectRequest, RealtimeClientEvent,
    RealtimeClientEventInputAudioBufferAppend, RealtimeClientEventResponseCancel,
    RealtimeCreateClientSecretRequest, RealtimeCreateClientSecretResponse, RealtimeSdp,
    RealtimeServerEvent,
};
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
        Socket, connect_socket, map_websocket_error, websocket_connector, websocket_request,
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
    pub async fn connect(
        &self,
        model: impl Into<ModelId>,
    ) -> Result<RealtimeWebSocket, Error> {
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
        let request = transport
            .request_builder(reqwest::Method::POST, url, "application/sdp")
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
        let request = websocket_request(
            &url,
            transport.authorization(),
            transport.organization(),
            transport.project(),
        )?;
        let connect = connect_socket(request, config.tungstenite(), connector);
        let (socket, response) = tokio::time::timeout(config.connect_timeout, connect)
            .await
            .map_err(|_| Error::WebSocketTransport("Realtime handshake timed out".into()))?
            .map_err(map_websocket_error)?;
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
                    let event = deserialize_json(text.as_bytes()).map_err(|error| Error::Decode {
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
    let call_id = resolved
        .path_segments()
        .and_then(Iterator::last)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::InvalidConfiguration("Realtime call Location has no call id".into())
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

operation!(CreateClientSecret, RealtimeCreateClientSecretRequest, RealtimeCreateClientSecretResponse, "/realtime/client_secrets", RequestEncoding::Json, ResponseMode::Json);
operation!(AcceptCall, RealtimeCallAcceptRequest, (), "/realtime/calls/{call_id}/accept", RequestEncoding::Json, ResponseMode::Empty);
operation!(RejectCall, RealtimeCallRejectRequest, (), "/realtime/calls/{call_id}/reject", RequestEncoding::Json, ResponseMode::Empty);
operation!(RejectCallDefault, (), (), "/realtime/calls/{call_id}/reject", RequestEncoding::None, ResponseMode::Empty);
operation!(HangupCall, RealtimeCallHangupRequest, (), "/realtime/calls/{call_id}/hangup", RequestEncoding::None, ResponseMode::Empty);
operation!(ReferCall, RealtimeCallReferRequest, (), "/realtime/calls/{call_id}/refer", RequestEncoding::Json, ResponseMode::Empty);
