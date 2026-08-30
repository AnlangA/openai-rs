//! Lossless wire types for the general-availability Realtime API.
//!
//! The event unions in this module are pinned to the checked-in OpenAPI
//! discriminator manifest. Known event tags decode strictly into their typed
//! payloads; unknown future tags retain the complete semantic JSON object.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::media::TranscriptionLanguage;
use crate::responses::{McpTool, PromptReference};
use crate::{ExtraFields, JsonText, Nullable, Omittable, WireSecret};

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

macro_rules! open_string_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $($variant,)+
            /// A string added by the service after this crate was released.
            Unknown(Box<str>),
        }

        impl $name {
            /// Returns the exact value used on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Unknown(value) => value,
                }
            }

            /// Preserves an arbitrary wire value while recognizing known values.
            #[must_use]
            pub fn from_raw(value: impl Into<Box<str>>) -> Self {
                let value = value.into();
                match value.as_ref() {
                    $($wire => Self::$variant,)+
                    _ => Self::Unknown(value),
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer).map(Self::from_raw)
            }
        }
    };
}

fn object_discriminator(value: &Value) -> Result<&str, &'static str> {
    value
        .as_object()
        .ok_or("tagged realtime value must be an object")?
        .get("type")
        .ok_or("tagged realtime object is missing string field `type`")?
        .as_str()
        .ok_or("tagged realtime object field `type` must be a string")
}

/// A future tagged Realtime object with every field retained.
#[derive(Clone, PartialEq)]
pub struct UnknownRealtimeObject {
    discriminator: Box<str>,
    raw: Map<String, Value>,
}

impl UnknownRealtimeObject {
    /// Validates and retains an unknown tagged object.
    pub fn from_value(value: Value) -> Result<Self, UnknownRealtimeObjectError> {
        let discriminator = object_discriminator(&value)
            .map_err(UnknownRealtimeObjectError::Invalid)?
            .into();
        let Value::Object(raw) = value else {
            return Err(UnknownRealtimeObjectError::Invalid(
                "tagged realtime value must be an object",
            ));
        };
        Ok(Self { discriminator, raw })
    }

    /// Returns the exact future discriminator.
    #[must_use]
    pub fn discriminator(&self) -> &str {
        &self.discriminator
    }

    /// Borrows the complete retained object, including `type`.
    #[must_use]
    pub const fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }

    /// Converts this value back into semantic JSON.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

impl fmt::Debug for UnknownRealtimeObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnknownRealtimeObject")
            .field("discriminator", &self.discriminator)
            .field("field_count", &self.raw.len())
            .finish()
    }
}

impl Serialize for UnknownRealtimeObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UnknownRealtimeObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A supplied value was not a valid tagged Realtime object.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnknownRealtimeObjectError {
    /// The value was not an object with a string `type` field.
    #[error("{0}")]
    Invalid(&'static str),
}

/// Base64-encoded audio bytes carried by Realtime JSON events.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RealtimeAudio(Box<[u8]>);

impl RealtimeAudio {
    /// Stores raw audio bytes for automatic base64 wire encoding.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into().into_boxed_slice())
    }

    /// Borrows the decoded audio bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes this value and returns decoded bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0.into_vec()
    }
}

impl fmt::Debug for RealtimeAudio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeAudio")
            .field("byte_len", &self.0.len())
            .finish()
    }
}

impl Serialize for RealtimeAudio {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for RealtimeAudio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map(Self::new)
            .map_err(D::Error::custom)
    }
}

open_string_enum! {
    /// A modality the Realtime model may emit.
    pub enum RealtimeOutputModality {
        Text => "text",
        Audio => "audio"
    }
}

open_string_enum! {
    /// String form of Realtime tracing configuration.
    pub enum RealtimeTracingMode {
        Auto => "auto"
    }
}

open_string_enum! {
    /// Anchor used to calculate client-secret expiration.
    pub enum RealtimeClientSecretExpirationAnchor {
        CreatedAt => "created_at"
    }
}

open_string_enum! {
    /// Lifecycle status of a Realtime response.
    pub enum RealtimeResponseStatus {
        InProgress => "in_progress",
        Completed => "completed",
        Cancelled => "cancelled",
        Failed => "failed",
        Incomplete => "incomplete"
    }
}

open_string_enum! {
    /// Lifecycle status of a Realtime conversation item.
    pub enum RealtimeItemStatus {
        InProgress => "in_progress",
        Completed => "completed",
        Incomplete => "incomplete"
    }
}

open_string_enum! {
    /// Reasoning effort for reasoning-capable Realtime models.
    pub enum RealtimeReasoningEffort {
        Minimal => "minimal",
        Low => "low",
        Medium => "medium",
        High => "high",
        XHigh => "xhigh"
    }
}

open_string_enum! {
    /// Built-in Realtime voice name.
    pub enum RealtimeVoiceName {
        Alloy => "alloy",
        Ash => "ash",
        Ballad => "ballad",
        Coral => "coral",
        Echo => "echo",
        Sage => "sage",
        Shimmer => "shimmer",
        Verse => "verse",
        Marin => "marin",
        Cedar => "cedar"
    }
}

open_string_enum! {
    /// Input noise-reduction mode.
    pub enum RealtimeNoiseReductionType {
        NearField => "near_field",
        FarField => "far_field"
    }
}

open_string_enum! {
    /// Semantic VAD eagerness.
    pub enum RealtimeVadEagerness {
        Low => "low",
        Medium => "medium",
        High => "high",
        Auto => "auto"
    }
}

open_string_enum! {
    /// Realtime tool-choice string mode.
    pub enum RealtimeToolChoiceMode {
        None => "none",
        Auto => "auto",
        Required => "required"
    }
}

open_string_enum! {
    /// Name of a Realtime rate-limit bucket.
    pub enum RealtimeRateLimitName {
        Requests => "requests",
        Tokens => "tokens"
    }
}

open_string_enum! {
    /// Reason a Realtime response stopped.
    pub enum RealtimeResponseStopReason {
        TurnDetected => "turn_detected",
        ClientCancelled => "client_cancelled",
        MaxOutputTokens => "max_output_tokens",
        ContentFilter => "content_filter"
    }
}

open_string_enum! {
    /// Status represented by a Realtime response status-details object.
    pub enum RealtimeResponseStatusDetailsType {
        Completed => "completed",
        Cancelled => "cancelled",
        Failed => "failed",
        Incomplete => "incomplete"
    }
}

open_string_enum! {
    /// Input image fidelity in a Realtime message.
    pub enum RealtimeImageDetail {
        Auto => "auto",
        Low => "low",
        High => "high"
    }
}

literal_tag!(RealtimePcmTag, Pcm, "audio/pcm");
literal_tag!(RealtimePcmuTag, Pcmu, "audio/pcmu");
literal_tag!(RealtimePcmaTag, Pcma, "audio/pcma");

/// PCM audio format. GA Realtime currently uses 24kHz PCM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimePcmAudioFormat {
    #[serde(rename = "type")]
    kind: RealtimePcmTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub rate: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimePcmAudioFormat {
    /// Creates the current 24kHz PCM format.
    #[must_use]
    pub fn pcm24k() -> Self {
        Self {
            kind: RealtimePcmTag::Pcm,
            rate: Omittable::Value(24_000),
            extra: ExtraFields::new(),
        }
    }
}

/// G.711 μ-law audio format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimePcmuAudioFormat {
    #[serde(rename = "type")]
    kind: RealtimePcmuTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimePcmuAudioFormat {
    fn default() -> Self {
        Self {
            kind: RealtimePcmuTag::Pcmu,
            extra: ExtraFields::new(),
        }
    }
}

/// G.711 A-law audio format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimePcmaAudioFormat {
    #[serde(rename = "type")]
    kind: RealtimePcmaTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimePcmaAudioFormat {
    fn default() -> Self {
        Self {
            kind: RealtimePcmaTag::Pcma,
            extra: ExtraFields::new(),
        }
    }
}

/// Audio format accepted by GA Realtime sessions.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeAudioFormat {
    Pcm(RealtimePcmAudioFormat),
    Pcmu(RealtimePcmuAudioFormat),
    Pcma(RealtimePcmaAudioFormat),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeAudioFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Pcm(value) => value.serialize(serializer),
            Self::Pcmu(value) => value.serialize(serializer),
            Self::Pcma(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeAudioFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "audio/pcm" => serde_json::from_value(value)
                .map(Self::Pcm)
                .map_err(D::Error::custom),
            "audio/pcmu" => serde_json::from_value(value)
                .map(Self::Pcmu)
                .map_err(D::Error::custom),
            "audio/pcma" => serde_json::from_value(value)
                .map(Self::Pcma)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<RealtimePcmAudioFormat> for RealtimeAudioFormat {
    fn from(value: RealtimePcmAudioFormat) -> Self {
        Self::Pcm(value)
    }
}

impl From<RealtimePcmuAudioFormat> for RealtimeAudioFormat {
    fn from(value: RealtimePcmuAudioFormat) -> Self {
        Self::Pcmu(value)
    }
}

impl From<RealtimePcmaAudioFormat> for RealtimeAudioFormat {
    fn from(value: RealtimePcmaAudioFormat) -> Self {
        Self::Pcma(value)
    }
}

/// Reference to a custom voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCustomVoice {
    pub id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeCustomVoice {
    /// Creates a custom voice reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// A built-in voice name or custom voice reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RealtimeVoice {
    BuiltIn(RealtimeVoiceName),
    Custom(RealtimeCustomVoice),
}

impl From<RealtimeVoiceName> for RealtimeVoice {
    fn from(value: RealtimeVoiceName) -> Self {
        Self::BuiltIn(value)
    }
}

impl From<RealtimeCustomVoice> for RealtimeVoice {
    fn from(value: RealtimeCustomVoice) -> Self {
        Self::Custom(value)
    }
}

/// Input-audio transcription configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeAudioTranscription {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub language: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub languages: Omittable<Vec<TranscriptionLanguage>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub keywords: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub delay: Omittable<RealtimeReasoningEffort>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Noise-reduction configuration for input audio.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeNoiseReduction {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeNoiseReductionType>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeServerVadTag, ServerVad, "server_vad");
literal_tag!(RealtimeSemanticVadTag, SemanticVad, "semantic_vad");

/// Server voice-activity detection configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeServerVad {
    #[serde(rename = "type")]
    kind: RealtimeServerVadTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub threshold: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prefix_padding_ms: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub silence_duration_ms: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub create_response: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub interrupt_response: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub idle_timeout_ms: Omittable<Nullable<i64>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimeServerVad {
    fn default() -> Self {
        Self {
            kind: RealtimeServerVadTag::ServerVad,
            threshold: Omittable::Omitted,
            prefix_padding_ms: Omittable::Omitted,
            silence_duration_ms: Omittable::Omitted,
            create_response: Omittable::Omitted,
            interrupt_response: Omittable::Omitted,
            idle_timeout_ms: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Semantic voice-activity detection configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSemanticVad {
    #[serde(rename = "type")]
    kind: RealtimeSemanticVadTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub eagerness: Omittable<RealtimeVadEagerness>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub create_response: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub interrupt_response: Omittable<bool>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimeSemanticVad {
    fn default() -> Self {
        Self {
            kind: RealtimeSemanticVadTag::SemanticVad,
            eagerness: Omittable::Omitted,
            create_response: Omittable::Omitted,
            interrupt_response: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Non-null turn-detection configuration. Use `Nullable::Null` to disable it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeTurnDetection {
    ServerVad(RealtimeServerVad),
    SemanticVad(RealtimeSemanticVad),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeTurnDetection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::ServerVad(value) => value.serialize(serializer),
            Self::SemanticVad(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeTurnDetection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "server_vad" => serde_json::from_value(value)
                .map(Self::ServerVad)
                .map_err(D::Error::custom),
            "semantic_vad" => serde_json::from_value(value)
                .map(Self::SemanticVad)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<RealtimeServerVad> for RealtimeTurnDetection {
    fn from(value: RealtimeServerVad) -> Self {
        Self::ServerVad(value)
    }
}

impl From<RealtimeSemanticVad> for RealtimeTurnDetection {
    fn from(value: RealtimeSemanticVad) -> Self {
        Self::SemanticVad(value)
    }
}

/// Realtime input-audio configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeAudioInputConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<RealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub transcription: Omittable<Nullable<RealtimeAudioTranscription>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub noise_reduction: Omittable<Nullable<RealtimeNoiseReduction>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub turn_detection: Omittable<Nullable<RealtimeTurnDetection>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Realtime output-audio configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeAudioOutputConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<RealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub voice: Omittable<RealtimeVoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub speed: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Input and output audio configuration for a Realtime session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSessionAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<RealtimeAudioInputConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeAudioOutputConfig>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective output-audio settings returned for a Realtime session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSessionAudioOutputState {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<RealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub voice: Omittable<RealtimeVoiceName>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub speed: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective input and output audio settings returned for a session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSessionAudioState {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<RealtimeAudioInputConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeSessionAudioOutputState>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Input-only audio configuration for a transcription session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<RealtimeAudioInputConfig>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Output-audio override accepted by `response.create`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseCreateAudioOutput {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<RealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub voice: Omittable<RealtimeVoice>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Audio configuration accepted by `response.create`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseCreateAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeResponseCreateAudioOutput>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective output-audio configuration on a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseAudioOutput {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<RealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub voice: Omittable<RealtimeVoiceName>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective audio configuration on a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeResponseAudioOutput>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Reasoning configuration for a Realtime session or response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeReasoning {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub effort: Omittable<RealtimeReasoningEffort>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Granular tracing configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTracingConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub workflow_name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub group_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<BTreeMap<String, Value>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Automatic or granular trace configuration. Use `Nullable::Null` to disable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RealtimeTracing {
    Mode(RealtimeTracingMode),
    Config(RealtimeTracingConfig),
}

/// Integer output-token limit or the wire string `"inf"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeMaxOutputTokens {
    Limited(i64),
    Unlimited,
}

impl Serialize for RealtimeMaxOutputTokens {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Limited(value) => serializer.serialize_i64(*value),
            Self::Unlimited => serializer.serialize_str("inf"),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeMaxOutputTokens {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::Number(value) => value
                .as_i64()
                .map(Self::Limited)
                .ok_or_else(|| D::Error::custom("Realtime max_output_tokens must be an integer")),
            Value::String(value) if value == "inf" => Ok(Self::Unlimited),
            _ => Err(D::Error::custom(
                "Realtime max_output_tokens must be an integer or `inf`",
            )),
        }
    }
}

open_string_enum! {
    /// String form of Realtime truncation policy.
    pub enum RealtimeTruncationMode {
        Auto => "auto",
        Disabled => "disabled"
    }
}

literal_tag!(RealtimeRetentionRatioTag, RetentionRatio, "retention_ratio");

/// Optional custom token limits for retention-ratio truncation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTruncationTokenLimits {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub post_instructions: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Retention-ratio truncation configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeRetentionRatioTruncation {
    #[serde(rename = "type")]
    kind: RealtimeRetentionRatioTag,
    pub retention_ratio: f64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub token_limits: Omittable<RealtimeTruncationTokenLimits>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeRetentionRatioTruncation {
    /// Creates retention-ratio truncation.
    #[must_use]
    pub fn new(retention_ratio: f64) -> Self {
        Self {
            kind: RealtimeRetentionRatioTag::RetentionRatio,
            retention_ratio,
            token_limits: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Realtime truncation policy.
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeTruncation {
    Mode(RealtimeTruncationMode),
    RetentionRatio(RealtimeRetentionRatioTruncation),
}

impl Serialize for RealtimeTruncation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Mode(value) => value.serialize(serializer),
            Self::RetentionRatio(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeTruncation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(value) => Ok(Self::Mode(RealtimeTruncationMode::from_raw(value))),
            Value::Object(_) => {
                if object_discriminator(&value).map_err(D::Error::custom)? != "retention_ratio" {
                    return Err(D::Error::custom("unknown Realtime truncation object type"));
                }
                serde_json::from_value(value)
                    .map(Self::RetentionRatio)
                    .map_err(D::Error::custom)
            }
            _ => Err(D::Error::custom(
                "Realtime truncation must be a string or object",
            )),
        }
    }
}

literal_tag!(RealtimeFunctionToolTag, Function, "function");

/// Function tool available during a Realtime session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeFunctionTool {
    #[serde(rename = "type")]
    kind: RealtimeFunctionToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parameters: Omittable<Value>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeFunctionTool {
    /// Creates a named function tool.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: RealtimeFunctionToolTag::Function,
            name: Omittable::Value(name.into()),
            description: Omittable::Omitted,
            parameters: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Tool available to a Realtime model.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeTool {
    Function(RealtimeFunctionTool),
    Mcp(McpTool),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeTool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Function(value) => value.serialize(serializer),
            Self::Mcp(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeTool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "function" => serde_json::from_value(value)
                .map(Self::Function)
                .map_err(D::Error::custom),
            "mcp" => serde_json::from_value(value)
                .map(Self::Mcp)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<RealtimeFunctionTool> for RealtimeTool {
    fn from(value: RealtimeFunctionTool) -> Self {
        Self::Function(value)
    }
}

impl From<McpTool> for RealtimeTool {
    fn from(value: McpTool) -> Self {
        Self::Mcp(value)
    }
}

literal_tag!(RealtimeFunctionChoiceTag, Function, "function");
literal_tag!(RealtimeMcpChoiceTag, Mcp, "mcp");

/// Force one named function tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeFunctionToolChoice {
    #[serde(rename = "type")]
    kind: RealtimeFunctionChoiceTag,
    pub name: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeFunctionToolChoice {
    /// Forces one named function.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: RealtimeFunctionChoiceTag::Function,
            name: name.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// Force one MCP server or tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpToolChoice {
    #[serde(rename = "type")]
    kind: RealtimeMcpChoiceTag,
    pub server_label: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeMcpToolChoice {
    /// Forces the model to use a specific MCP server.
    #[must_use]
    pub fn new(server_label: impl Into<String>) -> Self {
        Self {
            kind: RealtimeMcpChoiceTag::Mcp,
            server_label: server_label.into(),
            name: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Narrows the choice to one tool on that server.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(Nullable::Value(name.into()));
        self
    }
}

/// Realtime tool-choice policy.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeToolChoice {
    Mode(RealtimeToolChoiceMode),
    Function(RealtimeFunctionToolChoice),
    Mcp(RealtimeMcpToolChoice),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Mode(value) => value.serialize(serializer),
            Self::Function(value) => value.serialize(serializer),
            Self::Mcp(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if let Value::String(value) = value {
            return Ok(Self::Mode(RealtimeToolChoiceMode::from_raw(value)));
        }
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "function" => serde_json::from_value(value)
                .map(Self::Function)
                .map_err(D::Error::custom),
            "mcp" => serde_json::from_value(value)
                .map(Self::Mcp)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<RealtimeToolChoiceMode> for RealtimeToolChoice {
    fn from(value: RealtimeToolChoiceMode) -> Self {
        Self::Mode(value)
    }
}

impl From<RealtimeFunctionToolChoice> for RealtimeToolChoice {
    fn from(value: RealtimeFunctionToolChoice) -> Self {
        Self::Function(value)
    }
}

impl From<RealtimeMcpToolChoice> for RealtimeToolChoice {
    fn from(value: RealtimeMcpToolChoice) -> Self {
        Self::Mcp(value)
    }
}

literal_tag!(RealtimeSessionRequestTag, Realtime, "realtime");
literal_tag!(
    RealtimeTranscriptionRequestTag,
    Transcription,
    "transcription"
);

/// GA Realtime session configuration used for creation and `session.update`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSessionCreateRequest {
    #[serde(rename = "type")]
    kind: RealtimeSessionRequestTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub instructions: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeSessionAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tracing: Omittable<Nullable<RealtimeTracing>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tools: Omittable<Vec<RealtimeTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_choice: Omittable<RealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parallel_tool_calls: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning: Omittable<RealtimeReasoning>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_output_tokens: Omittable<RealtimeMaxOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub truncation: Omittable<RealtimeTruncation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<PromptReference>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimeSessionCreateRequest {
    fn default() -> Self {
        Self {
            kind: RealtimeSessionRequestTag::Realtime,
            output_modalities: Omittable::Omitted,
            model: Omittable::Omitted,
            instructions: Omittable::Omitted,
            audio: Omittable::Omitted,
            include: Omittable::Omitted,
            tracing: Omittable::Omitted,
            tools: Omittable::Omitted,
            tool_choice: Omittable::Omitted,
            parallel_tool_calls: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            truncation: Omittable::Omitted,
            prompt: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// GA transcription-session configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionSessionCreateRequest {
    #[serde(rename = "type")]
    kind: RealtimeTranscriptionRequestTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeTranscriptionAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Default for RealtimeTranscriptionSessionCreateRequest {
    fn default() -> Self {
        Self {
            kind: RealtimeTranscriptionRequestTag::Transcription,
            audio: Omittable::Omitted,
            include: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Session configuration carried by `session.update` and client-secret calls.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeSessionConfig {
    Realtime(Box<RealtimeSessionCreateRequest>),
    Transcription(Box<RealtimeTranscriptionSessionCreateRequest>),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeSessionConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Realtime(value) => value.serialize(serializer),
            Self::Transcription(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeSessionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "realtime" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Realtime)
                .map_err(D::Error::custom),
            "transcription" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Transcription)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<RealtimeSessionCreateRequest> for RealtimeSessionConfig {
    fn from(value: RealtimeSessionCreateRequest) -> Self {
        Self::Realtime(Box::new(value))
    }
}

impl From<RealtimeTranscriptionSessionCreateRequest> for RealtimeSessionConfig {
    fn from(value: RealtimeTranscriptionSessionCreateRequest) -> Self {
        Self::Transcription(Box::new(value))
    }
}

literal_tag!(RealtimeSessionObjectTag, Session, "realtime.session");

/// Effective GA Realtime session returned by the server.
///
/// The pinned OpenAPI response schema requires `id` and `object`. The pinned
/// generated Node/Python source aliases the request shape for session events;
/// this type intentionally follows the authoritative GA response schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSession {
    #[serde(rename = "type")]
    kind: RealtimeSessionRequestTag,
    pub id: String,
    #[serde(rename = "object")]
    object: RealtimeSessionObjectTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_at: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub instructions: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeSessionAudioState>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tracing: Omittable<Nullable<RealtimeTracing>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tools: Omittable<Vec<RealtimeTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_choice: Omittable<RealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning: Omittable<RealtimeReasoning>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_output_tokens: Omittable<RealtimeMaxOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub truncation: Omittable<RealtimeTruncation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<PromptReference>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective GA transcription session returned by the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionSession {
    #[serde(rename = "type")]
    kind: RealtimeTranscriptionRequestTag,
    pub id: String,
    pub object: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_at: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeTranscriptionAudio>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Effective Realtime or transcription session.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeSessionState {
    Realtime(Box<RealtimeSession>),
    Transcription(Box<RealtimeTranscriptionSession>),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeSessionState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Realtime(value) => value.serialize(serializer),
            Self::Transcription(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeSessionState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "realtime" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Realtime)
                .map_err(D::Error::custom),
            "transcription" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Transcription)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

open_string_enum! {
    /// Content kind in a system message.
    pub enum RealtimeSystemContentType {
        InputText => "input_text"
    }
}

open_string_enum! {
    /// Content kind in a user message.
    pub enum RealtimeUserContentType {
        InputText => "input_text",
        InputAudio => "input_audio",
        InputImage => "input_image"
    }
}

open_string_enum! {
    /// Content kind in an assistant message.
    pub enum RealtimeAssistantContentType {
        OutputText => "output_text",
        OutputAudio => "output_audio"
    }
}

/// One system-message content part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSystemContentPart {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeSystemContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeSystemContentPart {
    /// Creates an input-text system content part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: Omittable::Value(RealtimeSystemContentType::InputText),
            text: Omittable::Value(text.into()),
            extra: ExtraFields::new(),
        }
    }
}

/// One user-message content part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeUserContentPart {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeUserContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub detail: Omittable<RealtimeImageDetail>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub transcript: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeUserContentPart {
    /// Creates an input-text content part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: Omittable::Value(RealtimeUserContentType::InputText),
            text: Omittable::Value(text.into()),
            ..Self::default()
        }
    }

    /// Creates an input-audio content part from raw bytes.
    #[must_use]
    pub fn audio(audio: impl Into<Vec<u8>>) -> Self {
        Self {
            kind: Omittable::Value(RealtimeUserContentType::InputAudio),
            audio: Omittable::Value(RealtimeAudio::new(audio)),
            ..Self::default()
        }
    }

    /// Creates an input-image content part from a data URI.
    #[must_use]
    pub fn image(image_url: impl Into<String>) -> Self {
        Self {
            kind: Omittable::Value(RealtimeUserContentType::InputImage),
            image_url: Omittable::Value(image_url.into()),
            ..Self::default()
        }
    }
}

/// One assistant-message content part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeAssistantContentPart {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeAssistantContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub transcript: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeMessageItemTag, Message, "message");
literal_tag!(RealtimeSystemRoleTag, System, "system");
literal_tag!(RealtimeUserRoleTag, User, "user");
literal_tag!(RealtimeAssistantRoleTag, Assistant, "assistant");
literal_tag!(RealtimeFunctionCallItemTag, FunctionCall, "function_call");
literal_tag!(
    RealtimeFunctionCallOutputItemTag,
    FunctionCallOutput,
    "function_call_output"
);
literal_tag!(
    RealtimeMcpApprovalResponseItemTag,
    McpApprovalResponse,
    "mcp_approval_response"
);
literal_tag!(RealtimeMcpListToolsItemTag, McpListTools, "mcp_list_tools");
literal_tag!(RealtimeMcpCallItemTag, McpCall, "mcp_call");
literal_tag!(
    RealtimeMcpApprovalRequestItemTag,
    McpApprovalRequest,
    "mcp_approval_request"
);

/// System message in a Realtime conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemMessageSystem {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeItemStatus>,
    #[serde(rename = "type")]
    kind: RealtimeMessageItemTag,
    role: RealtimeSystemRoleTag,
    pub content: Vec<RealtimeSystemContentPart>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeConversationItemMessageSystem {
    /// Creates a system message.
    #[must_use]
    pub fn new(content: Vec<RealtimeSystemContentPart>) -> Self {
        Self {
            id: Omittable::Omitted,
            object: Omittable::Omitted,
            status: Omittable::Omitted,
            kind: RealtimeMessageItemTag::Message,
            role: RealtimeSystemRoleTag::System,
            content,
            extra: ExtraFields::new(),
        }
    }
}

/// User message in a Realtime conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemMessageUser {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeItemStatus>,
    #[serde(rename = "type")]
    kind: RealtimeMessageItemTag,
    role: RealtimeUserRoleTag,
    pub content: Vec<RealtimeUserContentPart>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeConversationItemMessageUser {
    /// Creates a user message.
    #[must_use]
    pub fn new(content: Vec<RealtimeUserContentPart>) -> Self {
        Self {
            id: Omittable::Omitted,
            object: Omittable::Omitted,
            status: Omittable::Omitted,
            kind: RealtimeMessageItemTag::Message,
            role: RealtimeUserRoleTag::User,
            content,
            extra: ExtraFields::new(),
        }
    }
}

/// Assistant message in a Realtime conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemMessageAssistant {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeItemStatus>,
    #[serde(rename = "type")]
    kind: RealtimeMessageItemTag,
    role: RealtimeAssistantRoleTag,
    pub content: Vec<RealtimeAssistantContentPart>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Function call in a Realtime conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemFunctionCall {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(rename = "type")]
    kind: RealtimeFunctionCallItemTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeItemStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub call_id: Omittable<String>,
    pub name: String,
    pub arguments: JsonText,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeConversationItemFunctionCall {
    /// Creates a function call from its complete argument string.
    #[must_use]
    pub fn new(name: impl Into<String>, arguments: JsonText) -> Self {
        Self {
            id: Omittable::Omitted,
            object: Omittable::Omitted,
            kind: RealtimeFunctionCallItemTag::FunctionCall,
            status: Omittable::Omitted,
            call_id: Omittable::Omitted,
            name: name.into(),
            arguments,
            extra: ExtraFields::new(),
        }
    }

    /// Serializes typed function arguments into the protocol string field.
    pub fn from_serializable<T: Serialize>(
        name: impl Into<String>,
        arguments: &T,
    ) -> Result<Self, serde_json::Error> {
        serde_json::to_string(arguments)
            .map(JsonText::from_raw)
            .map(|arguments| Self::new(name, arguments))
    }
}

/// Output for a preceding Realtime function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemFunctionCallOutput {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(rename = "type")]
    kind: RealtimeFunctionCallOutputItemTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeItemStatus>,
    pub call_id: String,
    pub output: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeConversationItemFunctionCallOutput {
    /// Creates an opaque function-call output.
    #[must_use]
    pub fn new(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            id: Omittable::Omitted,
            object: Omittable::Omitted,
            kind: RealtimeFunctionCallOutputItemTag::FunctionCallOutput,
            status: Omittable::Omitted,
            call_id: call_id.into(),
            output: output.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Serializes a typed function result into the protocol string field.
    pub fn from_serializable<T: Serialize>(
        call_id: impl Into<String>,
        output: &T,
    ) -> Result<Self, serde_json::Error> {
        serde_json::to_string(output).map(|output| Self::new(call_id, output))
    }
}

/// Client approval or rejection for an MCP request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpApprovalResponse {
    #[serde(rename = "type")]
    kind: RealtimeMcpApprovalResponseItemTag,
    pub id: String,
    pub approval_request_id: String,
    pub approve: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reason: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeMcpApprovalResponse {
    /// Approves or rejects an MCP approval request.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        approval_request_id: impl Into<String>,
        approve: bool,
    ) -> Self {
        Self {
            kind: RealtimeMcpApprovalResponseItemTag::McpApprovalResponse,
            id: id.into(),
            approval_request_id: approval_request_id.into(),
            approve,
            reason: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// One tool in a Realtime `mcp_list_tools` item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpListedTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<Nullable<String>>,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub annotations: Omittable<Nullable<Value>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Result of listing tools on a Realtime MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpListTools {
    #[serde(rename = "type")]
    kind: RealtimeMcpListToolsItemTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    pub server_label: String,
    pub tools: Vec<RealtimeMcpListedTool>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeMcpProtocolErrorTag, ProtocolError, "protocol_error");
literal_tag!(
    RealtimeMcpToolExecutionErrorTag,
    ToolExecutionError,
    "tool_execution_error"
);
literal_tag!(RealtimeMcpHttpErrorTag, HttpError, "http_error");

/// MCP protocol-level failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpProtocolError {
    #[serde(rename = "type")]
    kind: RealtimeMcpProtocolErrorTag,
    pub code: i64,
    pub message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// MCP tool execution failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpToolExecutionError {
    #[serde(rename = "type")]
    kind: RealtimeMcpToolExecutionErrorTag,
    pub message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// HTTP failure returned by an MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpHttpError {
    #[serde(rename = "type")]
    kind: RealtimeMcpHttpErrorTag,
    pub code: i64,
    pub message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Error attached to a Realtime MCP tool call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeMcpError {
    Protocol(RealtimeMcpProtocolError),
    ToolExecution(RealtimeMcpToolExecutionError),
    Http(RealtimeMcpHttpError),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeMcpError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Protocol(value) => value.serialize(serializer),
            Self::ToolExecution(value) => value.serialize(serializer),
            Self::Http(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeMcpError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "protocol_error" => serde_json::from_value(value)
                .map(Self::Protocol)
                .map_err(D::Error::custom),
            "tool_execution_error" => serde_json::from_value(value)
                .map(Self::ToolExecution)
                .map_err(D::Error::custom),
            "http_error" => serde_json::from_value(value)
                .map(Self::Http)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// Realtime MCP tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpToolCall {
    #[serde(rename = "type")]
    kind: RealtimeMcpCallItemTag,
    pub id: String,
    pub server_label: String,
    pub name: String,
    pub arguments: JsonText,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub approval_request_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub error: Omittable<Nullable<RealtimeMcpError>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Realtime MCP approval request generated by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpApprovalRequest {
    #[serde(rename = "type")]
    kind: RealtimeMcpApprovalRequestItemTag,
    pub id: String,
    pub server_label: String,
    pub name: String,
    pub arguments: JsonText,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// A single item within a GA Realtime conversation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RealtimeConversationItem {
    SystemMessage(RealtimeConversationItemMessageSystem),
    UserMessage(RealtimeConversationItemMessageUser),
    AssistantMessage(RealtimeConversationItemMessageAssistant),
    FunctionCall(RealtimeConversationItemFunctionCall),
    FunctionCallOutput(RealtimeConversationItemFunctionCallOutput),
    McpApprovalResponse(RealtimeMcpApprovalResponse),
    McpListTools(RealtimeMcpListTools),
    McpCall(RealtimeMcpToolCall),
    McpApprovalRequest(RealtimeMcpApprovalRequest),
    Unknown(UnknownRealtimeObject),
}

impl Serialize for RealtimeConversationItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::SystemMessage(value) => value.serialize(serializer),
            Self::UserMessage(value) => value.serialize(serializer),
            Self::AssistantMessage(value) => value.serialize(serializer),
            Self::FunctionCall(value) => value.serialize(serializer),
            Self::FunctionCallOutput(value) => value.serialize(serializer),
            Self::McpApprovalResponse(value) => value.serialize(serializer),
            Self::McpListTools(value) => value.serialize(serializer),
            Self::McpCall(value) => value.serialize(serializer),
            Self::McpApprovalRequest(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeConversationItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "message" => {
                let role = value
                    .as_object()
                    .and_then(|object| object.get("role"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| D::Error::custom("Realtime message is missing string `role`"))?;
                match role {
                    "system" => serde_json::from_value(value)
                        .map(Self::SystemMessage)
                        .map_err(D::Error::custom),
                    "user" => serde_json::from_value(value)
                        .map(Self::UserMessage)
                        .map_err(D::Error::custom),
                    "assistant" => serde_json::from_value(value)
                        .map(Self::AssistantMessage)
                        .map_err(D::Error::custom),
                    _ => Err(D::Error::custom(
                        "unknown role for known Realtime message tag",
                    )),
                }
            }
            "function_call" => serde_json::from_value(value)
                .map(Self::FunctionCall)
                .map_err(D::Error::custom),
            "function_call_output" => serde_json::from_value(value)
                .map(Self::FunctionCallOutput)
                .map_err(D::Error::custom),
            "mcp_approval_response" => serde_json::from_value(value)
                .map(Self::McpApprovalResponse)
                .map_err(D::Error::custom),
            "mcp_list_tools" => serde_json::from_value(value)
                .map(Self::McpListTools)
                .map_err(D::Error::custom),
            "mcp_call" => serde_json::from_value(value)
                .map(Self::McpCall)
                .map_err(D::Error::custom),
            "mcp_approval_request" => serde_json::from_value(value)
                .map(Self::McpApprovalRequest)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

macro_rules! impl_conversation_item_from {
    ($($ty:ty => $variant:ident),+ $(,)?) => {
        $(
            impl From<$ty> for RealtimeConversationItem {
                fn from(value: $ty) -> Self {
                    Self::$variant(value)
                }
            }
        )+
    };
}

impl_conversation_item_from!(
    RealtimeConversationItemMessageSystem => SystemMessage,
    RealtimeConversationItemMessageUser => UserMessage,
    RealtimeConversationItemMessageAssistant => AssistantMessage,
    RealtimeConversationItemFunctionCall => FunctionCall,
    RealtimeConversationItemFunctionCallOutput => FunctionCallOutput,
    RealtimeMcpApprovalResponse => McpApprovalResponse,
    RealtimeMcpListTools => McpListTools,
    RealtimeMcpToolCall => McpCall,
    RealtimeMcpApprovalRequest => McpApprovalRequest,
);

open_string_enum! {
    /// Conversation routing for one out-of-band or default response.
    pub enum RealtimeResponseConversation {
        Auto => "auto",
        None => "none"
    }
}

/// Parameters supplied by `response.create`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseCreateParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub instructions: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeResponseCreateAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tools: Omittable<Vec<RealtimeTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_choice: Omittable<RealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parallel_tool_calls: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning: Omittable<RealtimeReasoning>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_output_tokens: Omittable<RealtimeMaxOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub conversation: Omittable<RealtimeResponseConversation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<PromptReference>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<Vec<RealtimeConversationItem>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Error details attached to a failed Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseFailure {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub code: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Additional status details for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseStatusDetails {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeResponseStatusDetailsType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reason: Omittable<RealtimeResponseStopReason>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub error: Omittable<RealtimeResponseFailure>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Cached token breakdown for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCachedTokenDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Input-token breakdown for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeInputTokenDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub cached_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub cached_tokens_details: Omittable<RealtimeCachedTokenDetails>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Output-token breakdown for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeOutputTokenDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Token usage for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseUsage {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub total_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_token_details: Omittable<RealtimeInputTokenDetails>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_token_details: Omittable<RealtimeOutputTokenDetails>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeResponseObjectTag, Response, "realtime.response");

/// Realtime response resource carried by lifecycle events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponse {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(
        rename = "object",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    object: Omittable<RealtimeResponseObjectTag>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<RealtimeResponseStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status_details: Omittable<RealtimeResponseStatusDetails>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<Vec<RealtimeConversationItem>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeResponseAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<RealtimeResponseUsage>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub conversation_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_output_tokens: Omittable<Nullable<RealtimeMaxOutputTokens>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Log probability attached to input-audio transcription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionLogprob {
    pub token: String,
    pub logprob: f64,
    pub bytes: Vec<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeTranscriptTokenUsageTag, Tokens, "tokens");
literal_tag!(RealtimeTranscriptDurationUsageTag, Duration, "duration");

/// Input token details for transcription billing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptInputTokenDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Token-billed transcription usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptTokenUsage {
    #[serde(rename = "type")]
    kind: RealtimeTranscriptTokenUsageTag,
    pub input_tokens: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_token_details: Omittable<RealtimeTranscriptInputTokenDetails>,
    pub output_tokens: i64,
    pub total_tokens: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Duration-billed transcription usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptDurationUsage {
    #[serde(rename = "type")]
    kind: RealtimeTranscriptDurationUsageTag,
    pub seconds: f64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Usage reported by a Realtime transcription event.
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeTranscriptionUsage {
    Tokens(RealtimeTranscriptTokenUsage),
    Duration(RealtimeTranscriptDurationUsage),
}

impl Serialize for RealtimeTranscriptionUsage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Tokens(value) => value.serialize(serializer),
            Self::Duration(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RealtimeTranscriptionUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value).map_err(D::Error::custom)? {
            "tokens" => serde_json::from_value(value)
                .map(Self::Tokens)
                .map_err(D::Error::custom),
            "duration" => serde_json::from_value(value)
                .map(Self::Duration)
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom("unknown transcription usage type")),
        }
    }
}

/// Optional properties on a transcription failure.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionError {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub code: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub message: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub param: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Error payload in a GA Realtime `error` server event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeErrorDetails {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub code: Omittable<Nullable<String>>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub param: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub event_id: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Realtime conversation resource.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversation {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// One current Realtime rate-limit snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeRateLimit {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<RealtimeRateLimitName>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub remaining: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reset_seconds: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

open_string_enum! {
    /// Type of a Realtime response content part.
    pub enum RealtimeResponseContentType {
        Audio => "audio",
        Text => "text"
    }
}

/// Content part carried by response content lifecycle events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseContentPart {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<RealtimeResponseContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub transcript: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Client-secret expiration configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeClientSecretExpiration {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub anchor: Omittable<RealtimeClientSecretExpirationAnchor>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub seconds: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Request body for `POST /realtime/client_secrets`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCreateClientSecretRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_after: Omittable<RealtimeClientSecretExpiration>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub session: Omittable<RealtimeSessionConfig>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Client secret and effective session returned by the service.
#[derive(Clone, Serialize, Deserialize)]
pub struct RealtimeCreateClientSecretResponse {
    pub value: WireSecret,
    pub expires_at: i64,
    pub session: RealtimeSessionState,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl fmt::Debug for RealtimeCreateClientSecretResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeCreateClientSecretResponse")
            .field("value", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("session", &self.session)
            .field("extra", &self.extra)
            .finish()
    }
}

/// Validated translation client-secret lifetime in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct RealtimeTranslationSecretLifetime(u16);

impl RealtimeTranslationSecretLifetime {
    pub const MIN_SECONDS: u16 = 10;
    pub const MAX_SECONDS: u16 = 7_200;

    pub fn new(seconds: u16) -> Result<Self, RealtimeTranslationSecretLifetimeError> {
        if (Self::MIN_SECONDS..=Self::MAX_SECONDS).contains(&seconds) {
            Ok(Self(seconds))
        } else {
            Err(RealtimeTranslationSecretLifetimeError { seconds })
        }
    }

    #[must_use]
    pub const fn seconds(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for RealtimeTranslationSecretLifetime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds = u16::deserialize(deserializer)?;
        Self::new(seconds).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("translation client-secret lifetime {seconds} is outside 10..=7200 seconds")]
pub struct RealtimeTranslationSecretLifetimeError {
    pub seconds: u16,
}

literal_tag!(RealtimeTranslationExpirationAnchorTag, CreatedAt, "created_at");

/// Expiration configuration for a Realtime translation client secret.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationClientSecretExpiration {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    anchor: Omittable<RealtimeTranslationExpirationAnchorTag>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    seconds: Omittable<RealtimeTranslationSecretLifetime>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationClientSecretExpiration {
    pub fn new(seconds: u16) -> Result<Self, RealtimeTranslationSecretLifetimeError> {
        Ok(Self {
            anchor: Omittable::Value(RealtimeTranslationExpirationAnchorTag::CreatedAt),
            seconds: Omittable::Value(RealtimeTranslationSecretLifetime::new(seconds)?),
            extra: ExtraFields::new(),
        })
    }

    #[must_use]
    pub fn seconds(&self) -> Option<u16> {
        match self.seconds {
            Omittable::Value(value) => Some(value.seconds()),
            Omittable::Omitted => None,
        }
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Required source-transcription model for translation sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationTranscription {
    pub model: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationTranscription {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            extra: ExtraFields::new(),
        }
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Required translation input noise-reduction mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationNoiseReduction {
    #[serde(rename = "type")]
    pub kind: RealtimeNoiseReductionType,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationNoiseReduction {
    #[must_use]
    pub fn new(kind: RealtimeNoiseReductionType) -> Self {
        Self {
            kind,
            extra: ExtraFields::new(),
        }
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Translation input-audio configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationAudioInput {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub transcription: Omittable<Nullable<RealtimeTranslationTranscription>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub noise_reduction: Omittable<Nullable<RealtimeTranslationNoiseReduction>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationAudioInput {
    #[must_use]
    pub fn with_transcription(mut self, value: RealtimeTranslationTranscription) -> Self {
        self.transcription = Omittable::Value(Nullable::Value(value));
        self
    }

    #[must_use]
    pub fn with_transcription_null(mut self) -> Self {
        self.transcription = Omittable::Value(Nullable::Null);
        self
    }

    #[must_use]
    pub fn clear_transcription(mut self) -> Self {
        self.transcription = Omittable::Omitted;
        self
    }

    #[must_use]
    pub fn with_noise_reduction(mut self, value: RealtimeTranslationNoiseReduction) -> Self {
        self.noise_reduction = Omittable::Value(Nullable::Value(value));
        self
    }

    #[must_use]
    pub fn with_noise_reduction_null(mut self) -> Self {
        self.noise_reduction = Omittable::Value(Nullable::Null);
        self
    }

    #[must_use]
    pub fn clear_noise_reduction(mut self) -> Self {
        self.noise_reduction = Omittable::Omitted;
        self
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Translation output-audio configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationAudioOutput {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub language: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationAudioOutput {
    #[must_use]
    pub fn new(language: impl Into<String>) -> Self {
        Self {
            language: Omittable::Value(language.into()),
            extra: ExtraFields::new(),
        }
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Translation input/output audio settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<RealtimeTranslationAudioInput>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeTranslationAudioOutput>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationAudio {
    #[must_use]
    pub fn with_input(mut self, input: RealtimeTranslationAudioInput) -> Self {
        self.input = Omittable::Value(input);
        self
    }

    #[must_use]
    pub fn with_output(mut self, output: RealtimeTranslationAudioOutput) -> Self {
        self.output = Omittable::Value(output);
        self
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Configuration used to create a Realtime translation session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationSessionCreateRequest {
    pub model: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<RealtimeTranslationAudio>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationSessionCreateRequest {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            audio: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    #[must_use]
    pub fn with_audio(mut self, audio: RealtimeTranslationAudio) -> Self {
        self.audio = Omittable::Value(audio);
        self
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(RealtimeTranslationSessionTag, Translation, "translation");

/// Effective Realtime translation session returned by the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationSession {
    pub id: String,
    #[serde(rename = "type")]
    kind: RealtimeTranslationSessionTag,
    pub expires_at: i64,
    pub model: String,
    pub audio: RealtimeTranslationAudio,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationSession {
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request for a translation session client secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranslationClientSecretCreateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_after: Omittable<RealtimeTranslationClientSecretExpiration>,
    pub session: RealtimeTranslationSessionCreateRequest,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationClientSecretCreateRequest {
    #[must_use]
    pub fn new(session: RealtimeTranslationSessionCreateRequest) -> Self {
        Self {
            expires_after: Omittable::Omitted,
            session,
            extra: ExtraFields::new(),
        }
    }

    #[must_use]
    pub fn with_expires_after(
        mut self,
        expiration: RealtimeTranslationClientSecretExpiration,
    ) -> Self {
        self.expires_after = Omittable::Value(expiration);
        self
    }

    #[must_use]
    pub fn clear_expires_after(mut self) -> Self {
        self.expires_after = Omittable::Omitted;
        self
    }

    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Translation client secret and effective session.
#[derive(Clone, Serialize, Deserialize)]
pub struct RealtimeTranslationClientSecretCreateResponse {
    pub value: WireSecret,
    pub expires_at: i64,
    pub session: RealtimeTranslationSession,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeTranslationClientSecretCreateResponse {
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for RealtimeTranslationClientSecretCreateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeTranslationClientSecretCreateResponse")
            .field("value", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("session", &self.session)
            .field("extra", &self.extra)
            .finish()
    }
}

/// Ergonomic aliases matching the verb-first naming used by other Realtime DTOs.
pub type RealtimeCreateTranslationClientSecretRequest =
    RealtimeTranslationClientSecretCreateRequest;
pub type RealtimeCreateTranslationClientSecretResponse =
    RealtimeTranslationClientSecretCreateResponse;

/// Session Description Protocol text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RealtimeSdp(pub String);

impl RealtimeSdp {
    /// Wraps SDP text without normalizing line endings or attributes.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the SDP text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RealtimeSdp {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for RealtimeSdp {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Multipart request for creating a WebRTC Realtime call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallCreateRequest {
    pub sdp: RealtimeSdp,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub session: Omittable<RealtimeSessionCreateRequest>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeCallCreateRequest {
    /// Creates a call-signaling request containing an SDP offer.
    #[must_use]
    pub fn new(sdp: impl Into<RealtimeSdp>) -> Self {
        Self {
            sdp: sdp.into(),
            session: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Adds a typed initial Realtime session configuration.
    #[must_use]
    pub fn with_session(mut self, session: RealtimeSessionCreateRequest) -> Self {
        self.session = Omittable::Value(session);
        self
    }

    /// Omits the initial session configuration.
    #[must_use]
    pub fn clear_session(mut self) -> Self {
        self.session = Omittable::Omitted;
        self
    }

}

/// JSON request accepted when attaching a session to an incoming call.
pub type RealtimeCallAcceptRequest = RealtimeSessionCreateRequest;

/// Request to transfer an active SIP call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallReferRequest {
    pub target_uri: url::Url,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeCallReferRequest {
    /// Creates a SIP transfer request for an already parsed absolute URI.
    #[must_use]
    pub fn new(target_uri: url::Url) -> Self {
        Self {
            target_uri,
            extra: ExtraFields::new(),
        }
    }

    /// Parses and validates an absolute transfer URI.
    pub fn parse(target_uri: &str) -> Result<Self, url::ParseError> {
        url::Url::parse(target_uri).map(Self::new)
    }

}

/// Request to reject an incoming SIP call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallRejectRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status_code: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeCallRejectRequest {
    /// Creates a request using the service-default rejection status.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sends an explicit SIP rejection status code.
    #[must_use]
    pub fn with_status_code(mut self, status_code: i64) -> Self {
        self.status_code = Omittable::Value(status_code);
        self
    }

    /// Returns to the service-default rejection status.
    #[must_use]
    pub fn clear_status_code(mut self) -> Self {
        self.status_code = Omittable::Omitted;
        self
    }

}

/// Marker for the body-less Realtime call hangup operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RealtimeCallHangupRequest;

/// One SIP header delivered with an incoming-call webhook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSipHeader {
    pub name: String,
    pub value: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Data attached to `realtime.call.incoming`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallIncomingData {
    pub call_id: String,
    pub sip_headers: Vec<RealtimeSipHeader>,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(RealtimeIncomingWebhookObjectTag, Event, "event");
literal_tag!(
    RealtimeIncomingWebhookTypeTag,
    Incoming,
    "realtime.call.incoming"
);

/// Incoming SIP-call webhook event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookRealtimeCallIncoming {
    pub created_at: i64,
    pub id: String,
    pub data: RealtimeCallIncomingData,
    #[serde(rename = "object")]
    object: RealtimeIncomingWebhookObjectTag,
    #[serde(rename = "type")]
    kind: RealtimeIncomingWebhookTypeTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

macro_rules! client_event_struct {
    (
        $(#[$meta:meta])*
        $name:ident, $tag:ident, $variant:ident, $wire:literal,
        { $($(#[$field_meta:meta])* $field:ident: $field_ty:ty,)* }
    ) => {
        literal_tag!($tag, $variant, $wire);

        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            pub event_id: Omittable<String>,
            #[serde(rename = "type")]
            kind: $tag,
            $($(#[$field_meta])* pub $field: $field_ty,)*
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Adds a caller-generated event id.
            #[must_use]
            pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
                self.event_id = Omittable::Value(event_id.into());
                self
            }

            /// Returns future fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

client_event_struct! {
    /// Adds an item to the default Realtime conversation.
    RealtimeClientEventConversationItemCreate,
    RealtimeClientConversationItemCreateTag,
    ConversationItemCreate,
    "conversation.item.create",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        previous_item_id: Omittable<String>,
        item: RealtimeConversationItem,
    }
}

impl RealtimeClientEventConversationItemCreate {
    /// Creates an item insertion event.
    #[must_use]
    pub fn new(item: RealtimeConversationItem) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientConversationItemCreateTag::ConversationItemCreate,
            previous_item_id: Omittable::Omitted,
            item,
            extra: ExtraFields::new(),
        }
    }

    /// Inserts after a specific item.
    #[must_use]
    pub fn after(mut self, previous_item_id: impl Into<String>) -> Self {
        self.previous_item_id = Omittable::Value(previous_item_id.into());
        self
    }
}

client_event_struct! {
    /// Deletes one conversation item.
    RealtimeClientEventConversationItemDelete,
    RealtimeClientConversationItemDeleteTag,
    ConversationItemDelete,
    "conversation.item.delete",
    { item_id: String, }
}

impl RealtimeClientEventConversationItemDelete {
    /// Creates a delete event.
    #[must_use]
    pub fn new(item_id: impl Into<String>) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientConversationItemDeleteTag::ConversationItemDelete,
            item_id: item_id.into(),
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Retrieves one conversation item.
    RealtimeClientEventConversationItemRetrieve,
    RealtimeClientConversationItemRetrieveTag,
    ConversationItemRetrieve,
    "conversation.item.retrieve",
    { item_id: String, }
}

impl RealtimeClientEventConversationItemRetrieve {
    /// Creates a retrieve event.
    #[must_use]
    pub fn new(item_id: impl Into<String>) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientConversationItemRetrieveTag::ConversationItemRetrieve,
            item_id: item_id.into(),
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Truncates assistant audio already played by the client.
    RealtimeClientEventConversationItemTruncate,
    RealtimeClientConversationItemTruncateTag,
    ConversationItemTruncate,
    "conversation.item.truncate",
    {
        item_id: String,
        content_index: i64,
        audio_end_ms: i64,
    }
}

impl RealtimeClientEventConversationItemTruncate {
    /// Creates an audio truncation event.
    #[must_use]
    pub fn new(item_id: impl Into<String>, content_index: i64, audio_end_ms: i64) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientConversationItemTruncateTag::ConversationItemTruncate,
            item_id: item_id.into(),
            content_index,
            audio_end_ms,
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Appends decoded bytes to the input audio buffer.
    RealtimeClientEventInputAudioBufferAppend,
    RealtimeClientInputAudioBufferAppendTag,
    InputAudioBufferAppend,
    "input_audio_buffer.append",
    { audio: RealtimeAudio, }
}

impl RealtimeClientEventInputAudioBufferAppend {
    /// Creates an append event from raw bytes.
    #[must_use]
    pub fn new(audio: impl Into<Vec<u8>>) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientInputAudioBufferAppendTag::InputAudioBufferAppend,
            audio: RealtimeAudio::new(audio),
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Clears the input audio buffer.
    RealtimeClientEventInputAudioBufferClear,
    RealtimeClientInputAudioBufferClearTag,
    InputAudioBufferClear,
    "input_audio_buffer.clear",
    {}
}

impl Default for RealtimeClientEventInputAudioBufferClear {
    fn default() -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientInputAudioBufferClearTag::InputAudioBufferClear,
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Clears client-side playback audio.
    RealtimeClientEventOutputAudioBufferClear,
    RealtimeClientOutputAudioBufferClearTag,
    OutputAudioBufferClear,
    "output_audio_buffer.clear",
    {}
}

impl Default for RealtimeClientEventOutputAudioBufferClear {
    fn default() -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientOutputAudioBufferClearTag::OutputAudioBufferClear,
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Commits the current input audio buffer as a conversation item.
    RealtimeClientEventInputAudioBufferCommit,
    RealtimeClientInputAudioBufferCommitTag,
    InputAudioBufferCommit,
    "input_audio_buffer.commit",
    {}
}

impl Default for RealtimeClientEventInputAudioBufferCommit {
    fn default() -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientInputAudioBufferCommitTag::InputAudioBufferCommit,
            extra: ExtraFields::new(),
        }
    }
}

client_event_struct! {
    /// Cancels an in-progress Realtime response.
    RealtimeClientEventResponseCancel,
    RealtimeClientResponseCancelTag,
    ResponseCancel,
    "response.cancel",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        response_id: Omittable<String>,
    }
}

impl Default for RealtimeClientEventResponseCancel {
    fn default() -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientResponseCancelTag::ResponseCancel,
            response_id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

impl RealtimeClientEventResponseCancel {
    /// Cancels a specific response instead of the default in-progress response.
    #[must_use]
    pub fn for_response(response_id: impl Into<String>) -> Self {
        Self {
            response_id: Omittable::Value(response_id.into()),
            ..Self::default()
        }
    }
}

client_event_struct! {
    /// Starts a Realtime model response.
    RealtimeClientEventResponseCreate,
    RealtimeClientResponseCreateTag,
    ResponseCreate,
    "response.create",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        response: Omittable<RealtimeResponseCreateParams>,
    }
}

impl Default for RealtimeClientEventResponseCreate {
    fn default() -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientResponseCreateTag::ResponseCreate,
            response: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

impl RealtimeClientEventResponseCreate {
    /// Creates a response with explicit per-response configuration.
    #[must_use]
    pub fn with_response(response: RealtimeResponseCreateParams) -> Self {
        Self {
            response: Omittable::Value(response),
            ..Self::default()
        }
    }
}

client_event_struct! {
    /// Updates a Realtime or transcription session.
    RealtimeClientEventSessionUpdate,
    RealtimeClientSessionUpdateTag,
    SessionUpdate,
    "session.update",
    { session: RealtimeSessionConfig, }
}

impl RealtimeClientEventSessionUpdate {
    /// Creates a session update event.
    #[must_use]
    pub fn new(session: RealtimeSessionConfig) -> Self {
        Self {
            event_id: Omittable::Omitted,
            kind: RealtimeClientSessionUpdateTag::SessionUpdate,
            session,
            extra: ExtraFields::new(),
        }
    }
}

macro_rules! tagged_event_union {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident($ty:ty) => $wire:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant(Box<$ty>),)+
            /// A future event retained as complete semantic JSON.
            Unknown(UnknownRealtimeObject),
        }

        impl $name {
            /// Returns the exact event discriminator.
            #[must_use]
            pub fn event_type(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $wire,)+
                    Self::Unknown(value) => value.discriminator(),
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $(Self::$variant(value) => value.serialize(serializer),)+
                    Self::Unknown(value) => value.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Value::deserialize(deserializer)?;
                match object_discriminator(&value).map_err(D::Error::custom)? {
                    $($wire => serde_json::from_value::<$ty>(value)
                        .map(Box::new)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => UnknownRealtimeObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }

        $(
            impl From<$ty> for $name {
                fn from(value: $ty) -> Self {
                    Self::$variant(Box::new(value))
                }
            }
        )+
    };
}

tagged_event_union! {
    /// Events accepted by the GA Realtime WebSocket server.
    pub enum RealtimeClientEvent {
        ConversationItemCreate(RealtimeClientEventConversationItemCreate) => "conversation.item.create",
        ConversationItemDelete(RealtimeClientEventConversationItemDelete) => "conversation.item.delete",
        ConversationItemRetrieve(RealtimeClientEventConversationItemRetrieve) => "conversation.item.retrieve",
        ConversationItemTruncate(RealtimeClientEventConversationItemTruncate) => "conversation.item.truncate",
        InputAudioBufferAppend(RealtimeClientEventInputAudioBufferAppend) => "input_audio_buffer.append",
        InputAudioBufferClear(RealtimeClientEventInputAudioBufferClear) => "input_audio_buffer.clear",
        OutputAudioBufferClear(RealtimeClientEventOutputAudioBufferClear) => "output_audio_buffer.clear",
        InputAudioBufferCommit(RealtimeClientEventInputAudioBufferCommit) => "input_audio_buffer.commit",
        ResponseCancel(RealtimeClientEventResponseCancel) => "response.cancel",
        ResponseCreate(RealtimeClientEventResponseCreate) => "response.create",
        SessionUpdate(RealtimeClientEventSessionUpdate) => "session.update"
    }
}

/// Schema branches in the pinned `RealtimeClientEvent` discriminator manifest.
pub const REALTIME_CLIENT_EVENT_BRANCHES: &[&str] = &[
    "RealtimeClientEventConversationItemCreate",
    "RealtimeClientEventConversationItemDelete",
    "RealtimeClientEventConversationItemRetrieve",
    "RealtimeClientEventConversationItemTruncate",
    "RealtimeClientEventInputAudioBufferAppend",
    "RealtimeClientEventInputAudioBufferClear",
    "RealtimeClientEventInputAudioBufferCommit",
    "RealtimeClientEventOutputAudioBufferClear",
    "RealtimeClientEventResponseCancel",
    "RealtimeClientEventResponseCreate",
    "RealtimeClientEventSessionUpdate",
];

macro_rules! server_event_struct {
    (
        $(#[$meta:meta])*
        $name:ident, $tag:ident, $variant:ident, $wire:literal,
        { $($(#[$field_meta:meta])* $field:ident: $field_ty:ty,)* }
    ) => {
        literal_tag!($tag, $variant, $wire);

        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            pub event_id: String,
            #[serde(rename = "type")]
            kind: $tag,
            $($(#[$field_meta])* pub $field: $field_ty,)*
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns future fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

server_event_struct! {
    /// Announces the conversation resource created for a session.
    RealtimeServerEventConversationCreated,
    RealtimeServerConversationCreatedTag,
    ConversationCreated,
    "conversation.created",
    { conversation: RealtimeConversation, }
}

server_event_struct! {
    /// Confirms creation of a conversation item.
    RealtimeServerEventConversationItemCreated,
    RealtimeServerConversationItemCreatedTag,
    ConversationItemCreated,
    "conversation.item.created",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        previous_item_id: Omittable<Nullable<String>>,
        item: RealtimeConversationItem,
    }
}

server_event_struct! {
    /// Confirms deletion of a conversation item.
    RealtimeServerEventConversationItemDeleted,
    RealtimeServerConversationItemDeletedTag,
    ConversationItemDeleted,
    "conversation.item.deleted",
    { item_id: String, }
}

server_event_struct! {
    /// Final input-audio transcript and its billing usage.
    RealtimeServerEventConversationItemInputAudioTranscriptionCompleted,
    RealtimeServerInputAudioTranscriptionCompletedTag,
    InputAudioTranscriptionCompleted,
    "conversation.item.input_audio_transcription.completed",
    {
        item_id: String,
        content_index: i64,
        transcript: String,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        languages: Omittable<Vec<TranscriptionLanguage>>,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        logprobs: Omittable<Nullable<Vec<RealtimeTranscriptionLogprob>>>,
        usage: RealtimeTranscriptionUsage,
    }
}

server_event_struct! {
    /// Incremental input-audio transcription.
    RealtimeServerEventConversationItemInputAudioTranscriptionDelta,
    RealtimeServerInputAudioTranscriptionDeltaTag,
    InputAudioTranscriptionDelta,
    "conversation.item.input_audio_transcription.delta",
    {
        item_id: String,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        content_index: Omittable<i64>,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        delta: Omittable<String>,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        logprobs: Omittable<Nullable<Vec<RealtimeTranscriptionLogprob>>>,
    }
}

server_event_struct! {
    /// Input-audio transcription failure.
    RealtimeServerEventConversationItemInputAudioTranscriptionFailed,
    RealtimeServerInputAudioTranscriptionFailedTag,
    InputAudioTranscriptionFailed,
    "conversation.item.input_audio_transcription.failed",
    {
        item_id: String,
        content_index: i64,
        error: RealtimeTranscriptionError,
    }
}

server_event_struct! {
    /// Returns a requested conversation item.
    RealtimeServerEventConversationItemRetrieved,
    RealtimeServerConversationItemRetrievedTag,
    ConversationItemRetrieved,
    "conversation.item.retrieved",
    { item: RealtimeConversationItem, }
}

server_event_struct! {
    /// Confirms truncation of assistant audio.
    RealtimeServerEventConversationItemTruncated,
    RealtimeServerConversationItemTruncatedTag,
    ConversationItemTruncated,
    "conversation.item.truncated",
    {
        item_id: String,
        content_index: i64,
        audio_end_ms: i64,
    }
}

server_event_struct! {
    /// Protocol or request error.
    RealtimeServerEventError,
    RealtimeServerErrorTag,
    Error,
    "error",
    { error: RealtimeErrorDetails, }
}

server_event_struct! {
    /// Confirms the input audio buffer was cleared.
    RealtimeServerEventInputAudioBufferCleared,
    RealtimeServerInputAudioBufferClearedTag,
    InputAudioBufferCleared,
    "input_audio_buffer.cleared",
    {}
}

server_event_struct! {
    /// Confirms commit of the input audio buffer.
    RealtimeServerEventInputAudioBufferCommitted,
    RealtimeServerInputAudioBufferCommittedTag,
    InputAudioBufferCommitted,
    "input_audio_buffer.committed",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        previous_item_id: Omittable<Nullable<String>>,
        item_id: String,
    }
}

literal_tag!(
    RealtimeServerInputAudioBufferDtmfReceivedTag,
    InputAudioBufferDtmfReceived,
    "input_audio_buffer.dtmf_event_received"
);

/// A DTMF key received from a SIP call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeServerEventInputAudioBufferDtmfEventReceived {
    #[serde(rename = "type")]
    kind: RealtimeServerInputAudioBufferDtmfReceivedTag,
    pub event: String,
    pub received_at: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeServerEventInputAudioBufferDtmfEventReceived {
    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

server_event_struct! {
    /// Server VAD detected speech start.
    RealtimeServerEventInputAudioBufferSpeechStarted,
    RealtimeServerInputAudioBufferSpeechStartedTag,
    InputAudioBufferSpeechStarted,
    "input_audio_buffer.speech_started",
    {
        audio_start_ms: i64,
        item_id: String,
    }
}

server_event_struct! {
    /// Server VAD detected speech stop.
    RealtimeServerEventInputAudioBufferSpeechStopped,
    RealtimeServerInputAudioBufferSpeechStoppedTag,
    InputAudioBufferSpeechStopped,
    "input_audio_buffer.speech_stopped",
    {
        audio_end_ms: i64,
        item_id: String,
    }
}

server_event_struct! {
    /// Current request and token rate limits.
    RealtimeServerEventRateLimitsUpdated,
    RealtimeServerRateLimitsUpdatedTag,
    RateLimitsUpdated,
    "rate_limits.updated",
    { rate_limits: Vec<RealtimeRateLimit>, }
}

server_event_struct! {
    /// Base64-decoded output audio delta.
    RealtimeServerEventResponseAudioDelta,
    RealtimeServerResponseAudioDeltaTag,
    ResponseAudioDelta,
    "response.output_audio.delta",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        delta: RealtimeAudio,
    }
}

server_event_struct! {
    /// Output audio stream completed.
    RealtimeServerEventResponseAudioDone,
    RealtimeServerResponseAudioDoneTag,
    ResponseAudioDone,
    "response.output_audio.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
    }
}

server_event_struct! {
    /// Output-audio transcript delta.
    RealtimeServerEventResponseAudioTranscriptDelta,
    RealtimeServerResponseAudioTranscriptDeltaTag,
    ResponseAudioTranscriptDelta,
    "response.output_audio_transcript.delta",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        delta: String,
    }
}

server_event_struct! {
    /// Final output-audio transcript.
    RealtimeServerEventResponseAudioTranscriptDone,
    RealtimeServerResponseAudioTranscriptDoneTag,
    ResponseAudioTranscriptDone,
    "response.output_audio_transcript.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        transcript: String,
    }
}

server_event_struct! {
    /// A response content part was added.
    RealtimeServerEventResponseContentPartAdded,
    RealtimeServerResponseContentPartAddedTag,
    ResponseContentPartAdded,
    "response.content_part.added",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        part: RealtimeResponseContentPart,
    }
}

server_event_struct! {
    /// A response content part completed.
    RealtimeServerEventResponseContentPartDone,
    RealtimeServerResponseContentPartDoneTag,
    ResponseContentPartDone,
    "response.content_part.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        part: RealtimeResponseContentPart,
    }
}

server_event_struct! {
    /// A Realtime response was created.
    RealtimeServerEventResponseCreated,
    RealtimeServerResponseCreatedTag,
    ResponseCreated,
    "response.created",
    { response: RealtimeResponse, }
}

server_event_struct! {
    /// A Realtime response reached a terminal state.
    RealtimeServerEventResponseDone,
    RealtimeServerResponseDoneTag,
    ResponseDone,
    "response.done",
    { response: RealtimeResponse, }
}

server_event_struct! {
    /// Incremental function-call arguments.
    RealtimeServerEventResponseFunctionCallArgumentsDelta,
    RealtimeServerFunctionCallArgumentsDeltaTag,
    FunctionCallArgumentsDelta,
    "response.function_call_arguments.delta",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        call_id: String,
        delta: String,
    }
}

server_event_struct! {
    /// Final function-call arguments.
    RealtimeServerEventResponseFunctionCallArgumentsDone,
    RealtimeServerFunctionCallArgumentsDoneTag,
    FunctionCallArgumentsDone,
    "response.function_call_arguments.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        call_id: String,
        name: String,
        arguments: JsonText,
    }
}

server_event_struct! {
    /// A response output item was added.
    RealtimeServerEventResponseOutputItemAdded,
    RealtimeServerResponseOutputItemAddedTag,
    ResponseOutputItemAdded,
    "response.output_item.added",
    {
        response_id: String,
        output_index: i64,
        item: RealtimeConversationItem,
    }
}

server_event_struct! {
    /// A response output item completed.
    RealtimeServerEventResponseOutputItemDone,
    RealtimeServerResponseOutputItemDoneTag,
    ResponseOutputItemDone,
    "response.output_item.done",
    {
        response_id: String,
        output_index: i64,
        item: RealtimeConversationItem,
    }
}

server_event_struct! {
    /// Incremental output text.
    RealtimeServerEventResponseTextDelta,
    RealtimeServerResponseTextDeltaTag,
    ResponseTextDelta,
    "response.output_text.delta",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        delta: String,
    }
}

server_event_struct! {
    /// Final output text.
    RealtimeServerEventResponseTextDone,
    RealtimeServerResponseTextDoneTag,
    ResponseTextDone,
    "response.output_text.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        content_index: i64,
        text: String,
    }
}

server_event_struct! {
    /// Initial effective session configuration.
    RealtimeServerEventSessionCreated,
    RealtimeServerSessionCreatedTag,
    SessionCreated,
    "session.created",
    { session: RealtimeSessionState, }
}

server_event_struct! {
    /// Updated effective session configuration.
    RealtimeServerEventSessionUpdated,
    RealtimeServerSessionUpdatedTag,
    SessionUpdated,
    "session.updated",
    { session: RealtimeSessionState, }
}

server_event_struct! {
    /// Client playback of one response started.
    RealtimeServerEventOutputAudioBufferStarted,
    RealtimeServerOutputAudioBufferStartedTag,
    OutputAudioBufferStarted,
    "output_audio_buffer.started",
    { response_id: String, }
}

server_event_struct! {
    /// Client playback of one response stopped.
    RealtimeServerEventOutputAudioBufferStopped,
    RealtimeServerOutputAudioBufferStoppedTag,
    OutputAudioBufferStopped,
    "output_audio_buffer.stopped",
    { response_id: String, }
}

server_event_struct! {
    /// Client playback buffer was cleared.
    RealtimeServerEventOutputAudioBufferCleared,
    RealtimeServerOutputAudioBufferClearedTag,
    OutputAudioBufferCleared,
    "output_audio_buffer.cleared",
    { response_id: String, }
}

server_event_struct! {
    /// An item was appended to the conversation.
    RealtimeServerEventConversationItemAdded,
    RealtimeServerConversationItemAddedTag,
    ConversationItemAdded,
    "conversation.item.added",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        previous_item_id: Omittable<Nullable<String>>,
        item: RealtimeConversationItem,
    }
}

server_event_struct! {
    /// Conversation item processing completed.
    RealtimeServerEventConversationItemDone,
    RealtimeServerConversationItemDoneTag,
    ConversationItemDone,
    "conversation.item.done",
    {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        previous_item_id: Omittable<Nullable<String>>,
        item: RealtimeConversationItem,
    }
}

server_event_struct! {
    /// Idle timeout triggered a model response.
    RealtimeServerEventInputAudioBufferTimeoutTriggered,
    RealtimeServerInputAudioBufferTimeoutTriggeredTag,
    InputAudioBufferTimeoutTriggered,
    "input_audio_buffer.timeout_triggered",
    {
        audio_start_ms: i64,
        audio_end_ms: i64,
        item_id: String,
    }
}

server_event_struct! {
    /// Speaker-attributed transcription segment.
    RealtimeServerEventConversationItemInputAudioTranscriptionSegment,
    RealtimeServerInputAudioTranscriptionSegmentTag,
    InputAudioTranscriptionSegment,
    "conversation.item.input_audio_transcription.segment",
    {
        item_id: String,
        content_index: i64,
        text: String,
        id: String,
        speaker: String,
        start: f64,
        end: f64,
    }
}

server_event_struct! {
    /// MCP tool discovery started.
    RealtimeServerEventMCPListToolsInProgress,
    RealtimeServerMcpListToolsInProgressTag,
    McpListToolsInProgress,
    "mcp_list_tools.in_progress",
    { item_id: String, }
}

server_event_struct! {
    /// MCP tool discovery completed.
    RealtimeServerEventMCPListToolsCompleted,
    RealtimeServerMcpListToolsCompletedTag,
    McpListToolsCompleted,
    "mcp_list_tools.completed",
    { item_id: String, }
}

server_event_struct! {
    /// MCP tool discovery failed.
    RealtimeServerEventMCPListToolsFailed,
    RealtimeServerMcpListToolsFailedTag,
    McpListToolsFailed,
    "mcp_list_tools.failed",
    { item_id: String, }
}

server_event_struct! {
    /// Incremental MCP call arguments.
    RealtimeServerEventResponseMCPCallArgumentsDelta,
    RealtimeServerMcpCallArgumentsDeltaTag,
    McpCallArgumentsDelta,
    "response.mcp_call_arguments.delta",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        delta: String,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        obfuscation: Omittable<Nullable<String>>,
    }
}

server_event_struct! {
    /// Final MCP call arguments.
    RealtimeServerEventResponseMCPCallArgumentsDone,
    RealtimeServerMcpCallArgumentsDoneTag,
    McpCallArgumentsDone,
    "response.mcp_call_arguments.done",
    {
        response_id: String,
        item_id: String,
        output_index: i64,
        arguments: JsonText,
    }
}

server_event_struct! {
    /// MCP tool call began.
    RealtimeServerEventResponseMCPCallInProgress,
    RealtimeServerMcpCallInProgressTag,
    McpCallInProgress,
    "response.mcp_call.in_progress",
    {
        output_index: i64,
        item_id: String,
    }
}

server_event_struct! {
    /// MCP tool call completed.
    RealtimeServerEventResponseMCPCallCompleted,
    RealtimeServerMcpCallCompletedTag,
    McpCallCompleted,
    "response.mcp_call.completed",
    {
        output_index: i64,
        item_id: String,
    }
}

server_event_struct! {
    /// MCP tool call failed.
    RealtimeServerEventResponseMCPCallFailed,
    RealtimeServerMcpCallFailedTag,
    McpCallFailed,
    "response.mcp_call.failed",
    {
        output_index: i64,
        item_id: String,
    }
}

tagged_event_union! {
    /// Events emitted by the GA Realtime WebSocket server.
    pub enum RealtimeServerEvent {
        ConversationCreated(RealtimeServerEventConversationCreated) => "conversation.created",
        ConversationItemCreated(RealtimeServerEventConversationItemCreated) => "conversation.item.created",
        ConversationItemDeleted(RealtimeServerEventConversationItemDeleted) => "conversation.item.deleted",
        InputAudioTranscriptionCompleted(RealtimeServerEventConversationItemInputAudioTranscriptionCompleted) => "conversation.item.input_audio_transcription.completed",
        InputAudioTranscriptionDelta(RealtimeServerEventConversationItemInputAudioTranscriptionDelta) => "conversation.item.input_audio_transcription.delta",
        InputAudioTranscriptionFailed(RealtimeServerEventConversationItemInputAudioTranscriptionFailed) => "conversation.item.input_audio_transcription.failed",
        ConversationItemRetrieved(RealtimeServerEventConversationItemRetrieved) => "conversation.item.retrieved",
        ConversationItemTruncated(RealtimeServerEventConversationItemTruncated) => "conversation.item.truncated",
        Error(RealtimeServerEventError) => "error",
        InputAudioBufferCleared(RealtimeServerEventInputAudioBufferCleared) => "input_audio_buffer.cleared",
        InputAudioBufferCommitted(RealtimeServerEventInputAudioBufferCommitted) => "input_audio_buffer.committed",
        InputAudioBufferDtmfEventReceived(RealtimeServerEventInputAudioBufferDtmfEventReceived) => "input_audio_buffer.dtmf_event_received",
        InputAudioBufferSpeechStarted(RealtimeServerEventInputAudioBufferSpeechStarted) => "input_audio_buffer.speech_started",
        InputAudioBufferSpeechStopped(RealtimeServerEventInputAudioBufferSpeechStopped) => "input_audio_buffer.speech_stopped",
        RateLimitsUpdated(RealtimeServerEventRateLimitsUpdated) => "rate_limits.updated",
        ResponseAudioDelta(RealtimeServerEventResponseAudioDelta) => "response.output_audio.delta",
        ResponseAudioDone(RealtimeServerEventResponseAudioDone) => "response.output_audio.done",
        ResponseAudioTranscriptDelta(RealtimeServerEventResponseAudioTranscriptDelta) => "response.output_audio_transcript.delta",
        ResponseAudioTranscriptDone(RealtimeServerEventResponseAudioTranscriptDone) => "response.output_audio_transcript.done",
        ResponseContentPartAdded(RealtimeServerEventResponseContentPartAdded) => "response.content_part.added",
        ResponseContentPartDone(RealtimeServerEventResponseContentPartDone) => "response.content_part.done",
        ResponseCreated(RealtimeServerEventResponseCreated) => "response.created",
        ResponseDone(RealtimeServerEventResponseDone) => "response.done",
        ResponseFunctionCallArgumentsDelta(RealtimeServerEventResponseFunctionCallArgumentsDelta) => "response.function_call_arguments.delta",
        ResponseFunctionCallArgumentsDone(RealtimeServerEventResponseFunctionCallArgumentsDone) => "response.function_call_arguments.done",
        ResponseOutputItemAdded(RealtimeServerEventResponseOutputItemAdded) => "response.output_item.added",
        ResponseOutputItemDone(RealtimeServerEventResponseOutputItemDone) => "response.output_item.done",
        ResponseTextDelta(RealtimeServerEventResponseTextDelta) => "response.output_text.delta",
        ResponseTextDone(RealtimeServerEventResponseTextDone) => "response.output_text.done",
        SessionCreated(RealtimeServerEventSessionCreated) => "session.created",
        SessionUpdated(RealtimeServerEventSessionUpdated) => "session.updated",
        OutputAudioBufferStarted(RealtimeServerEventOutputAudioBufferStarted) => "output_audio_buffer.started",
        OutputAudioBufferStopped(RealtimeServerEventOutputAudioBufferStopped) => "output_audio_buffer.stopped",
        OutputAudioBufferCleared(RealtimeServerEventOutputAudioBufferCleared) => "output_audio_buffer.cleared",
        ConversationItemAdded(RealtimeServerEventConversationItemAdded) => "conversation.item.added",
        ConversationItemDone(RealtimeServerEventConversationItemDone) => "conversation.item.done",
        InputAudioBufferTimeoutTriggered(RealtimeServerEventInputAudioBufferTimeoutTriggered) => "input_audio_buffer.timeout_triggered",
        InputAudioTranscriptionSegment(RealtimeServerEventConversationItemInputAudioTranscriptionSegment) => "conversation.item.input_audio_transcription.segment",
        McpListToolsInProgress(RealtimeServerEventMCPListToolsInProgress) => "mcp_list_tools.in_progress",
        McpListToolsCompleted(RealtimeServerEventMCPListToolsCompleted) => "mcp_list_tools.completed",
        McpListToolsFailed(RealtimeServerEventMCPListToolsFailed) => "mcp_list_tools.failed",
        ResponseMcpCallArgumentsDelta(RealtimeServerEventResponseMCPCallArgumentsDelta) => "response.mcp_call_arguments.delta",
        ResponseMcpCallArgumentsDone(RealtimeServerEventResponseMCPCallArgumentsDone) => "response.mcp_call_arguments.done",
        ResponseMcpCallInProgress(RealtimeServerEventResponseMCPCallInProgress) => "response.mcp_call.in_progress",
        ResponseMcpCallCompleted(RealtimeServerEventResponseMCPCallCompleted) => "response.mcp_call.completed",
        ResponseMcpCallFailed(RealtimeServerEventResponseMCPCallFailed) => "response.mcp_call.failed"
    }
}

/// Schema branches in the pinned `RealtimeServerEvent` discriminator manifest.
pub const REALTIME_SERVER_EVENT_BRANCHES: &[&str] = &[
    "RealtimeServerEventConversationCreated",
    "RealtimeServerEventConversationItemAdded",
    "RealtimeServerEventConversationItemCreated",
    "RealtimeServerEventConversationItemDeleted",
    "RealtimeServerEventConversationItemDone",
    "RealtimeServerEventConversationItemInputAudioTranscriptionCompleted",
    "RealtimeServerEventConversationItemInputAudioTranscriptionDelta",
    "RealtimeServerEventConversationItemInputAudioTranscriptionFailed",
    "RealtimeServerEventConversationItemInputAudioTranscriptionSegment",
    "RealtimeServerEventConversationItemRetrieved",
    "RealtimeServerEventConversationItemTruncated",
    "RealtimeServerEventError",
    "RealtimeServerEventInputAudioBufferCleared",
    "RealtimeServerEventInputAudioBufferCommitted",
    "RealtimeServerEventInputAudioBufferDtmfEventReceived",
    "RealtimeServerEventInputAudioBufferSpeechStarted",
    "RealtimeServerEventInputAudioBufferSpeechStopped",
    "RealtimeServerEventInputAudioBufferTimeoutTriggered",
    "RealtimeServerEventMCPListToolsCompleted",
    "RealtimeServerEventMCPListToolsFailed",
    "RealtimeServerEventMCPListToolsInProgress",
    "RealtimeServerEventOutputAudioBufferCleared",
    "RealtimeServerEventOutputAudioBufferStarted",
    "RealtimeServerEventOutputAudioBufferStopped",
    "RealtimeServerEventRateLimitsUpdated",
    "RealtimeServerEventResponseAudioDelta",
    "RealtimeServerEventResponseAudioDone",
    "RealtimeServerEventResponseAudioTranscriptDelta",
    "RealtimeServerEventResponseAudioTranscriptDone",
    "RealtimeServerEventResponseContentPartAdded",
    "RealtimeServerEventResponseContentPartDone",
    "RealtimeServerEventResponseCreated",
    "RealtimeServerEventResponseDone",
    "RealtimeServerEventResponseFunctionCallArgumentsDelta",
    "RealtimeServerEventResponseFunctionCallArgumentsDone",
    "RealtimeServerEventResponseMCPCallArgumentsDelta",
    "RealtimeServerEventResponseMCPCallArgumentsDone",
    "RealtimeServerEventResponseMCPCallCompleted",
    "RealtimeServerEventResponseMCPCallFailed",
    "RealtimeServerEventResponseMCPCallInProgress",
    "RealtimeServerEventResponseOutputItemAdded",
    "RealtimeServerEventResponseOutputItemDone",
    "RealtimeServerEventResponseTextDelta",
    "RealtimeServerEventResponseTextDone",
    "RealtimeServerEventSessionCreated",
    "RealtimeServerEventSessionUpdated",
];

/// Wire discriminators in the pinned GA Realtime client-event union.
pub const REALTIME_CLIENT_EVENT_TAGS: &[&str] = &[
    "conversation.item.create",
    "conversation.item.delete",
    "conversation.item.retrieve",
    "conversation.item.truncate",
    "input_audio_buffer.append",
    "input_audio_buffer.clear",
    "input_audio_buffer.commit",
    "output_audio_buffer.clear",
    "response.cancel",
    "response.create",
    "session.update",
];

/// Wire discriminators in the pinned 46-branch GA Realtime server-event union.
pub const REALTIME_SERVER_EVENT_TAGS: &[&str] = &[
    "conversation.created",
    "conversation.item.added",
    "conversation.item.created",
    "conversation.item.deleted",
    "conversation.item.done",
    "conversation.item.input_audio_transcription.completed",
    "conversation.item.input_audio_transcription.delta",
    "conversation.item.input_audio_transcription.failed",
    "conversation.item.input_audio_transcription.segment",
    "conversation.item.retrieved",
    "conversation.item.truncated",
    "error",
    "input_audio_buffer.cleared",
    "input_audio_buffer.committed",
    "input_audio_buffer.dtmf_event_received",
    "input_audio_buffer.speech_started",
    "input_audio_buffer.speech_stopped",
    "input_audio_buffer.timeout_triggered",
    "mcp_list_tools.completed",
    "mcp_list_tools.failed",
    "mcp_list_tools.in_progress",
    "output_audio_buffer.cleared",
    "output_audio_buffer.started",
    "output_audio_buffer.stopped",
    "rate_limits.updated",
    "response.content_part.added",
    "response.content_part.done",
    "response.created",
    "response.done",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "response.mcp_call.completed",
    "response.mcp_call.failed",
    "response.mcp_call.in_progress",
    "response.mcp_call_arguments.delta",
    "response.mcp_call_arguments.done",
    "response.output_audio.delta",
    "response.output_audio.done",
    "response.output_audio_transcript.delta",
    "response.output_audio_transcript.done",
    "response.output_item.added",
    "response.output_item.done",
    "response.output_text.delta",
    "response.output_text.done",
    "session.created",
    "session.updated",
];

macro_rules! impl_extra_fields {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $ty {
                /// Returns future fields retained while decoding.
                #[must_use]
                pub const fn extra_fields(&self) -> &ExtraFields {
                    &self.extra
                }
            }
        )+
    };
}

impl_extra_fields!(
    RealtimePcmAudioFormat,
    RealtimePcmuAudioFormat,
    RealtimePcmaAudioFormat,
    RealtimeCustomVoice,
    RealtimeAudioTranscription,
    RealtimeNoiseReduction,
    RealtimeServerVad,
    RealtimeSemanticVad,
    RealtimeAudioInputConfig,
    RealtimeAudioOutputConfig,
    RealtimeSessionAudio,
    RealtimeSessionAudioOutputState,
    RealtimeSessionAudioState,
    RealtimeTranscriptionAudio,
    RealtimeResponseCreateAudioOutput,
    RealtimeResponseCreateAudio,
    RealtimeResponseAudioOutput,
    RealtimeResponseAudio,
    RealtimeReasoning,
    RealtimeTracingConfig,
    RealtimeTruncationTokenLimits,
    RealtimeRetentionRatioTruncation,
    RealtimeFunctionTool,
    RealtimeFunctionToolChoice,
    RealtimeMcpToolChoice,
    RealtimeSessionCreateRequest,
    RealtimeTranscriptionSessionCreateRequest,
    RealtimeSession,
    RealtimeTranscriptionSession,
    RealtimeSystemContentPart,
    RealtimeUserContentPart,
    RealtimeAssistantContentPart,
    RealtimeConversationItemMessageSystem,
    RealtimeConversationItemMessageUser,
    RealtimeConversationItemMessageAssistant,
    RealtimeConversationItemFunctionCall,
    RealtimeConversationItemFunctionCallOutput,
    RealtimeMcpApprovalResponse,
    RealtimeMcpListedTool,
    RealtimeMcpListTools,
    RealtimeMcpProtocolError,
    RealtimeMcpToolExecutionError,
    RealtimeMcpHttpError,
    RealtimeMcpToolCall,
    RealtimeMcpApprovalRequest,
    RealtimeResponseCreateParams,
    RealtimeResponseFailure,
    RealtimeResponseStatusDetails,
    RealtimeCachedTokenDetails,
    RealtimeInputTokenDetails,
    RealtimeOutputTokenDetails,
    RealtimeResponseUsage,
    RealtimeResponse,
    RealtimeTranscriptionLogprob,
    RealtimeTranscriptInputTokenDetails,
    RealtimeTranscriptTokenUsage,
    RealtimeTranscriptDurationUsage,
    RealtimeTranscriptionError,
    RealtimeErrorDetails,
    RealtimeConversation,
    RealtimeRateLimit,
    RealtimeResponseContentPart,
    RealtimeClientSecretExpiration,
    RealtimeCreateClientSecretRequest,
    RealtimeCreateClientSecretResponse,
    RealtimeCallCreateRequest,
    RealtimeCallReferRequest,
    RealtimeCallRejectRequest,
    RealtimeSipHeader,
    RealtimeCallIncomingData,
    WebhookRealtimeCallIncoming,
);

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Map, Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(RealtimeClientEvent: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(RealtimeServerEvent: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(RealtimeConversationItem: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(RealtimeAudio: Serialize, DeserializeOwned, Send, Sync);

    fn manifest_branches(schema: &str) -> Vec<String> {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/discriminators.json"))
                .expect("pinned discriminator manifest is valid JSON");
        let entries = manifest["entries"]
            .as_array()
            .expect("manifest entries are an array");
        let entry = entries
            .iter()
            .find(|entry| entry["schema"].as_str() == Some(schema))
            .expect("Realtime discriminator entry is present");
        entry["branch_refs"]
            .as_array()
            .expect("branch_refs is an array")
            .iter()
            .map(|reference| {
                reference
                    .as_str()
                    .and_then(|reference| reference.rsplit('/').next())
                    .expect("branch ref has a final component")
                    .to_owned()
            })
            .collect()
    }

    fn openapi_tags(branches: &[&str]) -> Vec<String> {
        let openapi: Value = serde_json::from_str(include_str!(
            "../../../spec/upstream/openapi-2026-08-29.json"
        ))
        .expect("pinned OpenAPI is valid JSON");
        let schemas = openapi["components"]["schemas"]
            .as_object()
            .expect("OpenAPI schemas are an object");
        let mut tags: Vec<String> = branches
            .iter()
            .map(|branch| {
                schemas[*branch]["properties"]["type"]["enum"]
                    .as_array()
                    .and_then(|values| values.first())
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{branch} has one string type enum"))
                    .to_owned()
            })
            .collect();
        tags.sort();
        tags
    }

    fn function_output_item() -> Value {
        json!({
            "type": "function_call_output",
            "call_id": "call_1",
            "output": "{}"
        })
    }

    fn client_fixture(tag: &str) -> Value {
        match tag {
            "conversation.item.create" => json!({
                "type": tag,
                "item": function_output_item()
            }),
            "conversation.item.delete" | "conversation.item.retrieve" => {
                json!({"type": tag, "item_id": "item_1"})
            }
            "conversation.item.truncate" => json!({
                "type": tag,
                "item_id": "item_1",
                "content_index": 0,
                "audio_end_ms": 12
            }),
            "input_audio_buffer.append" => json!({"type": tag, "audio": "AQID"}),
            "input_audio_buffer.clear"
            | "input_audio_buffer.commit"
            | "output_audio_buffer.clear"
            | "response.cancel"
            | "response.create" => json!({"type": tag}),
            "session.update" => json!({
                "type": tag,
                "session": {"type": "realtime"}
            }),
            other => panic!("missing client fixture for {other}"),
        }
    }

    fn insert(object: &mut Map<String, Value>, key: &str, value: Value) {
        object.insert(key.to_owned(), value);
    }

    fn server_fixture(tag: &str) -> Value {
        let mut object = Map::new();
        insert(&mut object, "event_id", json!("event_1"));
        insert(&mut object, "type", json!(tag));
        let item = function_output_item();

        match tag {
            "conversation.created" => insert(&mut object, "conversation", json!({})),
            "conversation.item.added"
            | "conversation.item.created"
            | "conversation.item.done"
            | "conversation.item.retrieved" => insert(&mut object, "item", item),
            "conversation.item.deleted" => insert(&mut object, "item_id", json!("item_1")),
            "conversation.item.input_audio_transcription.completed" => {
                insert(&mut object, "item_id", json!("item_1"));
                insert(&mut object, "content_index", json!(0));
                insert(&mut object, "transcript", json!("hello"));
                insert(
                    &mut object,
                    "usage",
                    json!({"type": "duration", "seconds": 0.5}),
                );
            }
            "conversation.item.input_audio_transcription.delta" => {
                insert(&mut object, "item_id", json!("item_1"));
            }
            "conversation.item.input_audio_transcription.failed" => {
                insert(&mut object, "item_id", json!("item_1"));
                insert(&mut object, "content_index", json!(0));
                insert(&mut object, "error", json!({}));
            }
            "conversation.item.input_audio_transcription.segment" => {
                insert(&mut object, "item_id", json!("item_1"));
                insert(&mut object, "content_index", json!(0));
                insert(&mut object, "text", json!("hello"));
                insert(&mut object, "id", json!("segment_1"));
                insert(&mut object, "speaker", json!("speaker_1"));
                insert(&mut object, "start", json!(0.0));
                insert(&mut object, "end", json!(0.5));
            }
            "conversation.item.truncated" => {
                insert(&mut object, "item_id", json!("item_1"));
                insert(&mut object, "content_index", json!(0));
                insert(&mut object, "audio_end_ms", json!(12));
            }
            "error" => insert(
                &mut object,
                "error",
                json!({"type": "invalid_request_error", "message": "bad"}),
            ),
            "input_audio_buffer.cleared" => {}
            "input_audio_buffer.committed" => {
                insert(&mut object, "item_id", json!("item_1"));
            }
            "input_audio_buffer.dtmf_event_received" => {
                object.remove("event_id");
                insert(&mut object, "event", json!("5"));
                insert(&mut object, "received_at", json!(123));
            }
            "input_audio_buffer.speech_started" => {
                insert(&mut object, "audio_start_ms", json!(0));
                insert(&mut object, "item_id", json!("item_1"));
            }
            "input_audio_buffer.speech_stopped" => {
                insert(&mut object, "audio_end_ms", json!(10));
                insert(&mut object, "item_id", json!("item_1"));
            }
            "input_audio_buffer.timeout_triggered" => {
                insert(&mut object, "audio_start_ms", json!(0));
                insert(&mut object, "audio_end_ms", json!(10));
                insert(&mut object, "item_id", json!("item_1"));
            }
            "mcp_list_tools.completed" | "mcp_list_tools.failed" | "mcp_list_tools.in_progress" => {
                insert(&mut object, "item_id", json!("item_1"));
            }
            "output_audio_buffer.cleared"
            | "output_audio_buffer.started"
            | "output_audio_buffer.stopped" => {
                insert(&mut object, "response_id", json!("resp_1"));
            }
            "rate_limits.updated" => insert(&mut object, "rate_limits", json!([])),
            "response.content_part.added" | "response.content_part.done" => {
                response_location_fields(&mut object);
                insert(&mut object, "part", json!({}));
            }
            "response.created" | "response.done" => {
                insert(&mut object, "response", json!({}));
            }
            "response.function_call_arguments.delta" => {
                response_item_fields(&mut object);
                insert(&mut object, "call_id", json!("call_1"));
                insert(&mut object, "delta", json!("{"));
            }
            "response.function_call_arguments.done" => {
                response_item_fields(&mut object);
                insert(&mut object, "call_id", json!("call_1"));
                insert(&mut object, "name", json!("weather"));
                insert(&mut object, "arguments", json!("{}"));
            }
            "response.mcp_call.completed"
            | "response.mcp_call.failed"
            | "response.mcp_call.in_progress" => {
                insert(&mut object, "output_index", json!(0));
                insert(&mut object, "item_id", json!("item_1"));
            }
            "response.mcp_call_arguments.delta" => {
                response_item_fields(&mut object);
                insert(&mut object, "delta", json!("{"));
            }
            "response.mcp_call_arguments.done" => {
                response_item_fields(&mut object);
                insert(&mut object, "arguments", json!("{}"));
            }
            "response.output_audio.delta" => {
                response_location_fields(&mut object);
                insert(&mut object, "delta", json!("AQID"));
            }
            "response.output_audio.done" => response_location_fields(&mut object),
            "response.output_audio_transcript.delta" => {
                response_location_fields(&mut object);
                insert(&mut object, "delta", json!("hello"));
            }
            "response.output_audio_transcript.done" => {
                response_location_fields(&mut object);
                insert(&mut object, "transcript", json!("hello"));
            }
            "response.output_item.added" | "response.output_item.done" => {
                insert(&mut object, "response_id", json!("resp_1"));
                insert(&mut object, "output_index", json!(0));
                insert(&mut object, "item", item);
            }
            "response.output_text.delta" => {
                response_location_fields(&mut object);
                insert(&mut object, "delta", json!("hello"));
            }
            "response.output_text.done" => {
                response_location_fields(&mut object);
                insert(&mut object, "text", json!("hello"));
            }
            "session.created" | "session.updated" => insert(
                &mut object,
                "session",
                json!({
                    "type": "realtime",
                    "id": "sess_1",
                    "object": "realtime.session"
                }),
            ),
            other => panic!("missing server fixture for {other}"),
        }
        Value::Object(object)
    }

    fn response_item_fields(object: &mut Map<String, Value>) {
        insert(object, "response_id", json!("resp_1"));
        insert(object, "item_id", json!("item_1"));
        insert(object, "output_index", json!(0));
    }

    fn response_location_fields(object: &mut Map<String, Value>) {
        response_item_fields(object);
        insert(object, "content_index", json!(0));
    }

    #[test]
    fn event_unions_match_the_pinned_discriminator_manifest() {
        assert_eq!(
            manifest_branches("RealtimeClientEvent"),
            REALTIME_CLIENT_EVENT_BRANCHES
        );
        assert_eq!(
            manifest_branches("RealtimeServerEvent"),
            REALTIME_SERVER_EVENT_BRANCHES
        );
        assert_eq!(REALTIME_CLIENT_EVENT_BRANCHES.len(), 11);
        assert_eq!(REALTIME_SERVER_EVENT_BRANCHES.len(), 46);
        assert_eq!(REALTIME_CLIENT_EVENT_TAGS.len(), 11);
        assert_eq!(REALTIME_SERVER_EVENT_TAGS.len(), 46);

        let mut expected_client_tags: Vec<String> = REALTIME_CLIENT_EVENT_TAGS
            .iter()
            .map(|tag| (*tag).to_owned())
            .collect();
        expected_client_tags.sort();
        assert_eq!(
            openapi_tags(REALTIME_CLIENT_EVENT_BRANCHES),
            expected_client_tags
        );

        let mut expected_server_tags: Vec<String> = REALTIME_SERVER_EVENT_TAGS
            .iter()
            .map(|tag| (*tag).to_owned())
            .collect();
        expected_server_tags.sort();
        assert_eq!(
            openapi_tags(REALTIME_SERVER_EVENT_BRANCHES),
            expected_server_tags
        );
    }

    #[test]
    fn every_client_event_branch_decodes_strictly() {
        for tag in REALTIME_CLIENT_EVENT_TAGS {
            let fixture = client_fixture(tag);
            let decoded: RealtimeClientEvent =
                serde_json::from_value(fixture.clone()).expect("known client event decodes");
            assert_eq!(decoded.event_type(), *tag);
            assert!(!matches!(decoded, RealtimeClientEvent::Unknown(_)));
            assert_eq!(serde_json::to_value(decoded).expect("encode"), fixture);
        }

        for tag in [
            "conversation.item.create",
            "conversation.item.delete",
            "conversation.item.retrieve",
            "conversation.item.truncate",
            "input_audio_buffer.append",
            "session.update",
        ] {
            assert!(
                serde_json::from_value::<RealtimeClientEvent>(json!({"type": tag})).is_err(),
                "known malformed client tag must fail: {tag}"
            );
        }
    }

    #[test]
    fn every_server_event_branch_decodes_and_known_malformed_never_becomes_unknown() {
        for tag in REALTIME_SERVER_EVENT_TAGS {
            let fixture = server_fixture(tag);
            let decoded: RealtimeServerEvent =
                serde_json::from_value(fixture.clone()).expect("known server event decodes");
            assert_eq!(decoded.event_type(), *tag);
            assert!(!matches!(decoded, RealtimeServerEvent::Unknown(_)));
            assert_eq!(serde_json::to_value(decoded).expect("encode"), fixture);
            assert!(
                serde_json::from_value::<RealtimeServerEvent>(json!({"type": tag})).is_err(),
                "known malformed server tag must fail: {tag}"
            );
        }
    }

    #[test]
    fn unknown_event_retains_its_complete_object() {
        let fixture = json!({
            "type": "future.realtime.event",
            "event_id": "event_future",
            "nested": {"secret": false},
            "array": [1, 2, 3]
        });
        let decoded: RealtimeServerEvent =
            serde_json::from_value(fixture.clone()).expect("unknown event decodes");
        let RealtimeServerEvent::Unknown(unknown) = &decoded else {
            panic!("future tag must remain unknown");
        };
        assert_eq!(unknown.discriminator(), "future.realtime.event");
        assert_eq!(serde_json::to_value(decoded).expect("encode"), fixture);
    }

    #[test]
    fn known_event_retains_future_fields() {
        let fixture = json!({
            "event_id": "event_1",
            "type": "input_audio_buffer.cleared",
            "future": {"value": 7}
        });
        let decoded: RealtimeServerEvent =
            serde_json::from_value(fixture.clone()).expect("known event decodes");
        let RealtimeServerEvent::InputAudioBufferCleared(event) = &decoded else {
            panic!("known event routed to the wrong variant");
        };
        assert!(event.extra_fields().contains_key("future"));
        assert_eq!(serde_json::to_value(decoded).expect("encode"), fixture);
    }

    #[test]
    fn base64_audio_is_typed_and_debug_does_not_dump_bytes() {
        let audio: RealtimeAudio =
            serde_json::from_value(json!("AQID")).expect("valid base64 audio decodes");
        assert_eq!(audio.as_bytes(), &[1, 2, 3]);
        assert_eq!(serde_json::to_value(&audio).expect("encode"), json!("AQID"));
        assert_eq!(format!("{audio:?}"), "RealtimeAudio { byte_len: 3 }");
        assert!(serde_json::from_value::<RealtimeAudio>(json!("%%%invalid")).is_err());
    }

    #[test]
    fn session_update_preserves_missing_null_value_and_extra_fields() {
        for fixture in [
            json!({"type": "realtime", "future": 1}),
            json!({
                "type": "realtime",
                "audio": {"input": {"turn_detection": null}}
            }),
            json!({
                "type": "realtime",
                "audio": {"input": {"turn_detection": {"type": "server_vad"}}}
            }),
            json!({"type": "realtime", "tracing": null}),
            json!({"type": "realtime", "tracing": "auto"}),
            json!({
                "type": "realtime",
                "tracing": {"workflow_name": "voice-agent", "future": true}
            }),
        ] {
            let decoded: RealtimeSessionCreateRequest =
                serde_json::from_value(fixture.clone()).expect("session decodes");
            assert_eq!(serde_json::to_value(decoded).expect("encode"), fixture);
        }

        let response_fixture = json!({
            "conversation_id": null,
            "max_output_tokens": null,
            "future": "retained"
        });
        let response: RealtimeResponse = serde_json::from_value(response_fixture.clone())
            .expect("nullable response fields decode");
        assert_eq!(
            serde_json::to_value(response).expect("encode response"),
            response_fixture
        );
    }

    #[test]
    fn conversation_item_uses_type_then_role_and_rejects_known_malformed_payloads() {
        let user = json!({"type": "message", "role": "user", "content": []});
        let decoded: RealtimeConversationItem =
            serde_json::from_value(user.clone()).expect("user message decodes");
        assert!(matches!(decoded, RealtimeConversationItem::UserMessage(_)));
        assert_eq!(serde_json::to_value(decoded).expect("encode"), user);

        assert!(
            serde_json::from_value::<RealtimeConversationItem>(
                json!({"type": "message", "role": "user"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RealtimeConversationItem>(
                json!({"type": "message", "role": "future", "content": []})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RealtimeConversationItem>(json!({
                "type": "mcp_call",
                "id": "mcp_1",
                "server_label": "server",
                "name": "tool",
                "arguments": "{}",
                "error": {"type": "protocol_error"}
            }))
            .is_err()
        );
    }

    #[test]
    fn client_secret_call_and_sip_dtos_round_trip_without_exposing_secret_debug() {
        let secret_fixture = json!({
            "value": "ek_test_secret",
            "expires_at": 123,
            "session": {
                "type": "realtime",
                "id": "sess_1",
                "object": "realtime.session"
            }
        });
        let secret: RealtimeCreateClientSecretResponse =
            serde_json::from_value(secret_fixture.clone()).expect("secret response decodes");
        assert!(!format!("{secret:?}").contains("ek_test_secret"));
        assert_eq!(
            serde_json::to_value(secret).expect("encode"),
            secret_fixture
        );

        let call = RealtimeCallCreateRequest::new("v=0\r\n")
            .with_session(RealtimeSessionCreateRequest::default());
        assert_eq!(
            serde_json::to_value(call).expect("encode call"),
            json!({"sdp": "v=0\r\n", "session": {"type": "realtime"}})
        );

        let refer = RealtimeCallReferRequest::parse("sip:agent@example.com")
            .expect("absolute SIP URI");
        assert_eq!(
            serde_json::to_value(refer).expect("encode refer"),
            json!({"target_uri": "sip:agent@example.com"})
        );
        assert_eq!(
            serde_json::to_value(RealtimeCallRejectRequest::new().with_status_code(486))
                .expect("encode reject"),
            json!({"status_code": 486})
        );

        let webhook = json!({
            "created_at": 123,
            "id": "evt_1",
            "data": {
                "call_id": "rtc_1",
                "sip_headers": [{"name": "From", "value": "sip:user@example.com"}]
            },
            "object": "event",
            "type": "realtime.call.incoming"
        });
        let decoded: WebhookRealtimeCallIncoming =
            serde_json::from_value(webhook.clone()).expect("webhook decodes");
        assert_eq!(serde_json::to_value(decoded).expect("encode"), webhook);
    }

    #[test]
    fn translation_session_preserves_nullability_extras_and_secret_privacy() {
        assert!(RealtimeTranslationClientSecretExpiration::new(9).is_err());
        assert_eq!(
            RealtimeTranslationClientSecretExpiration::new(10)
                .expect("minimum lifetime")
                .seconds(),
            Some(10)
        );
        assert!(RealtimeTranslationClientSecretExpiration::new(7_200).is_ok());
        assert!(RealtimeTranslationClientSecretExpiration::new(7_201).is_err());
        assert!(
            serde_json::from_value::<RealtimeTranslationClientSecretExpiration>(
                json!({"seconds": 9})
            )
            .is_err()
        );

        let input = RealtimeTranslationAudioInput::default()
            .with_transcription(RealtimeTranslationTranscription::new(
                "gpt-realtime-whisper",
            ))
            .with_noise_reduction_null();
        let session = RealtimeTranslationSessionCreateRequest::new("gpt-realtime-translate")
            .with_audio(
                RealtimeTranslationAudio::default()
                    .with_input(input)
                    .with_output(RealtimeTranslationAudioOutput::new("es")),
            );
        let request = RealtimeTranslationClientSecretCreateRequest::new(session)
            .with_expires_after(
                RealtimeTranslationClientSecretExpiration::new(600)
                    .expect("translation lifetime"),
            );
        assert_eq!(
            serde_json::to_value(request).expect("encode translation request"),
            json!({
                "expires_after": {"anchor": "created_at", "seconds": 600},
                "session": {
                    "model": "gpt-realtime-translate",
                    "audio": {
                        "input": {
                            "transcription": {"model": "gpt-realtime-whisper"},
                            "noise_reduction": null
                        },
                        "output": {"language": "es"}
                    }
                }
            })
        );

        let fixture = json!({
            "value": "ek_translation_private",
            "expires_at": 1_756_310_470_i64,
            "session": {
                "id": "sess_translation",
                "type": "translation",
                "expires_at": 1_756_310_470_i64,
                "model": "gpt-realtime-translate",
                "audio": {
                    "input": {
                        "transcription": null,
                        "noise_reduction": null,
                        "future_input": true
                    },
                    "output": {"language": "es"}
                },
                "future_session": {"enabled": true}
            },
            "future_response": 1
        });
        let response: RealtimeTranslationClientSecretCreateResponse =
            serde_json::from_value(fixture.clone()).expect("translation response decodes");
        assert!(!format!("{response:?}").contains("ek_translation_private"));
        assert_eq!(
            response.extra_fields().get("future_response"),
            Some(&json!(1))
        );
        assert_eq!(
            response.session.extra_fields().get("future_session"),
            Some(&json!({"enabled": true}))
        );
        assert_eq!(serde_json::to_value(response).expect("roundtrip"), fixture);

        assert!(
            serde_json::from_value::<RealtimeTranslationSession>(json!({
                "id": "sess_1",
                "type": "realtime",
                "expires_at": 1,
                "model": "gpt-realtime-translate",
                "audio": {}
            }))
            .is_err()
        );
    }
}
