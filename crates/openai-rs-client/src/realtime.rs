//! GA Realtime WebSocket, WebRTC signaling, and SIP call-control clients.
//!
//! This module establishes signaling and event transports. It intentionally
//! does not capture devices, encode media, implement RTP, or manage a WebRTC
//! peer connection.

use std::{fmt, time::Duration};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http::{Method, StatusCode, header};
use openai_rs_types::realtime::{
    RealtimeCallAcceptRequest, RealtimeCallCreateRequest, RealtimeCallReferRequest,
    RealtimeCallRejectRequest, RealtimeClientEvent, RealtimeClientEventInputAudioBufferAppend,
    RealtimeClientEventResponseCancel, RealtimeCreateClientSecretRequest,
    RealtimeCreateClientSecretResponse, RealtimeSdp, RealtimeServerEvent,
    RealtimeTranslationClientSecretCreateRequest, RealtimeTranslationClientSecretCreateResponse,
};
use openai_rs_types::{ModelId, Omittable};
use reqwest::multipart::{Form, Part};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_tungstenite::tungstenite::{
    Message, Utf8Bytes,
    protocol::{WebSocketConfig as TungsteniteConfig, frame::CloseFrame},
};
use url::Url;

use tracing::Instrument;

use crate::{
    ApiResponse, BodyPreview, Client, Error, ResponseMeta,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    responses_websocket::{
        Socket, connect_socket, derive_websocket_url, is_unauthorized_websocket_error,
        map_websocket_error, websocket_connector, websocket_request,
    },
    trace,
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];
const CREATED: StatusCode = StatusCode::CREATED;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WRITE_BUFFER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_QUEUED_WRITE_BYTES: usize = 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Payload carried by every keepalive probe ping. The peer's reply is judged
/// by the silence window below, never by matching this payload.
const KEEPALIVE_PING_PAYLOAD: &[u8] = b"openai-rs";
/// Reason reported once the keepalive silence window elapses.
const KEEPALIVE_TIMEOUT_REASON: &str =
    "Realtime keepalive timed out: no inbound frames within ping_interval + pong_timeout";

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
    /// configured Platform base and cannot be supplied by the caller. The
    /// handshake itself is single-shot and never retried; see
    /// [`RealtimeWebSocket::connect`] for the rationale.
    pub async fn connect(&self, model: impl Into<ModelId>) -> Result<RealtimeWebSocket, Error> {
        self.connect_with(model, RealtimeWebSocketConfig::default())
            .await
    }

    pub async fn connect_with(
        &self,
        model: impl Into<ModelId>,
        config: RealtimeWebSocketConfig,
    ) -> Result<RealtimeWebSocket, Error> {
        self.connect_target_with(RealtimeConnectTarget::model(model), config)
            .await
    }

    /// Opens a GA transcription Realtime WebSocket with
    /// `?intent=transcription`. A model must not be pinned on transcription
    /// sessions, so this target never sends `model`.
    pub async fn connect_transcription(&self) -> Result<RealtimeWebSocket, Error> {
        self.connect_transcription_with(RealtimeWebSocketConfig::default())
            .await
    }

    pub async fn connect_transcription_with(
        &self,
        config: RealtimeWebSocketConfig,
    ) -> Result<RealtimeWebSocket, Error> {
        self.connect_target_with(RealtimeConnectTarget::TranscriptionIntent, config)
            .await
    }

    /// Opens a GA Realtime WebSocket for an explicit connection target: one
    /// model, the transcription intent, or an in-progress `call_id`.
    pub async fn connect_target(
        &self,
        target: RealtimeConnectTarget,
    ) -> Result<RealtimeWebSocket, Error> {
        self.connect_target_with(target, RealtimeWebSocketConfig::default())
            .await
    }

    pub async fn connect_target_with(
        &self,
        target: RealtimeConnectTarget,
        config: RealtimeWebSocketConfig,
    ) -> Result<RealtimeWebSocket, Error> {
        RealtimeWebSocket::connect(&self.client, target, config).await
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

    /// Creates a short-lived client secret for a typed translation session.
    pub async fn create_translation_client_secret(
        &self,
        request: RealtimeTranslationClientSecretCreateRequest,
    ) -> Result<ApiResponse<RealtimeTranslationClientSecretCreateResponse>, Error> {
        let path = [
            PathSegment::literal("realtime"),
            PathSegment::literal("translations"),
            PathSegment::literal("client_secrets"),
        ];
        self.client
            .transport()
            .execute_json::<CreateTranslationClientSecret, ()>(&path, None, Some(&request))
            .await
    }

    /// Exchanges an SDP offer and optional typed session configuration for an
    /// SDP answer. Media capture, codecs, ICE, DTLS, RTP, and peer-connection
    /// management remain the caller's responsibility.
    ///
    /// Encoding (5-03): with a session configuration the request is the pinned
    /// two-part `multipart/form-data` body (`sdp` as `application/sdp`,
    /// `session` as `application/json`); with the session omitted it is the
    /// bare SDP text under `Content-Type: application/sdp` — the switch both
    /// official baselines make for a sdp-only body — never a one-part
    /// multipart. `Accept` stays `application/sdp` in both shapes. Unknown
    /// keys captured in [`RealtimeCallCreateRequest`]'s extra fields are
    /// dropped: the pinned encoding table defines exactly the `sdp` and
    /// `session` parts, so the multipart path sends those two alone and the
    /// bare path carries the SDP text by itself (decode still keeps extras
    /// lossless for round-tripping).
    ///
    /// Retry classification (3-20): this operation is the equivalent of
    /// `RetryClass::Never`. Creating a call is a side-effecting mutation — a
    /// replayed attempt could place two live calls — so it keeps the same
    /// conservative classification as the accept/reject/hangup/refer actions
    /// below and is always sent exactly once. The official Python client
    /// retries every request by default; that divergence is recorded in
    /// decisions.md (3-30 item 6).
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
        let authorization = transport.authorization().await?;
        let builder = match session {
            Omittable::Value(session) => {
                let session = serde_json::to_string(&session).map_err(Error::Encode)?;
                let sdp_part = Part::text(sdp.0)
                    .mime_str("application/sdp")
                    .map_err(Error::from_reqwest)?;
                let session_part = Part::text(session)
                    .mime_str("application/json")
                    .map_err(Error::from_reqwest)?;
                transport
                    .request_builder(
                        reqwest::Method::POST,
                        url,
                        "application/sdp",
                        authorization.header.clone(),
                    )
                    .timeout(transport.overall_timeout())
                    .multipart(
                        Form::new()
                            .part("sdp", sdp_part)
                            .part("session", session_part),
                    )
            }
            // 5-03: with no session part the pinned encoding table leaves the
            // SDP alone, and both official clients send the bare sdp text
            // under `application/sdp` instead of a one-part multipart.
            Omittable::Omitted => transport
                .request_builder(
                    reqwest::Method::POST,
                    url,
                    "application/sdp",
                    authorization.header.clone(),
                )
                .timeout(transport.overall_timeout())
                .header(header::CONTENT_TYPE, "application/sdp")
                .body(sdp.0),
            _ => {
                return Err(Error::InvalidConfiguration(
                    "unsupported Realtime call session state".into(),
                ));
            }
        };
        let request = builder.build().map_err(Error::from_reqwest)?;
        transport.ensure_same_origin(request.url())?;
        // This multipart SDP exchange cannot ride the transport's `send` loop
        // (JSON-only request encoding; 201 + Location + SDP-text response
        // shape), so its retry classification lives here instead of in an
        // `Operation`: the request is deliberately single-shot and never
        // replayed — the equivalent of `RetryClass::Never`, matching the
        // accept/reject/hangup/refer operations (3-20).
        let span = trace::http_request_span("realtime.create_call", "POST", "/realtime/calls");
        let response = async {
            let response = transport
                .http()
                .execute(request)
                .await
                .map_err(Error::from_reqwest)?;
            trace::record_http_outcome(0, &response);
            if response.status() != CREATED {
                if response.status() == StatusCode::UNAUTHORIZED {
                    let _ = transport
                        .invalidate_authorization(authorization.generation)
                        .await;
                    // create_call is single-shot: the credential is invalidated
                    // but no retry follows.
                    trace::emit_auth_refresh_no_retry();
                }
                return Err(transport.error_from_response(response).await);
            }
            Ok(response)
        }
        .instrument(span)
        .await?;
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

/// Opt-in WebSocket keepalive for one GA Realtime connection (3-28).
///
/// While [`RealtimeWebSocket::recv`] is being awaited, an enabled keepalive
/// sends a small RFC 6455 `Ping` every [`ping_interval`](Self::ping_interval)
/// and fails with [`Error::WebSocketProtocol`] once more than
/// `ping_interval + pong_timeout` has elapsed without *any* inbound frame.
/// Liveness is judged from the most recent inbound frame of any kind — server
/// event, `Ping`, or the `Pong` echo of our own probe — rather than by
/// matching individual `Pong` replies, so an intermediary that swallows
/// control frames cannot cause a spurious timeout while other traffic keeps
/// flowing. A keepalive timeout retires the socket with an error and is never
/// followed by an automatic reconnect (D0122/D0148).
///
/// The silence window only counts while `recv` is being awaited (7-09): an
/// application that stops polling `recv` also stops probing and stops the
/// window, and when polling resumes after a gap of at least one ping interval
/// the window is re-anchored to the resume instant — time spent away from
/// `recv` is never held against the connection.
///
/// The openai-python Realtime client ships defaults of a 20s ping interval
/// with a 20s pong timeout; this crate keeps keepalive opt-in (disabled by
/// default) and treats those values only as a tuning reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeKeepalive {
    ping_interval: Duration,
    pong_timeout: Duration,
}

impl RealtimeKeepalive {
    /// Creates a keepalive policy from a ping interval and a pong timeout.
    ///
    /// Both durations must be non-zero; the combined silence window
    /// (`ping_interval + pong_timeout`) is additionally checked when the
    /// owning [`RealtimeWebSocketConfig`] is validated at connect time.
    pub fn new(ping_interval: Duration, pong_timeout: Duration) -> Result<Self, Error> {
        if ping_interval.is_zero() || pong_timeout.is_zero() {
            return Err(Error::InvalidConfiguration(
                "Realtime keepalive intervals must be non-zero".into(),
            ));
        }
        Ok(Self {
            ping_interval,
            pong_timeout,
        })
    }

    /// Cadence at which a keepalive `Ping` is sent while awaiting frames.
    #[must_use]
    pub const fn ping_interval(&self) -> Duration {
        self.ping_interval
    }

    /// Extra allowance after a ping before the connection is declared dead.
    #[must_use]
    pub const fn pong_timeout(&self) -> Duration {
        self.pong_timeout
    }

    /// Maximum allowed silence between inbound frames. Saturating so a
    /// pathological overflow degrades into "never times out" instead of a
    /// panic; overflow is rejected during config validation anyway.
    const fn silence_window(&self) -> Duration {
        self.ping_interval.saturating_add(self.pong_timeout)
    }
}

/// Bounds and liveness policy for a GA Realtime WebSocket. There is
/// deliberately no automatic reconnect policy because client events may have
/// side effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeWebSocketConfig {
    max_message_bytes: usize,
    max_frame_bytes: usize,
    write_buffer_bytes: usize,
    max_queued_write_bytes: usize,
    connect_timeout: Duration,
    keepalive: Option<RealtimeKeepalive>,
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
            keepalive: None,
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

    /// Bounds tungstenite's write buffer, mirroring the Responses WebSocket
    /// config. `max_queued_write_bytes` must stay strictly larger.
    #[must_use]
    pub const fn write_buffer_bytes(mut self, value: usize) -> Self {
        self.write_buffer_bytes = value;
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

    /// Enables or disables WebSocket keepalive. `None` (the default) leaves
    /// the socket free-running: no probes are sent and an idle connection
    /// never times out from the client side.
    #[must_use]
    pub const fn with_keepalive(mut self, value: Option<RealtimeKeepalive>) -> Self {
        self.keepalive = value;
        self
    }

    /// Enables WebSocket keepalive from its two fields directly, for callers
    /// that have not imported [`RealtimeKeepalive`]. Both durations must be
    /// non-zero; see that type for the probing contract.
    pub fn with_keepalive_intervals(
        self,
        ping_interval: Duration,
        pong_timeout: Duration,
    ) -> Result<Self, Error> {
        Ok(self.with_keepalive(Some(RealtimeKeepalive::new(ping_interval, pong_timeout)?)))
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
        if let Some(keepalive) = self.keepalive {
            if keepalive
                .ping_interval
                .checked_add(keepalive.pong_timeout)
                .is_none()
            {
                return Err(Error::InvalidConfiguration(
                    "Realtime keepalive silence window overflows a Duration".into(),
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

impl Default for RealtimeWebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Connection target for one GA Realtime WebSocket.
///
/// The official clients select exactly one of a model, the GA transcription
/// intent, or an in-progress call identifier. Modeling the choice as an enum
/// makes the mutual exclusion structural: a transcription session can never
/// carry `model`, and a sideband call connection can never carry `intent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RealtimeConnectTarget {
    /// A model-backed session, connected with `?model=...`.
    Model(ModelId),
    /// A GA transcription session, connected with `?intent=transcription`.
    TranscriptionIntent,
    /// A sideband control connection attached to an in-progress call with
    /// `?call_id=...`.
    CallId(String),
}

impl RealtimeConnectTarget {
    /// Targets a model-backed session. An empty model id is refused when the
    /// connection URL is derived (the connect entry points return
    /// [`Error::InvalidConfiguration`]) rather than reaching the wire as a
    /// bare `?model=` key (5-09).
    #[must_use]
    pub fn model(model: impl Into<ModelId>) -> Self {
        Self::Model(model.into())
    }

    /// Targets a sideband control connection for one in-progress call. An
    /// empty call id is refused when the connection URL is derived (the
    /// connect entry points return [`Error::InvalidConfiguration`]) rather
    /// than reaching the wire as a bare `?call_id=` key (5-09).
    #[must_use]
    pub fn call_id(call_id: impl Into<String>) -> Self {
        Self::CallId(call_id.into())
    }

    fn query_pair(&self) -> (&'static str, &str) {
        match self {
            Self::Model(model) => ("model", model.as_str()),
            Self::TranscriptionIntent => ("intent", "transcription"),
            Self::CallId(call_id) => ("call_id", call_id.as_str()),
        }
    }
}

impl From<ModelId> for RealtimeConnectTarget {
    fn from(model: ModelId) -> Self {
        Self::Model(model)
    }
}

/// Runtime state for one enabled [`RealtimeKeepalive`] policy.
struct RealtimeKeepaliveState {
    config: RealtimeKeepalive,
    ticker: tokio::time::Interval,
    last_inbound: Instant,
    /// When the `recv` loop last ran (7-09). The silence window only counts
    /// while `recv` is being awaited, so `recv` compares this against the ping
    /// interval on entry to detect a polling gap and re-anchor `last_inbound`
    /// instead of judging the window against a stale, pre-pause anchor.
    last_poll: Instant,
}

/// A typed GA Realtime WebSocket.
pub struct RealtimeWebSocket {
    socket: Socket,
    meta: ResponseMeta,
    max_message_bytes: usize,
    keepalive: Option<RealtimeKeepaliveState>,
    closed: bool,
    last_close: Option<(u16, String)>,
}

impl RealtimeWebSocket {
    /// Opens one Realtime WebSocket against the derived URL.
    ///
    /// The handshake is deliberately single-shot (4-20): a failed or timed-out
    /// handshake surfaces as [`Error::WebSocketHandshake`] /
    /// [`Error::WebSocketTransport`] and is never retried automatically. Both
    /// official baselines agree — openai-node and openai-python do not retry
    /// WebSocket connections — and a blind reconnect could attach to a
    /// half-provisioned target. The Responses WebSocket exposes initial-connect
    /// retries as an opt-in surface
    /// (`WebSocketReconnectPolicy::InitialConnect` in `responses_websocket`);
    /// Realtime intentionally does not offer that knob, because a Realtime
    /// connection is bound to session state (one model, the transcription
    /// intent, or one in-progress call) rather than to a replayable request.
    /// Callers that want connection retries can loop on the returned error.
    async fn connect(
        client: &Client,
        target: RealtimeConnectTarget,
        config: RealtimeWebSocketConfig,
    ) -> Result<Self, Error> {
        let config = config.validate()?;
        let url = realtime_websocket_url(client.base_url(), &target)?;
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
                transport.client_request_id(),
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
        let keepalive = config.keepalive.map(|config| {
            // The first probe is one full interval away; a pause in recv
            // polling must not surface as a catch-up burst of pings.
            let mut ticker =
                interval_at(Instant::now() + config.ping_interval, config.ping_interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            RealtimeKeepaliveState {
                config,
                ticker,
                last_inbound: Instant::now(),
                last_poll: Instant::now(),
            }
        });
        Ok(Self {
            socket,
            meta,
            max_message_bytes: config.max_message_bytes,
            keepalive,
            closed: false,
            last_close: None,
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

    /// Sends one typed client event.
    ///
    /// A transport failure while *writing* the frame retires the socket
    /// (`is_closed` becomes `true`), extending the recv-side posture (4-19,
    /// D0212): a connection that cannot be written to is not usable again, so
    /// later `send`/`recv` calls report the closed state instead of polling a
    /// half-broken socket. Local validation failures — an event that fails to
    /// encode or exceeds the configured message limit — leave the connection
    /// open, because nothing reached the wire and the socket remains healthy.
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
        match self.socket.send(Message::text(encoded)).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.closed = true;
                Err(map_websocket_error(error))
            }
        }
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

    /// Receives the next typed server event.
    ///
    /// With keepalive enabled, awaiting this call also drives the probe: a
    /// `Ping` is sent every configured interval and the call fails with
    /// [`Error::WebSocketProtocol`] once the silence window elapses with no
    /// inbound frame of any kind. Keepalive is therefore recv-driven — an
    /// application that stops polling `recv` also opts out of liveness
    /// detection, and the window is re-anchored when polling resumes after a
    /// gap of at least one ping interval, so a pause never retires an
    /// otherwise healthy connection (7-09). The connection is never
    /// reconnected automatically.
    ///
    /// Failure posture (4-19): every transport or protocol failure — a broken
    /// connection, a keepalive timeout, a failed probe write, or a frame that
    /// violates the Realtime event-transport contract — retires the socket
    /// (`is_closed` becomes `true`, matching openai-node, which destroys the
    /// WebSocket on any error). A failed event *decode* is the one
    /// recoverable path: the connection stays open so a malformed event need
    /// not take down an otherwise healthy session.
    pub async fn recv(&mut self) -> Result<Option<RealtimeServerEvent>, Error> {
        if self.closed {
            return Ok(None);
        }
        loop {
            if let Some(keepalive) = self.keepalive.as_mut() {
                // 7-09: the silence window only counts while `recv` is being
                // awaited. When polling resumes after a gap of at least one
                // ping interval — the application was busy elsewhere, so no
                // probe was sent and no frame was read — the window is
                // re-anchored to *now* before any tick can judge it. Without
                // this reset, the first post-gap tick would compare against
                // the pre-pause `last_inbound` and retire an otherwise
                // healthy connection on the spot.
                let now = Instant::now();
                if keepalive.last_poll.elapsed() >= keepalive.config.ping_interval {
                    keepalive.last_inbound = now;
                }
                keepalive.last_poll = now;
            }
            let message = match self.keepalive.as_mut() {
                Some(keepalive) => {
                    tokio::select! {
                        // Inbound data wins over the ticker: frames buffered
                        // while recv was not being polled must be drained (and
                        // refresh the window) before a tick can judge silence.
                        biased;
                        message = self.socket.next() => message,
                        _ = keepalive.ticker.tick() => {
                            // Judge liveness by "any inbound frame within the
                            // window", never by matching a specific Pong, so
                            // intermediaries that eat control frames cannot
                            // cause spurious timeouts while traffic flows.
                            if keepalive.last_inbound.elapsed() >= keepalive.config.silence_window() {
                                self.closed = true;
                                return Err(Error::WebSocketProtocol(
                                    KEEPALIVE_TIMEOUT_REASON,
                                ));
                            }
                            // A probe that cannot be written is just as fatal
                            // as the silence window above: the socket is
                            // retired instead of being polled again (4-19).
                            if let Err(error) = self
                                .socket
                                .send(Message::Ping(Bytes::from_static(KEEPALIVE_PING_PAYLOAD)))
                                .await
                            {
                                self.closed = true;
                                return Err(map_websocket_error(error));
                            }
                            continue;
                        }
                    }
                }
                None => self.socket.next().await,
            };
            let Some(message) = message else {
                self.closed = true;
                return Ok(None);
            };
            // Refresh before classification: every inbound frame — event,
            // Ping, or the Pong echo of our own probe — proves liveness.
            if let Some(keepalive) = self.keepalive.as_mut() {
                keepalive.last_inbound = Instant::now();
            }
            // A read failure leaves the underlying connection unusable, so it
            // retires the socket like every other non-decode error path.
            let message = match message {
                Ok(message) => message,
                Err(error) => {
                    self.closed = true;
                    return Err(map_websocket_error(error));
                }
            };
            match classify_realtime_inbound(message) {
                RealtimeInbound::Event(text) => {
                    if text.len() > self.max_message_bytes {
                        self.closed = true;
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
                RealtimeInbound::Ignore => {}
                RealtimeInbound::Closed(frame) => {
                    self.closed = true;
                    if let Some(frame) = frame {
                        self.last_close =
                            Some((u16::from(frame.code), frame.reason.as_str().to_owned()));
                    }
                    return Ok(None);
                }
                RealtimeInbound::Reject(reason) => {
                    // A frame that violates the event-transport contract
                    // retires the socket (4-19); the stream is only usable for
                    // well-formed Realtime frames.
                    self.closed = true;
                    return Err(Error::WebSocketProtocol(reason));
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
            .field(
                "keepalive",
                &self.keepalive.as_ref().map(|state| state.config),
            )
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

/// What [`RealtimeWebSocket::recv`] does with one inbound WebSocket message.
#[derive(Debug, PartialEq, Eq)]
enum RealtimeInbound {
    /// A JSON text frame to decode into a typed server event.
    Event(Utf8Bytes),
    /// Ignore the frame and keep reading.
    Ignore,
    /// The peer completed its close handshake, carrying its close frame when
    /// one was sent (4-18). `None` means a frameless EOF, which stays
    /// distinguishable from a coded close through
    /// [`RealtimeWebSocket::close_code`].
    Closed(Option<CloseFrame>),
    /// The frame violates the Realtime event-transport contract.
    Reject(&'static str),
}

/// Classifies one inbound WebSocket message for the `recv` loop.
///
/// `Ping` (and the unsolicited `Pong` echo) classify as [`RealtimeInbound::Ignore`]
/// because tungstenite 0.29 already answers pings itself: reading the Ping
/// frame (`read_message_frame`) queues the automatic RFC 6455 Pong, and the
/// next poll or write flushes it to the wire. Sending an explicit Pong from
/// `recv` would only add a redundant second reply path on top of the automatic
/// one, so the receive loop deliberately performs no write for pings.
fn classify_realtime_inbound(message: Message) -> RealtimeInbound {
    match message {
        Message::Text(text) => RealtimeInbound::Event(text),
        Message::Ping(_) | Message::Pong(_) => RealtimeInbound::Ignore,
        Message::Close(frame) => RealtimeInbound::Closed(frame),
        Message::Binary(_) => {
            RealtimeInbound::Reject("Realtime WebSocket sent a binary data message")
        }
        Message::Frame(_) => {
            RealtimeInbound::Reject("Realtime WebSocket exposed an unexpected raw frame")
        }
    }
}

/// Query keys that select a Realtime connection target.
const TARGET_QUERY_KEYS: [&str; 3] = ["model", "intent", "call_id"];

fn realtime_websocket_url(base: &Url, target: &RealtimeConnectTarget) -> Result<Url, Error> {
    // 7-22: scheme mapping, the `realtime` path segment, fragment dropping,
    // and base-query preservation come from the shared derivation used by all
    // three WebSocket faces; only the target key handling below is Realtime's.
    let mut url = derive_websocket_url(base, "realtime", "Realtime")?;
    // Exactly one target key may appear on the wire, so a base URL that already
    // pins `model`, `intent`, or `call_id` is rejected instead of being merged
    // into a conflicting pair such as `intent=transcription&model=...`.
    if url
        .query_pairs()
        .any(|(key, _)| TARGET_QUERY_KEYS.contains(&key.as_ref()))
    {
        return Err(Error::InvalidConfiguration(
            "Realtime base URL already pins a connection-target query parameter".into(),
        ));
    }
    let (key, value) = target.query_pair();
    // 5-09: an empty `model` or `call_id` would reach the wire as a bare
    // `?model=` / `?call_id=` key. openai-node rejects empty targets outright,
    // so the empty value is refused here (the connect entry points surface it
    // as `Error::InvalidConfiguration`) instead of being encoded.
    if value.is_empty() {
        return Err(Error::InvalidConfiguration(
            "Realtime connection target must not be an empty string".into(),
        ));
    }
    url.query_pairs_mut().append_pair(key, value);
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
    ($name:ident, $request:ty, $response:ty, $route:literal, $encoding:expr, $mode:expr, $retry:expr) => {
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
                retry: $retry,
                success_statuses: OK,
            };
        }
    };
}

// Client-secret issuance is idempotent, so transient 429/5xx responses are
// retried like every other replayable mutation. The call-control actions
// below keep `RetryClass::Never` because accepting, rejecting, hanging up,
// or transferring a live call twice is observable by the caller. `create_call`
// above is a handwritten multipart path and carries the same `Never`
// classification as an inline decision at its send site (3-20).
operation!(
    CreateClientSecret,
    RealtimeCreateClientSecretRequest,
    RealtimeCreateClientSecretResponse,
    "/realtime/client_secrets",
    RequestEncoding::Json,
    ResponseMode::Json,
    RetryClass::Replayable
);
operation!(
    CreateTranslationClientSecret,
    RealtimeTranslationClientSecretCreateRequest,
    RealtimeTranslationClientSecretCreateResponse,
    "/realtime/translations/client_secrets",
    RequestEncoding::Json,
    ResponseMode::Json,
    RetryClass::Replayable
);
operation!(
    AcceptCall,
    RealtimeCallAcceptRequest,
    (),
    "/realtime/calls/{call_id}/accept",
    RequestEncoding::Json,
    ResponseMode::Empty,
    RetryClass::Never
);
operation!(
    RejectCall,
    RealtimeCallRejectRequest,
    (),
    "/realtime/calls/{call_id}/reject",
    RequestEncoding::Json,
    ResponseMode::Empty,
    RetryClass::Never
);
operation!(
    RejectCallDefault,
    (),
    (),
    "/realtime/calls/{call_id}/reject",
    RequestEncoding::None,
    ResponseMode::Empty,
    RetryClass::Never
);
operation!(
    HangupCall,
    (),
    (),
    "/realtime/calls/{call_id}/hangup",
    RequestEncoding::None,
    ResponseMode::Empty,
    RetryClass::Never
);
operation!(
    ReferCall,
    RealtimeCallReferRequest,
    (),
    "/realtime/calls/{call_id}/refer",
    RequestEncoding::Json,
    ResponseMode::Empty,
    RetryClass::Never
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
    use openai_rs_types::realtime::{
        RealtimeSessionCreateRequest, RealtimeTranslationAudio, RealtimeTranslationAudioInput,
        RealtimeTranslationAudioOutput, RealtimeTranslationClientSecretCreateRequest,
        RealtimeTranslationClientSecretExpiration, RealtimeTranslationSessionCreateRequest,
    };
    use serde_json::{Value, json};
    use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener, sync::oneshot};
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server,
            protocol::frame::{CloseFrame, Frame, coding::CloseCode},
        },
    };

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

    /// Accepts one WebSocket, then stays completely silent — neither reading
    /// nor writing — until released. Not reading is what keeps the client's
    /// keepalive probes unanswered: tungstenite only queues its automatic
    /// `Pong` while frames are actually being read. Once released, the server
    /// drains the frames that arrived meanwhile and reports every ping
    /// payload it saw.
    async fn silent_websocket_server()
    -> (Client, oneshot::Sender<()>, oneshot::Receiver<Vec<Bytes>>) {
        periodic_event_websocket_server(Duration::ZERO, 0).await
    }

    /// Accepts one WebSocket, sends `rounds` unknown server events spaced by
    /// `period`, then parks silently (no reads, no writes) until released —
    /// the same drain-and-report contract as [`silent_websocket_server`].
    async fn periodic_event_websocket_server(
        period: Duration,
        rounds: usize,
    ) -> (Client, oneshot::Sender<()>, oneshot::Receiver<Vec<Bytes>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind keepalive server");
        let address = listener.local_addr().expect("keepalive server address");
        let (release_sender, release_receiver) = oneshot::channel();
        let (pings_sender, pings_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept keepalive socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("keepalive server handshake");
            // `interval` rejects a zero period, and the silent-server shape
            // (zero rounds) never ticks at all.
            if rounds > 0 {
                let mut ticker = tokio::time::interval(period);
                ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                for round in 0..rounds {
                    ticker.tick().await;
                    socket
                        .send(Message::text(
                            json!({
                                "type": "future.server.event",
                                "event_id": format!("evt_keepalive_{round}"),
                                "payload": {"ok": true}
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send periodic Realtime event");
                }
            }
            // Park without reading so the auto-Pong machinery never answers
            // the client's probes and nothing else is ever sent.
            let _ = release_receiver.await;
            let mut pings = Vec::new();
            while let Some(message) = socket.next().await {
                match message {
                    Ok(Message::Ping(payload)) => pings.push(payload),
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            let _ = pings_sender.send(pings);
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("keepalive server base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("keepalive server client");
        (client, release_sender, pings_receiver)
    }

    /// Accepts one WebSocket and then only *reads* — never writing an event —
    /// so the peer stays live but quiet: tungstenite answers every probe with
    /// its automatic `Pong` while the inbound lane never carries an event.
    /// The read loop ends when the client goes away, and every ping payload
    /// seen meanwhile is reported.
    async fn live_quiet_websocket_server() -> (Client, oneshot::Receiver<Vec<Bytes>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind live-quiet server");
        let address = listener.local_addr().expect("live-quiet server address");
        let (pings_sender, pings_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept live-quiet socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("live-quiet server handshake");
            let mut pings = Vec::new();
            while let Some(message) = socket.next().await {
                match message {
                    Ok(Message::Ping(payload)) => pings.push(payload),
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            let _ = pings_sender.send(pings);
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("live-quiet client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("live-quiet client");
        (client, pings_receiver)
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
    async fn websocket_answers_one_ping_with_exactly_one_pong() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ping-count server");
        let address = listener.local_addr().expect("ping-count server address");
        let (pongs_sender, pongs_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept ping-count socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("ping-count server handshake");
            socket
                .send(Message::Ping(Bytes::from_static(b"realtime-keepalive")))
                .await
                .expect("send Realtime ping");
            socket
                .send(Message::text(
                    json!({
                        "type": "future.server.event",
                        "event_id": "evt_after_ping",
                        "payload": {"ok": true}
                    })
                    .to_string(),
                ))
                .await
                .expect("send event after ping");
            let mut pongs = Vec::new();
            while let Some(message) = socket.next().await {
                match message.expect("valid ping-count frame") {
                    Message::Pong(payload) => pongs.push(payload),
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            let _ = pongs_sender.send(pongs);
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("ping-count client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("ping-count client");
        let mut socket = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect Realtime WebSocket");
        let event = socket
            .recv()
            .await
            .expect("receive event after ping")
            .expect("one event after ping");
        assert_eq!(event.event_type(), "future.server.event");
        socket.close().await.expect("close Realtime socket");

        let pongs = pongs_receiver.await.expect("captured Realtime pongs");
        assert_eq!(
            pongs,
            vec![Bytes::from_static(b"realtime-keepalive")],
            "tungstenite's automatic reply must be the only Pong per Ping"
        );
    }

    /// Accepts one WebSocket and immediately closes it with a non-1000
    /// status code, so the client's recv observes a coded close frame.
    async fn coded_close_websocket_server() -> (Client, oneshot::Receiver<Option<(u16, String)>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind coded-close server");
        let address = listener.local_addr().expect("coded-close server address");
        let (close_sender, close_receiver) = oneshot::channel();
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
                    reason: Utf8Bytes::from_static("upstream model failed"),
                })))
                .await
                .expect("send coded close frame");
            // Report before draining: the drain loop only ends when the client
            // drops its side, so awaiting it from the test would deadlock.
            let _ = close_sender.send(Some((1011_u16, "upstream model failed".to_owned())));
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("coded-close client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("coded-close client");
        (client, close_receiver)
    }

    #[tokio::test]
    async fn websocket_close_code_and_reason_survive_the_close_handshake() {
        let (client, server_close) = coded_close_websocket_server().await;
        let mut socket = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect Realtime WebSocket");
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
        // 4-18: a coded close (1011) must stay distinguishable from a clean
        // frameless EOF, and the reason text must remain readable.
        assert_eq!(socket.close_code(), Some(1011));
        assert_eq!(socket.close_reason(), Some("upstream model failed"));
        drop(socket);
        drop(client);
        let observed = tokio::time::timeout(Duration::from_secs(5), server_close)
            .await
            .expect("timely server drain")
            .expect("server completed its side");
        assert_eq!(
            observed,
            Some((1011_u16, "upstream model failed".to_owned())),
            "the server saw its coded close accepted"
        );
    }

    #[tokio::test]
    async fn rejected_frame_retires_the_realtime_socket() {
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
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect Realtime WebSocket");

        // 4-19: a frame that violates the event-transport contract retires the
        // socket instead of leaving it half-alive.
        match socket.recv().await {
            Err(Error::WebSocketProtocol(reason)) => {
                assert_eq!(reason, "Realtime WebSocket sent a binary data message");
            }
            unexpected => panic!("expected a protocol rejection, got {unexpected:?}"),
        }
        assert!(
            socket.is_closed(),
            "a rejected frame must retire the socket"
        );
        assert!(
            socket.recv().await.expect("recv after rejection").is_none(),
            "a retired socket reports EOF on every later recv"
        );
    }

    #[tokio::test]
    async fn event_decode_failure_keeps_the_realtime_socket_open() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind decode-failure server");
        let address = listener
            .local_addr()
            .expect("decode-failure server address");
        tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept decode-failure socket");
            let mut socket = accept_hdr_async(
                stream,
                |_request: &server::Request, response: server::Response| {
                    Ok::<_, server::ErrorResponse>(response)
                },
            )
            .await
            .expect("decode-failure server handshake");
            socket
                .send(Message::text("{not json"))
                .await
                .expect("send malformed event");
            socket
                .send(Message::text(
                    json!({
                        "type": "future.server.event",
                        "event_id": "evt_after_garbage",
                        "payload": {"ok": true}
                    })
                    .to_string(),
                ))
                .await
                .expect("send well-formed event");
            while socket.next().await.is_some() {}
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("decode-failure client base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("decode-failure client");
        let mut socket = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect Realtime WebSocket");

        // 4-19: only event decoding is recoverable (openai-node keeps the
        // WebSocket usable for the next frame); the socket stays open.
        assert!(
            socket.recv().await.is_err(),
            "a malformed event must surface as a decode error"
        );
        assert!(
            !socket.is_closed(),
            "a decode failure must not retire the socket"
        );
        let event = socket
            .recv()
            .await
            .expect("the connection survives a decode failure")
            .expect("the following event still decodes");
        assert_eq!(event.event_type(), "future.server.event");
    }

    #[tokio::test]
    async fn keepalive_probe_write_failure_retires_the_realtime_socket() {
        // The probe write fails deterministically when the probe frame cannot
        // fit tungstenite's bounded write buffer: a 10-byte ping payload needs
        // a 12-byte frame, which an 8-byte cap rejects with WriteBufferFull.
        // That is the only in-process way to make the *write* fail while the
        // read side stays healthy.
        let (client, release, pings) = silent_websocket_server().await;
        let keepalive =
            RealtimeKeepalive::new(Duration::from_millis(40), Duration::from_millis(60))
                .expect("valid keepalive");
        let mut socket = client
            .realtime()
            .connect_with(
                "gpt-realtime/test",
                RealtimeWebSocketConfig::new()
                    .write_buffer_bytes(1)
                    .max_queued_write_bytes(8)
                    .with_keepalive(Some(keepalive)),
            )
            .await
            .expect("connect Realtime WebSocket with a tiny write buffer");
        match socket.recv().await {
            Err(Error::WebSocketTransport(reason)) => {
                assert!(
                    reason.to_lowercase().contains("buffer"),
                    "expected a write-buffer failure, got {reason}"
                );
            }
            unexpected => panic!("expected a probe write failure, got {unexpected:?}"),
        }
        // 4-19: a failed keepalive probe write retires the socket instead of
        // polling a connection that can no longer be written to.
        assert!(socket.is_closed());
        assert!(
            socket
                .recv()
                .await
                .expect("recv after probe failure")
                .is_none()
        );
        drop(socket);
        drop(client);
        let _ = release.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), pings).await;
    }

    /// Serves one raw HTTP rejection — head and body in a single TCP write —
    /// so the WebSocket handshake fails and tungstenite buffers the JSON body
    /// beside the response head. `declared_body_len` may exceed the bytes
    /// actually sent to exercise honest truncation flagging.
    async fn raw_handshake_rejection_server(
        body: &'static str,
        declared_body_len: usize,
    ) -> Client {
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
                "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\nx-request-id: req_realtime_401\r\ncontent-length: {declared_body_len}\r\n\r\n{body}"
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
        // 4-17: a 401 handshake must surface status, request id, and a
        // sanitized preview of the JSON error body instead of discarding it.
        let body = r#"{"error":{"message":"no such realtime session","type":"invalid_request_error","code":"session_not_found"}}"#;
        let client = raw_handshake_rejection_server(body, body.len()).await;
        let error = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect_err("401 handshake rejection");
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.request_id(), Some("req_realtime_401"));
        let preview = error.handshake_body().expect("handshake body preview");
        assert!(
            preview.as_str().contains("no such realtime session"),
            "the rejection body must survive the handshake failure, got {}",
            preview.as_str()
        );
        assert!(!preview.is_truncated(), "the whole body was buffered");

        // A declared length longer than the buffered tail is flagged honestly.
        let client = raw_handshake_rejection_server(body, body.len() * 2).await;
        let error = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect_err("401 handshake rejection with a short tail");
        assert!(
            error
                .handshake_body()
                .expect("handshake body preview")
                .is_truncated(),
            "a tail shorter than the declared body must be flagged as truncated"
        );
    }

    /// A received Ping must classify as "ignore and keep reading": tungstenite
    /// 0.29 queues the automatic Pong while the Ping frame is read, so `recv`
    /// must not write an explicit Pong of its own. This locks the branch that
    /// the wire-level test above cannot isolate, because tungstenite coalesces
    /// a user Pong with the pending automatic one (`set_additional` replaces a
    /// queued Pong instead of appending).
    #[test]
    fn realtime_recv_does_not_explicitly_pong_inbound_pings() {
        assert_eq!(
            classify_realtime_inbound(Message::Ping(Bytes::from_static(b"keepalive"))),
            RealtimeInbound::Ignore
        );
        assert_eq!(
            classify_realtime_inbound(Message::Pong(Bytes::from_static(b"keepalive"))),
            RealtimeInbound::Ignore
        );
        assert!(matches!(
            classify_realtime_inbound(Message::text("{}")),
            RealtimeInbound::Event(_)
        ));
        assert_eq!(
            classify_realtime_inbound(Message::Close(None)),
            RealtimeInbound::Closed(None),
            "a frameless EOF stays distinguishable from a coded close"
        );
        let close = CloseFrame {
            code: CloseCode::Error,
            reason: Utf8Bytes::from_static("server exploded"),
        };
        assert_eq!(
            classify_realtime_inbound(Message::Close(Some(close.clone()))),
            RealtimeInbound::Closed(Some(close)),
            "the peer's close frame must survive classification (4-18)"
        );
        assert_eq!(
            classify_realtime_inbound(Message::Binary(Bytes::from_static(b"[]"))),
            RealtimeInbound::Reject("Realtime WebSocket sent a binary data message")
        );
        assert_eq!(
            classify_realtime_inbound(Message::Frame(Frame::ping(Bytes::new()))),
            RealtimeInbound::Reject("Realtime WebSocket exposed an unexpected raw frame")
        );
    }

    #[test]
    fn keepalive_policy_is_opt_in_and_validates_its_fields() {
        let config = RealtimeWebSocketConfig::new();
        assert_eq!(config, RealtimeWebSocketConfig::default());
        assert!(
            config.keepalive.is_none(),
            "keepalive is opt-in and disabled by default"
        );
        // The openai-python reference defaults (20s/20s) must construct cleanly.
        let python_reference =
            RealtimeKeepalive::new(Duration::from_secs(20), Duration::from_secs(20))
                .expect("python-reference keepalive");
        assert_eq!(python_reference.ping_interval(), Duration::from_secs(20));
        assert_eq!(python_reference.pong_timeout(), Duration::from_secs(20));
        assert_eq!(
            python_reference.silence_window(),
            Duration::from_secs(40),
            "the silence window is ping_interval + pong_timeout"
        );

        let enabled = config
            .with_keepalive(Some(python_reference))
            .with_keepalive(None);
        assert!(enabled.keepalive.is_none(), "with_keepalive(None) disables");
        assert!(
            config
                .with_keepalive(Some(python_reference))
                .keepalive
                .is_some()
        );
        let via_intervals = config
            .with_keepalive_intervals(Duration::from_millis(40), Duration::from_millis(60))
            .expect("intervals keepalive");
        assert_eq!(
            via_intervals.keepalive,
            Some(
                RealtimeKeepalive::new(Duration::from_millis(40), Duration::from_millis(60))
                    .expect("rebuild intervals keepalive")
            )
        );

        assert!(matches!(
            RealtimeKeepalive::new(Duration::ZERO, Duration::from_secs(20)),
            Err(Error::InvalidConfiguration(_))
        ));
        assert!(matches!(
            RealtimeKeepalive::new(Duration::from_secs(20), Duration::ZERO),
            Err(Error::InvalidConfiguration(_))
        ));
        // A silence window that would overflow a Duration is rejected at
        // connect time even though both fields are individually non-zero.
        let overflowing =
            RealtimeKeepalive::new(Duration::MAX, Duration::from_secs(1)).expect("non-zero fields");
        assert!(matches!(
            RealtimeWebSocketConfig::new()
                .with_keepalive(Some(overflowing))
                .validate(),
            Err(Error::InvalidConfiguration(_))
        ));
    }

    #[tokio::test]
    async fn keepalive_times_out_idle_connection_without_inbound_frames() {
        let (client, release, pings) = silent_websocket_server().await;
        let keepalive =
            RealtimeKeepalive::new(Duration::from_millis(40), Duration::from_millis(60))
                .expect("valid keepalive");
        let mut socket = client
            .realtime()
            .connect_with(
                "gpt-realtime/test",
                RealtimeWebSocketConfig::new().with_keepalive(Some(keepalive)),
            )
            .await
            .expect("connect Realtime WebSocket with keepalive");
        let started = std::time::Instant::now();
        match socket.recv().await {
            Err(Error::WebSocketProtocol(reason)) => {
                assert_eq!(reason, KEEPALIVE_TIMEOUT_REASON);
            }
            unexpected => panic!("expected a keepalive timeout, got {unexpected:?}"),
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed >= keepalive.silence_window(),
            "timeout must respect the {:?} silence window (fired after {elapsed:?})",
            keepalive.silence_window()
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout must fire promptly (took {elapsed:?})"
        );
        assert!(socket.is_closed(), "a keepalive timeout retires the socket");
        // No automatic reconnect (D0122/D0148): the next recv reports EOF.
        assert!(
            socket
                .recv()
                .await
                .expect("recv after keepalive timeout")
                .is_none()
        );
        drop(socket);
        drop(client);
        let _ = release.send(());
        let pings = tokio::time::timeout(Duration::from_secs(5), pings)
            .await
            .expect("timely server drain")
            .expect("captured keepalive pings");
        assert!(
            !pings.is_empty(),
            "at least one keepalive probe must reach the server"
        );
        assert!(
            pings
                .iter()
                .all(|payload| payload == &Bytes::from_static(KEEPALIVE_PING_PAYLOAD)),
            "every probe must carry the {KEEPALIVE_PING_PAYLOAD:?} payload, got {pings:?}"
        );
    }

    #[tokio::test]
    async fn keepalive_disabled_by_default_leaves_idle_connection_unchanged() {
        let (client, release, pings) = silent_websocket_server().await;
        let mut socket = client
            .realtime()
            .connect("gpt-realtime/test")
            .await
            .expect("connect default Realtime WebSocket");
        let outcome = tokio::time::timeout(Duration::from_millis(250), socket.recv()).await;
        assert!(
            outcome.is_err(),
            "without keepalive a silent socket must stay pending, got {outcome:?}"
        );
        drop(socket);
        drop(client);
        let _ = release.send(());
        let pings = tokio::time::timeout(Duration::from_secs(5), pings)
            .await
            .expect("timely server drain")
            .expect("captured frames");
        assert!(
            pings.is_empty(),
            "no keepalive probes may be sent while the option is disabled"
        );
    }

    #[tokio::test]
    async fn keepalive_silence_window_refreshes_on_every_inbound_frame() {
        // Events every 30ms stay well inside the 120ms silence window
        // (40ms ping interval + 80ms pong timeout), so none of the eight
        // recvs may time out; only the silence after the last event may.
        let (client, release, pings) =
            periodic_event_websocket_server(Duration::from_millis(30), 8).await;
        let keepalive =
            RealtimeKeepalive::new(Duration::from_millis(40), Duration::from_millis(80))
                .expect("valid keepalive");
        let mut socket = client
            .realtime()
            .connect_with(
                "gpt-realtime/test",
                RealtimeWebSocketConfig::new().with_keepalive(Some(keepalive)),
            )
            .await
            .expect("connect Realtime WebSocket with keepalive");
        for _ in 0..8 {
            let event = socket
                .recv()
                .await
                .expect("events inside the silence window must not time out")
                .expect("one periodic event");
            assert_eq!(event.event_type(), "future.server.event");
        }
        // The server now neither sends events nor reads (so no auto-Pong):
        // the connection is fully idle and must hit the silence window.
        match socket.recv().await {
            Err(Error::WebSocketProtocol(reason)) => {
                assert_eq!(reason, KEEPALIVE_TIMEOUT_REASON);
            }
            unexpected => {
                panic!("expected a timeout after the stream went quiet, got {unexpected:?}")
            }
        }
        assert!(socket.is_closed(), "a keepalive timeout retires the socket");
        drop(socket);
        drop(client);
        let _ = release.send(());
        let pings = tokio::time::timeout(Duration::from_secs(5), pings)
            .await
            .expect("timely server drain")
            .expect("captured keepalive pings");
        assert!(
            !pings.is_empty(),
            "the idle tail must still have probed the peer"
        );
    }

    #[tokio::test]
    async fn keepalive_reanchors_when_recv_polling_resumes_after_a_gap() {
        // 7-09: the silence window only counts while `recv` is being awaited.
        // The peer stays live and answers probes, so leaving `recv` unpolled
        // for three silence windows must not be held against the connection:
        // the window re-anchors to the resume instant instead of the first
        // post-gap tick judging the stale pre-pause anchor and retiring a
        // healthy socket essentially immediately.
        let (client, pings) = live_quiet_websocket_server().await;
        let keepalive =
            RealtimeKeepalive::new(Duration::from_millis(40), Duration::from_millis(60))
                .expect("valid keepalive");
        let mut socket = client
            .realtime()
            .connect_with(
                "gpt-realtime/test",
                RealtimeWebSocketConfig::new().with_keepalive(Some(keepalive)),
            )
            .await
            .expect("connect Realtime WebSocket with keepalive");
        tokio::time::sleep(keepalive.silence_window() * 3).await;
        // Resumed polling must stay pending against the live peer: the probes
        // it sends are answered by the automatic Pong, which refreshes the
        // window like any inbound frame.
        let outcome = tokio::time::timeout(Duration::from_millis(250), socket.recv()).await;
        assert!(
            outcome.is_err(),
            "recv must stay pending on the live peer instead of timing out on the stale anchor, got {outcome:?}"
        );
        assert!(
            !socket.is_closed(),
            "a paused recv must not retire a healthy socket"
        );
        drop(socket);
        drop(client);
        let pings = tokio::time::timeout(Duration::from_secs(5), pings)
            .await
            .expect("timely server drain")
            .expect("captured keepalive pings");
        assert!(
            !pings.is_empty(),
            "probing must resume together with recv polling"
        );
    }

    #[tokio::test]
    async fn send_write_failure_retires_the_realtime_socket() {
        // 7-22: a failed send write leaves the socket unusable in both
        // directions, so it is retired like every recv-side failure (4-19,
        // D0212) instead of staying half-open. The write fails
        // deterministically against a `max_write_buffer_size` smaller than one
        // text frame (a 12+-byte frame against an 8-byte cap).
        let (client, pings) = live_quiet_websocket_server().await;
        let mut socket = client
            .realtime()
            .connect_with(
                "gpt-realtime/test",
                RealtimeWebSocketConfig::new()
                    .write_buffer_bytes(1)
                    .max_queued_write_bytes(8),
            )
            .await
            .expect("connect Realtime WebSocket with a tiny write buffer");
        match socket.append_audio(vec![0, 1, 2]).await {
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
            "a failed send must retire the Realtime socket"
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
                socket.append_audio(vec![3]).await,
                Err(Error::WebSocketProtocol(_))
            ),
            "a later send must report the closed socket"
        );
        drop(socket);
        drop(client);
        let _ = tokio::time::timeout(Duration::from_secs(5), pings).await;
    }

    #[test]
    fn websocket_url_derives_model_intent_and_call_id_targets() {
        let base = Url::parse("https://api.openai.com/v1/").expect("platform base");
        let model = realtime_websocket_url(&base, &RealtimeConnectTarget::model("gpt-realtime"))
            .expect("model target URL");
        assert_eq!(
            model.as_str(),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime"
        );

        let intent = realtime_websocket_url(&base, &RealtimeConnectTarget::TranscriptionIntent)
            .expect("transcription intent URL");
        assert_eq!(
            intent.as_str(),
            "wss://api.openai.com/v1/realtime?intent=transcription"
        );
        assert!(
            !intent.query_pairs().any(|(key, _)| key == "model"),
            "transcription sessions never carry a model"
        );

        let call =
            realtime_websocket_url(&base, &RealtimeConnectTarget::call_id("call_123 space/a"))
                .expect("call attach URL");
        assert_eq!(call.path(), "/v1/realtime");
        assert_eq!(
            call.query_pairs()
                .find(|(key, _)| key == "call_id")
                .map(|(_, value)| value),
            Some("call_123 space/a".into())
        );
        assert_eq!(
            call.as_str(),
            "wss://api.openai.com/v1/realtime?call_id=call_123+space%2Fa"
        );

        // The connect(model) convenience maps onto the same Model branch.
        let from_model =
            realtime_websocket_url(&base, &RealtimeConnectTarget::from(ModelId::new("gpt-a")))
                .expect("From<ModelId> target URL");
        assert_eq!(
            from_model.as_str(),
            "wss://api.openai.com/v1/realtime?model=gpt-a"
        );

        // Unrelated base query parameters survive alongside the target key.
        let versioned = Url::parse("https://gateway.example/v1/?api-version=2026-01-01")
            .expect("versioned base");
        let url = realtime_websocket_url(&versioned, &RealtimeConnectTarget::model("gpt-realtime"))
            .expect("versioned base keeps its own query");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "api-version")
                .map(|(_, value)| value),
            Some("2026-01-01".into())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "model")
                .map(|(_, value)| value),
            Some("gpt-realtime".into())
        );
    }

    #[test]
    fn websocket_url_rejects_conflicting_target_query_keys() {
        let pinned_model = Url::parse("https://api.openai.com/v1/?model=stale").expect("base");
        assert!(matches!(
            realtime_websocket_url(&pinned_model, &RealtimeConnectTarget::TranscriptionIntent),
            Err(Error::InvalidConfiguration(_))
        ));

        let pinned_intent =
            Url::parse("https://api.openai.com/v1/?intent=transcription").expect("base");
        assert!(matches!(
            realtime_websocket_url(
                &pinned_intent,
                &RealtimeConnectTarget::model("gpt-realtime")
            ),
            Err(Error::InvalidConfiguration(_))
        ));

        let pinned_call = Url::parse("https://api.openai.com/v1/?call_id=call_1").expect("base");
        assert!(matches!(
            realtime_websocket_url(&pinned_call, &RealtimeConnectTarget::TranscriptionIntent),
            Err(Error::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn websocket_url_rejects_empty_target_values() {
        // 5-09: an empty model or call id must never reach the wire as a bare
        // `?model=` / `?call_id=` key; every connect entry point surfaces
        // InvalidConfiguration instead (openai-node rejects empty targets
        // outright). The guard also covers the `From<ModelId>` path and direct
        // enum construction, which bypass the named constructors.
        let base = Url::parse("https://api.openai.com/v1/").expect("platform base");
        for target in [
            RealtimeConnectTarget::model(""),
            RealtimeConnectTarget::call_id(""),
            RealtimeConnectTarget::from(ModelId::new("")),
        ] {
            assert!(
                matches!(
                    realtime_websocket_url(&base, &target),
                    Err(Error::InvalidConfiguration(_))
                ),
                "an empty target must be rejected, got {target:?}"
            );
        }
    }

    #[tokio::test]
    async fn transcription_intent_connection_uses_intent_query_without_model() {
        let (client, handshake, events) = websocket_server().await;
        let mut socket = client
            .realtime()
            .connect_transcription()
            .await
            .expect("connect transcription Realtime WebSocket");
        socket
            .append_audio(vec![3, 4])
            .await
            .expect("append transcription audio");
        let _ = socket.recv().await.expect("receive future event");
        socket
            .cancel_response(None)
            .await
            .expect("cancel transcription response");
        socket.close().await.expect("close transcription socket");

        let handshake = handshake.await.expect("captured transcription handshake");
        let url = Url::parse(&format!("http://loopback{}", handshake.path_and_query))
            .expect("captured transcription URL");
        assert_eq!(url.path(), "/v1/realtime");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(
            pairs,
            vec![("intent".to_owned(), "transcription".to_owned())]
        );
        assert_eq!(
            handshake.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let events = events.await.expect("captured transcription events");
        assert_eq!(events[0]["type"], "input_audio_buffer.append");
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
        assert_eq!(
            CreateClientSecret::META.retry,
            RetryClass::Replayable,
            "idempotent secret issuance is retryable"
        );
    }

    #[tokio::test]
    async fn translation_client_secret_uses_fixed_typed_route_and_redacts_secret() {
        let (client, captured) = http_server(
            StatusCode::OK,
            "application/json",
            r#"{"value":"ek_translation_private","expires_at":123,"session":{"id":"sess_translation","type":"translation","expires_at":123,"model":"gpt-realtime-translate","audio":{"input":{"transcription":null,"noise_reduction":null},"output":{"language":"es"}}}}"#,
            None,
        )
        .await;
        let session = RealtimeTranslationSessionCreateRequest::new("gpt-realtime-translate")
            .with_audio(
                RealtimeTranslationAudio::default()
                    .with_input(RealtimeTranslationAudioInput::default().with_transcription_null())
                    .with_output(RealtimeTranslationAudioOutput::new("es")),
            );
        let request = RealtimeTranslationClientSecretCreateRequest::new(session)
            .with_expires_after(RealtimeTranslationClientSecretExpiration::new(600));
        let response = client
            .realtime()
            .create_translation_client_secret(request)
            .await
            .expect("translation client secret");
        assert!(!format!("{:?}", response.body()).contains("ek_translation_private"));
        assert_eq!(response.session.model, "gpt-realtime-translate");
        assert_eq!(response.request_id(), Some("req_realtime_http"));

        let captured = captured.await.expect("captured translation secret request");
        assert_eq!(captured.method, reqwest::Method::POST);
        assert_eq!(captured.path, "/v1/realtime/translations/client_secrets");
        let body: Value = serde_json::from_slice(&captured.body).expect("translation JSON");
        assert_eq!(body["expires_after"]["anchor"], "created_at");
        assert_eq!(body["expires_after"]["seconds"], 600);
        assert_eq!(body["session"]["model"], "gpt-realtime-translate");
        assert_eq!(
            body["session"]["audio"]["input"]["transcription"],
            Value::Null
        );
        assert_eq!(
            CreateTranslationClientSecret::META.retry,
            RetryClass::Replayable,
            "idempotent translation secret issuance is retryable"
        );
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
    async fn create_call_without_session_sends_a_bare_sdp_request() {
        // 5-03: omitting the session switches to the sdp-only content type —
        // the raw offer text under `application/sdp`, not a one-part
        // multipart. `RealtimeCallCreateRequest::new` starts omitted.
        let (client, captured) = http_server(
            StatusCode::CREATED,
            "application/sdp",
            "v=0\r\na=answer\r\n",
            Some("/v1/realtime/calls/call_bare"),
        )
        .await;
        let response = client
            .realtime()
            .create_call(RealtimeCallCreateRequest::new("v=0\r\na=offer\r\n"))
            .await
            .expect("created bare Realtime call");
        assert_eq!(response.call_id(), "call_bare");
        assert_eq!(response.sdp().as_str(), "v=0\r\na=answer\r\n");

        let captured = captured.await.expect("captured bare call request");
        assert_eq!(captured.method, reqwest::Method::POST);
        assert_eq!(captured.path, "/v1/realtime/calls");
        assert_eq!(
            captured.content_type.as_deref(),
            Some("application/sdp"),
            "the sdp-only body must ride a bare content type, got {:?}",
            captured.content_type
        );
        assert_eq!(
            captured.body,
            b"v=0\r\na=offer\r\n".as_slice(),
            "the body must be the raw SDP text"
        );
    }

    #[tokio::test]
    async fn create_call_is_single_shot_even_after_a_retryable_looking_error() {
        // 3-20: creating a call is side-effecting (a replay could place two
        // live calls), so a 500 — a status the transport would retry for
        // replayable operations — must surface after exactly one attempt.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind call-retry server");
        let address = listener.local_addr().expect("call-retry server address");
        let attempts = Arc::new(Mutex::new(0_usize));
        let service_attempts = Arc::clone(&attempts);
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept call-retry HTTP");
            let service = service_fn(move |request: Request<Incoming>| {
                let attempts = Arc::clone(&service_attempts);
                async move {
                    *attempts.lock().expect("call-retry lock") += 1;
                    let _ = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read call-retry body");
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(Full::new(Bytes::from_static(
                                b"{\"error\":{\"message\":\"boom\",\"type\":\"server_error\",\"code\":null}}",
                            )))
                            .expect("build call-retry response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve call-retry HTTP");
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("call-retry base");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("call-retry client");
        let error = client
            .realtime()
            .create_call(RealtimeCallCreateRequest::new("v=0\r\na=offer\r\n"))
            .await
            .expect_err("a 500 must surface as an error");
        assert!(
            matches!(error, Error::Api(_)),
            "the surfaced failure must be the API error itself, got {error:?}"
        );
        assert_eq!(
            *attempts.lock().expect("call-retry lock"),
            1,
            "create_call must never replay its multipart request"
        );
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
        for operation in [
            ("accept", AcceptCall::META.retry),
            ("reject", RejectCall::META.retry),
            ("reject default", RejectCallDefault::META.retry),
            ("hangup", HangupCall::META.retry),
            ("refer", ReferCall::META.retry),
        ] {
            assert_eq!(
                operation.1,
                RetryClass::Never,
                "call-control action {} keeps its never-retry guard",
                operation.0
            );
        }

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

    #[tokio::test]
    async fn reject_call_covers_typed_and_default_wire_modes() {
        let (client, typed) = http_server(StatusCode::OK, "application/json", "", None).await;
        let response = client
            .realtime()
            .reject_call(
                "call/a b",
                RealtimeCallRejectRequest::new().with_status_code(486),
            )
            .await
            .expect("typed reject SIP call");
        assert_eq!(response.request_id(), Some("req_realtime_http"));
        let captured = typed.await.expect("captured typed reject");
        assert_eq!(captured.method, reqwest::Method::POST);
        assert_eq!(captured.path, "/v1/realtime/calls/call%2Fa%20b/reject");
        assert_eq!(captured.content_type.as_deref(), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("typed reject JSON"),
            json!({"status_code": 486})
        );

        let (client, default) = http_server(StatusCode::OK, "application/json", "", None).await;
        let response = client
            .realtime()
            .reject_call_default("call/a b")
            .await
            .expect("default reject SIP call");
        assert_eq!(response.request_id(), Some("req_realtime_http"));
        let captured = default.await.expect("captured default reject");
        assert_eq!(captured.method, reqwest::Method::POST);
        assert_eq!(captured.path, "/v1/realtime/calls/call%2Fa%20b/reject");
        assert!(captured.content_type.is_none());
        assert!(captured.body.is_empty());
    }
}
