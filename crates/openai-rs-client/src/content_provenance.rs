//! Multipart Content Provenance checks and their typed result union.
//!
//! The response DTOs live here temporarily because the types crate does not
//! yet expose the pinned `ProvenanceResource` schema family.

use std::fmt;

use openai_rs_types::{ExtraFields, Nullable, ReplayableMultipartSource, UnknownTaggedObject};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{
    ApiResponse, Client, Error,
    multipart::{PreparedReplayableSource, ReplayableMultipartForm},
    transport::PathSegment,
};

const JSON_MIME: &str = "application/json";

openai_rs_types::open_string_enum! {
    /// Object discriminator returned by a Content Provenance check.
    pub enum ContentProvenanceObjectType {
        ContentProvenanceCheck = "content_provenance_check",
    }
}

openai_rs_types::open_string_enum! {
    /// Whether a supported OpenAI provenance signal was detected.
    pub enum ProvenanceDetectionOutcome {
        Detected = "detected",
        NotDetected = "not_detected",
    }
}

openai_rs_types::open_string_enum! {
    /// Validation state of a C2PA manifest.
    pub enum C2paValidationState {
        Trusted = "trusted",
        Valid = "valid",
        Invalid = "invalid",
        NotPresent = "not_present",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum C2paResultTag {
    #[serde(rename = "c2pa")]
    C2pa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SynthIdResultTag {
    #[serde(rename = "synthid")]
    SynthId,
}

/// Replayable multipart body for `POST /content_provenance_checks`.
#[derive(Clone, PartialEq, Eq)]
pub struct CreateContentProvenanceCheckRequest {
    file: ReplayableMultipartSource,
}

impl CreateContentProvenanceCheckRequest {
    /// Creates a request from immutable bytes or a filesystem path.
    #[must_use]
    pub const fn new(file: ReplayableMultipartSource) -> Self {
        Self { file }
    }

    /// Returns the image or audio source to be checked.
    #[must_use]
    pub const fn file(&self) -> &ReplayableMultipartSource {
        &self.file
    }
}

impl fmt::Debug for CreateContentProvenanceCheckRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source_kind = if self.file.as_bytes().is_some() {
            "bytes"
        } else {
            "path"
        };
        formatter
            .debug_struct("CreateContentProvenanceCheckRequest")
            .field("source_kind", &source_kind)
            .field("byte_length", &self.file.as_bytes().map(<[u8]>::len))
            .finish_non_exhaustive()
    }
}

/// A C2PA result returned for an uploaded image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct C2paProvenanceResult {
    #[serde(rename = "type")]
    kind: C2paResultTag,
    outcome: ProvenanceDetectionOutcome,
    validation_state: C2paValidationState,
    issuer: Nullable<String>,
    model: Nullable<String>,
    generated_at: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl C2paProvenanceResult {
    /// Returns whether the supported C2PA signal was detected.
    #[must_use]
    pub const fn outcome(&self) -> &ProvenanceDetectionOutcome {
        &self.outcome
    }

    /// Returns the C2PA manifest validation state.
    #[must_use]
    pub const fn validation_state(&self) -> &C2paValidationState {
        &self.validation_state
    }

    /// Returns the exact required-nullable manifest issuer.
    #[must_use]
    pub const fn issuer(&self) -> &Nullable<String> {
        &self.issuer
    }

    /// Returns the exact required-nullable recorded model.
    #[must_use]
    pub const fn model(&self) -> &Nullable<String> {
        &self.model
    }

    /// Returns the exact required-nullable recorded RFC 3339 timestamp.
    #[must_use]
    pub const fn generated_at(&self) -> &Nullable<String> {
        &self.generated_at
    }

    /// Returns response properties added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A SynthID result returned for an uploaded image or audio file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthIdProvenanceResult {
    #[serde(rename = "type")]
    kind: SynthIdResultTag,
    outcome: ProvenanceDetectionOutcome,
    model: Nullable<String>,
    generated_at: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SynthIdProvenanceResult {
    /// Returns whether the supported SynthID signal was detected.
    #[must_use]
    pub const fn outcome(&self) -> &ProvenanceDetectionOutcome {
        &self.outcome
    }

    /// Returns the exact required-nullable recorded model.
    #[must_use]
    pub const fn model(&self) -> &Nullable<String> {
        &self.model
    }

    /// Returns the exact required-nullable recorded RFC 3339 timestamp.
    #[must_use]
    pub const fn generated_at(&self) -> &Nullable<String> {
        &self.generated_at
    }

    /// Returns response properties added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One provenance result selected by its `type` discriminator.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ContentProvenanceResult {
    /// C2PA manifest result for an image.
    C2pa(C2paProvenanceResult),
    /// SynthID watermark result for an image or audio file.
    SynthId(SynthIdProvenanceResult),
    /// A future result type retained as its complete semantic JSON object.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ContentProvenanceResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::C2pa(value) => value.serialize(serializer),
            Self::SynthId(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ContentProvenanceResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match result_discriminator(&value).map_err(serde::de::Error::custom)? {
            "c2pa" => serde_json::from_value(value)
                .map(Self::C2pa)
                .map_err(serde::de::Error::custom),
            "synthid" => serde_json::from_value(value)
                .map(Self::SynthId)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

fn result_discriminator(value: &Value) -> Result<&str, &'static str> {
    value
        .as_object()
        .ok_or("content provenance result must be a JSON object")?
        .get("type")
        .ok_or("content provenance result is missing string field `type`")?
        .as_str()
        .ok_or("content provenance result field `type` must be a string")
}

/// Response from one Content Provenance check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContentProvenanceCheck {
    object: ContentProvenanceObjectType,
    created_at: i64,
    results: Vec<ContentProvenanceResult>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContentProvenanceCheck {
    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &ContentProvenanceObjectType {
        &self.object
    }

    /// Returns the check creation time in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns every provenance result that applies to the uploaded file.
    #[must_use]
    pub fn results(&self) -> &[ContentProvenanceResult] {
        &self.results
    }

    /// Returns response properties added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Operations for checking supported OpenAI content provenance signals.
#[derive(Clone, Debug)]
pub struct ContentProvenanceChecks {
    client: Client,
}

impl ContentProvenanceChecks {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Checks one replayable image or audio source for provenance signals.
    pub async fn create(
        &self,
        request: CreateContentProvenanceCheckRequest,
    ) -> Result<ApiResponse<ContentProvenanceCheck>, Error> {
        let source = PreparedReplayableSource::prepare(request.file()).await?;
        let form = ReplayableMultipartForm::new().part("file", source);
        let path = [PathSegment::literal("content_provenance_checks")];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, JSON_MIME)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http::{Method, StatusCode};
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use serde_json::json;
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        authorization: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(
        status: StatusCode,
        response_body: String,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind provenance contract server");
        let address = listener.local_addr().expect("provenance server address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept provenance request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let response_body = response_body.clone();
                let sender = Arc::clone(&sender);
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let authorization = request
                        .headers()
                        .get(http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let content_type = request
                        .headers()
                        .get(http::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("collect provenance request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("provenance sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            authorization,
                            content_type,
                            body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(status)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_provenance")
                            .body(Full::new(Bytes::from(response_body)))
                            .expect("provenance response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve provenance request");
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("provenance base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("provenance client");
        (client, receiver)
    }

    #[test]
    fn result_union_preserves_required_nulls_and_future_variants() {
        let fixture = json!({
            "object": "content_provenance_check",
            "created_at": 1,
            "results": [
                {
                    "type": "c2pa",
                    "outcome": "detected",
                    "validation_state": "trusted",
                    "issuer": "OpenAI",
                    "model": null,
                    "generated_at": "2026-08-30T00:00:00Z",
                    "future_c2pa": true
                },
                {
                    "type": "synthid",
                    "outcome": "not_detected",
                    "model": null,
                    "generated_at": null
                },
                {
                    "type": "future_watermark",
                    "outcome": "detected",
                    "payload": {"kept": true}
                }
            ],
            "future_check": 7
        });
        let decoded: ContentProvenanceCheck =
            serde_json::from_value(fixture.clone()).expect("decode provenance response");
        assert_eq!(decoded.results().len(), 3);
        let ContentProvenanceResult::C2pa(c2pa) = &decoded.results()[0] else {
            panic!("expected C2PA result");
        };
        assert_eq!(c2pa.outcome(), &ProvenanceDetectionOutcome::Detected);
        assert!(c2pa.model().is_null());
        assert_eq!(c2pa.extra_fields().get("future_c2pa"), Some(&json!(true)));
        assert!(matches!(
            decoded.results()[2],
            ContentProvenanceResult::Unknown(_)
        ));
        assert_eq!(
            serde_json::to_value(decoded).expect("round-trip provenance response"),
            fixture
        );

        assert!(
            serde_json::from_value::<ContentProvenanceResult>(json!({
                "type": "c2pa",
                "outcome": "detected"
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn create_uses_one_fixed_multipart_file_field_and_typed_response() {
        let response = json!({
            "object": "content_provenance_check",
            "created_at": 2,
            "results": [{
                "type": "synthid",
                "outcome": "detected",
                "model": "gpt-image-1",
                "generated_at": null
            }]
        })
        .to_string();
        let (client, captured) = serve_once(StatusCode::OK, response).await;
        let source =
            ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"raw-image-bytes"[..]))
                .try_with_file_name("asset.png")
                .expect("safe provenance filename")
                .try_with_media_type("image/png")
                .expect("safe provenance media type");

        let response = ContentProvenanceChecks::new(client)
            .create(CreateContentProvenanceCheckRequest::new(source))
            .await
            .expect("content provenance response");
        assert_eq!(response.request_id(), Some("req_provenance"));
        assert_eq!(response.created_at(), 2);
        assert!(matches!(
            response.results()[0],
            ContentProvenanceResult::SynthId(_)
        ));

        let captured = captured.await.expect("captured provenance request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/content_provenance_checks");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(
            captured
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let text = String::from_utf8_lossy(&captured.body);
        assert!(text.contains("name=\"file\"; filename=\"asset.png\""));
        assert!(text.contains("Content-Type: image/png"));
        assert!(
            captured
                .body
                .windows(b"raw-image-bytes".len())
                .any(|window| window == b"raw-image-bytes")
        );
        assert!(!text.contains("cmF3LWltYWdlLWJ5dGVz"));
    }

    #[tokio::test]
    async fn multipart_api_errors_keep_status_code_and_request_id() {
        let response = json!({
            "error": {
                "message": "rate limited",
                "type": "rate_limit_error",
                "param": null,
                "code": "rate_limit_exceeded"
            }
        })
        .to_string();
        let (client, _captured) = serve_once(StatusCode::TOO_MANY_REQUESTS, response).await;
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"bytes"[..]));
        let error = ContentProvenanceChecks::new(client)
            .create(CreateContentProvenanceCheckRequest::new(source))
            .await
            .expect_err("provenance API error");

        let Error::Api(error) = error else {
            panic!("expected typed API error");
        };
        assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.request_id(), Some("req_provenance"));
        assert_eq!(error.code(), Some("rate_limit_exceeded"));
    }

    #[test]
    fn request_debug_does_not_expose_file_bytes_or_paths() {
        let request = CreateContentProvenanceCheckRequest::new(
            ReplayableMultipartSource::from_path("/private/customer/audio.wav"),
        );
        let debug = format!("{request:?}");
        assert!(debug.contains("path"));
        assert!(!debug.contains("customer"));
        assert!(!debug.contains("audio.wav"));
    }
}
