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
use crate::responses::McpTool;
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

/// Reference to a custom voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCustomVoice {
    pub id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// A built-in voice name or custom voice reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RealtimeVoice {
    BuiltIn(RealtimeVoiceName),
    Custom(RealtimeCustomVoice),
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
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
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

/// Input-only audio configuration for a transcription session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeTranscriptionAudio {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<RealtimeAudioInputConfig>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Output-only audio configuration for one Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseAudioConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output: Omittable<RealtimeAudioOutputConfig>,
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

/// Force one MCP server or tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeMcpToolChoice {
    #[serde(rename = "type")]
    kind: RealtimeMcpChoiceTag,
    pub server_label: String,
    pub name: Nullable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
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

literal_tag!(RealtimeSessionRequestTag, Realtime, "realtime");
literal_tag!(RealtimeTranscriptionRequestTag, Transcription, "transcription");

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
    pub tracing: Omittable<Value>,
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
    pub prompt: Omittable<Value>,
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
    Realtime(RealtimeSessionCreateRequest),
    Transcription(RealtimeTranscriptionSessionCreateRequest),
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
                .map(Self::Realtime)
                .map_err(D::Error::custom),
            "transcription" => serde_json::from_value(value)
                .map(Self::Transcription)
                .map_err(D::Error::custom),
            _ => UnknownRealtimeObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

literal_tag!(RealtimeSessionObjectTag, Session, "realtime.session");

/// Effective GA Realtime session returned by the server.
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
    pub audio: Omittable<RealtimeSessionAudio>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tracing: Omittable<Nullable<Value>>,
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
    pub prompt: Omittable<Value>,
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
    Realtime(RealtimeSession),
    Transcription(RealtimeTranscriptionSession),
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
                .map(Self::Realtime)
                .map_err(D::Error::custom),
            "transcription" => serde_json::from_value(value)
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
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<RealtimeSystemContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// One user-message content part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeUserContentPart {
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
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

/// One assistant-message content part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeAssistantContentPart {
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
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

macro_rules! message_item_common {
    () => {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub id: Omittable<String>,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub object: Omittable<String>,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub status: Omittable<RealtimeItemStatus>,
    };
}

/// System message in a Realtime conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationItemMessageSystem {
    message_item_common!();
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
    message_item_common!();
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
    message_item_common!();
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
    pub error: Omittable<Nullable<Value>>,
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
                    _ => Err(D::Error::custom("unknown role for known Realtime message tag")),
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
    pub audio: Omittable<RealtimeResponseAudioConfig>,
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
    pub prompt: Omittable<Value>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input: Omittable<Vec<RealtimeConversationItem>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Error details attached to a failed Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseFailure {
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub code: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Additional status details for a Realtime response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeResponseStatusDetails {
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<RealtimeResponseStatus>,
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
    #[serde(rename = "object", default, skip_serializing_if = "Omittable::is_omitted")]
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
    pub audio: Omittable<RealtimeResponseAudioConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<RealtimeResponseUsage>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub conversation_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_output_tokens: Omittable<RealtimeMaxOutputTokens>,
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
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
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
    #[serde(rename = "type", default, skip_serializing_if = "Omittable::is_omitted")]
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
    pub anchor: Omittable<String>,
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
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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

/// Session Description Protocol text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RealtimeSdp(pub String);

/// Multipart request for creating a WebRTC Realtime call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallCreateRequest {
    pub sdp: RealtimeSdp,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub session: Omittable<RealtimeSessionCreateRequest>,
    #[serde(flatten)]
    extra: ExtraFields,
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

/// Request to reject an incoming SIP call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallRejectRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status_code: Omittable<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
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
            $($variant($ty),)+
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
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => UnknownRealtimeObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
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
