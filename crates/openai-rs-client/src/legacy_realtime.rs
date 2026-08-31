//! Deprecated pre-GA Realtime session-token endpoints.
//!
//! New integrations should use GA Realtime client secrets and calls. This
//! compatibility facade exists only behind the default-off `legacy-realtime`
//! feature.

use http::{Method, StatusCode};
use openai_rs_types::legacy_realtime::{
    LegacyRealtimeSessionCreateRequest, LegacyRealtimeSessionCreateResponse,
    LegacyRealtimeTranscriptionSessionCreateRequest,
    LegacyRealtimeTranscriptionSessionCreateResponse,
};

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Deprecated REST session-token facade.
///
/// Use GA `Client::realtime().create_client_secret` and the GA Realtime calls
/// APIs for new integrations.
#[deprecated(
    since = "0.1.0",
    note = "use GA Realtime client_secrets and calls APIs instead"
)]
#[derive(Clone, Debug)]
pub struct LegacyRealtimeSessions {
    client: Client,
}

#[allow(deprecated)]
impl LegacyRealtimeSessions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a legacy flat Realtime session and ephemeral token.
    ///
    /// The pinned operations declare no `OpenAI-Beta` header, so no beta
    /// header is sent; `assistants=v2` belongs to the Assistants family.
    pub async fn create(
        &self,
        request: LegacyRealtimeSessionCreateRequest,
    ) -> Result<ApiResponse<LegacyRealtimeSessionCreateResponse>, Error> {
        let path = [
            PathSegment::literal("realtime"),
            PathSegment::literal("sessions"),
        ];
        self.client
            .transport()
            .execute_json::<CreateLegacyRealtimeSession, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates a legacy flat transcription session and ephemeral token.
    pub async fn create_transcription(
        &self,
        request: LegacyRealtimeTranscriptionSessionCreateRequest,
    ) -> Result<ApiResponse<LegacyRealtimeTranscriptionSessionCreateResponse>, Error> {
        let path = [
            PathSegment::literal("realtime"),
            PathSegment::literal("transcription_sessions"),
        ];
        self.client
            .transport()
            .execute_json::<CreateLegacyRealtimeTranscriptionSession, ()>(
                &path,
                None,
                Some(&request),
            )
            .await
    }
}

struct CreateLegacyRealtimeSession;

impl Sealed for CreateLegacyRealtimeSession {}

impl Operation for CreateLegacyRealtimeSession {
    type Request = LegacyRealtimeSessionCreateRequest;
    type Response = LegacyRealtimeSessionCreateResponse;

    const META: OperationMeta = OperationMeta {
        id: "create-realtime-session",
        method: Method::POST,
        route: "/realtime/sessions",
        auth: AuthScope::Platform,
        request_encoding: RequestEncoding::Json,
        response_mode: ResponseMode::Json,
        retry: RetryClass::Replayable,
        success_statuses: OK,
    };
}

struct CreateLegacyRealtimeTranscriptionSession;

impl Sealed for CreateLegacyRealtimeTranscriptionSession {}

impl Operation for CreateLegacyRealtimeTranscriptionSession {
    type Request = LegacyRealtimeTranscriptionSessionCreateRequest;
    type Response = LegacyRealtimeTranscriptionSessionCreateResponse;

    const META: OperationMeta = OperationMeta {
        id: "create-realtime-transcription-session",
        method: Method::POST,
        route: "/realtime/transcription_sessions",
        auth: AuthScope::Platform,
        request_encoding: RequestEncoding::Json,
        response_mode: ResponseMode::Json,
        retry: RetryClass::Replayable,
        success_statuses: OK,
    };
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        Nullable,
        legacy_realtime::{
            LegacyRealtimeAudioFormat, LegacyRealtimeSessionCreateRequest,
            LegacyRealtimeTranscriptionSessionCreateRequest,
        },
        realtime::RealtimeOutputModality,
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        authorization: Option<String>,
        beta: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(response_body: String) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind legacy Realtime contract server");
        let address = listener.local_addr().expect("legacy Realtime address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept legacy request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let response_body = response_body.clone();
                let sender = Arc::clone(&sender);
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let authorization = header_string(&request, http::header::AUTHORIZATION);
                    let beta = header_string(&request, "OpenAI-Beta");
                    let content_type = header_string(&request, http::header::CONTENT_TYPE);
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("collect legacy request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("legacy sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            authorization,
                            beta,
                            content_type,
                            body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_legacy_realtime")
                            .body(Full::new(Bytes::from(response_body)))
                            .expect("legacy Realtime response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve legacy Realtime request");
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("legacy Realtime base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("legacy Realtime client");
        (client, receiver)
    }

    fn header_string(
        request: &Request<Incoming>,
        name: impl http::header::AsHeaderName,
    ) -> Option<String> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }

    #[tokio::test]
    async fn session_creation_uses_pinned_route_without_beta_header() {
        let response = json!({
            "id": "sess_001",
            "object": "realtime.session",
            "model": "gpt-realtime",
            "modalities": ["audio", "text"],
            "client_secret": {
                "value": "ek_private_session",
                "expires_at": 1234567890
            }
        })
        .to_string();
        let (client, captured) = serve_once(response).await;
        let request = LegacyRealtimeSessionCreateRequest::new()
            .with_model("gpt-realtime")
            .with_modalities(vec![
                RealtimeOutputModality::Audio,
                RealtimeOutputModality::Text,
            ])
            .with_instructions("friendly");
        let response = client
            .legacy_realtime_sessions()
            .create(request)
            .await
            .expect("legacy session response");

        assert_eq!(response.request_id(), Some("req_legacy_realtime"));
        assert_eq!(response.id(), Some("sess_001"));
        assert!(!format!("{response:?}").contains("ek_private_session"));

        let captured = captured.await.expect("captured legacy session request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/realtime/sessions");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(
            captured.beta.is_none(),
            "the pinned operation declares no OpenAI-Beta header"
        );
        assert_eq!(captured.content_type.as_deref(), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("legacy session body"),
            json!({
                "model": "gpt-realtime",
                "modalities": ["audio", "text"],
                "instructions": "friendly"
            })
        );
        assert_eq!(CreateLegacyRealtimeSession::META.method, Method::POST);
        assert_eq!(
            CreateLegacyRealtimeSession::META.route,
            "/realtime/sessions"
        );
        assert_eq!(
            CreateLegacyRealtimeSession::META.retry,
            RetryClass::Replayable,
            "legacy session-token issuance is idempotent"
        );
    }

    #[tokio::test]
    async fn transcription_creation_uses_pinned_route_and_nullable_secret() {
        let response = json!({
            "id": "sess_transcription",
            "object": "realtime.transcription_session",
            "input_audio_format": "pcm16",
            "client_secret": null
        })
        .to_string();
        let (client, captured) = serve_once(response).await;
        let request = LegacyRealtimeTranscriptionSessionCreateRequest::new()
            .with_input_audio_format(LegacyRealtimeAudioFormat::Pcm16)
            .include("item.input_audio_transcription.logprobs");
        let response = client
            .legacy_realtime_sessions()
            .create_transcription(request)
            .await
            .expect("legacy transcription session response");

        assert_eq!(response.id(), Some("sess_transcription"));
        assert!(matches!(response.client_secret(), Nullable::Null));
        assert!(!format!("{response:?}").contains("ek_"));

        let captured = captured
            .await
            .expect("captured legacy transcription request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/realtime/transcription_sessions");
        assert!(
            captured.beta.is_none(),
            "the pinned operation declares no OpenAI-Beta header"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body)
                .expect("legacy transcription request body"),
            json!({
                "input_audio_format": "pcm16",
                "include": ["item.input_audio_transcription.logprobs"]
            })
        );
        assert_eq!(
            CreateLegacyRealtimeTranscriptionSession::META.route,
            "/realtime/transcription_sessions"
        );
        assert_eq!(
            CreateLegacyRealtimeTranscriptionSession::META.retry,
            RetryClass::Replayable,
            "legacy transcription-token issuance is idempotent"
        );
    }
}
