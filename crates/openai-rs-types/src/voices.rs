//! Access-controlled Custom Voice wire types.
//!
//! Custom voices are limited to eligible customers. Enabling the default-off
//! `custom-voice` feature only exposes client bindings; it does not grant
//! service access or imply account eligibility.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ExtraFields, Nullable, Omittable, ReplayableMultipartSource};

/// Maximum consent recording or audio sample size (10 MiB).
pub const MAX_CUSTOM_VOICE_AUDIO_BYTES: u64 = 10 * 1024 * 1024;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Box<str>);

        impl $name {
            /// Creates an opaque id without imposing a prefix.
            #[must_use]
            pub fn new(value: impl Into<Box<str>>) -> Self {
                Self(value.into())
            }

            /// Borrows the wire value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

opaque_id!(VoiceConsentId);
opaque_id!(VoiceId);

/// Invalid multipart custom-voice input rejected at request construction.
///
/// # Two-phase audio size checks
///
/// The pinned 10 MiB limit ([`MAX_CUSTOM_VOICE_AUDIO_BYTES`]) is enforced in
/// two phases that deliberately report through different error channels:
///
/// - **Construction (this type).** [`CreateVoiceConsentRequest::new`] and
///   [`CreateVoiceRequest::new`] measure in-memory byte sources eagerly, so an
///   oversized buffer fails before any client call with
///   [`VoiceRequestError::AudioTooLarge`].
/// - **Send time (client error channel).** File- and stream-backed sources
///   have no known length at construction, so the client re-checks the
///   prepared length against the same constant just before upload. That
///   failure is discovered while preparing the transport request, not while
///   validating input values, so it surfaces through the client's own error
///   type rather than this one.
///
/// Both phases compare against the same [`MAX_CUSTOM_VOICE_AUDIO_BYTES`]
/// constant, so a source accepted here can only be rejected later when its
/// length was unknowable at construction time.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum VoiceRequestError {
    /// An audio source did not declare its MIME type.
    #[error("custom-voice audio requires an explicit supported MIME type")]
    MissingAudioMediaType,
    /// The declared MIME type is not in the pinned allowlist.
    #[error("unsupported custom-voice audio MIME type")]
    UnsupportedAudioMediaType,
    /// In-memory audio exceeded 10 MiB at construction time.
    ///
    /// This covers only sources whose bytes are already available
    /// ([`ReplayableMultipartSource::as_bytes`]); file- and stream-backed
    /// sources are re-checked at send time through the client error channel.
    /// See the enum documentation for the full two-phase split.
    #[error("custom-voice audio exceeds the 10 MiB limit")]
    AudioTooLarge,
    /// A required text field was empty.
    #[error("custom-voice name and language must not be empty")]
    EmptyTextField,
}

/// Returns whether a media type is accepted by the pinned voice schemas.
#[must_use]
pub fn is_supported_voice_audio_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "audio/mpeg"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/ogg"
            | "audio/aac"
            | "audio/flac"
            | "audio/webm"
            | "audio/mp4"
    )
}

/// Construction-phase validation shared by both multipart request builders.
///
/// This is the first of the two audio size checks: it only measures sources
/// whose bytes are already in memory. Sources whose length becomes known only
/// after client-side preparation are re-checked at send time through the
/// client error channel; see [`VoiceRequestError`] for the full split.
fn validate_source(source: &ReplayableMultipartSource) -> Result<(), VoiceRequestError> {
    let media_type = source
        .media_type()
        .ok_or(VoiceRequestError::MissingAudioMediaType)?;
    if !is_supported_voice_audio_media_type(media_type) {
        return Err(VoiceRequestError::UnsupportedAudioMediaType);
    }
    if let Some(bytes) = source.as_bytes() {
        let length = u64::try_from(bytes.len()).map_err(|_| VoiceRequestError::AudioTooLarge)?;
        if length > MAX_CUSTOM_VOICE_AUDIO_BYTES {
            return Err(VoiceRequestError::AudioTooLarge);
        }
    }
    Ok(())
}

/// Multipart body for `POST /audio/voice_consents`.
///
/// This type intentionally implements neither Serialize nor Deserialize.
#[derive(Clone, PartialEq, Eq)]
pub struct CreateVoiceConsentRequest {
    name: String,
    recording: ReplayableMultipartSource,
    language: String,
}

impl CreateVoiceConsentRequest {
    /// Creates a consent upload after validating text and audio metadata.
    pub fn new(
        name: impl Into<String>,
        language: impl Into<String>,
        recording: ReplayableMultipartSource,
    ) -> Result<Self, VoiceRequestError> {
        let name = name.into();
        let language = language.into();
        if name.trim().is_empty() || language.trim().is_empty() {
            return Err(VoiceRequestError::EmptyTextField);
        }
        validate_source(&recording)?;
        Ok(Self {
            name,
            recording,
            language,
        })
    }

    /// Returns the consent label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the BCP 47 language tag.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Returns the replayable recording descriptor.
    #[must_use]
    pub const fn recording(&self) -> &ReplayableMultipartSource {
        &self.recording
    }
}

impl fmt::Debug for CreateVoiceConsentRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateVoiceConsentRequest")
            .field("name", &"[REDACTED]")
            .field("language", &"[REDACTED]")
            .field("recording", &"[REDACTED BIOMETRIC AUDIO]")
            .finish()
    }
}

/// JSON body for renaming a voice consent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateVoiceConsentRequest {
    name: String,
}

impl UpdateVoiceConsentRequest {
    /// Creates a rename request.
    pub fn new(name: impl Into<String>) -> Result<Self, VoiceRequestError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(VoiceRequestError::EmptyTextField);
        }
        Ok(Self { name })
    }

    /// Returns the new label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for UpdateVoiceConsentRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateVoiceConsentRequest")
            .field("name", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum VoiceConsentObjectTag {
    #[serde(rename = "audio.voice_consent")]
    VoiceConsent,
}

/// Consent recording metadata. Raw biometric audio is never returned here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceConsent {
    #[serde(rename = "object")]
    object: VoiceConsentObjectTag,
    id: VoiceConsentId,
    name: String,
    language: String,
    created_at: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl VoiceConsent {
    /// Returns consent id.
    #[must_use]
    pub const fn id(&self) -> &VoiceConsentId {
        &self.id
    }

    /// Returns consent label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the language tag.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Returns creation time.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns future response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Consent deletion result. The pinned discriminator remains
/// `audio.voice_consent` rather than a separate `.deleted` value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedVoiceConsent {
    id: VoiceConsentId,
    #[serde(rename = "object")]
    object: VoiceConsentObjectTag,
    deleted: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl DeletedVoiceConsent {
    /// Returns whether deletion completed.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted
    }
}

/// Query parameters for listing voice consents.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListVoiceConsentsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<VoiceConsentId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
}

impl ListVoiceConsentsParams {
    /// Creates empty list parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an opaque cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<VoiceConsentId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Sets page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Returns starting cursor.
    #[must_use]
    pub const fn after_ref(&self) -> Option<&VoiceConsentId> {
        match &self.after {
            Omittable::Value(id) => Some(id),
            Omittable::Omitted => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum VoiceConsentListObjectTag {
    #[serde(rename = "list")]
    List,
}

/// Cursor page of voice consents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceConsentList {
    #[serde(rename = "object")]
    object: VoiceConsentListObjectTag,
    data: Vec<VoiceConsent>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    first_id: Omittable<Nullable<VoiceConsentId>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    last_id: Omittable<Nullable<VoiceConsentId>>,
    has_more: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl VoiceConsentList {
    /// Returns consents.
    #[must_use]
    pub fn data(&self) -> &[VoiceConsent] {
        &self.data
    }

    /// Returns whether another page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Returns the last cursor when present and non-null.
    #[must_use]
    pub const fn last_id(&self) -> Option<&VoiceConsentId> {
        match &self.last_id {
            Omittable::Value(Nullable::Value(id)) => Some(id),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Multipart body for creating a custom voice.
///
/// This type intentionally implements neither Serialize nor Deserialize.
#[derive(Clone, PartialEq, Eq)]
pub struct CreateVoiceRequest {
    name: String,
    audio_sample: ReplayableMultipartSource,
    consent: VoiceConsentId,
}

impl CreateVoiceRequest {
    /// Creates a custom voice upload after validating audio metadata.
    pub fn new(
        name: impl Into<String>,
        consent: impl Into<VoiceConsentId>,
        audio_sample: ReplayableMultipartSource,
    ) -> Result<Self, VoiceRequestError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(VoiceRequestError::EmptyTextField);
        }
        validate_source(&audio_sample)?;
        Ok(Self {
            name,
            audio_sample,
            consent: consent.into(),
        })
    }

    /// Returns voice name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns consent id.
    #[must_use]
    pub const fn consent(&self) -> &VoiceConsentId {
        &self.consent
    }

    /// Returns replayable sample descriptor.
    #[must_use]
    pub const fn audio_sample(&self) -> &ReplayableMultipartSource {
        &self.audio_sample
    }
}

impl fmt::Debug for CreateVoiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateVoiceRequest")
            .field("name", &"[REDACTED]")
            .field("consent", &"[REDACTED]")
            .field("audio_sample", &"[REDACTED BIOMETRIC AUDIO]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum VoiceObjectTag {
    #[serde(rename = "audio.voice")]
    Voice,
}

/// Custom voice metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    #[serde(rename = "object")]
    object: VoiceObjectTag,
    id: VoiceId,
    name: String,
    created_at: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Voice {
    /// Returns voice id.
    #[must_use]
    pub const fn id(&self) -> &VoiceId {
        &self.id
    }

    /// Returns voice name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns creation time.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns future response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// OpenAPI resource-name aliases.
pub type VoiceConsentResource = VoiceConsent;
/// OpenAPI resource-name alias.
pub type VoiceConsentDeletedResource = DeletedVoiceConsent;
/// OpenAPI resource-name alias.
pub type VoiceConsentListResource = VoiceConsentList;
/// OpenAPI resource-name alias.
pub type VoiceResource = Voice;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::*;

    fn source(bytes: &[u8]) -> ReplayableMultipartSource {
        ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(bytes))
            .try_with_file_name("voice.wav")
            .expect("safe filename")
            .try_with_media_type("audio/x-wav")
            .expect("safe MIME")
    }

    assert_impl_all!(VoiceConsent: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(VoiceConsentList: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Voice: Serialize, DeserializeOwned, Send, Sync);
    assert_not_impl_any!(CreateVoiceConsentRequest: Serialize, DeserializeOwned);
    assert_not_impl_any!(CreateVoiceRequest: Serialize, DeserializeOwned);

    #[test]
    fn multipart_requests_validate_and_redact_biometric_inputs() {
        let consent = CreateVoiceConsentRequest::new("private-person", "en-US", source(b"audio"))
            .expect("valid consent request");
        let debug = format!("{consent:?}");
        assert!(!debug.contains("private-person"));
        assert!(!debug.contains("audio"));

        let voice = CreateVoiceRequest::new(
            "private voice",
            "cons_secret",
            source(b"BIOMETRIC_PAYLOAD_987"),
        )
        .expect("valid voice request");
        let debug = format!("{voice:?}");
        assert!(!debug.contains("private voice"));
        assert!(!debug.contains("cons_secret"));
        assert!(!debug.contains("BIOMETRIC_PAYLOAD_987"));
    }

    #[test]
    fn invalid_mime_missing_mime_and_large_bytes_fail_before_transport() {
        let missing = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"x"[..]));
        assert_eq!(
            CreateVoiceConsentRequest::new("name", "en-US", missing),
            Err(VoiceRequestError::MissingAudioMediaType)
        );
        let unsupported = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"x"[..]))
            .try_with_media_type("application/octet-stream")
            .expect("syntactically valid MIME");
        assert_eq!(
            CreateVoiceConsentRequest::new("name", "en-US", unsupported),
            Err(VoiceRequestError::UnsupportedAudioMediaType)
        );
        let large = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(vec![
            0;
            MAX_CUSTOM_VOICE_AUDIO_BYTES
                as usize
                + 1
        ]))
        .try_with_media_type("audio/wav")
        .expect("valid MIME");
        assert_eq!(
            CreateVoiceRequest::new("voice", "cons_1", large),
            Err(VoiceRequestError::AudioTooLarge)
        );
    }

    #[test]
    fn consent_resources_list_cursors_and_extras_round_trip() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "object": "audio.voice_consent",
                "id": "cons_1",
                "name": "Owner",
                "language": "en-US",
                "created_at": 1,
                "future_consent": true
            }],
            "first_id": null,
            "last_id": null,
            "has_more": false,
            "future_page": 1
        });
        let list: VoiceConsentList =
            serde_json::from_value(fixture.clone()).expect("decode consent list");
        assert_eq!(list.data()[0].id().as_str(), "cons_1");
        assert!(list.last_id().is_none());
        assert_eq!(
            serde_json::to_value(list).expect("round-trip list"),
            fixture
        );
    }

    #[test]
    fn update_and_voice_resources_are_typed() {
        let update = UpdateVoiceConsentRequest::new("renamed").expect("valid rename");
        assert_eq!(
            serde_json::to_value(update).expect("encode update"),
            json!({"name": "renamed"})
        );
        let voice: Voice = serde_json::from_value(json!({
            "object": "audio.voice",
            "id": "voice_1",
            "name": "Voice",
            "created_at": 2,
            "future": "kept"
        }))
        .expect("decode voice");
        assert_eq!(voice.id().as_str(), "voice_1");
        assert_eq!(voice.extra_fields().get("future"), Some(&json!("kept")));
    }
}
