//! Bidirectional wire DTOs for OpenAI Audio and Images APIs.
//!
//! JSON request metadata is Serde-native. Multipart request containers reuse
//! [`ReplayableMultipartSource`] and deliberately do not implement Serde, so
//! paths, handles, and binary payloads cannot accidentally enter JSON.
//! Videos and Sora are intentionally outside this module.

use std::{fmt, marker::PhantomData};

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    ExtraFields, FileId, ModelId, Nullable, Omittable, files::ReplayableMultipartSource,
    responses::UnknownTaggedObject,
};

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

macro_rules! strict_tagged_union {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident($ty:ty) = $wire:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// A future event variant retained with every JSON field.
            Unknown(UnknownTaggedObject),
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
                let tag = media_discriminator(&value).map_err(D::Error::custom)?;
                match tag {
                    $($wire => serde_json::from_value::<$ty>(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

fn media_discriminator(value: &Value) -> Result<&str, &'static str> {
    let Value::Object(object) = value else {
        return Err("tagged media value must be a JSON object");
    };
    object
        .get("type")
        .ok_or("tagged media object is missing string field `type`")?
        .as_str()
        .ok_or("tagged media object field `type` must be a string")
}

/// Invalid bounded media request value.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{field} must be between {min} and {max}, got {actual}")]
pub struct MediaRangeError {
    field: &'static str,
    min: u8,
    max: u8,
    actual: u64,
}

impl MediaRangeError {
    /// Wire field whose value was rejected.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// Rejected value.
    #[must_use]
    pub const fn actual(&self) -> u64 {
        self.actual
    }
}

macro_rules! bounded_u8 {
    ($(#[$meta:meta])* $name:ident, $field:literal, $min:literal, $max:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u8);

        impl $name {
            /// Validate and construct the bounded value.
            pub fn new(value: u8) -> Result<Self, MediaRangeError> {
                if ($min..=$max).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(MediaRangeError {
                        field: $field,
                        min: $min,
                        max: $max,
                        actual: u64::from(value),
                    })
                }
            }

            /// Return the validated numeric value.
            #[must_use]
            pub const fn get(self) -> u8 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u64::deserialize(deserializer)?;
                let value = u8::try_from(value).map_err(|_| {
                    D::Error::custom(MediaRangeError {
                        field: $field,
                        min: $min,
                        max: $max,
                        actual: value,
                    })
                })?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

bounded_u8! {
    /// Number of images generated by one Images request.
    ImageCount, "n", 1, 10
}

bounded_u8! {
    /// Number of partial images requested from a streaming Images operation.
    PartialImageCount, "partial_images", 0, 3
}

bounded_u8! {
    /// Output compression percentage.
    ImageCompression, "output_compression", 0, 100
}

/// Non-streaming media request typestate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaNonStreaming;

/// Streaming media request typestate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaStreaming;

mod stream_mode_private {
    pub trait Sealed {}
    impl Sealed for super::MediaNonStreaming {}
    impl Sealed for super::MediaStreaming {}
}

/// Sealed constraint for media request streaming typestates.
pub trait MediaStreamMode: stream_mode_private::Sealed {
    /// Whether the request expects event-stream output.
    const STREAMING: bool;
}

impl MediaStreamMode for MediaNonStreaming {
    const STREAMING: bool = false;
}

impl MediaStreamMode for MediaStreaming {
    const STREAMING: bool = true;
}

crate::open_string_enum! {
    /// Encoded audio format returned by the speech endpoint.
    pub enum SpeechResponseFormat {
        Mp3 = "mp3",
        Opus = "opus",
        Aac = "aac",
        Flac = "flac",
        Wav = "wav",
        Pcm = "pcm"
    }
}

crate::open_string_enum! {
    /// Speech transport mode.
    pub enum SpeechStreamFormat {
        Sse = "sse",
        Audio = "audio"
    }
}

/// Built-in or custom speech voice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SpeechVoice {
    /// Built-in name or a future named voice.
    Named(String),
    /// Custom voice identifier.
    Custom(SpeechCustomVoice),
}

impl From<String> for SpeechVoice {
    fn from(value: String) -> Self {
        Self::Named(value)
    }
}

impl From<&str> for SpeechVoice {
    fn from(value: &str) -> Self {
        Self::Named(value.to_owned())
    }
}

/// Custom speech voice reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechCustomVoice {
    /// Voice identifier.
    pub id: String,
}

/// JSON body for `POST /audio/speech`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(bound(serialize = ""))]
pub struct CreateSpeechRequest<M = MediaNonStreaming>
where
    M: MediaStreamMode,
{
    /// Speech model identifier.
    pub model: ModelId,
    /// Text to synthesize.
    pub input: String,
    /// Built-in or custom voice.
    pub voice: SpeechVoice,
    /// Voice-control instructions.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub instructions: Omittable<String>,
    /// Encoded audio format.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<SpeechResponseFormat>,
    /// Playback speed.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub speed: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_format: Omittable<SpeechStreamFormat>,
    #[serde(skip)]
    mode: PhantomData<fn() -> M>,
}

#[derive(Deserialize)]
struct CreateSpeechRequestWire {
    model: ModelId,
    input: String,
    voice: SpeechVoice,
    #[serde(default)]
    instructions: Omittable<String>,
    #[serde(default)]
    response_format: Omittable<SpeechResponseFormat>,
    #[serde(default)]
    speed: Omittable<f64>,
    #[serde(default)]
    stream_format: Omittable<SpeechStreamFormat>,
}

impl<'de, M> Deserialize<'de> for CreateSpeechRequest<M>
where
    M: MediaStreamMode,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateSpeechRequestWire::deserialize(deserializer)?;
        let sse = matches!(
            wire.stream_format,
            Omittable::Value(SpeechStreamFormat::Sse)
        );
        if sse != M::STREAMING {
            return Err(D::Error::custom(if M::STREAMING {
                "streaming speech request requires `stream_format: sse`"
            } else {
                "non-streaming speech request cannot use `stream_format: sse`"
            }));
        }
        Ok(Self {
            model: wire.model,
            input: wire.input,
            voice: wire.voice,
            instructions: wire.instructions,
            response_format: wire.response_format,
            speed: wire.speed,
            stream_format: wire.stream_format,
            mode: PhantomData,
        })
    }
}

impl CreateSpeechRequest<MediaNonStreaming> {
    /// Construct a speech request returning a raw audio body.
    #[must_use]
    pub fn new(
        model: impl Into<ModelId>,
        input: impl Into<String>,
        voice: impl Into<SpeechVoice>,
    ) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            voice: voice.into(),
            instructions: Omittable::Omitted,
            response_format: Omittable::Omitted,
            speed: Omittable::Omitted,
            stream_format: Omittable::Omitted,
            mode: PhantomData,
        }
    }

    /// Switch to SSE speech output.
    #[must_use]
    pub fn into_streaming(self) -> CreateSpeechRequest<MediaStreaming> {
        CreateSpeechRequest {
            model: self.model,
            input: self.input,
            voice: self.voice,
            instructions: self.instructions,
            response_format: self.response_format,
            speed: self.speed,
            stream_format: Omittable::Value(SpeechStreamFormat::Sse),
            mode: PhantomData,
        }
    }
}

impl<M> CreateSpeechRequest<M>
where
    M: MediaStreamMode,
{
    /// Set voice-control instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(instructions.into());
        self
    }

    /// Select the encoded audio format.
    #[must_use]
    pub fn with_response_format(mut self, format: SpeechResponseFormat) -> Self {
        self.response_format = Omittable::Value(format);
        self
    }

    /// Set playback speed.
    #[must_use]
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = Omittable::Value(speed);
        self
    }
}

/// Non-streaming speech request alias.
pub type SpeechRequest = CreateSpeechRequest<MediaNonStreaming>;

/// SSE speech request alias.
pub type SpeechStreamRequest = CreateSpeechRequest<MediaStreaming>;

literal_tag!(SpeechAudioDeltaTag, Delta, "speech.audio.delta");

/// Base64 audio chunk emitted by streaming speech synthesis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechAudioDeltaEvent {
    #[serde(rename = "type")]
    kind: SpeechAudioDeltaTag,
    /// Base64-encoded audio chunk.
    pub audio: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SpeechAudioDeltaEvent {
    /// Decode the base64 audio payload.
    pub fn decode_audio(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.audio)
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Token usage reported when streaming speech completes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechUsage {
    /// Input text tokens.
    pub input_tokens: u64,
    /// Generated audio tokens.
    pub output_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SpeechUsage {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(SpeechAudioDoneTag, Done, "speech.audio.done");

/// Terminal speech synthesis stream event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechAudioDoneEvent {
    #[serde(rename = "type")]
    kind: SpeechAudioDoneTag,
    /// Request token usage.
    pub usage: SpeechUsage,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SpeechAudioDoneEvent {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// One event from a streaming speech response.
    pub enum SpeechStreamEvent {
        AudioDelta(SpeechAudioDeltaEvent) = "speech.audio.delta",
        AudioDone(SpeechAudioDoneEvent) = "speech.audio.done"
    }
}

impl SpeechStreamEvent {
    /// Whether this event terminates a healthy speech stream.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::AudioDone(_))
    }
}

crate::open_string_enum! {
    /// Requested transcription output format.
    pub enum TranscriptionResponseFormat {
        Json = "json",
        Text = "text",
        Srt = "srt",
        VerboseJson = "verbose_json",
        Vtt = "vtt",
        DiarizedJson = "diarized_json"
    }
}

crate::open_string_enum! {
    /// Requested translation output format.
    pub enum TranslationResponseFormat {
        Json = "json",
        Text = "text",
        Srt = "srt",
        VerboseJson = "verbose_json",
        Vtt = "vtt"
    }
}

crate::open_string_enum! {
    /// Optional information included with a transcription.
    pub enum TranscriptionInclude {
        Logprobs = "logprobs"
    }
}

crate::open_string_enum! {
    /// Timestamp detail requested for verbose transcription.
    pub enum TranscriptionTimestampGranularity {
        Word = "word",
        Segment = "segment"
    }
}

crate::open_string_enum! {
    /// Automatic transcription chunking mode.
    pub enum TranscriptionChunkingMode {
        Auto = "auto"
    }
}

literal_tag!(ServerVadTag, ServerVad, "server_vad");

/// Server-side voice-activity-detection settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionVadConfig {
    #[serde(rename = "type")]
    kind: ServerVadTag,
    /// Audio included before detected speech, in milliseconds.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prefix_padding_ms: Omittable<u64>,
    /// Silence duration used to detect speech stop, in milliseconds.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub silence_duration_ms: Omittable<u64>,
    /// VAD sensitivity threshold.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub threshold: Omittable<f64>,
}

impl TranscriptionVadConfig {
    /// Construct server VAD with service defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: ServerVadTag::ServerVad,
            prefix_padding_ms: Omittable::Omitted,
            silence_duration_ms: Omittable::Omitted,
            threshold: Omittable::Omitted,
        }
    }
}

impl Default for TranscriptionVadConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Automatic or manually configured transcription chunking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum TranscriptionChunkingStrategy {
    /// `auto` or a future string mode.
    Mode(TranscriptionChunkingMode),
    /// Explicit server VAD settings.
    ServerVad(TranscriptionVadConfig),
}

/// Serde metadata encoded as multipart fields for transcription creation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionRequestMetadata {
    /// Audio model identifier.
    pub model: ModelId,
    /// ISO-639-1 input language.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub language: Omittable<String>,
    /// Candidate input languages.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub languages: Omittable<Vec<String>>,
    /// Words or phrases that guide recognition.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub keywords: Omittable<Vec<String>>,
    /// Style/continuation prompt.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<String>,
    /// Response representation.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<TranscriptionResponseFormat>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub temperature: Omittable<f64>,
    /// Additional response information.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<TranscriptionInclude>>,
    /// Timestamp detail for verbose output.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub timestamp_granularities: Omittable<Vec<TranscriptionTimestampGranularity>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream: Omittable<Nullable<bool>>,
    /// Audio chunking strategy or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub chunking_strategy: Omittable<Nullable<TranscriptionChunkingStrategy>>,
    /// Known speaker names.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub known_speaker_names: Omittable<Vec<String>>,
    /// Matching known-speaker sample data URLs.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub known_speaker_references: Omittable<Vec<String>>,
}

impl TranscriptionRequestMetadata {
    fn new(model: impl Into<ModelId>) -> Self {
        Self {
            model: model.into(),
            language: Omittable::Omitted,
            languages: Omittable::Omitted,
            keywords: Omittable::Omitted,
            prompt: Omittable::Omitted,
            response_format: Omittable::Omitted,
            temperature: Omittable::Omitted,
            include: Omittable::Omitted,
            timestamp_granularities: Omittable::Omitted,
            stream: Omittable::Omitted,
            chunking_strategy: Omittable::Omitted,
            known_speaker_names: Omittable::Omitted,
            known_speaker_references: Omittable::Omitted,
        }
    }

    /// Whether `stream: true` is encoded in these fields.
    #[must_use]
    pub const fn is_streaming(&self) -> bool {
        matches!(self.stream, Omittable::Value(Nullable::Value(true)))
    }
}

/// Multipart transcription request. Binary data is never Serde-encoded.
#[derive(Clone, PartialEq)]
pub struct CreateTranscriptionRequest<M = MediaNonStreaming>
where
    M: MediaStreamMode,
{
    file: ReplayableMultipartSource,
    /// Serde-compatible multipart text metadata.
    pub metadata: TranscriptionRequestMetadata,
    mode: PhantomData<fn() -> M>,
}

impl<M> fmt::Debug for CreateTranscriptionRequest<M>
where
    M: MediaStreamMode,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateTranscriptionRequest")
            .field("file", &self.file)
            .field("metadata", &self.metadata)
            .field("streaming", &M::STREAMING)
            .finish()
    }
}

impl CreateTranscriptionRequest<MediaNonStreaming> {
    /// Construct a multipart transcription request.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource, model: impl Into<ModelId>) -> Self {
        Self {
            file,
            metadata: TranscriptionRequestMetadata::new(model),
            mode: PhantomData,
        }
    }

    /// Switch to SSE transcription output and encode `stream: true`.
    #[must_use]
    pub fn into_streaming(self) -> CreateTranscriptionRequest<MediaStreaming> {
        let mut metadata = self.metadata;
        metadata.stream = Omittable::Value(Nullable::Value(true));
        CreateTranscriptionRequest {
            file: self.file,
            metadata,
            mode: PhantomData,
        }
    }
}

impl<M> CreateTranscriptionRequest<M>
where
    M: MediaStreamMode,
{
    /// Binary multipart source.
    #[must_use]
    pub const fn file(&self) -> &ReplayableMultipartSource {
        &self.file
    }

    /// Select the response format.
    #[must_use]
    pub fn with_response_format(mut self, format: TranscriptionResponseFormat) -> Self {
        self.metadata.response_format = Omittable::Value(format);
        self
    }

    /// Provide the input language.
    #[must_use]
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.metadata.language = Omittable::Value(language.into());
        self
    }

    /// Request token log probabilities.
    #[must_use]
    pub fn with_logprobs(mut self) -> Self {
        self.metadata.include = Omittable::Value(vec![TranscriptionInclude::Logprobs]);
        self
    }

    /// Configure transcription chunking.
    #[must_use]
    pub fn with_chunking_strategy(mut self, strategy: TranscriptionChunkingStrategy) -> Self {
        self.metadata.chunking_strategy = Omittable::Value(Nullable::Value(strategy));
        self
    }

    /// Add a known speaker and encode its raw sample as a data URL.
    #[must_use]
    pub fn with_known_speaker(
        mut self,
        name: impl Into<String>,
        media_type: &str,
        sample: impl AsRef<[u8]>,
    ) -> Self {
        let encoded = base64::engine::general_purpose::STANDARD.encode(sample.as_ref());
        let reference = format!("data:{media_type};base64,{encoded}");
        match &mut self.metadata.known_speaker_names {
            Omittable::Value(names) => names.push(name.into()),
            Omittable::Omitted => {
                self.metadata.known_speaker_names = Omittable::Value(vec![name.into()]);
            }
        }
        match &mut self.metadata.known_speaker_references {
            Omittable::Value(references) => references.push(reference),
            Omittable::Omitted => {
                self.metadata.known_speaker_references = Omittable::Value(vec![reference]);
            }
        }
        self
    }
}

/// Non-streaming multipart transcription request.
pub type TranscriptionRequest = CreateTranscriptionRequest<MediaNonStreaming>;

/// Streaming multipart transcription request.
pub type TranscriptionStreamRequest = CreateTranscriptionRequest<MediaStreaming>;

literal_tag!(UsageTokensTag, Tokens, "tokens");

/// Input token breakdown for transcription.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionInputTokenDetails {
    /// Text tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<u64>,
    /// Audio tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<u64>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionInputTokenDetails {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Token-billed transcription usage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionTokenUsage {
    #[serde(rename = "type")]
    kind: UsageTokensTag,
    /// Input tokens billed.
    pub input_tokens: u64,
    /// Optional input token breakdown.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_token_details: Omittable<TranscriptionInputTokenDetails>,
    /// Generated tokens.
    pub output_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionTokenUsage {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(UsageDurationTag, Duration, "duration");

/// Duration-billed transcription usage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionDurationUsage {
    #[serde(rename = "type")]
    kind: UsageDurationTag,
    /// Input audio duration in seconds.
    pub seconds: f64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionDurationUsage {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// Token- or duration-billed transcription usage.
    pub enum TranscriptionUsage {
        Tokens(TranscriptionTokenUsage) = "tokens",
        Duration(TranscriptionDurationUsage) = "duration"
    }
}

/// One language detected in transcription audio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionLanguage {
    /// Detected language code.
    pub code: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionLanguage {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Optional token log-probability record in non-streaming transcription JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionLogprob {
    /// Token text.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub token: Omittable<String>,
    /// Token log probability.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprob: Omittable<f64>,
    /// Byte values; the frozen schema uses JSON numbers for this response.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub bytes: Omittable<Vec<f64>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionLogprob {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Standard JSON transcription response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transcription {
    /// Complete transcript text.
    pub text: String,
    /// Detected languages.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub languages: Omittable<Vec<TranscriptionLanguage>>,
    /// Requested token logprobs.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprobs: Omittable<Vec<TranscriptionLogprob>>,
    /// Token or duration usage.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<TranscriptionUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Transcription {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Word timestamp in verbose transcription or translation output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionWord {
    /// Word text.
    pub word: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionWord {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Segment details in verbose transcription or translation output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Numeric segment identifier.
    pub id: u64,
    /// Seek offset.
    pub seek: u64,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Segment text.
    pub text: String,
    /// Model token IDs.
    pub tokens: Vec<u64>,
    /// Sampling temperature used.
    pub temperature: f64,
    /// Average log probability.
    pub avg_logprob: f64,
    /// Compression ratio.
    pub compression_ratio: f64,
    /// Probability of no speech.
    pub no_speech_prob: f64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionSegment {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Verbose JSON transcription response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerboseTranscription {
    /// Input audio language.
    pub language: String,
    /// Input duration in seconds.
    pub duration: f64,
    /// Complete transcript text.
    pub text: String,
    /// Requested word timestamps.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub words: Omittable<Vec<TranscriptionWord>>,
    /// Requested segment details.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub segments: Omittable<Vec<TranscriptionSegment>>,
    /// Duration-based usage.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<TranscriptionDurationUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VerboseTranscription {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(DiarizedSegmentTag, Segment, "transcript.text.segment");

/// Speaker-labelled transcription segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiarizedTranscriptionSegment {
    #[serde(rename = "type")]
    kind: DiarizedSegmentTag,
    /// Segment identifier.
    pub id: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Segment transcript.
    pub text: String,
    /// Known or generated speaker label.
    pub speaker: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DiarizedTranscriptionSegment {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Diarized JSON transcription response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiarizedTranscription {
    /// Task discriminator.
    pub task: DiarizedTask,
    /// Input duration in seconds.
    pub duration: f64,
    /// Complete concatenated transcript.
    pub text: String,
    /// Speaker-labelled segments.
    pub segments: Vec<DiarizedTranscriptionSegment>,
    /// Token or duration usage.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<TranscriptionUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Public task discriminator retained as an open string.
    pub enum DiarizedTask {
        Transcribe = "transcribe"
    }
}

impl DiarizedTranscription {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Optional logprob entry in transcription stream events.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionStreamLogprob {
    /// Token text.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub token: Omittable<String>,
    /// Token log probability.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprob: Omittable<f64>,
    /// Token bytes.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub bytes: Omittable<Vec<u8>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionStreamLogprob {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(TranscriptTextDeltaTag, Delta, "transcript.text.delta");

/// Incremental transcription text event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionTextDeltaEvent {
    #[serde(rename = "type")]
    kind: TranscriptTextDeltaTag,
    /// Newly transcribed text.
    pub delta: String,
    /// Requested log probabilities.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprobs: Omittable<Vec<TranscriptionStreamLogprob>>,
    /// Diarized segment identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub segment_id: Omittable<String>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionTextDeltaEvent {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(TranscriptTextSegmentTag, Segment, "transcript.text.segment");

/// Completed diarized segment stream event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionTextSegmentEvent {
    #[serde(rename = "type")]
    kind: TranscriptTextSegmentTag,
    /// Segment identifier.
    pub id: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Segment text.
    pub text: String,
    /// Speaker label.
    pub speaker: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionTextSegmentEvent {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(TranscriptTextDoneTag, Done, "transcript.text.done");

/// Terminal transcription stream event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionTextDoneEvent {
    #[serde(rename = "type")]
    kind: TranscriptTextDoneTag,
    /// Complete transcript.
    pub text: String,
    /// Detected languages.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub languages: Omittable<Vec<TranscriptionLanguage>>,
    /// Requested log probabilities.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprobs: Omittable<Vec<TranscriptionStreamLogprob>>,
    /// Token usage.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<TranscriptionTokenUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl TranscriptionTextDoneEvent {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// One event from a streaming transcription response.
    pub enum TranscriptionStreamEvent {
        TextDelta(TranscriptionTextDeltaEvent) = "transcript.text.delta",
        TextSegment(TranscriptionTextSegmentEvent) = "transcript.text.segment",
        TextDone(TranscriptionTextDoneEvent) = "transcript.text.done"
    }
}

impl TranscriptionStreamEvent {
    /// Whether this event completes the stream.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::TextDone(_))
    }
}

/// Serde metadata encoded as multipart fields for audio translation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranslationRequestMetadata {
    /// Audio model identifier.
    pub model: ModelId,
    /// English style/continuation prompt.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt: Omittable<String>,
    /// Response representation.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<TranslationResponseFormat>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub temperature: Omittable<f64>,
}

/// Multipart translation request. Binary data is never Serde-encoded.
#[derive(Clone, PartialEq)]
pub struct CreateTranslationRequest {
    file: ReplayableMultipartSource,
    /// Serde-compatible multipart text metadata.
    pub metadata: TranslationRequestMetadata,
}

impl CreateTranslationRequest {
    /// Construct a multipart translation request.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource, model: impl Into<ModelId>) -> Self {
        Self {
            file,
            metadata: TranslationRequestMetadata {
                model: model.into(),
                prompt: Omittable::Omitted,
                response_format: Omittable::Omitted,
                temperature: Omittable::Omitted,
            },
        }
    }

    /// Binary multipart source.
    #[must_use]
    pub const fn file(&self) -> &ReplayableMultipartSource {
        &self.file
    }

    /// Attach an English style/continuation prompt.
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.metadata.prompt = Omittable::Value(prompt.into());
        self
    }

    /// Select response representation.
    #[must_use]
    pub fn with_response_format(mut self, format: TranslationResponseFormat) -> Self {
        self.metadata.response_format = Omittable::Value(format);
        self
    }
}

impl fmt::Debug for CreateTranslationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateTranslationRequest")
            .field("file", &self.file)
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Standard JSON translation response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Translation {
    /// English translation text.
    pub text: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Translation {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Verbose JSON translation response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerboseTranslation {
    /// Output language, currently English.
    pub language: String,
    /// Input duration in seconds.
    pub duration: f64,
    /// Complete translated text.
    pub text: String,
    /// Segment details.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub segments: Omittable<Vec<TranscriptionSegment>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VerboseTranslation {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Requested image quality across supported image model families.
    pub enum ImageQuality {
        Standard = "standard",
        Hd = "hd",
        Low = "low",
        Medium = "medium",
        High = "high",
        Auto = "auto"
    }
}

crate::open_string_enum! {
    /// Requested or reported image dimensions.
    pub enum ImageSize {
        Auto = "auto",
        Square256 = "256x256",
        Square512 = "512x512",
        Square1024 = "1024x1024",
        Portrait1024x1536 = "1024x1536",
        Landscape1536x1024 = "1536x1024",
        Portrait1024x1792 = "1024x1792",
        Landscape1792x1024 = "1792x1024"
    }
}

crate::open_string_enum! {
    /// Legacy Images response representation.
    pub enum ImageResponseFormat {
        Url = "url",
        Base64Json = "b64_json"
    }
}

crate::open_string_enum! {
    /// Encoded output image format.
    pub enum ImageOutputFormat {
        Png = "png",
        Jpeg = "jpeg",
        Webp = "webp"
    }
}

crate::open_string_enum! {
    /// Image background behavior.
    pub enum ImageBackground {
        Transparent = "transparent",
        Opaque = "opaque",
        Auto = "auto"
    }
}

crate::open_string_enum! {
    /// Image-generation moderation level.
    pub enum ImageModeration {
        Low = "low",
        Auto = "auto"
    }
}

crate::open_string_enum! {
    /// DALL-E 3 image style.
    pub enum ImageStyle {
        Vivid = "vivid",
        Natural = "natural"
    }
}

crate::open_string_enum! {
    /// Fidelity used to preserve input-image features during editing.
    pub enum ImageInputFidelity {
        High = "high",
        Low = "low"
    }
}

/// JSON image edit input referencing exactly one URL/data URL or uploaded file.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageReference {
    /// Fully-qualified URL or base64 data URL.
    Url(String),
    /// Uploaded File API identifier.
    File(FileId),
}

impl ImageReference {
    /// Construct a URL or data-URL image reference.
    #[must_use]
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Construct an uploaded-file reference.
    #[must_use]
    pub fn file(file_id: impl Into<FileId>) -> Self {
        Self::File(file_id.into())
    }

    /// Encode raw bytes into an image data URL.
    #[must_use]
    pub fn from_bytes(media_type: &str, bytes: impl AsRef<[u8]>) -> Self {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
        Self::Url(format!("data:{media_type};base64,{encoded}"))
    }
}

impl Serialize for ImageReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut object = Map::new();
        match self {
            Self::Url(url) => object.insert("image_url".to_owned(), Value::String(url.clone())),
            Self::File(file_id) => object.insert(
                "file_id".to_owned(),
                Value::String(file_id.as_str().to_owned()),
            ),
        };
        object.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImageReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let object = Map::<String, Value>::deserialize(deserializer)?;
        if object.len() != 1 {
            return Err(D::Error::custom(
                "image reference requires exactly one of `image_url` or `file_id`",
            ));
        }
        if let Some(value) = object.get("image_url") {
            return value
                .as_str()
                .map(|value| Self::Url(value.to_owned()))
                .ok_or_else(|| D::Error::custom("`image_url` must be a string"));
        }
        if let Some(value) = object.get("file_id") {
            return value
                .as_str()
                .map(|value| Self::File(FileId::new(value)))
                .ok_or_else(|| D::Error::custom("`file_id` must be a string"));
        }
        Err(D::Error::custom(
            "image reference requires `image_url` or `file_id`",
        ))
    }
}

/// Fields shared by streaming and non-streaming image generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationRequestBody {
    /// Text description of desired images.
    pub prompt: String,
    /// Model identifier or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<Nullable<ModelId>>,
    /// Number of images or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n: Omittable<Nullable<ImageCount>>,
    /// Requested quality or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quality: Omittable<Nullable<ImageQuality>>,
    /// Legacy URL/base64 response choice or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<Nullable<ImageResponseFormat>>,
    /// Encoded image format or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_format: Omittable<Nullable<ImageOutputFormat>>,
    /// Output compression or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_compression: Omittable<Nullable<ImageCompression>>,
    /// Requested dimensions or explicit null. Arbitrary future/resolution
    /// strings are preserved by [`ImageSize::Unknown`].
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub size: Omittable<Nullable<ImageSize>>,
    /// Moderation level or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub moderation: Omittable<Nullable<ImageModeration>>,
    /// Background behavior or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub background: Omittable<Nullable<ImageBackground>>,
    /// DALL-E 3 style or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub style: Omittable<Nullable<ImageStyle>>,
    /// Stable end-user identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<String>,
}

impl ImageGenerationRequestBody {
    fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: Omittable::Omitted,
            n: Omittable::Omitted,
            quality: Omittable::Omitted,
            response_format: Omittable::Omitted,
            output_format: Omittable::Omitted,
            output_compression: Omittable::Omitted,
            size: Omittable::Omitted,
            moderation: Omittable::Omitted,
            background: Omittable::Omitted,
            style: Omittable::Omitted,
            user: Omittable::Omitted,
        }
    }
}

/// JSON body for `POST /images/generations`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(bound(serialize = ""))]
pub struct CreateImageRequest<M = MediaNonStreaming>
where
    M: MediaStreamMode,
{
    /// Generation fields shared by both response modes.
    #[serde(flatten)]
    pub body: ImageGenerationRequestBody,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    partial_images: Omittable<Nullable<PartialImageCount>>,
    #[serde(skip)]
    mode: PhantomData<fn() -> M>,
}

#[derive(Deserialize)]
struct CreateImageRequestWire {
    #[serde(flatten)]
    body: ImageGenerationRequestBody,
    #[serde(default)]
    stream: Omittable<Nullable<bool>>,
    #[serde(default)]
    partial_images: Omittable<Nullable<PartialImageCount>>,
}

impl<'de, M> Deserialize<'de> for CreateImageRequest<M>
where
    M: MediaStreamMode,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateImageRequestWire::deserialize(deserializer)?;
        let streaming = matches!(wire.stream, Omittable::Value(Nullable::Value(true)));
        if streaming != M::STREAMING {
            return Err(D::Error::custom(if M::STREAMING {
                "streaming image generation requires `stream: true`"
            } else {
                "non-streaming image generation cannot carry `stream: true`"
            }));
        }
        if !M::STREAMING && wire.partial_images.is_value() {
            return Err(D::Error::custom(
                "non-streaming image generation cannot carry `partial_images`",
            ));
        }
        Ok(Self {
            body: wire.body,
            stream: wire.stream,
            partial_images: wire.partial_images,
            mode: PhantomData,
        })
    }
}

impl CreateImageRequest<MediaNonStreaming> {
    /// Construct a non-streaming image generation request.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            body: ImageGenerationRequestBody::new(prompt),
            stream: Omittable::Omitted,
            partial_images: Omittable::Omitted,
            mode: PhantomData,
        }
    }

    /// Switch to image generation SSE output.
    #[must_use]
    pub fn into_streaming(self) -> CreateImageRequest<MediaStreaming> {
        CreateImageRequest {
            body: self.body,
            stream: Omittable::Value(Nullable::Value(true)),
            partial_images: Omittable::Omitted,
            mode: PhantomData,
        }
    }
}

impl CreateImageRequest<MediaStreaming> {
    /// Request between zero and three partial images.
    #[must_use]
    pub fn with_partial_images(mut self, count: PartialImageCount) -> Self {
        self.partial_images = Omittable::Value(Nullable::Value(count));
        self
    }
}

impl<M> CreateImageRequest<M>
where
    M: MediaStreamMode,
{
    /// Select a model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<ModelId>) -> Self {
        self.body.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }

    /// Select output quality.
    #[must_use]
    pub fn with_quality(mut self, quality: ImageQuality) -> Self {
        self.body.quality = Omittable::Value(Nullable::Value(quality));
        self
    }

    /// Select dimensions.
    #[must_use]
    pub fn with_size(mut self, size: ImageSize) -> Self {
        self.body.size = Omittable::Value(Nullable::Value(size));
        self
    }

    /// Select encoded output format.
    #[must_use]
    pub fn with_output_format(mut self, format: ImageOutputFormat) -> Self {
        self.body.output_format = Omittable::Value(Nullable::Value(format));
        self
    }

    /// Select background behavior.
    #[must_use]
    pub fn with_background(mut self, background: ImageBackground) -> Self {
        self.body.background = Omittable::Value(Nullable::Value(background));
        self
    }
}

/// Non-streaming image generation request alias.
pub type ImageGenerationRequest = CreateImageRequest<MediaNonStreaming>;

/// Streaming image generation request alias.
pub type ImageGenerationStreamRequest = CreateImageRequest<MediaStreaming>;

/// JSON image-edit fields shared by streaming and non-streaming requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageEditJsonRequestBody {
    /// One to sixteen URL/data-URL/file references.
    pub images: Vec<ImageReference>,
    /// Edit instruction.
    pub prompt: String,
    /// Model identifier or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<Nullable<ModelId>>,
    /// Optional mask reference.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub mask: Omittable<ImageReference>,
    /// Number of edited images or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n: Omittable<Nullable<ImageCount>>,
    /// Output quality or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quality: Omittable<Nullable<ImageQuality>>,
    /// Input fidelity or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_fidelity: Omittable<Nullable<ImageInputFidelity>>,
    /// Output size or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub size: Omittable<Nullable<ImageSize>>,
    /// Stable end-user identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<String>,
    /// Encoded output format or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_format: Omittable<Nullable<ImageOutputFormat>>,
    /// Compression percentage or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_compression: Omittable<Nullable<ImageCompression>>,
    /// Moderation level or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub moderation: Omittable<Nullable<ImageModeration>>,
    /// Background behavior or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub background: Omittable<Nullable<ImageBackground>>,
}

impl ImageEditJsonRequestBody {
    fn new(image: ImageReference, prompt: impl Into<String>) -> Self {
        Self {
            images: vec![image],
            prompt: prompt.into(),
            model: Omittable::Omitted,
            mask: Omittable::Omitted,
            n: Omittable::Omitted,
            quality: Omittable::Omitted,
            input_fidelity: Omittable::Omitted,
            size: Omittable::Omitted,
            user: Omittable::Omitted,
            output_format: Omittable::Omitted,
            output_compression: Omittable::Omitted,
            moderation: Omittable::Omitted,
            background: Omittable::Omitted,
        }
    }
}

/// JSON body for `POST /images/edits` using image references.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(bound(serialize = ""))]
pub struct CreateImageEditJsonRequest<M = MediaNonStreaming>
where
    M: MediaStreamMode,
{
    /// JSON edit fields.
    #[serde(flatten)]
    pub body: ImageEditJsonRequestBody,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    partial_images: Omittable<Nullable<PartialImageCount>>,
    #[serde(skip)]
    mode: PhantomData<fn() -> M>,
}

#[derive(Deserialize)]
struct CreateImageEditJsonRequestWire {
    #[serde(flatten)]
    body: ImageEditJsonRequestBody,
    #[serde(default)]
    stream: Omittable<Nullable<bool>>,
    #[serde(default)]
    partial_images: Omittable<Nullable<PartialImageCount>>,
}

impl<'de, M> Deserialize<'de> for CreateImageEditJsonRequest<M>
where
    M: MediaStreamMode,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateImageEditJsonRequestWire::deserialize(deserializer)?;
        if wire.body.images.is_empty() || wire.body.images.len() > 16 {
            return Err(D::Error::custom(
                "JSON image edit requires between one and sixteen images",
            ));
        }
        let streaming = matches!(wire.stream, Omittable::Value(Nullable::Value(true)));
        if streaming != M::STREAMING {
            return Err(D::Error::custom(if M::STREAMING {
                "streaming JSON image edit requires `stream: true`"
            } else {
                "non-streaming JSON image edit cannot carry `stream: true`"
            }));
        }
        if !M::STREAMING && wire.partial_images.is_value() {
            return Err(D::Error::custom(
                "non-streaming JSON image edit cannot carry `partial_images`",
            ));
        }
        Ok(Self {
            body: wire.body,
            stream: wire.stream,
            partial_images: wire.partial_images,
            mode: PhantomData,
        })
    }
}

impl CreateImageEditJsonRequest<MediaNonStreaming> {
    /// Construct a non-streaming JSON edit request.
    #[must_use]
    pub fn new(image: ImageReference, prompt: impl Into<String>) -> Self {
        Self {
            body: ImageEditJsonRequestBody::new(image, prompt),
            stream: Omittable::Omitted,
            partial_images: Omittable::Omitted,
            mode: PhantomData,
        }
    }

    /// Switch to JSON edit SSE output.
    #[must_use]
    pub fn into_streaming(self) -> CreateImageEditJsonRequest<MediaStreaming> {
        CreateImageEditJsonRequest {
            body: self.body,
            stream: Omittable::Value(Nullable::Value(true)),
            partial_images: Omittable::Omitted,
            mode: PhantomData,
        }
    }
}

impl CreateImageEditJsonRequest<MediaStreaming> {
    /// Request between zero and three partial images.
    #[must_use]
    pub fn with_partial_images(mut self, count: PartialImageCount) -> Self {
        self.partial_images = Omittable::Value(Nullable::Value(count));
        self
    }
}

impl<M> CreateImageEditJsonRequest<M>
where
    M: MediaStreamMode,
{
    /// Append another input image reference.
    #[must_use]
    pub fn with_image(mut self, image: ImageReference) -> Self {
        self.body.images.push(image);
        self
    }

    /// Set an edit mask.
    #[must_use]
    pub fn with_mask(mut self, mask: ImageReference) -> Self {
        self.body.mask = Omittable::Value(mask);
        self
    }

    /// Select a model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<ModelId>) -> Self {
        self.body.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }

    /// Select output quality.
    #[must_use]
    pub fn with_quality(mut self, quality: ImageQuality) -> Self {
        self.body.quality = Omittable::Value(Nullable::Value(quality));
        self
    }

    /// Select input fidelity.
    #[must_use]
    pub fn with_input_fidelity(mut self, fidelity: ImageInputFidelity) -> Self {
        self.body.input_fidelity = Omittable::Value(Nullable::Value(fidelity));
        self
    }
}

/// Non-streaming JSON image edit request.
pub type ImageEditJsonRequest = CreateImageEditJsonRequest<MediaNonStreaming>;

/// Streaming JSON image edit request.
pub type ImageEditJsonStreamRequest = CreateImageEditJsonRequest<MediaStreaming>;

/// Serde metadata encoded as text fields in a multipart image edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageEditMultipartMetadata {
    /// Edit instruction.
    pub prompt: String,
    /// Background behavior or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub background: Omittable<Nullable<ImageBackground>>,
    /// Model identifier or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<Nullable<ModelId>>,
    /// Number of edited images or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n: Omittable<Nullable<ImageCount>>,
    /// Output dimensions or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub size: Omittable<Nullable<ImageSize>>,
    /// Legacy URL/base64 response mode or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<Nullable<ImageResponseFormat>>,
    /// Encoded output format or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_format: Omittable<Nullable<ImageOutputFormat>>,
    /// Compression percentage or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_compression: Omittable<Nullable<ImageCompression>>,
    /// Stable end-user identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<String>,
    /// Input fidelity or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_fidelity: Omittable<Nullable<ImageInputFidelity>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    partial_images: Omittable<Nullable<PartialImageCount>>,
    /// Output quality or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quality: Omittable<Nullable<ImageQuality>>,
}

impl ImageEditMultipartMetadata {
    fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            background: Omittable::Omitted,
            model: Omittable::Omitted,
            n: Omittable::Omitted,
            size: Omittable::Omitted,
            response_format: Omittable::Omitted,
            output_format: Omittable::Omitted,
            output_compression: Omittable::Omitted,
            user: Omittable::Omitted,
            input_fidelity: Omittable::Omitted,
            stream: Omittable::Omitted,
            partial_images: Omittable::Omitted,
            quality: Omittable::Omitted,
        }
    }

    /// Whether `stream: true` is encoded.
    #[must_use]
    pub const fn is_streaming(&self) -> bool {
        matches!(self.stream, Omittable::Value(Nullable::Value(true)))
    }
}

/// Invalid number of multipart image edit sources.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("multipart image edit requires between 1 and 16 images, got {count}")]
pub struct ImageEditSourceCountError {
    count: usize,
}

impl ImageEditSourceCountError {
    /// Rejected source count.
    #[must_use]
    pub const fn count(self) -> usize {
        self.count
    }
}

/// Multipart image edit request. Binary images and mask never implement Serde.
#[derive(Clone, PartialEq)]
pub struct CreateImageEditMultipartRequest<M = MediaNonStreaming>
where
    M: MediaStreamMode,
{
    images: Vec<ReplayableMultipartSource>,
    mask: Omittable<ReplayableMultipartSource>,
    /// Serde-compatible text metadata.
    pub metadata: ImageEditMultipartMetadata,
    mode: PhantomData<fn() -> M>,
}

impl<M> fmt::Debug for CreateImageEditMultipartRequest<M>
where
    M: MediaStreamMode,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateImageEditMultipartRequest")
            .field("images", &self.images)
            .field("mask", &self.mask)
            .field("metadata", &self.metadata)
            .field("streaming", &M::STREAMING)
            .finish()
    }
}

impl CreateImageEditMultipartRequest<MediaNonStreaming> {
    /// Construct an edit request with one binary image.
    #[must_use]
    pub fn new(image: ReplayableMultipartSource, prompt: impl Into<String>) -> Self {
        Self {
            images: vec![image],
            mask: Omittable::Omitted,
            metadata: ImageEditMultipartMetadata::new(prompt),
            mode: PhantomData,
        }
    }

    /// Construct an edit request with one to sixteen binary images.
    pub fn from_images(
        images: impl IntoIterator<Item = ReplayableMultipartSource>,
        prompt: impl Into<String>,
    ) -> Result<Self, ImageEditSourceCountError> {
        let images: Vec<_> = images.into_iter().collect();
        if images.is_empty() || images.len() > 16 {
            return Err(ImageEditSourceCountError {
                count: images.len(),
            });
        }
        Ok(Self {
            images,
            mask: Omittable::Omitted,
            metadata: ImageEditMultipartMetadata::new(prompt),
            mode: PhantomData,
        })
    }

    /// Switch to multipart edit SSE output.
    #[must_use]
    pub fn into_streaming(self) -> CreateImageEditMultipartRequest<MediaStreaming> {
        let mut metadata = self.metadata;
        metadata.stream = Omittable::Value(Nullable::Value(true));
        CreateImageEditMultipartRequest {
            images: self.images,
            mask: self.mask,
            metadata,
            mode: PhantomData,
        }
    }
}

impl CreateImageEditMultipartRequest<MediaStreaming> {
    /// Request between zero and three partial images.
    #[must_use]
    pub fn with_partial_images(mut self, count: PartialImageCount) -> Self {
        self.metadata.partial_images = Omittable::Value(Nullable::Value(count));
        self
    }
}

impl<M> CreateImageEditMultipartRequest<M>
where
    M: MediaStreamMode,
{
    /// Binary input images in multipart field order.
    #[must_use]
    pub fn images(&self) -> &[ReplayableMultipartSource] {
        &self.images
    }

    /// Optional binary mask.
    #[must_use]
    pub fn mask(&self) -> Option<&ReplayableMultipartSource> {
        match &self.mask {
            Omittable::Value(mask) => Some(mask),
            Omittable::Omitted => None,
        }
    }

    /// Set a binary edit mask.
    #[must_use]
    pub fn with_mask(mut self, mask: ReplayableMultipartSource) -> Self {
        self.mask = Omittable::Value(mask);
        self
    }

    /// Select output quality.
    #[must_use]
    pub fn with_quality(mut self, quality: ImageQuality) -> Self {
        self.metadata.quality = Omittable::Value(Nullable::Value(quality));
        self
    }

    /// Select output format.
    #[must_use]
    pub fn with_output_format(mut self, format: ImageOutputFormat) -> Self {
        self.metadata.output_format = Omittable::Value(Nullable::Value(format));
        self
    }
}

/// Non-streaming multipart image edit request.
pub type ImageEditMultipartRequest = CreateImageEditMultipartRequest<MediaNonStreaming>;

/// Streaming multipart image edit request.
pub type ImageEditMultipartStreamRequest = CreateImageEditMultipartRequest<MediaStreaming>;

/// One generated or edited image.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeneratedImage {
    /// Base64-encoded image bytes.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub b64_json: Omittable<String>,
    /// Temporary image URL.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub url: Omittable<String>,
    /// Revised prompt, when returned by a compatible model.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub revised_prompt: Omittable<String>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl GeneratedImage {
    /// Decode the base64 image when present.
    pub fn decode(&self) -> Result<Option<Vec<u8>>, base64::DecodeError> {
        match &self.b64_json {
            Omittable::Value(data) => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map(Some),
            Omittable::Omitted => Ok(None),
        }
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Input token breakdown for image generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageInputTokenDetails {
    /// Text input tokens.
    pub text_tokens: u64,
    /// Image input tokens.
    pub image_tokens: u64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageInputTokenDetails {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Output token breakdown for image generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageOutputTokenDetails {
    /// Image output tokens.
    pub image_tokens: u64,
    /// Text output tokens.
    pub text_tokens: u64,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageOutputTokenDetails {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Token usage for image generation and edit responses/events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageUsage {
    /// Input tokens.
    pub input_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Required input-token breakdown.
    pub input_tokens_details: ImageInputTokenDetails,
    /// Output-token detail returned by newer generation responses.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_tokens_details: Omittable<ImageOutputTokenDetails>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageUsage {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Non-streaming Images API response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImagesResponse {
    /// Creation timestamp in Unix seconds.
    pub created: u64,
    /// Generated images.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub data: Omittable<Vec<GeneratedImage>>,
    /// Effective background.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub background: Omittable<ImageBackground>,
    /// Effective encoded output format.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_format: Omittable<ImageOutputFormat>,
    /// Effective output dimensions.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub size: Omittable<ImageSize>,
    /// Effective quality.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quality: Omittable<ImageQuality>,
    /// Token usage.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<ImageUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImagesResponse {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(
    ImageGenerationPartialTag,
    Partial,
    "image_generation.partial_image"
);

/// Partial image emitted during generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationPartialEvent {
    #[serde(rename = "type")]
    kind: ImageGenerationPartialTag,
    /// Base64 image snapshot.
    pub b64_json: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Effective dimensions.
    pub size: ImageSize,
    /// Effective quality.
    pub quality: ImageQuality,
    /// Effective background.
    pub background: ImageBackground,
    /// Encoded output format.
    pub output_format: ImageOutputFormat,
    /// Zero-based partial image index.
    pub partial_image_index: u32,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageGenerationPartialEvent {
    /// Decode the base64 snapshot.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.b64_json)
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(
    ImageGenerationCompletedTag,
    Completed,
    "image_generation.completed"
);

/// Terminal image generation event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationCompletedEvent {
    #[serde(rename = "type")]
    kind: ImageGenerationCompletedTag,
    /// Base64 final image.
    pub b64_json: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Effective dimensions.
    pub size: ImageSize,
    /// Effective quality.
    pub quality: ImageQuality,
    /// Effective background.
    pub background: ImageBackground,
    /// Encoded output format.
    pub output_format: ImageOutputFormat,
    /// Token usage.
    pub usage: ImageUsage,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageGenerationCompletedEvent {
    /// Decode the final base64 image.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.b64_json)
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// One image-generation SSE event.
    pub enum ImageGenerationStreamEvent {
        Partial(ImageGenerationPartialEvent) = "image_generation.partial_image",
        Completed(ImageGenerationCompletedEvent) = "image_generation.completed"
    }
}

impl ImageGenerationStreamEvent {
    /// Whether this event completes image generation.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}

literal_tag!(ImageEditPartialTag, Partial, "image_edit.partial_image");

/// Partial image emitted during editing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageEditPartialEvent {
    #[serde(rename = "type")]
    kind: ImageEditPartialTag,
    /// Base64 image snapshot.
    pub b64_json: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Effective dimensions.
    pub size: ImageSize,
    /// Effective quality.
    pub quality: ImageQuality,
    /// Effective background.
    pub background: ImageBackground,
    /// Encoded output format.
    pub output_format: ImageOutputFormat,
    /// Zero-based partial image index.
    pub partial_image_index: u32,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageEditPartialEvent {
    /// Decode the base64 snapshot.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.b64_json)
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(ImageEditCompletedTag, Completed, "image_edit.completed");

/// Terminal image edit event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageEditCompletedEvent {
    #[serde(rename = "type")]
    kind: ImageEditCompletedTag,
    /// Base64 final image.
    pub b64_json: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Effective dimensions.
    pub size: ImageSize,
    /// Effective quality.
    pub quality: ImageQuality,
    /// Effective background.
    pub background: ImageBackground,
    /// Encoded output format.
    pub output_format: ImageOutputFormat,
    /// Token usage.
    pub usage: ImageUsage,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ImageEditCompletedEvent {
    /// Decode the final base64 image.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.b64_json)
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// One image-edit SSE event.
    pub enum ImageEditStreamEvent {
        Partial(ImageEditPartialEvent) = "image_edit.partial_image",
        Completed(ImageEditCompletedEvent) = "image_edit.completed"
    }
}

impl ImageEditStreamEvent {
    /// Whether this event completes image editing.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}
