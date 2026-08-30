//! Deprecated, pre-GA Realtime session-token wire models.
//!
//! These endpoints use the historical flat session shape. They intentionally
//! do not alias the GA nested `audio` session types. Only leaf values whose
//! JSON representation is identical are shared with [`crate::realtime`].

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    ExtraFields, ModelId, Nullable, Omittable, WireSecret, open_string_enum,
    realtime::{
        RealtimeAudioTranscription, RealtimeClientSecretExpirationAnchor, RealtimeFunctionTool,
        RealtimeNoiseReduction, RealtimeOutputModality, RealtimeTracing, RealtimeTruncation,
        RealtimeTurnDetection, RealtimeVoice,
    },
    responses::PromptReference,
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

/// Validated spoken-response speed for a legacy session.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LegacyRealtimeSpeed(f64);

impl LegacyRealtimeSpeed {
    /// Creates a finite speed within `0.25..=1.5`.
    pub fn new(value: f64) -> Result<Self, LegacyRealtimeValidationError> {
        if value.is_finite() && (0.25..=1.5).contains(&value) {
            Ok(Self(value))
        } else {
            Err(LegacyRealtimeValidationError::InvalidSpeed {
                value: value.to_string(),
            })
        }
    }

    /// Returns the validated speed.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LegacyRealtimeSpeed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(f64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Validated sampling temperature for a legacy session.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LegacyRealtimeTemperature(f64);

impl LegacyRealtimeTemperature {
    /// Creates a finite temperature within `0.6..=1.2`.
    pub fn new(value: f64) -> Result<Self, LegacyRealtimeValidationError> {
        if value.is_finite() && (0.6..=1.2).contains(&value) {
            Ok(Self(value))
        } else {
            Err(LegacyRealtimeValidationError::InvalidTemperature {
                value: value.to_string(),
            })
        }
    }

    /// Returns the validated temperature.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LegacyRealtimeTemperature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(f64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Integer output-token limit or the wire string `"inf"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LegacyRealtimeMaxResponseOutputTokens {
    /// A limit in `1..=4096`.
    Limited(u16),
    /// The historical `"inf"` service default.
    Unlimited,
}

impl LegacyRealtimeMaxResponseOutputTokens {
    /// Creates a finite token limit.
    pub fn limited(tokens: i64) -> Result<Self, LegacyRealtimeValidationError> {
        if (1..=4096).contains(&tokens) {
            Ok(Self::Limited(tokens as u16))
        } else {
            Err(LegacyRealtimeValidationError::InvalidMaxResponseOutputTokens { tokens })
        }
    }

    /// Returns the finite limit, or `None` for `"inf"`.
    #[must_use]
    pub const fn finite(self) -> Option<u16> {
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
            Self::Limited(tokens) => serializer.serialize_u16(*tokens),
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
                .ok_or_else(|| D::Error::custom("legacy Realtime token limit must be an integer"))
                .and_then(|tokens| Self::limited(tokens).map_err(D::Error::custom)),
            serde_json::Value::String(value) if value == "inf" => Ok(Self::Unlimited),
            _ => Err(D::Error::custom(
                "legacy Realtime token limit must be an integer or `inf`",
            )),
        }
    }
}

/// Expiration policy nested inside legacy client-secret options.
#[derive(Clone, Debug, PartialEq, Serialize)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRealtimeSecretExpirationWire {
    anchor: RealtimeClientSecretExpirationAnchor,
    #[serde(default)]
    seconds: Omittable<i64>,
}

impl<'de> Deserialize<'de> for LegacyRealtimeSecretExpiration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LegacyRealtimeSecretExpirationWire::deserialize(deserializer)?;
        if let Omittable::Value(seconds) = wire.seconds {
            validate_secret_lifetime(seconds).map_err(D::Error::custom)?;
        }
        Ok(Self {
            anchor: wire.anchor,
            seconds: wire.seconds,
        })
    }
}

fn validate_secret_lifetime(seconds: i64) -> Result<(), LegacyRealtimeValidationError> {
    if (10..=7200).contains(&seconds) {
        Ok(())
    } else {
        Err(LegacyRealtimeValidationError::InvalidSecretLifetime { seconds })
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
    voice: Omittable<RealtimeVoice>,
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
    turn_detection: Omittable<Nullable<RealtimeTurnDetection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<RealtimeFunctionTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<LegacyRealtimeToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<LegacyRealtimeTemperature>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_response_output_tokens: Omittable<LegacyRealtimeMaxResponseOutputTokens>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<RealtimeTruncation>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<PromptReference>>,
}

impl LegacyRealtimeSessionCreateRequest {
    /// Creates an empty request that uses service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
    pub fn with_voice(mut self, voice: impl Into<RealtimeVoice>) -> Self {
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
    pub fn with_turn_detection(mut self, turn_detection: RealtimeTurnDetection) -> Self {
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
    pub fn with_tools(mut self, tools: Vec<RealtimeFunctionTool>) -> Self {
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

    /// Sets legacy truncation configuration.
    #[must_use]
    pub fn with_truncation(mut self, truncation: RealtimeTruncation) -> Self {
        self.truncation = Omittable::Value(truncation);
        self
    }

    /// Sets a prompt reference.
    #[must_use]
    pub fn with_prompt(mut self, prompt: PromptReference) -> Self {
        self.prompt = Omittable::Value(Nullable::Value(prompt));
        self
    }

    /// Explicitly clears the prompt reference.
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
    turn_detection: Omittable<Nullable<RealtimeTurnDetection>>,
}

impl LegacyRealtimeTranscriptionSessionCreateRequest {
    /// Creates an empty request that uses transcription defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
    pub fn with_turn_detection(mut self, turn_detection: RealtimeTurnDetection) -> Self {
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
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyRealtimeSessionCreateResponse {
    client_secret: LegacyRealtimeClientSecret,
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
    voice: Omittable<RealtimeVoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_audio_format: Omittable<LegacyRealtimeAudioFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_audio_transcription: Omittable<Nullable<RealtimeAudioTranscription>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    turn_detection: Omittable<Nullable<RealtimeTurnDetection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<RealtimeFunctionTool>>,
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
    prompt: Omittable<Nullable<PromptReference>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl LegacyRealtimeSessionCreateResponse {
    /// Returns the redacting client secret wrapper.
    #[must_use]
    pub const fn client_secret(&self) -> &LegacyRealtimeClientSecret {
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
    turn_detection: Omittable<Nullable<RealtimeTurnDetection>>,
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
            );
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
                "max_response_output_tokens": 200
            })
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
        assert_eq!(
            response.client_secret().with_exposed(ToOwned::to_owned),
            "ek_private_value"
        );
        assert!(!format!("{response:?}").contains("ek_private_value"));
        assert!(!format!("{:?}", response.client_secret()).contains("ek_private_value"));
        assert_eq!(
            serde_json::to_value(response).expect("round-trip legacy session response"),
            fixture
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
