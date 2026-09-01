//! Access-controlled Custom Voice client operations.
//!
//! These bindings require the default-off `custom-voice` feature and service
//! access granted by OpenAI to an eligible customer.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    CreateVoiceConsentRequest, CreateVoiceRequest, DeletedVoiceConsent, ListVoiceConsentsParams,
    MAX_CUSTOM_VOICE_AUDIO_BYTES, UpdateVoiceConsentRequest, Voice, VoiceConsent, VoiceConsentId,
    VoiceConsentList,
};

use crate::{
    ApiResponse, Client, Error,
    multipart::{PreparedReplayableSource, ReplayableMultipartForm},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const JSON_MIME: &str = "application/json";

/// Pages returned by the access-controlled consent collection.
pub type VoiceConsentPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<VoiceConsentList>, Error>> + Send + 'static>>;

/// Access-controlled custom voice operations.
#[derive(Clone, Debug)]
pub struct Voices {
    client: Client,
}

impl Voices {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a custom voice from an eligible customer's sample and consent.
    pub async fn create(&self, request: &CreateVoiceRequest) -> Result<ApiResponse<Voice>, Error> {
        let sample = prepare_bounded(request.audio_sample()).await?;
        let form = ReplayableMultipartForm::new()
            .text("name", request.name())
            .text("consent", request.consent().as_str())
            .part("audio_sample", sample);
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("voices"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form("CreateVoice", &path, &form, JSON_MIME)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }

    /// Returns voice-consent operations.
    #[must_use]
    pub fn consents(&self) -> VoiceConsents {
        VoiceConsents::new(self.client.clone())
    }
}

/// Access-controlled voice consent operations.
#[derive(Clone, Debug)]
pub struct VoiceConsents {
    client: Client,
}

impl VoiceConsents {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Lists consent recordings visible to this eligible project.
    pub async fn list(
        &self,
        params: ListVoiceConsentsParams,
    ) -> Result<ApiResponse<VoiceConsentList>, Error> {
        let path = voice_consents_path();
        self.client
            .transport()
            .execute_json::<ListVoiceConsents, _>(&path, Some(&params), None)
            .await
    }

    /// Uploads a consent recording.
    pub async fn create(
        &self,
        request: &CreateVoiceConsentRequest,
    ) -> Result<ApiResponse<VoiceConsent>, Error> {
        let recording = prepare_bounded(request.recording()).await?;
        let form = ReplayableMultipartForm::new()
            .text("name", request.name())
            .text("language", request.language())
            .part("recording", recording);
        let path = voice_consents_path();
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form("CreateVoiceConsent", &path, &form, JSON_MIME)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }

    /// Retrieves one consent recording's metadata.
    pub async fn retrieve(
        &self,
        consent_id: &VoiceConsentId,
    ) -> Result<ApiResponse<VoiceConsent>, Error> {
        let path = voice_consent_path(consent_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveVoiceConsent, ()>(&path, None, None)
            .await
    }

    /// Renames one consent recording.
    pub async fn update(
        &self,
        consent_id: &VoiceConsentId,
        request: UpdateVoiceConsentRequest,
    ) -> Result<ApiResponse<VoiceConsent>, Error> {
        let path = voice_consent_path(consent_id)?;
        self.client
            .transport()
            .execute_json::<UpdateVoiceConsent, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes one consent recording.
    pub async fn delete(
        &self,
        consent_id: &VoiceConsentId,
    ) -> Result<ApiResponse<DeletedVoiceConsent>, Error> {
        let path = voice_consent_path(consent_id)?;
        self.client
            .transport()
            .execute_json::<DeleteVoiceConsent, ()>(&path, None, None)
            .await
    }

    /// Streams consent pages and rejects missing or repeated cursors.
    #[must_use]
    pub fn list_pages(&self, params: ListVoiceConsentsParams) -> VoiceConsentPageStream {
        let consents = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            crate::pagination::seed_seen(
                &mut seen,
                params.after_ref().map(|cursor| cursor.as_str()),
            );
            loop {
                let page = consents.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    page.last_id().map(|id| id.as_str()),
                    page.data().last().map(|consent| consent.id().as_str()),
                    &mut seen,
                    "voice-consent",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(VoiceConsentId::new(cursor)),
                    None => break,
                }
            }
        })
    }
}

async fn prepare_bounded(
    source: &openai_rs_types::ReplayableMultipartSource,
) -> Result<PreparedReplayableSource, Error> {
    let prepared = PreparedReplayableSource::prepare(source).await?;
    if prepared.length() > MAX_CUSTOM_VOICE_AUDIO_BYTES {
        // Send-time half of the two-phase check: in-memory sources fail at
        // construction with `VoiceRequestError::AudioTooLarge`, while file and
        // stream sources only become measurable here.
        return Err(Error::RequestPayloadTooLarge {
            limit_bytes: MAX_CUSTOM_VOICE_AUDIO_BYTES as usize,
        });
    }
    Ok(prepared)
}

fn voice_consents_path() -> [PathSegment<'static>; 2] {
    [
        PathSegment::literal("audio"),
        PathSegment::literal("voice_consents"),
    ]
}

fn voice_consent_path(consent_id: &VoiceConsentId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("audio"),
        PathSegment::literal("voice_consents"),
        PathSegment::parameter("consent_id", consent_id.as_str())?,
    ])
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr,
        retry = $retry:expr $(,)?
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
                response_mode: ResponseMode::Json,
                retry: $retry,
                success_statuses: OK,
            };
        }
    };
}

operation!(
    ListVoiceConsents,
    request = (),
    response = VoiceConsentList,
    method = Method::GET,
    route = "/audio/voice_consents",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    RetrieveVoiceConsent,
    request = (),
    response = VoiceConsent,
    method = Method::GET,
    route = "/audio/voice_consents/{consent_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    UpdateVoiceConsent,
    request = UpdateVoiceConsentRequest,
    response = VoiceConsent,
    method = Method::POST,
    route = "/audio/voice_consents/{consent_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    DeleteVoiceConsent,
    request = (),
    response = DeletedVoiceConsent,
    method = Method::DELETE,
    route = "/audio/voice_consents/{consent_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable
);

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::ReplayableMultipartSource;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::mpsc};
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Client, mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (sender, receiver) = mpsc::channel(16);
        tokio::spawn(async move {
            loop {
                if responses.lock().expect("response queue lock").is_empty() {
                    break;
                }
                let (stream, _) = listener.accept().await.expect("accept request");
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
                        let content_type = request
                            .headers()
                            .get(http::header::CONTENT_TYPE)
                            .and_then(|value| value.to_str().ok())
                            .map(ToOwned::to_owned);
                        let body = request
                            .into_body()
                            .collect()
                            .await
                            .expect("read request body")
                            .to_bytes()
                            .to_vec();
                        sender
                            .send(CapturedRequest {
                                method,
                                path_and_query,
                                content_type,
                                body,
                            })
                            .await
                            .expect("capture request");
                        let (status, body) = responses
                            .lock()
                            .expect("response queue lock")
                            .pop_front()
                            .expect("response per request");
                        Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .status(status)
                                .header(http::header::CONTENT_TYPE, "application/json")
                                .header(http::header::CONNECTION, "close")
                                .header("x-request-id", "req_voice")
                                .body(Full::new(Bytes::from(body)))
                                .expect("build response"),
                        )
                    }
                });
                http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .expect("serve request");
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("loopback URL");
        let key = ApiKey::new("test-placeholder-key").expect("valid key");
        let client = Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("build client");
        (client, receiver)
    }

    fn source(bytes: &'static [u8]) -> ReplayableMultipartSource {
        ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(bytes))
            .try_with_file_name("voice.wav")
            .expect("safe filename")
            .try_with_media_type("audio/x-wav")
            .expect("safe MIME")
    }

    #[tokio::test]
    async fn oversized_send_time_audio_reports_request_payload_too_large() {
        // The byte source exceeds the pinned 10 MiB cap by one byte; only the
        // send-time half of the two-phase check can observe prepared length.
        let oversized: &'static [u8] =
            Box::leak(vec![0u8; 10 * 1024 * 1024 + 1].into_boxed_slice());
        let error = super::prepare_bounded(&source(oversized))
            .await
            .map(|_| ())
            .expect_err("oversized audio must fail before transport");
        assert!(matches!(
            error,
            crate::error::Error::RequestPayloadTooLarge { limit_bytes }
                if limit_bytes == 10 * 1024 * 1024
        ));
    }

    fn consent_json() -> Value {
        json!({
            "object": "audio.voice_consent",
            "id": "cons_1",
            "name": "Owner",
            "language": "en-US",
            "created_at": 1
        })
    }

    #[test]
    fn operation_manifest_covers_json_routes() {
        let operations = [
            &ListVoiceConsents::META,
            &RetrieveVoiceConsent::META,
            &UpdateVoiceConsent::META,
            &DeleteVoiceConsent::META,
        ];
        assert_eq!(operations.len(), 4);
        assert_eq!(ListVoiceConsents::META.route, "/audio/voice_consents");
        assert_eq!(UpdateVoiceConsent::META.method, Method::POST);
        assert_eq!(DeleteVoiceConsent::META.method, Method::DELETE);
        assert!(
            operations
                .iter()
                .all(|operation| operation.auth == AuthScope::Platform)
        );
    }

    #[tokio::test]
    async fn consent_page_stream_advances_cursor_and_stops() {
        // 8-20: the consent pagination glue reads its cursor from the Option
        // source `last_id()` (an omitted-or-null envelope cursor), so a null
        // `last_id` with `has_more` must fall back to the last item's id —
        // the same D0147 resolution order as the other surfaces.
        let first = json!({
            "object": "list",
            "data": [consent_json()],
            "first_id": "cons_1",
            "last_id": null,
            "has_more": true
        });
        let second = json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": null,
            "has_more": false
        });
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, first.to_string()),
            (StatusCode::OK, second.to_string()),
        ])
        .await;
        let consents = client.voices().consents();

        let pages = consents
            .list_pages(ListVoiceConsentsParams::new().limit(2))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));
        assert_eq!(pages[0].as_ref().expect("first page").data().len(), 1);

        let first_request = captured.recv().await.expect("first consent page request");
        let second_request = captured.recv().await.expect("second consent page request");
        assert_eq!(
            first_request.path_and_query,
            "/v1/audio/voice_consents?limit=2"
        );
        // The null envelope cursor fell back to the last item id `cons_1`.
        assert_eq!(
            second_request.path_and_query,
            "/v1/audio/voice_consents?after=cons_1&limit=2"
        );
        assert!(captured.recv().await.is_none());
    }

    #[tokio::test]
    async fn all_six_operations_use_fixed_typed_wire_contracts() {
        let list = json!({
            "object": "list",
            "data": [consent_json()],
            "first_id": "cons_1",
            "last_id": "cons_1",
            "has_more": false
        });
        let deleted = json!({
            "object": "audio.voice_consent",
            "id": "cons_1",
            "deleted": true
        });
        let voice = json!({
            "object": "audio.voice",
            "id": "voice_1",
            "name": "Voice",
            "created_at": 2
        });
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, list.to_string()),
            (StatusCode::OK, consent_json().to_string()),
            (StatusCode::OK, consent_json().to_string()),
            (StatusCode::OK, consent_json().to_string()),
            (StatusCode::OK, deleted.to_string()),
            (StatusCode::OK, voice.to_string()),
        ])
        .await;
        let voices = client.voices();
        let consents = voices.consents();

        consents
            .list(ListVoiceConsentsParams::new().limit(2))
            .await
            .expect("list consents");
        consents
            .create(
                &CreateVoiceConsentRequest::new("Owner", "en-US", source(b"CONSENT_AUDIO"))
                    .expect("valid consent upload"),
            )
            .await
            .expect("create consent");
        consents
            .retrieve(&VoiceConsentId::new("cons_1"))
            .await
            .expect("retrieve consent");
        consents
            .update(
                &VoiceConsentId::new("cons_1"),
                UpdateVoiceConsentRequest::new("Renamed").expect("valid rename"),
            )
            .await
            .expect("update consent");
        consents
            .delete(&VoiceConsentId::new("cons_1"))
            .await
            .expect("delete consent");
        voices
            .create(
                &CreateVoiceRequest::new("Voice", "cons_1", source(b"VOICE_SAMPLE"))
                    .expect("valid voice upload"),
            )
            .await
            .expect("create voice");

        let mut requests = Vec::new();
        for _ in 0..6 {
            requests.push(captured.recv().await.expect("captured operation"));
        }
        assert_eq!(
            requests
                .iter()
                .map(|request| request.method.clone())
                .collect::<Vec<_>>(),
            vec![
                Method::GET,
                Method::POST,
                Method::GET,
                Method::POST,
                Method::DELETE,
                Method::POST,
            ]
        );
        assert!(
            requests[0]
                .path_and_query
                .starts_with("/v1/audio/voice_consents?")
        );
        assert_eq!(&requests[1].path_and_query, "/v1/audio/voice_consents");
        assert_eq!(
            &requests[2].path_and_query,
            "/v1/audio/voice_consents/cons_1"
        );
        assert_eq!(&requests[5].path_and_query, "/v1/audio/voices");

        let consent_body = String::from_utf8_lossy(&requests[1].body);
        assert!(consent_body.contains("name=\"recording\""));
        assert!(consent_body.contains("CONSENT_AUDIO"));
        let voice_body = String::from_utf8_lossy(&requests[5].body);
        assert!(voice_body.contains("name=\"audio_sample\""));
        assert!(voice_body.contains("VOICE_SAMPLE"));
        assert!(
            requests[1]
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let update_body: Value = serde_json::from_slice(&requests[3].body).expect("update JSON");
        assert_eq!(update_body, json!({"name": "Renamed"}));
    }

    #[tokio::test]
    async fn ineligible_project_error_stays_typed() {
        let (client, _captured) = serve_sequence(vec![(
            StatusCode::FORBIDDEN,
            json!({
                "error": {
                    "message": "custom voices are not enabled for this project",
                    "type": "permission_error",
                    "param": null,
                    "code": "feature_not_enabled"
                }
            })
            .to_string(),
        )])
        .await;
        let error = client
            .voices()
            .consents()
            .list(ListVoiceConsentsParams::new())
            .await
            .expect_err("ineligible project must fail");
        assert_eq!(error.status(), Some(StatusCode::FORBIDDEN));
        assert_eq!(error.request_id(), Some("req_voice"));
    }
}
