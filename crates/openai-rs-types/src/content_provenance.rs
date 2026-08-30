//! Typed multipart request and response models for Content Provenance checks.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{
    ExtraFields, Nullable, ReplayableMultipartSource, open_string_enum,
    responses::UnknownTaggedObject,
};

open_string_enum! {
    /// Object discriminator returned by a Content Provenance check.
    pub enum ContentProvenanceObjectType {
        ContentProvenanceCheck = "content_provenance_check",
    }
}

open_string_enum! {
    /// Whether a supported OpenAI provenance signal was detected.
    pub enum ProvenanceDetectionOutcome {
        Detected = "detected",
        NotDetected = "not_detected",
    }
}

open_string_enum! {
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
///
/// Multipart sources intentionally do not implement Serde. Immutable bytes
/// and snapshotted paths can be rebuilt safely for an explicitly permitted
/// retry without buffering an entire path-backed file.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;

    use super::*;

    fn assert_json_dto<T>()
    where
        T: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
    }

    #[test]
    fn response_types_are_owned_bidirectional_json_dtos() {
        assert_json_dto::<ContentProvenanceObjectType>();
        assert_json_dto::<ProvenanceDetectionOutcome>();
        assert_json_dto::<C2paValidationState>();
        assert_json_dto::<C2paProvenanceResult>();
        assert_json_dto::<SynthIdProvenanceResult>();
        assert_json_dto::<ContentProvenanceResult>();
        assert_json_dto::<ContentProvenanceCheck>();
    }

    #[test]
    fn results_preserve_required_nulls_extras_and_future_variants() {
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

    #[test]
    fn multipart_request_is_replayable_and_debug_redacted() {
        let data: Arc<[u8]> = Arc::from(&b"secret bytes"[..]);
        let bytes =
            CreateContentProvenanceCheckRequest::new(ReplayableMultipartSource::from_bytes(data));
        let bytes_debug = format!("{bytes:?}");
        assert!(bytes_debug.contains("bytes"));
        assert!(!bytes_debug.contains("secret"));

        let path = CreateContentProvenanceCheckRequest::new(ReplayableMultipartSource::from_path(
            "/private/customer/audio.wav",
        ));
        let path_debug = format!("{path:?}");
        assert!(path_debug.contains("path"));
        assert!(!path_debug.contains("customer"));
        assert!(!path_debug.contains("audio.wav"));
    }
}
