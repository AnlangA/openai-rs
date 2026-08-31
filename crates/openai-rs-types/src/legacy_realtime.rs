//! Deprecated, pre-GA Realtime session-token wire models.
//!
//! These endpoints use the historical flat session shape. They intentionally
//! do not alias the GA nested `audio` session types. Only leaf values whose
//! JSON representation is identical are shared with [`crate::realtime`].
//!
//! Numeric ranges follow the crate-wide opt-in policy (D0015/D0017/D0153):
//! request constructors reject out-of-range values, Serde decode stays a
//! lossless pass-through, and the request-level `validate` hooks re-check
//! values that entered through Serde before they are sent.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    ExtraFields, ModelId, Nullable, Omittable, WireSecret, open_string_enum,
    realtime::{
        RealtimeAudioTranscription, RealtimeClientSecretExpirationAnchor, RealtimeNoiseReduction,
        RealtimeOutputModality, RealtimeTracing, RealtimeTruncation, RealtimeVadEagerness,
    },
};

open_string_enum! {
    /// Historical flat audio-format string.
    pub enum LegacyRealtimeAudioFormat {
        Pcm16 = "pcm16",
        G711Ulaw = "g711_ulaw",
        G711Alaw = "g711_alaw",
    }
}

open_string_enum! {
    /// Historical string-only tool-choice mode.
    pub enum LegacyRealtimeToolChoice {
        Auto = "auto",
        None = "none",
        Required = "required",
    }
}

open_string_enum! {
    /// Object discriminator returned for a legacy Realtime session.
    pub enum LegacyRealtimeSessionObjectType {
        Session = "realtime.session",
    }
}

open_string_enum! {
    /// Object discriminator returned for a legacy transcription session.
    pub enum LegacyRealtimeTranscriptionSessionObjectType {
        TranscriptionSession = "realtime.transcription_session",
    }
}

open_string_enum! {
    /// Optional discriminator in the permissive legacy VAD object.
    pub enum LegacyRealtimeTurnDetectionType {
        ServerVad = "server_vad",
        SemanticVad = "semantic_vad",
    }
}

open_string_enum! {
    /// Built-in voice names accepted by the legacy flat session shape.
    pub enum LegacyRealtimeVoiceName {
        Alloy = "alloy",
        Ash = "ash",
        Ballad = "ballad",
        Coral = "coral",
        Echo = "echo",
        Sage = "sage",
        Shimmer = "shimmer",
        Verse = "verse",
        Marin = "marin",
        Cedar = "cedar",
    }
}

open_string_enum! {
    /// Optional function-tool discriminator in the legacy schema.
    pub enum LegacyRealtimeFunctionToolType {
        Function = "function",
    }
}

/// Strict custom-voice reference accepted by the pinned legacy schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeCustomVoice {
    id: String,
}

impl LegacyRealtimeCustomVoice {
    /// Creates a custom-voice reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    /// Returns the custom voice id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Built-in or custom voice in the historical flat field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LegacyRealtimeVoice {
    /// A built-in or future string voice name.
    BuiltIn(LegacyRealtimeVoiceName),
    /// A strict `{ "id": ... }` custom voice reference.
    Custom(LegacyRealtimeCustomVoice),
}

impl From<LegacyRealtimeVoiceName> for LegacyRealtimeVoice {
    fn from(value: LegacyRealtimeVoiceName) -> Self {
        Self::BuiltIn(value)
    }
}

impl From<LegacyRealtimeCustomVoice> for LegacyRealtimeVoice {
    fn from(value: LegacyRealtimeCustomVoice) -> Self {
        Self::Custom(value)
    }
}

/// Permissive legacy VAD object whose discriminator is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyRealtimeTurnDetection {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<LegacyRealtimeTurnDetectionType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    threshold: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prefix_padding_ms: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    silence_duration_ms: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    create_response: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    interrupt_response: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    eagerness: Omittable<RealtimeVadEagerness>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeTurnDetection {
    /// Creates a server-VAD object.
    #[must_use]
    pub fn server_vad() -> Self {
        Self {
            kind: Omittable::Value(LegacyRealtimeTurnDetectionType::ServerVad),
            ..Self::default()
        }
    }

    /// Creates a semantic-VAD object.
    #[must_use]
    pub fn semantic_vad() -> Self {
        Self {
            kind: Omittable::Value(LegacyRealtimeTurnDetectionType::SemanticVad),
            ..Self::default()
        }
    }

    /// Sets the activation threshold.
    #[must_use]
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = Omittable::Value(threshold);
        self
    }

    /// Sets prefix padding in milliseconds.
    #[must_use]
    pub fn with_prefix_padding_ms(mut self, milliseconds: i64) -> Self {
        self.prefix_padding_ms = Omittable::Value(milliseconds);
        self
    }

    /// Sets silence duration in milliseconds.
    #[must_use]
    pub fn with_silence_duration_ms(mut self, milliseconds: i64) -> Self {
        self.silence_duration_ms = Omittable::Value(milliseconds);
        self
    }

    /// Returns future fields retained from a response.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Legacy function-tool object; every pinned property is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyRealtimeFunctionTool {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<LegacyRealtimeFunctionToolType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parameters: Omittable<BTreeMap<String, serde_json::Value>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeFunctionTool {
    /// Creates a named function tool with the historical optional shape.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: Omittable::Value(LegacyRealtimeFunctionToolType::Function),
            name: Omittable::Value(name.into()),
            ..Self::default()
        }
    }

    /// Sets a description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Sets a JSON Schema object.
    #[must_use]
    pub fn with_parameters(mut self, parameters: BTreeMap<String, serde_json::Value>) -> Self {
        self.parameters = Omittable::Value(parameters);
        self
    }

    /// Returns future fields retained from a response.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Legacy prompt reference preserving nullable version and variables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LegacyRealtimePromptReference {
    id: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    version: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    variables: Omittable<Nullable<BTreeMap<String, serde_json::Value>>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimePromptReference {
    /// Creates a prompt reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: Omittable::Omitted,
            variables: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Pins a prompt version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Omittable::Value(Nullable::Value(version.into()));
        self
    }

    /// Sends an explicit null version.
    #[must_use]
    pub fn with_version_null(mut self) -> Self {
        self.version = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns future fields retained from a response.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Validation failure for a legacy Realtime session request value.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum LegacyRealtimeValidationError {
    /// Ephemeral secret lifetimes are limited to 10 seconds through two hours.
    #[error("legacy Realtime client-secret lifetime must be 10..=7200 seconds, got {seconds}")]
    InvalidSecretLifetime {
        /// Rejected duration.
        seconds: i64,
    },
    /// Speech speed must remain within the historical service bounds.
    #[error("legacy Realtime speed must be finite and within 0.25..=1.5, got {value}")]
    InvalidSpeed {
        /// Rejected value rendered without retaining a floating-point error field.
        value: String,
    },
    /// Sampling temperature must remain within the historical service bounds.
    #[error("legacy Realtime temperature must be finite and within 0.6..=1.2, got {value}")]
    InvalidTemperature {
        /// Rejected value rendered without retaining a floating-point error field.
        value: String,
    },
    /// The historical token limit accepts 1 through 4096.
    #[error("legacy Realtime max response output tokens must be 1..=4096, got {tokens}")]
    InvalidMaxResponseOutputTokens {
        /// Rejected token count.
        tokens: i64,
    },
}

fn validate_speed(value: f64) -> Result<(), LegacyRealtimeValidationError> {
    if value.is_finite() && (0.25..=1.5).contains(&value) {
        Ok(())
    } else {
        Err(LegacyRealtimeValidationError::InvalidSpeed {
            value: value.to_string(),
        })
    }
}

fn validate_temperature(value: f64) -> Result<(), LegacyRealtimeValidationError> {
    if value.is_finite() && (0.6..=1.2).contains(&value) {
        Ok(())
    } else {
        Err(LegacyRealtimeValidationError::InvalidTemperature {
            value: value.to_string(),
        })
    }
}

fn validate_max_response_output_tokens(tokens: i64) -> Result<(), LegacyRealtimeValidationError> {
    if (1..=4096).contains(&tokens) {
        Ok(())
    } else {
        Err(LegacyRealtimeValidationError::InvalidMaxResponseOutputTokens { tokens })
    }
}

fn validate_secret_lifetime(seconds: i64) -> Result<(), LegacyRealtimeValidationError> {
    if (10..=7200).contains(&seconds) {
        Ok(())
    } else {
        Err(LegacyRealtimeValidationError::InvalidSecretLifetime { seconds })
    }
}

/// Spoken-response speed for a legacy session.
///
/// Construction enforces the pinned `0.25..=1.5` range; Serde decode is a
/// lossless pass-through, and decoded values are re-checked by
/// [`LegacyRealtimeSessionCreateRequest::validate`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LegacyRealtimeSpeed(f64);

impl LegacyRealtimeSpeed {
    /// Creates a finite speed within `0.25..=1.5`.
    pub fn new(value: f64) -> Result<Self, LegacyRealtimeValidationError> {
        validate_speed(value)?;
        Ok(Self(value))
    }

    /// Returns the speed.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Sampling temperature for a legacy session.
///
/// Construction enforces the documented `0.6..=1.2` range; Serde decode is a
/// lossless pass-through, and decoded values are re-checked by
/// [`LegacyRealtimeSessionCreateRequest::validate`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LegacyRealtimeTemperature(f64);

impl LegacyRealtimeTemperature {
    /// Creates a finite temperature within `0.6..=1.2`.
    pub fn new(value: f64) -> Result<Self, LegacyRealtimeValidationError> {
        validate_temperature(value)?;
        Ok(Self(value))
    }

    /// Returns the temperature.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Integer output-token limit or the wire string `"inf"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LegacyRealtimeMaxResponseOutputTokens {
    /// A finite token limit; construction enforces the documented `1..=4096`.
    Limited(i64),
    /// The historical `"inf"` service default.
    Unlimited,
}

impl LegacyRealtimeMaxResponseOutputTokens {
    /// Creates a finite token limit in `1..=4096`.
    pub fn limited(tokens: i64) -> Result<Self, LegacyRealtimeValidationError> {
        validate_max_response_output_tokens(tokens)?;
        Ok(Self::Limited(tokens))
    }

    /// Returns the finite limit, or `None` for `"inf"`.
    #[must_use]
    pub const fn finite(self) -> Option<i64> {
        match self {
            Self::Limited(tokens) => Some(tokens),
            Self::Unlimited => None,
        }
    }
}

impl Serialize for LegacyRealtimeMaxResponseOutputTokens {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Limited(tokens) => serializer.serialize_i64(*tokens),
            Self::Unlimited => serializer.serialize_str("inf"),
        }
    }
}

impl<'de> Deserialize<'de> for LegacyRealtimeMaxResponseOutputTokens {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(number) => number
                .as_i64()
                .map(Self::Limited)
                .ok_or_else(|| D::Error::custom("legacy Realtime token limit must be an integer")),
            serde_json::Value::String(value) if value == "inf" => Ok(Self::Unlimited),
            _ => Err(D::Error::custom(
                "legacy Realtime token limit must be an integer or `inf`",
            )),
        }
    }
}

/// Expiration policy nested inside legacy client-secret options.
///
/// Construction enforces the documented `10..=7200`-second lifetime; Serde
/// decode is a lossless pass-through, and decoded values are re-checked by the
/// legacy request `validate` hooks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeSecretExpiration {
    anchor: RealtimeClientSecretExpirationAnchor,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    seconds: Omittable<i64>,
}

impl LegacyRealtimeSecretExpiration {
    /// Creates a `created_at`-anchored lifetime in `10..=7200` seconds.
    pub fn new(seconds: i64) -> Result<Self, LegacyRealtimeValidationError> {
        validate_secret_lifetime(seconds)?;
        Ok(Self {
            anchor: RealtimeClientSecretExpirationAnchor::CreatedAt,
            seconds: Omittable::Value(seconds),
        })
    }

    /// Returns the anchor.
    #[must_use]
    pub const fn anchor(&self) -> &RealtimeClientSecretExpirationAnchor {
        &self.anchor
    }

    /// Returns the selected lifetime when present.
    #[must_use]
    pub fn seconds(&self) -> Option<i64> {
        present(&self.seconds).copied()
    }
}

/// Client-secret options for `/realtime/sessions`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeClientSecretOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_after: Omittable<LegacyRealtimeSecretExpiration>,
}

impl LegacyRealtimeClientSecretOptions {
    /// Creates options with a validated secret lifetime.
    pub fn new(seconds: i64) -> Result<Self, LegacyRealtimeValidationError> {
        Ok(Self {
            expires_after: Omittable::Value(LegacyRealtimeSecretExpiration::new(seconds)?),
        })
    }

    /// Returns the exact expiration presence state.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<LegacyRealtimeSecretExpiration> {
        &self.expires_after
    }
}

/// Client-secret options for `/realtime/transcription_sessions`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeTranscriptionClientSecretOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_at: Omittable<LegacyRealtimeSecretExpiration>,
}

impl LegacyRealtimeTranscriptionClientSecretOptions {
    /// Creates options with a validated secret lifetime.
    pub fn new(seconds: i64) -> Result<Self, LegacyRealtimeValidationError> {
        Ok(Self {
            expires_at: Omittable::Value(LegacyRealtimeSecretExpiration::new(seconds)?),
        })
    }

    /// Returns the exact expiration presence state.
    #[must_use]
    pub const fn expires_at(&self) -> &Omittable<LegacyRealtimeSecretExpiration> {
        &self.expires_at
    }
}

/// Deprecated flat request body for `POST /realtime/sessions`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    client_secret: Omittable<LegacyRealtimeClientSecretOptions>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<ModelId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    voice: Omittable<LegacyRealtimeVoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_noise_reduction: Omittable<Nullable<RealtimeNoiseReduction>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_transcription: Omittable<Nullable<RealtimeAudioTranscription>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    speed: Omittable<LegacyRealtimeSpeed>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tracing: Omittable<Nullable<RealtimeTracing>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    turn_detection: Omittable<Nullable<LegacyRealtimeTurnDetection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<LegacyRealtimeFunctionTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<LegacyRealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<LegacyRealtimeTemperature>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_response_output_tokens: Omittable<LegacyRealtimeMaxResponseOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<RealtimeTruncation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<LegacyRealtimePromptReference>>,
}

impl LegacyRealtimeSessionCreateRequest {
    /// Creates an empty request that uses service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks the documented legacy value ranges without sending the request.
    ///
    /// Serde decode is a lossless pass-through, so values that entered the
    /// request through Serde are re-checked here: the client-secret lifetime,
    /// `speed`, `temperature`, and `max_response_output_tokens`. Builder
    /// construction already rejects out-of-range values at the leaf types.
    pub fn validate(&self) -> Result<(), LegacyRealtimeValidationError> {
        if let Omittable::Value(options) = &self.client_secret
            && let Omittable::Value(expiration) = &options.expires_after
            && let Some(seconds) = expiration.seconds()
        {
            validate_secret_lifetime(seconds)?;
        }
        if let Omittable::Value(speed) = self.speed {
            validate_speed(speed.get())?;
        }
        if let Omittable::Value(temperature) = self.temperature {
            validate_temperature(temperature.get())?;
        }
        if let Omittable::Value(tokens) = &self.max_response_output_tokens
            && let Some(tokens) = tokens.finite()
        {
            validate_max_response_output_tokens(tokens)?;
        }
        Ok(())
    }

    /// Selects the legacy Realtime model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<ModelId>) -> Self {
        self.model = Omittable::Value(model.into());
        self
    }

    /// Selects output modalities.
    #[must_use]
    pub fn with_modalities(mut self, modalities: Vec<RealtimeOutputModality>) -> Self {
        self.modalities = Omittable::Value(modalities);
        self
    }

    /// Sets system instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(instructions.into());
        self
    }

    /// Sets client-secret expiration options.
    #[must_use]
    pub fn with_client_secret(mut self, options: LegacyRealtimeClientSecretOptions) -> Self {
        self.client_secret = Omittable::Value(options);
        self
    }

    /// Sets the historical flat input audio format.
    #[must_use]
    pub fn with_input_audio_format(mut self, format: LegacyRealtimeAudioFormat) -> Self {
        self.input_audio_format = Omittable::Value(format);
        self
    }

    /// Sets the historical flat output audio format.
    #[must_use]
    pub fn with_output_audio_format(mut self, format: LegacyRealtimeAudioFormat) -> Self {
        self.output_audio_format = Omittable::Value(format);
        self
    }

    /// Sets the voice.
    #[must_use]
    pub fn with_voice(mut self, voice: impl Into<LegacyRealtimeVoice>) -> Self {
        self.voice = Omittable::Value(voice.into());
        self
    }

    /// Enables input transcription.
    #[must_use]
    pub fn with_input_audio_transcription(mut self, value: RealtimeAudioTranscription) -> Self {
        self.input_audio_transcription = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Explicitly disables input transcription.
    #[must_use]
    pub fn with_input_audio_transcription_null(mut self) -> Self {
        self.input_audio_transcription = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets input noise reduction.
    #[must_use]
    pub fn with_input_audio_noise_reduction(mut self, value: RealtimeNoiseReduction) -> Self {
        self.input_audio_noise_reduction = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Explicitly disables input noise reduction.
    #[must_use]
    pub fn with_input_audio_noise_reduction_null(mut self) -> Self {
        self.input_audio_noise_reduction = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets validated speech speed.
    #[must_use]
    pub fn with_speed(mut self, speed: LegacyRealtimeSpeed) -> Self {
        self.speed = Omittable::Value(speed);
        self
    }

    /// Sets tracing configuration.
    #[must_use]
    pub fn with_tracing(mut self, tracing: RealtimeTracing) -> Self {
        self.tracing = Omittable::Value(Nullable::Value(tracing));
        self
    }

    /// Explicitly disables tracing.
    #[must_use]
    pub fn with_tracing_null(mut self) -> Self {
        self.tracing = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets turn detection.
    #[must_use]
    pub fn with_turn_detection(mut self, turn_detection: LegacyRealtimeTurnDetection) -> Self {
        self.turn_detection = Omittable::Value(Nullable::Value(turn_detection));
        self
    }

    /// Explicitly disables turn detection.
    #[must_use]
    pub fn with_turn_detection_null(mut self) -> Self {
        self.turn_detection = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets available function tools.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<LegacyRealtimeFunctionTool>) -> Self {
        self.tools = Omittable::Value(tools);
        self
    }

    /// Sets the string-only legacy tool choice.
    #[must_use]
    pub fn with_tool_choice(mut self, tool_choice: LegacyRealtimeToolChoice) -> Self {
        self.tool_choice = Omittable::Value(tool_choice);
        self
    }

    /// Sets validated sampling temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: LegacyRealtimeTemperature) -> Self {
        self.temperature = Omittable::Value(temperature);
        self
    }

    /// Sets the maximum response output tokens.
    #[must_use]
    pub fn with_max_response_output_tokens(
        mut self,
        tokens: LegacyRealtimeMaxResponseOutputTokens,
    ) -> Self {
        self.max_response_output_tokens = Omittable::Value(tokens);
        self
    }

    /// Sets the conversation truncation policy.
    #[must_use]
    pub fn with_truncation(mut self, truncation: RealtimeTruncation) -> Self {
        self.truncation = Omittable::Value(truncation);
        self
    }

    /// Sets a reusable prompt template reference.
    #[must_use]
    pub fn with_prompt(mut self, prompt: LegacyRealtimePromptReference) -> Self {
        self.prompt = Omittable::Value(Nullable::Value(prompt));
        self
    }

    /// Sends official `prompt: null`.
    #[must_use]
    pub fn with_prompt_null(mut self) -> Self {
        self.prompt = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the exact model presence state.
    #[must_use]
    pub const fn model(&self) -> &Omittable<ModelId> {
        &self.model
    }
}

/// Deprecated flat request body for transcription-session tokens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRealtimeTranscriptionSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    client_secret: Omittable<LegacyRealtimeTranscriptionClientSecretOptions>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_noise_reduction: Omittable<Nullable<RealtimeNoiseReduction>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_transcription: Omittable<RealtimeAudioTranscription>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    turn_detection: Omittable<Nullable<LegacyRealtimeTurnDetection>>,
}

impl LegacyRealtimeTranscriptionSessionCreateRequest {
    /// Creates an empty request that uses transcription defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks the documented legacy secret lifetime without sending the
    /// request.
    ///
    /// Serde decode is a lossless pass-through, so a
    /// `client_secret.expires_at.seconds` value that entered the request
    /// through Serde is re-checked here; builder construction already rejects
    /// out-of-range lifetimes at the leaf type.
    pub fn validate(&self) -> Result<(), LegacyRealtimeValidationError> {
        if let Omittable::Value(options) = &self.client_secret
            && let Omittable::Value(expiration) = &options.expires_at
            && let Some(seconds) = expiration.seconds()
        {
            validate_secret_lifetime(seconds)?;
        }
        Ok(())
    }

    /// Sets client-secret expiration options.
    #[must_use]
    pub fn with_client_secret(
        mut self,
        options: LegacyRealtimeTranscriptionClientSecretOptions,
    ) -> Self {
        self.client_secret = Omittable::Value(options);
        self
    }

    /// Adds one include selector.
    #[must_use]
    pub fn include(mut self, include: impl Into<String>) -> Self {
        let mut values = match std::mem::take(&mut self.include) {
            Omittable::Value(values) => values,
            _ => Vec::new(),
        };
        values.push(include.into());
        self.include = Omittable::Value(values);
        self
    }

    /// Sets the historical flat input audio format.
    #[must_use]
    pub fn with_input_audio_format(mut self, format: LegacyRealtimeAudioFormat) -> Self {
        self.input_audio_format = Omittable::Value(format);
        self
    }

    /// Sets input noise reduction.
    #[must_use]
    pub fn with_input_audio_noise_reduction(mut self, value: RealtimeNoiseReduction) -> Self {
        self.input_audio_noise_reduction = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Explicitly disables input noise reduction.
    #[must_use]
    pub fn with_input_audio_noise_reduction_null(mut self) -> Self {
        self.input_audio_noise_reduction = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets input transcription.
    #[must_use]
    pub fn with_input_audio_transcription(mut self, value: RealtimeAudioTranscription) -> Self {
        self.input_audio_transcription = Omittable::Value(value);
        self
    }

    /// Sets the legacy modalities field retained by official SDKs.
    #[must_use]
    pub fn with_modalities(mut self, modalities: Vec<RealtimeOutputModality>) -> Self {
        self.modalities = Omittable::Value(modalities);
        self
    }

    /// Sets turn detection.
    #[must_use]
    pub fn with_turn_detection(mut self, turn_detection: LegacyRealtimeTurnDetection) -> Self {
        self.turn_detection = Omittable::Value(Nullable::Value(turn_detection));
        self
    }

    /// Explicitly disables turn detection.
    #[must_use]
    pub fn with_turn_detection_null(mut self) -> Self {
        self.turn_detection = Omittable::Value(Nullable::Null);
        self
    }
}

/// Ephemeral key returned by the deprecated session-token endpoints.
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyRealtimeClientSecret {
    value: WireSecret,
    expires_at: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeClientSecret {
    /// Exposes the secret only for the duration of a caller-supplied operation.
    pub fn with_exposed<R>(&self, operation: impl FnOnce(&str) -> R) -> R {
        self.value.with_exposed(operation)
    }

    /// Returns the Unix expiration time.
    #[must_use]
    pub const fn expires_at(&self) -> i64 {
        self.expires_at
    }

    /// Returns future response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for LegacyRealtimeClientSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyRealtimeClientSecret")
            .field("value", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("extra", &self.extra)
            .finish()
    }
}

/// Response from deprecated `POST /realtime/sessions`.
///
/// The historical flat shape always embeds `client_secret`; the newer nested
/// session shape drops it in favor of a top-level `expires_at`. The field is
/// therefore [`Omittable`] so the new shape decodes, with its undiscovered
/// fields retained in [`ExtraFields`].
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyRealtimeSessionCreateResponse {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    client_secret: Omittable<LegacyRealtimeClientSecret>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    object: Omittable<LegacyRealtimeSessionObjectType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<ModelId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    voice: Omittable<LegacyRealtimeVoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_transcription: Omittable<Nullable<RealtimeAudioTranscription>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    turn_detection: Omittable<Nullable<LegacyRealtimeTurnDetection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<LegacyRealtimeFunctionTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<LegacyRealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<LegacyRealtimeTemperature>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_response_output_tokens: Omittable<LegacyRealtimeMaxResponseOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    speed: Omittable<LegacyRealtimeSpeed>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tracing: Omittable<Nullable<RealtimeTracing>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<RealtimeTruncation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<LegacyRealtimePromptReference>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeSessionCreateResponse {
    /// Returns the exact client-secret presence state.
    ///
    /// The flat shape always embeds a secret; the newer nested session shape
    /// omits the field and surfaces `expires_at` at the top level instead
    /// (retained in [`Self::extra_fields`]).
    #[must_use]
    pub const fn client_secret(&self) -> &Omittable<LegacyRealtimeClientSecret> {
        &self.client_secret
    }

    /// Returns the optional session id.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        present(&self.id).map(String::as_str)
    }

    /// Returns the optional effective model.
    #[must_use]
    pub fn model(&self) -> Option<&ModelId> {
        present(&self.model)
    }

    /// Returns the optional modalities.
    #[must_use]
    pub fn modalities(&self) -> Option<&[RealtimeOutputModality]> {
        present(&self.modalities).map(Vec::as_slice)
    }

    /// Returns future response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for LegacyRealtimeSessionCreateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyRealtimeSessionCreateResponse")
            .field("client_secret", &"[REDACTED]")
            .field("id", &self.id())
            .field("model", &self.model())
            .field("result_field_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

/// Response from deprecated `POST /realtime/transcription_sessions`.
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyRealtimeTranscriptionSessionCreateResponse {
    client_secret: Nullable<LegacyRealtimeClientSecret>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    object: Omittable<LegacyRealtimeTranscriptionSessionObjectType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_at: Omittable<i64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    modalities: Omittable<Vec<RealtimeOutputModality>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_transcription: Omittable<RealtimeAudioTranscription>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    turn_detection: Omittable<Nullable<LegacyRealtimeTurnDetection>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeTranscriptionSessionCreateResponse {
    /// Returns the exact required-nullable client-secret state.
    #[must_use]
    pub const fn client_secret(&self) -> &Nullable<LegacyRealtimeClientSecret> {
        &self.client_secret
    }

    /// Returns the optional session id.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        present(&self.id).map(String::as_str)
    }

    /// Returns future response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for LegacyRealtimeTranscriptionSessionCreateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyRealtimeTranscriptionSessionCreateResponse")
            .field("client_secret", &"[REDACTED]")
            .field("id", &self.id())
            .field("result_field_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

fn present<T>(value: &Omittable<T>) -> Option<&T> {
    match value {
        Omittable::Value(value) => Some(value),
        Omittable::Omitted => None,
    }
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(LegacyRealtimeSessionCreateRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(LegacyRealtimeSessionCreateResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(LegacyRealtimeTranscriptionSessionCreateRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(LegacyRealtimeTranscriptionSessionCreateResponse: Serialize, DeserializeOwned, Send, Sync);

    #[test]
    fn empty_requests_and_flat_fields_round_trip() {
        let empty = serde_json::to_value(LegacyRealtimeTranscriptionSessionCreateRequest::new())
            .expect("encode empty transcription session request");
        assert_eq!(empty, json!({}));

        let request = LegacyRealtimeSessionCreateRequest::new()
            .with_model("gpt-realtime")
            .with_modalities(vec![
                RealtimeOutputModality::Audio,
                RealtimeOutputModality::Text,
            ])
            .with_instructions("friendly")
            .with_input_audio_format(LegacyRealtimeAudioFormat::Pcm16)
            .with_output_audio_format(LegacyRealtimeAudioFormat::G711Ulaw)
            .with_turn_detection_null()
            .with_speed(LegacyRealtimeSpeed::new(1.1).expect("valid speed"))
            .with_temperature(LegacyRealtimeTemperature::new(0.7).expect("valid temperature"))
            .with_max_response_output_tokens(
                LegacyRealtimeMaxResponseOutputTokens::limited(200).expect("valid output limit"),
            )
            .with_truncation(RealtimeTruncation::Mode(
                crate::realtime::RealtimeTruncationMode::Auto,
            ))
            .with_prompt_null();
        assert_eq!(
            serde_json::to_value(request).expect("encode legacy session request"),
            json!({
                "model": "gpt-realtime",
                "modalities": ["audio", "text"],
                "instructions": "friendly",
                "input_audio_format": "pcm16",
                "output_audio_format": "g711_ulaw",
                "turn_detection": null,
                "speed": 1.1,
                "temperature": 0.7,
                "max_response_output_tokens": 200,
                "truncation": "auto",
                "prompt": null
            })
        );

        let prompt_request = LegacyRealtimeSessionCreateRequest::new()
            .with_prompt(LegacyRealtimePromptReference::new("pmpt_1"));
        assert_eq!(
            serde_json::to_value(prompt_request).expect("encode prompt")["prompt"]["id"],
            "pmpt_1"
        );
        assert!(
            serde_json::from_value::<LegacyRealtimeSessionCreateRequest>(json!({
                "truncation": "auto",
                "prompt": null
            }))
            .is_ok(),
            "pinned RealtimeSessionCreateRequest includes truncation and prompt"
        );
    }

    #[test]
    fn response_secret_debug_is_redacted_and_json_round_trips() {
        let fixture = json!({
            "id": "sess_001",
            "object": "realtime.session",
            "model": "gpt-realtime",
            "modalities": ["audio", "text"],
            "client_secret": {
                "value": "ek_private_value",
                "expires_at": 1234567890
            },
            "future": true
        });
        let response: LegacyRealtimeSessionCreateResponse =
            serde_json::from_value(fixture.clone()).expect("decode legacy session response");
        assert_eq!(response.id(), Some("sess_001"));
        let secret = match response.client_secret() {
            Omittable::Value(secret) => secret,
            Omittable::Omitted => panic!("flat session shape embeds a client secret"),
        };
        assert_eq!(secret.with_exposed(ToOwned::to_owned), "ek_private_value");
        assert!(!format!("{response:?}").contains("ek_private_value"));
        assert!(!format!("{:?}", response.client_secret()).contains("ek_private_value"));
        assert_eq!(
            serde_json::to_value(response).expect("round-trip legacy session response"),
            fixture
        );
    }

    #[test]
    fn session_response_without_client_secret_decodes_new_nested_shape() {
        let fixture = json!({
            "id": "sess_001",
            "object": "realtime.session",
            "expires_at": 1_756_310_470_i64,
            "model": "gpt-realtime",
            "output_modalities": ["text"],
            "instructions": "friendly",
            "audio": {
                "input": { "format": "pcm16" },
                "output": { "format": "pcm16" }
            },
            "max_output_tokens": "inf",
            "include": ["item.input_audio_transcription.logprobs"]
        });
        let response: LegacyRealtimeSessionCreateResponse = serde_json::from_value(fixture.clone())
            .expect("decode new nested session shape without client_secret");
        assert!(response.client_secret().is_omitted());
        assert_eq!(response.id(), Some("sess_001"));
        for field in [
            "expires_at",
            "output_modalities",
            "audio",
            "max_output_tokens",
            "include",
        ] {
            assert!(
                response.extra_fields().get(field).is_some(),
                "new-shape field {field} should be retained in ExtraFields"
            );
        }
        assert!(!format!("{response:?}").contains("ek_"));
        assert_eq!(
            serde_json::to_value(response).expect("round-trip new nested session shape"),
            fixture
        );
    }

    #[test]
    fn out_of_range_numeric_values_decode_losslessly_and_round_trip() {
        let speed: LegacyRealtimeSpeed =
            serde_json::from_value(json!(0.24)).expect("decode out-of-range speed");
        assert_eq!(
            serde_json::to_value(speed).expect("re-encode speed"),
            json!(0.24)
        );

        let temperature: LegacyRealtimeTemperature =
            serde_json::from_value(json!(1.21)).expect("decode out-of-range temperature");
        assert_eq!(
            serde_json::to_value(temperature).expect("re-encode temperature"),
            json!(1.21)
        );

        let tokens: LegacyRealtimeMaxResponseOutputTokens =
            serde_json::from_value(json!(8192)).expect("decode out-of-range token limit");
        assert_eq!(tokens.finite(), Some(8192));
        assert_eq!(
            serde_json::to_value(tokens).expect("re-encode token limit"),
            json!(8192)
        );

        let expiration: LegacyRealtimeSecretExpiration =
            serde_json::from_value(json!({ "anchor": "created_at", "seconds": 7_201_i64 }))
                .expect("decode out-of-range secret lifetime");
        assert_eq!(expiration.seconds(), Some(7_201));
        assert_eq!(
            serde_json::to_value(expiration).expect("re-encode secret lifetime"),
            json!({ "anchor": "created_at", "seconds": 7_201_i64 })
        );

        let response_fixture = json!({
            "id": "sess_001",
            "object": "realtime.session",
            "client_secret": {
                "value": "ek_private_value",
                "expires_at": 1234567890
            },
            "speed": 2.0,
            "temperature": 0.5,
            "max_response_output_tokens": 8192
        });
        let response: LegacyRealtimeSessionCreateResponse =
            serde_json::from_value(response_fixture.clone())
                .expect("decode session with out-of-range echoes");
        assert_eq!(
            serde_json::to_value(response).expect("round-trip out-of-range echoes"),
            response_fixture
        );

        let request_fixture = json!({
            "client_secret": { "expires_after": { "anchor": "created_at", "seconds": 5_i64 } },
            "speed": 2.0,
            "temperature": 0.5,
            "max_response_output_tokens": 0
        });
        let request: LegacyRealtimeSessionCreateRequest =
            serde_json::from_value(request_fixture.clone())
                .expect("decode request with out-of-range values");
        assert_eq!(
            serde_json::to_value(request).expect("round-trip out-of-range request"),
            request_fixture
        );
    }

    #[test]
    fn request_validate_rejects_decoded_out_of_range_values() {
        let decode = |body: serde_json::Value| {
            serde_json::from_value::<LegacyRealtimeSessionCreateRequest>(body)
                .expect("lossless request decode")
        };

        assert_eq!(
            decode(json!({ "speed": 1.75 })).validate(),
            Err(LegacyRealtimeValidationError::InvalidSpeed {
                value: "1.75".to_owned()
            })
        );
        assert_eq!(
            decode(json!({ "temperature": 1.5 })).validate(),
            Err(LegacyRealtimeValidationError::InvalidTemperature {
                value: "1.5".to_owned()
            })
        );
        assert_eq!(
            decode(json!({ "max_response_output_tokens": 0 })).validate(),
            Err(LegacyRealtimeValidationError::InvalidMaxResponseOutputTokens { tokens: 0 })
        );
        assert_eq!(
            decode(json!({
                "client_secret": {
                    "expires_after": { "anchor": "created_at", "seconds": 7_201_i64 }
                }
            }))
            .validate(),
            Err(LegacyRealtimeValidationError::InvalidSecretLifetime { seconds: 7_201 })
        );

        let transcription: LegacyRealtimeTranscriptionSessionCreateRequest =
            serde_json::from_value(json!({
                "client_secret": {
                    "expires_at": { "anchor": "created_at", "seconds": 9_i64 }
                }
            }))
            .expect("lossless transcription request decode");
        assert_eq!(
            transcription.validate(),
            Err(LegacyRealtimeValidationError::InvalidSecretLifetime { seconds: 9 })
        );

        let in_range = decode(json!({
            "speed": 1.5,
            "temperature": 0.6,
            "max_response_output_tokens": 4096,
            "client_secret": {
                "expires_after": { "anchor": "created_at", "seconds": 7_200_i64 }
            }
        }));
        assert_eq!(in_range.validate(), Ok(()));
        assert_eq!(
            decode(json!({ "max_response_output_tokens": "inf" })).validate(),
            Ok(())
        );
        assert_eq!(
            LegacyRealtimeTranscriptionSessionCreateRequest::new().validate(),
            Ok(())
        );
    }

    #[test]
    fn transcription_response_accepts_documented_null_secret() {
        let fixture = json!({
            "id": "sess_transcription",
            "object": "realtime.transcription_session",
            "input_audio_format": "pcm16",
            "client_secret": null
        });
        let response: LegacyRealtimeTranscriptionSessionCreateResponse =
            serde_json::from_value(fixture.clone()).expect("decode transcription session");
        assert!(response.client_secret().is_null());
        assert!(!format!("{response:?}").contains("ek_"));
        assert_eq!(
            serde_json::to_value(response).expect("round-trip transcription session"),
            fixture
        );
    }

    #[test]
    fn official_legacy_transcription_session_language_null_decodes() {
        let fixture = json!({
            "id": "sess_BBwZc7cFV3XizEyKGDCGL",
            "object": "realtime.transcription_session",
            "expires_at": 1_742_188_264_i64,
            "modalities": ["audio", "text"],
            "input_audio_format": "pcm16",
            "input_audio_transcription": {
                "model": "gpt-4o-transcribe",
                "language": null,
                "prompt": ""
            },
            "client_secret": null
        });
        let response: LegacyRealtimeTranscriptionSessionCreateResponse =
            serde_json::from_value(fixture.clone())
                .expect("official transcription-session language null");
        assert!(response.client_secret().is_null());
        assert_eq!(
            serde_json::to_value(response).expect("round-trip official example"),
            fixture
        );
    }

    #[test]
    fn bounded_values_reject_out_of_range_and_non_finite_numbers() {
        assert!(LegacyRealtimeSpeed::new(0.24).is_err());
        assert!(LegacyRealtimeSpeed::new(f64::NAN).is_err());
        assert!(LegacyRealtimeTemperature::new(1.21).is_err());
        assert!(LegacyRealtimeMaxResponseOutputTokens::limited(0).is_err());
        assert!(LegacyRealtimeMaxResponseOutputTokens::limited(4096).is_ok());
        assert!(LegacyRealtimeSecretExpiration::new(9).is_err());
        assert!(LegacyRealtimeSecretExpiration::new(7200).is_ok());
    }
}
