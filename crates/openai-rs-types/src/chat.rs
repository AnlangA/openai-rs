//! Bidirectional wire types for the Chat Completions API.
//!
//! Request builders cover text, image, audio, file, function-tool, custom-tool,
//! and structured-output inputs without requiring callers to format JSON text.
//! Tagged unions reject malformed payloads for known tags while retaining a
//! future tag and its complete semantic JSON object.

use std::{collections::BTreeMap, fmt, marker::PhantomData};

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    ExtraFields, FileId, JsonText, MAX_RESPONSE_METADATA_KEY_CHARS, MAX_RESPONSE_METADATA_PAIRS,
    MAX_RESPONSE_METADATA_VALUE_CHARS, MAX_SAFETY_IDENTIFIER_CHARS, MAX_TOP_LOGPROBS, ModelId,
    ModerationInputType, Nullable, Omittable,
    responses::{ModerationPolicy, UnknownTaggedObject},
};

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
            /// A future tagged object retained without losing fields.
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
                let tag = type_discriminator(&value).map_err(D::Error::custom)?;
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

fn type_discriminator(value: &Value) -> Result<&str, &'static str> {
    let Value::Object(object) = value else {
        return Err("tagged Chat value must be a JSON object");
    };
    object
        .get("type")
        .ok_or("tagged Chat object is missing string field `type`")?
        .as_str()
        .ok_or("tagged Chat object field `type` must be a string")
}

fn role_discriminator(value: &Value) -> Result<&str, &'static str> {
    let Value::Object(object) = value else {
        return Err("Chat message must be a JSON object");
    };
    object
        .get("role")
        .ok_or("Chat message is missing string field `role`")?
        .as_str()
        .ok_or("Chat message field `role` must be a string")
}

fn serialize_object<T: Serialize>(
    value: &T,
    context: &'static str,
) -> Result<Map<String, Value>, serde_json::Error> {
    match serde_json::to_value(value)? {
        Value::Object(object) => Ok(object),
        _ => Err(<serde_json::Error as serde::ser::Error>::custom(context)),
    }
}

crate::open_string_enum! {
    /// Role carried by Chat messages and streaming deltas.
    pub enum ChatRole {
        Developer = "developer",
        System = "system",
        User = "user",
        Assistant = "assistant",
        Tool = "tool",
        Function = "function"
    }
}

crate::open_string_enum! {
    /// Image fidelity requested for an image content part.
    pub enum ChatImageDetail {
        Auto = "auto",
        Low = "low",
        High = "high"
    }
}

crate::open_string_enum! {
    /// Encoded input-audio format accepted by Chat Completions.
    pub enum ChatInputAudioFormat {
        Wav = "wav",
        Mp3 = "mp3"
    }
}

crate::open_string_enum! {
    /// Audio format requested from an audio-capable Chat model.
    pub enum ChatOutputAudioFormat {
        Wav = "wav",
        Aac = "aac",
        Mp3 = "mp3",
        Flac = "flac",
        Opus = "opus",
        Pcm16 = "pcm16"
    }
}

crate::open_string_enum! {
    /// Output modality requested from the model.
    pub enum ChatModality {
        Text = "text",
        Audio = "audio"
    }
}

crate::open_string_enum! {
    /// Why a Chat choice stopped producing tokens.
    pub enum ChatFinishReason {
        Stop = "stop",
        Length = "length",
        ToolCalls = "tool_calls",
        ContentFilter = "content_filter",
        FunctionCall = "function_call"
    }
}

crate::open_string_enum! {
    /// Service tier requested or reported for a Chat completion.
    pub enum ChatServiceTier {
        Auto = "auto",
        Default = "default",
        Flex = "flex",
        Scale = "scale",
        Priority = "priority",
        Fast = "fast"
    }
}

crate::open_string_enum! {
    /// Reasoning effort requested from a compatible model.
    pub enum ChatReasoningEffort {
        None = "none",
        Minimal = "minimal",
        Low = "low",
        Medium = "medium",
        High = "high",
        XHigh = "xhigh",
        Max = "max"
    }
}

crate::open_string_enum! {
    /// Requested verbosity of the generated answer.
    pub enum ChatVerbosity {
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

crate::open_string_enum! {
    /// High-level tool selection mode.
    pub enum ChatToolChoiceMode {
        None = "none",
        Auto = "auto",
        Required = "required"
    }
}

crate::open_string_enum! {
    /// Selection mode within an allowed-tools constraint.
    pub enum ChatAllowedToolsMode {
        Auto = "auto",
        Required = "required"
    }
}

crate::open_string_enum! {
    /// Kind of a tool or tool-call chunk.
    pub enum ChatToolKind {
        Function = "function",
        Custom = "custom"
    }
}

crate::open_string_enum! {
    /// Amount of context allocated to Chat web search.
    pub enum ChatWebSearchContextSize {
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

crate::open_string_enum! {
    /// Retention requested for prompt-cache entries.
    pub enum ChatPromptCacheRetention {
        InMemory = "in_memory",
        TwentyFourHours = "24h"
    }
}

/// Shared with Responses; the pinned wire values are `implicit` / `explicit`.
pub use crate::responses::PromptCacheMode as ChatPromptCacheMode;
/// Shared with Responses; the pinned wire value is `30m`.
pub use crate::responses::PromptCacheTtl as ChatPromptCacheTtl;

crate::open_string_enum! {
    /// Grammar syntax used by a custom tool.
    pub enum ChatGrammarSyntax {
        Lark = "lark",
        Regex = "regex"
    }
}

crate::open_string_enum! {
    /// Object discriminator for Chat completion responses.
    pub enum ChatCompletionObject {
        Completion = "chat.completion",
        Chunk = "chat.completion.chunk"
    }
}

/// Shared with Responses; the pinned wire object is `{ "mode": "explicit" }`.
pub use crate::responses::PromptCacheBreakpoint as ChatPromptCacheBreakpoint;

/// Chat create-request prompt-cache options.
///
/// Official `CreateChatCompletionRequest` `$ref`s `PromptCacheOptionsParam`
/// (no required properties). Chat completions do not echo the response
/// `PromptCacheOptions` object, which requires both `ttl` and `mode`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPromptCacheOptions {
    /// Minimum cache lifetime.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub ttl: Omittable<ChatPromptCacheTtl>,
    /// Whether implicit cache breakpoints are enabled.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub mode: Omittable<ChatPromptCacheMode>,
}

literal_tag!(TextContentTag, Text, "text");

/// Text input content part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatTextContentPart {
    #[serde(rename = "type")]
    kind: TextContentTag,
    /// Text supplied to the model.
    pub text: String,
    /// Optional cache boundary after this part.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
}

impl ChatTextContentPart {
    /// Construct a text part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: TextContentTag::Text,
            text: text.into(),
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Mark a cache boundary after this part.
    #[must_use]
    pub fn with_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(ChatPromptCacheBreakpoint::explicit());
        self
    }
}

literal_tag!(ImageContentTag, ImageUrl, "image_url");

/// URL or data URL supplied as an image input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatImageUrl {
    /// Image URL or base64 data URL.
    pub url: String,
    /// Requested fidelity.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub detail: Omittable<ChatImageDetail>,
}

impl ChatImageUrl {
    /// Construct an image reference.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            detail: Omittable::Omitted,
        }
    }

    /// Select image fidelity.
    #[must_use]
    pub fn with_detail(mut self, detail: ChatImageDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }
}

/// Image input content part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatImageContentPart {
    #[serde(rename = "type")]
    kind: ImageContentTag,
    /// Image location and fidelity.
    pub image_url: ChatImageUrl,
    /// Optional cache boundary after this part.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
}

impl ChatImageContentPart {
    /// Construct an image content part from a URL or data URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            kind: ImageContentTag::ImageUrl,
            image_url: ChatImageUrl::new(url),
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Construct an inline image data URL from raw bytes.
    #[must_use]
    pub fn from_bytes(media_type: &str, bytes: impl AsRef<[u8]>) -> Self {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
        Self::new(format!("data:{media_type};base64,{data}"))
    }

    /// Select image fidelity.
    #[must_use]
    pub fn with_detail(mut self, detail: ChatImageDetail) -> Self {
        self.image_url = self.image_url.with_detail(detail);
        self
    }

    /// Mark a cache boundary after this part.
    #[must_use]
    pub fn with_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(ChatPromptCacheBreakpoint::explicit());
        self
    }
}

literal_tag!(AudioContentTag, InputAudio, "input_audio");

/// Base64 input audio and its encoding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatInputAudio {
    /// Base64-encoded audio bytes.
    pub data: String,
    /// Encoding of `data`.
    pub format: ChatInputAudioFormat,
}

/// Audio input content part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAudioContentPart {
    #[serde(rename = "type")]
    kind: AudioContentTag,
    /// Encoded audio input.
    pub input_audio: ChatInputAudio,
    /// Optional cache boundary after this part.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
}

impl ChatAudioContentPart {
    /// Construct an audio content part from base64 data.
    #[must_use]
    pub fn new(data: impl Into<String>, format: ChatInputAudioFormat) -> Self {
        Self {
            kind: AudioContentTag::InputAudio,
            input_audio: ChatInputAudio {
                data: data.into(),
                format,
            },
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Encode raw audio bytes into the required base64 wire representation.
    #[must_use]
    pub fn from_bytes(bytes: impl AsRef<[u8]>, format: ChatInputAudioFormat) -> Self {
        Self::new(
            base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()),
            format,
        )
    }

    /// Mark a cache boundary after this part.
    #[must_use]
    pub fn with_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(ChatPromptCacheBreakpoint::explicit());
        self
    }
}

literal_tag!(FileContentTag, File, "file");

/// File payload referenced by a Chat content part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatInputFile {
    /// Filename used with inline base64 data.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub filename: Omittable<String>,
    /// Base64-encoded inline file data.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_data: Omittable<String>,
    /// Previously uploaded file identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_id: Omittable<FileId>,
}

impl ChatInputFile {
    /// Reference an uploaded file.
    #[must_use]
    pub fn from_id(file_id: impl Into<FileId>) -> Self {
        Self {
            file_id: Omittable::Value(file_id.into()),
            ..Self::default()
        }
    }

    /// Supply inline base64 data with its filename.
    #[must_use]
    pub fn from_base64(filename: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            filename: Omittable::Value(filename.into()),
            file_data: Omittable::Value(data.into()),
            file_id: Omittable::Omitted,
        }
    }

    /// Encode raw file bytes into the inline base64 wire representation.
    #[must_use]
    pub fn from_bytes(filename: impl Into<String>, bytes: impl AsRef<[u8]>) -> Self {
        Self::from_base64(
            filename,
            base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()),
        )
    }
}

/// File input content part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFileContentPart {
    #[serde(rename = "type")]
    kind: FileContentTag,
    /// Uploaded or inline file payload.
    pub file: ChatInputFile,
    /// Optional cache boundary after this part.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
}

impl ChatFileContentPart {
    /// Construct a file content part.
    #[must_use]
    pub fn new(file: ChatInputFile) -> Self {
        Self {
            kind: FileContentTag::File,
            file,
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Mark a cache boundary after this part.
    #[must_use]
    pub fn with_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(ChatPromptCacheBreakpoint::explicit());
        self
    }
}

literal_tag!(RefusalContentTag, Refusal, "refusal");

/// Assistant refusal content part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatRefusalContentPart {
    #[serde(rename = "type")]
    kind: RefusalContentTag,
    /// Refusal text.
    pub refusal: String,
}

impl ChatRefusalContentPart {
    /// Construct a refusal part.
    #[must_use]
    pub fn new(refusal: impl Into<String>) -> Self {
        Self {
            kind: RefusalContentTag::Refusal,
            refusal: refusal.into(),
        }
    }
}

strict_tagged_union! {
    /// Content parts accepted in a user message.
    pub enum ChatUserContentPart {
        Text(ChatTextContentPart) = "text",
        Image(ChatImageContentPart) = "image_url",
        Audio(ChatAudioContentPart) = "input_audio",
        File(ChatFileContentPart) = "file"
    }
}

strict_tagged_union! {
    /// Content parts accepted in an assistant request message.
    pub enum ChatAssistantContentPart {
        Text(ChatTextContentPart) = "text",
        Refusal(ChatRefusalContentPart) = "refusal"
    }
}

impl From<ChatTextContentPart> for ChatUserContentPart {
    fn from(value: ChatTextContentPart) -> Self {
        Self::Text(value)
    }
}

impl From<ChatImageContentPart> for ChatUserContentPart {
    fn from(value: ChatImageContentPart) -> Self {
        Self::Image(value)
    }
}

impl From<ChatAudioContentPart> for ChatUserContentPart {
    fn from(value: ChatAudioContentPart) -> Self {
        Self::Audio(value)
    }
}

impl From<ChatFileContentPart> for ChatUserContentPart {
    fn from(value: ChatFileContentPart) -> Self {
        Self::File(value)
    }
}

impl From<ChatTextContentPart> for ChatAssistantContentPart {
    fn from(value: ChatTextContentPart) -> Self {
        Self::Text(value)
    }
}

impl From<ChatRefusalContentPart> for ChatAssistantContentPart {
    fn from(value: ChatRefusalContentPart) -> Self {
        Self::Refusal(value)
    }
}

/// System or developer message content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatInstructionContent {
    /// Plain text shorthand.
    Text(String),
    /// Explicit text content parts.
    Parts(Vec<ChatTextContentPart>),
}

impl From<String> for ChatInstructionContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ChatInstructionContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// User message content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatUserContent {
    /// Plain text shorthand.
    Text(String),
    /// Multimodal parts.
    Parts(Vec<ChatUserContentPart>),
}

impl From<String> for ChatUserContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ChatUserContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// Assistant message content used when replaying prior assistant output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatAssistantContent {
    /// Plain assistant text.
    Text(String),
    /// Text/refusal content parts.
    Parts(Vec<ChatAssistantContentPart>),
}

/// Tool result content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatToolContent {
    /// Opaque tool output string.
    Text(String),
    /// Explicit text parts.
    Parts(Vec<ChatTextContentPart>),
}

literal_tag!(FunctionToolCallTag, Function, "function");

/// Name and JSON-text arguments of a completed function invocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionInvocation {
    /// Function name selected by the model.
    pub name: String,
    /// JSON encoded inside the wire string. Parsing remains lazy.
    pub arguments: JsonText,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatFunctionInvocation {
    /// Construct from already retained JSON text.
    #[must_use]
    pub fn new(name: impl Into<String>, arguments: JsonText) -> Self {
        Self {
            name: name.into(),
            arguments,
            extra: ExtraFields::new(),
        }
    }

    /// Serialize typed arguments into the inner JSON wire string.
    pub fn from_serializable<T: Serialize>(
        name: impl Into<String>,
        arguments: &T,
    ) -> Result<Self, serde_json::Error> {
        JsonText::from_serializable(arguments).map(|arguments| Self::new(name, arguments.cast()))
    }

    /// Parses the JSON arguments into a declared Rust type.
    pub fn arguments_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_str(self.arguments.as_str())
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A completed function tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionToolCall {
    /// Tool-call identifier used by the matching tool message.
    pub id: String,
    #[serde(rename = "type")]
    kind: FunctionToolCallTag,
    /// Function invocation details.
    pub function: ChatFunctionInvocation,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatFunctionToolCall {
    /// Construct from a function invocation.
    #[must_use]
    pub fn new(id: impl Into<String>, function: ChatFunctionInvocation) -> Self {
        Self {
            id: id.into(),
            kind: FunctionToolCallTag::Function,
            function,
            extra: ExtraFields::new(),
        }
    }

    /// Serialize typed arguments into the nested function-call string.
    pub fn from_serializable<T: Serialize>(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: &T,
    ) -> Result<Self, serde_json::Error> {
        ChatFunctionInvocation::from_serializable(name, arguments)
            .map(|function| Self::new(id, function))
    }

    /// Parses the JSON arguments into a declared Rust type.
    pub fn arguments_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        self.function.arguments_as()
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(CustomToolCallTag, Custom, "custom");

/// Name and free-form input of a completed custom-tool invocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCustomInvocation {
    /// Custom tool name.
    pub name: String,
    /// Free-form input generated for the tool.
    pub input: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCustomInvocation {
    /// Construct a custom invocation.
    #[must_use]
    pub fn new(name: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            input: input.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A completed custom tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCustomToolCall {
    /// Tool-call identifier.
    pub id: String,
    #[serde(rename = "type")]
    kind: CustomToolCallTag,
    /// Custom invocation details.
    pub custom: ChatCustomInvocation,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCustomToolCall {
    /// Construct a custom tool call.
    #[must_use]
    pub fn new(id: impl Into<String>, custom: ChatCustomInvocation) -> Self {
        Self {
            id: id.into(),
            kind: CustomToolCallTag::Custom,
            custom,
            extra: ExtraFields::new(),
        }
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// A function or custom tool call in an assistant message.
    pub enum ChatToolCall {
        Function(ChatFunctionToolCall) = "function",
        Custom(ChatCustomToolCall) = "custom"
    }
}

impl From<ChatFunctionToolCall> for ChatToolCall {
    fn from(value: ChatFunctionToolCall) -> Self {
        Self::Function(value)
    }
}

impl From<ChatCustomToolCall> for ChatToolCall {
    fn from(value: ChatCustomToolCall) -> Self {
        Self::Custom(value)
    }
}

/// Deprecated function call representation retained for wire compatibility.
pub type ChatLegacyFunctionCall = ChatFunctionInvocation;

literal_tag!(DeveloperMessageTag, Developer, "developer");

/// Developer instruction message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatDeveloperMessage {
    /// Instruction content.
    pub content: ChatInstructionContent,
    #[serde(rename = "role")]
    role: DeveloperMessageTag,
    /// Optional participant name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

impl ChatDeveloperMessage {
    /// Construct a developer message.
    #[must_use]
    pub fn new(content: impl Into<ChatInstructionContent>) -> Self {
        Self {
            content: content.into(),
            role: DeveloperMessageTag::Developer,
            name: Omittable::Omitted,
        }
    }

    /// Attach a participant name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }
}

literal_tag!(SystemMessageTag, System, "system");

/// System instruction message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatSystemMessage {
    /// Instruction content.
    pub content: ChatInstructionContent,
    #[serde(rename = "role")]
    role: SystemMessageTag,
    /// Optional participant name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

impl ChatSystemMessage {
    /// Construct a system message.
    #[must_use]
    pub fn new(content: impl Into<ChatInstructionContent>) -> Self {
        Self {
            content: content.into(),
            role: SystemMessageTag::System,
            name: Omittable::Omitted,
        }
    }

    /// Attach a participant name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }
}

literal_tag!(UserMessageTag, User, "user");

/// User message, including multimodal content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatUserMessage {
    /// Text or multimodal content.
    pub content: ChatUserContent,
    #[serde(rename = "role")]
    role: UserMessageTag,
    /// Optional participant name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

impl ChatUserMessage {
    /// Construct a plain-text user message.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(ChatUserContent::Text(text.into()))
    }

    /// Construct a user message from multimodal parts.
    #[must_use]
    pub fn parts(parts: impl IntoIterator<Item = ChatUserContentPart>) -> Self {
        Self::new(ChatUserContent::Parts(parts.into_iter().collect()))
    }

    /// Construct from an explicit content representation.
    #[must_use]
    pub fn new(content: ChatUserContent) -> Self {
        Self {
            content,
            role: UserMessageTag::User,
            name: Omittable::Omitted,
        }
    }

    /// Attach a participant name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }
}

literal_tag!(AssistantMessageTag, Assistant, "assistant");

/// Assistant message supplied as prior conversation state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAssistantMessage {
    #[serde(rename = "role")]
    role: AssistantMessageTag,
    /// Assistant content; may be omitted when tool calls are present.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub content: Omittable<Nullable<ChatAssistantContent>>,
    /// Optional refusal text.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub refusal: Omittable<Nullable<String>>,
    /// Optional participant name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    /// IDs of earlier audio output reused in a conversation.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<Nullable<ChatAssistantAudioReference>>,
    /// Tool calls generated by this assistant turn.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_calls: Omittable<Vec<ChatToolCall>>,
    /// Deprecated single-function call.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function_call: Omittable<Nullable<ChatLegacyFunctionCall>>,
}

impl ChatAssistantMessage {
    /// Construct a text assistant message.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            role: AssistantMessageTag::Assistant,
            content: Omittable::Value(Nullable::Value(ChatAssistantContent::Text(text.into()))),
            refusal: Omittable::Omitted,
            name: Omittable::Omitted,
            audio: Omittable::Omitted,
            tool_calls: Omittable::Omitted,
            function_call: Omittable::Omitted,
        }
    }

    /// Construct an assistant message containing tool calls and explicit null
    /// content.
    #[must_use]
    pub fn tool_calls(tool_calls: impl IntoIterator<Item = ChatToolCall>) -> Self {
        Self {
            role: AssistantMessageTag::Assistant,
            content: Omittable::Value(Nullable::Null),
            refusal: Omittable::Omitted,
            name: Omittable::Omitted,
            audio: Omittable::Omitted,
            tool_calls: Omittable::Value(tool_calls.into_iter().collect()),
            function_call: Omittable::Omitted,
        }
    }
}

/// Reference to a prior assistant audio response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAssistantAudioReference {
    /// Audio response identifier.
    pub id: String,
}

literal_tag!(ToolMessageTag, Tool, "tool");

/// Tool output associated with a prior tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatToolMessage {
    #[serde(rename = "role")]
    role: ToolMessageTag,
    /// Tool result content.
    pub content: ChatToolContent,
    /// ID from the matching assistant tool call.
    pub tool_call_id: String,
}

impl ChatToolMessage {
    /// Construct a string tool result.
    #[must_use]
    pub fn new(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ToolMessageTag::Tool,
            content: ChatToolContent::Text(content.into()),
            tool_call_id: tool_call_id.into(),
        }
    }

    /// Serialize a typed tool result into the Chat string field.
    pub fn from_serializable<T: Serialize>(
        tool_call_id: impl Into<String>,
        content: &T,
    ) -> Result<Self, serde_json::Error> {
        serde_json::to_string(content).map(|content| Self::new(tool_call_id, content))
    }
}

literal_tag!(FunctionMessageTag, Function, "function");

/// Deprecated function result message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionMessage {
    #[serde(rename = "role")]
    role: FunctionMessageTag,
    /// Function output or explicit null.
    pub content: Nullable<String>,
    /// Function name.
    pub name: String,
}

impl ChatFunctionMessage {
    /// Construct a deprecated function result message.
    #[must_use]
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: FunctionMessageTag::Function,
            content: Nullable::Value(content.into()),
            name: name.into(),
        }
    }
}

/// A future message role and all of its wire fields.
#[derive(Clone, PartialEq)]
pub struct UnknownChatMessage {
    role: Box<str>,
    raw: Map<String, Value>,
}

impl UnknownChatMessage {
    fn from_value(value: Value) -> Result<Self, &'static str> {
        let role = role_discriminator(&value)?.into();
        let Value::Object(raw) = value else {
            return Err("Chat message must be a JSON object");
        };
        Ok(Self { role, raw })
    }

    /// Exact future role string.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Complete retained object, including `role`.
    #[must_use]
    pub const fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }
}

impl fmt::Debug for UnknownChatMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnknownChatMessage")
            .field("role", &self.role)
            .field("field_count", &self.raw.len())
            .finish()
    }
}

impl Serialize for UnknownChatMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

/// Any Chat request message.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatMessage {
    /// Developer instruction.
    Developer(ChatDeveloperMessage),
    /// System instruction.
    System(ChatSystemMessage),
    /// User input.
    User(ChatUserMessage),
    /// Prior assistant output.
    Assistant(ChatAssistantMessage),
    /// Tool result.
    Tool(ChatToolMessage),
    /// Deprecated function result.
    Function(ChatFunctionMessage),
    /// Future role retained verbatim.
    Unknown(UnknownChatMessage),
}

impl Serialize for ChatMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Developer(value) => value.serialize(serializer),
            Self::System(value) => value.serialize(serializer),
            Self::User(value) => value.serialize(serializer),
            Self::Assistant(value) => value.serialize(serializer),
            Self::Tool(value) => value.serialize(serializer),
            Self::Function(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let role = role_discriminator(&value).map_err(D::Error::custom)?;
        match role {
            "developer" => serde_json::from_value(value)
                .map(Self::Developer)
                .map_err(D::Error::custom),
            "system" => serde_json::from_value(value)
                .map(Self::System)
                .map_err(D::Error::custom),
            "user" => serde_json::from_value(value)
                .map(Self::User)
                .map_err(D::Error::custom),
            "assistant" => serde_json::from_value(value)
                .map(Self::Assistant)
                .map_err(D::Error::custom),
            "tool" => serde_json::from_value(value)
                .map(Self::Tool)
                .map_err(D::Error::custom),
            "function" => serde_json::from_value(value)
                .map(Self::Function)
                .map_err(D::Error::custom),
            _ => UnknownChatMessage::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ChatDeveloperMessage> for ChatMessage {
    fn from(value: ChatDeveloperMessage) -> Self {
        Self::Developer(value)
    }
}

impl From<ChatSystemMessage> for ChatMessage {
    fn from(value: ChatSystemMessage) -> Self {
        Self::System(value)
    }
}

impl From<ChatUserMessage> for ChatMessage {
    fn from(value: ChatUserMessage) -> Self {
        Self::User(value)
    }
}

impl From<ChatAssistantMessage> for ChatMessage {
    fn from(value: ChatAssistantMessage) -> Self {
        Self::Assistant(value)
    }
}

impl From<ChatToolMessage> for ChatMessage {
    fn from(value: ChatToolMessage) -> Self {
        Self::Tool(value)
    }
}

impl From<ChatFunctionMessage> for ChatMessage {
    fn from(value: ChatFunctionMessage) -> Self {
        Self::Function(value)
    }
}

/// Function definition supplied to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionDefinition {
    /// Function name.
    pub name: String,
    /// Description used for tool selection.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
    /// JSON Schema object for function arguments.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parameters: Omittable<Map<String, Value>>,
    /// Strict schema-adherence setting.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub strict: Omittable<Nullable<bool>>,
}

impl ChatFunctionDefinition {
    /// Construct a function definition without a parameter schema.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Omittable::Omitted,
            parameters: Omittable::Omitted,
            strict: Omittable::Omitted,
        }
    }

    /// Attach a human-readable description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Serialize a typed schema representation into the parameters object.
    pub fn with_parameters<T: Serialize>(
        mut self,
        parameters: &T,
    ) -> Result<Self, serde_json::Error> {
        self.parameters = Omittable::Value(serialize_object(
            parameters,
            "function parameters must serialize as a JSON object",
        )?);
        Ok(self)
    }

    /// Enable or disable strict schema adherence.
    #[must_use]
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Omittable::Value(Nullable::Value(strict));
        self
    }
}

/// Deprecated function entry for the legacy `functions` request field.
///
/// Mirrors pinned `ChatCompletionFunctions`: unlike
/// [`ChatFunctionDefinition`] (the `tools[].function` shape), the legacy
/// entry carries no `strict` schema-adherence field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionFunction {
    /// Function name.
    pub name: String,
    /// Description used for tool selection.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
    /// JSON Schema object for function arguments.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parameters: Omittable<Map<String, Value>>,
}

impl ChatCompletionFunction {
    /// Construct a legacy function entry without a parameter schema.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Omittable::Omitted,
            parameters: Omittable::Omitted,
        }
    }

    /// Attach a human-readable description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Serialize a typed schema representation into the parameters object.
    pub fn with_parameters<T: Serialize>(
        mut self,
        parameters: &T,
    ) -> Result<Self, serde_json::Error> {
        self.parameters = Omittable::Value(serialize_object(
            parameters,
            "function parameters must serialize as a JSON object",
        )?);
        Ok(self)
    }
}

literal_tag!(FunctionToolTag, Function, "function");

/// A function tool definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionTool {
    #[serde(rename = "type")]
    kind: FunctionToolTag,
    /// Function exposed to the model.
    pub function: ChatFunctionDefinition,
}

impl ChatFunctionTool {
    /// Construct a function tool.
    #[must_use]
    pub const fn new(function: ChatFunctionDefinition) -> Self {
        Self {
            kind: FunctionToolTag::Function,
            function,
        }
    }

    /// Builds a strict Chat function tool from `T`'s `schemars` JSON Schema definition.
    #[cfg(feature = "structured-output")]
    pub fn for_type<T: schemars::JsonSchema>(
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Self, crate::StructuredError> {
        let name = name.into();
        let description = description.into();
        let mut schema = serde_json::to_value(schemars::schema_for!(T))
            .map_err(crate::StructuredError::Encode)?;
        crate::structured::normalize_strict_schema(&mut schema)?;
        let obj = schema
            .as_object()
            .cloned()
            .ok_or(crate::StructuredError::RootMustBeObject)?;
        let mut function = ChatFunctionDefinition::new(name)
            .with_description(description)
            .with_strict(true);
        function.parameters = Omittable::Value(obj);
        Ok(Self::new(function))
    }
}

literal_tag!(CustomTextFormatTag, Text, "text");

/// Unconstrained custom-tool text input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCustomTextFormat {
    #[serde(rename = "type")]
    kind: CustomTextFormatTag,
}

impl ChatCustomTextFormat {
    /// Construct a free-form text format.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: CustomTextFormatTag::Text,
        }
    }
}

impl Default for ChatCustomTextFormat {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(CustomGrammarFormatTag, Grammar, "grammar");

/// Grammar definition for a custom tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCustomGrammar {
    /// Grammar source.
    pub definition: String,
    /// Lark or regular-expression syntax.
    pub syntax: ChatGrammarSyntax,
}

/// Grammar-constrained custom-tool input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCustomGrammarFormat {
    #[serde(rename = "type")]
    kind: CustomGrammarFormatTag,
    /// Grammar definition and syntax.
    pub grammar: ChatCustomGrammar,
}

impl ChatCustomGrammarFormat {
    /// Construct a grammar format.
    #[must_use]
    pub fn new(definition: impl Into<String>, syntax: ChatGrammarSyntax) -> Self {
        Self {
            kind: CustomGrammarFormatTag::Grammar,
            grammar: ChatCustomGrammar {
                definition: definition.into(),
                syntax,
            },
        }
    }
}

strict_tagged_union! {
    /// Input format for a custom Chat tool.
    pub enum ChatCustomToolFormat {
        Text(ChatCustomTextFormat) = "text",
        Grammar(ChatCustomGrammarFormat) = "grammar"
    }
}

/// Properties of a custom tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCustomToolDefinition {
    /// Custom tool name.
    pub name: String,
    /// Description used by the model.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
    /// Free-form or grammar-constrained input.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub format: Omittable<ChatCustomToolFormat>,
}

impl ChatCustomToolDefinition {
    /// Construct a custom tool using the service's default text format.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Omittable::Omitted,
            format: Omittable::Omitted,
        }
    }

    /// Attach a description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Select an explicit custom-tool input format.
    #[must_use]
    pub fn with_format(mut self, format: ChatCustomToolFormat) -> Self {
        self.format = Omittable::Value(format);
        self
    }
}

literal_tag!(CustomToolTag, Custom, "custom");

/// Custom tool definition supplied to Chat Completions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCustomTool {
    #[serde(rename = "type")]
    kind: CustomToolTag,
    /// Custom tool properties.
    pub custom: ChatCustomToolDefinition,
}

impl ChatCustomTool {
    /// Construct a custom tool.
    #[must_use]
    pub const fn new(custom: ChatCustomToolDefinition) -> Self {
        Self {
            kind: CustomToolTag::Custom,
            custom,
        }
    }
}

strict_tagged_union! {
    /// A tool exposed to a Chat model.
    pub enum ChatTool {
        Function(ChatFunctionTool) = "function",
        Custom(ChatCustomTool) = "custom"
    }
}

impl From<ChatFunctionTool> for ChatTool {
    fn from(value: ChatFunctionTool) -> Self {
        Self::Function(value)
    }
}

impl From<ChatCustomTool> for ChatTool {
    fn from(value: ChatCustomTool) -> Self {
        Self::Custom(value)
    }
}

literal_tag!(NamedFunctionChoiceTag, Function, "function");

/// Forces the model to call a named function tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatNamedFunctionChoice {
    #[serde(rename = "type")]
    kind: NamedFunctionChoiceTag,
    /// Selected function.
    pub function: ChatNamedTool,
}

impl ChatNamedFunctionChoice {
    /// Construct a named function choice.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: NamedFunctionChoiceTag::Function,
            function: ChatNamedTool { name: name.into() },
        }
    }
}

literal_tag!(NamedCustomChoiceTag, Custom, "custom");

/// Forces the model to call a named custom tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatNamedCustomChoice {
    #[serde(rename = "type")]
    kind: NamedCustomChoiceTag,
    /// Selected custom tool.
    pub custom: ChatNamedTool,
}

impl ChatNamedCustomChoice {
    /// Construct a named custom-tool choice.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: NamedCustomChoiceTag::Custom,
            custom: ChatNamedTool { name: name.into() },
        }
    }
}

/// Name-only tool reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatNamedTool {
    /// Tool name.
    pub name: String,
}

strict_tagged_union! {
    /// A concrete tool reference used by tool-choice constraints.
pub enum ChatToolReference {
        Function(ChatNamedFunctionChoice) = "function",
        Custom(ChatNamedCustomChoice) = "custom"
    }
}

impl From<ChatNamedFunctionChoice> for ChatToolReference {
    fn from(value: ChatNamedFunctionChoice) -> Self {
        Self::Function(value)
    }
}

impl From<ChatNamedCustomChoice> for ChatToolReference {
    fn from(value: ChatNamedCustomChoice) -> Self {
        Self::Custom(value)
    }
}

/// One tool descriptor inside an allowed-tools constraint.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatAllowedTool {
    /// A typed name-only function or custom tool reference.
    Reference(ChatToolReference),
    /// An arbitrary descriptor allowed by the forward-compatible wire schema.
    Arbitrary(Map<String, Value>),
}

impl ChatAllowedTool {
    /// Construct an arbitrary descriptor from a typed serializable object.
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        serialize_object(value, "allowed tool must serialize as a JSON object").map(Self::Arbitrary)
    }
}

impl Serialize for ChatAllowedTool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Reference(reference) => reference.serialize(serializer),
            Self::Arbitrary(object) => object.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatAllowedTool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(object) = &value else {
            return Err(D::Error::custom("allowed tool must be a JSON object"));
        };

        match object.get("type").and_then(Value::as_str) {
            Some("function" | "custom") if is_name_only_allowed_tool(object) => {
                serde_json::from_value(value)
                    .map(Self::Reference)
                    .map_err(D::Error::custom)
            }
            _ => Ok(Self::Arbitrary(object.clone())),
        }
    }
}

fn is_name_only_allowed_tool(object: &Map<String, Value>) -> bool {
    let Some(kind) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    let nested_key = match kind {
        "function" => "function",
        "custom" => "custom",
        _ => return false,
    };
    if object.keys().any(|key| key != "type" && key != nested_key) {
        return false;
    }
    match object.get(nested_key) {
        Some(Value::Object(nested)) => {
            nested.len() == 1 && nested.get("name").is_some_and(Value::is_string)
        }
        _ => false,
    }
}

impl From<ChatToolReference> for ChatAllowedTool {
    fn from(value: ChatToolReference) -> Self {
        Self::Reference(value)
    }
}

impl From<ChatNamedFunctionChoice> for ChatAllowedTool {
    fn from(value: ChatNamedFunctionChoice) -> Self {
        Self::Reference(value.into())
    }
}

impl From<ChatNamedCustomChoice> for ChatAllowedTool {
    fn from(value: ChatNamedCustomChoice) -> Self {
        Self::Reference(value.into())
    }
}

/// Predefined set of tools available to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAllowedTools {
    /// Whether using an allowed tool is optional or required.
    pub mode: ChatAllowedToolsMode,
    /// Named tool references.
    pub tools: Vec<ChatAllowedTool>,
}

impl ChatAllowedTools {
    /// Construct an allowed-tools constraint.
    #[must_use]
    pub fn new<I, T>(mode: ChatAllowedToolsMode, tools: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<ChatAllowedTool>,
    {
        Self {
            mode,
            tools: tools.into_iter().map(Into::into).collect(),
        }
    }
}

literal_tag!(AllowedToolsChoiceTag, AllowedTools, "allowed_tools");

/// Tool choice constrained to an allowed set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAllowedToolsChoice {
    #[serde(rename = "type")]
    kind: AllowedToolsChoiceTag,
    /// Allowed tool set.
    pub allowed_tools: ChatAllowedTools,
}

impl ChatAllowedToolsChoice {
    /// Construct an allowed-tools choice.
    #[must_use]
    pub const fn new(allowed_tools: ChatAllowedTools) -> Self {
        Self {
            kind: AllowedToolsChoiceTag::AllowedTools,
            allowed_tools,
        }
    }
}

/// Tool selection policy.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatToolChoice {
    /// `none`, `auto`, `required`, or a future string mode.
    Mode(ChatToolChoiceMode),
    /// Force one function tool.
    Function(ChatNamedFunctionChoice),
    /// Force one custom tool.
    Custom(ChatNamedCustomChoice),
    /// Constrain selection to an allowed set.
    Allowed(ChatAllowedToolsChoice),
    /// Future object form retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ChatToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Mode(value) => value.serialize(serializer),
            Self::Function(value) => value.serialize(serializer),
            Self::Custom(value) => value.serialize(serializer),
            Self::Allowed(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if let Value::String(mode) = value {
            return Ok(Self::Mode(ChatToolChoiceMode::from_raw(mode)));
        }

        let tag = type_discriminator(&value).map_err(D::Error::custom)?;
        match tag {
            "function" => serde_json::from_value(value)
                .map(Self::Function)
                .map_err(D::Error::custom),
            "custom" => serde_json::from_value(value)
                .map(Self::Custom)
                .map_err(D::Error::custom),
            "allowed_tools" => serde_json::from_value(value)
                .map(Self::Allowed)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ChatToolChoiceMode> for ChatToolChoice {
    fn from(value: ChatToolChoiceMode) -> Self {
        Self::Mode(value)
    }
}

impl From<ChatNamedFunctionChoice> for ChatToolChoice {
    fn from(value: ChatNamedFunctionChoice) -> Self {
        Self::Function(value)
    }
}

impl From<ChatNamedCustomChoice> for ChatToolChoice {
    fn from(value: ChatNamedCustomChoice) -> Self {
        Self::Custom(value)
    }
}

impl From<ChatAllowedToolsChoice> for ChatToolChoice {
    fn from(value: ChatAllowedToolsChoice) -> Self {
        Self::Allowed(value)
    }
}

literal_tag!(ResponseFormatTextTag, Text, "text");

/// Plain text response format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponseFormatText {
    #[serde(rename = "type")]
    kind: ResponseFormatTextTag,
}

impl ChatResponseFormatText {
    /// Construct the plain text response format.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: ResponseFormatTextTag::Text,
        }
    }
}

impl Default for ChatResponseFormatText {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(ResponseFormatJsonObjectTag, JsonObject, "json_object");

/// Legacy JSON-object response mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponseFormatJsonObject {
    #[serde(rename = "type")]
    kind: ResponseFormatJsonObjectTag,
}

impl ChatResponseFormatJsonObject {
    /// Construct JSON-object mode.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: ResponseFormatJsonObjectTag::JsonObject,
        }
    }
}

impl Default for ChatResponseFormatJsonObject {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(ResponseFormatJsonSchemaTag, JsonSchema, "json_schema");

/// Named JSON Schema used for structured Chat output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatJsonSchemaDefinition {
    /// Schema name.
    pub name: String,
    /// Optional model-facing description.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
    /// JSON Schema value.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub schema: Omittable<Map<String, Value>>,
    /// Strict schema-adherence setting.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub strict: Omittable<Nullable<bool>>,
}

impl ChatJsonSchemaDefinition {
    /// Construct a named definition.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Omittable::Omitted,
            schema: Omittable::Omitted,
            strict: Omittable::Omitted,
        }
    }

    /// Serialize a typed schema representation.
    pub fn with_schema<T: Serialize>(mut self, schema: &T) -> Result<Self, serde_json::Error> {
        self.schema = Omittable::Value(serialize_object(
            schema,
            "response format schema must serialize as a JSON object",
        )?);
        Ok(self)
    }

    /// Attach a model-facing description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Enable or disable strict schema adherence.
    #[must_use]
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Omittable::Value(Nullable::Value(strict));
        self
    }
}

/// Structured JSON Schema response format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatResponseFormatJsonSchema {
    #[serde(rename = "type")]
    kind: ResponseFormatJsonSchemaTag,
    /// Structured-output definition.
    pub json_schema: ChatJsonSchemaDefinition,
}

impl ChatResponseFormatJsonSchema {
    /// Construct a JSON Schema response format.
    #[must_use]
    pub const fn new(json_schema: ChatJsonSchemaDefinition) -> Self {
        Self {
            kind: ResponseFormatJsonSchemaTag::JsonSchema,
            json_schema,
        }
    }
}

strict_tagged_union! {
    /// Chat output format.
    pub enum ChatResponseFormat {
        Text(ChatResponseFormatText) = "text",
        JsonObject(ChatResponseFormatJsonObject) = "json_object",
        JsonSchema(ChatResponseFormatJsonSchema) = "json_schema"
    }
}

impl From<ChatResponseFormatText> for ChatResponseFormat {
    fn from(value: ChatResponseFormatText) -> Self {
        Self::Text(value)
    }
}

impl From<ChatResponseFormatJsonObject> for ChatResponseFormat {
    fn from(value: ChatResponseFormatJsonObject) -> Self {
        Self::JsonObject(value)
    }
}

impl From<ChatResponseFormatJsonSchema> for ChatResponseFormat {
    fn from(value: ChatResponseFormatJsonSchema) -> Self {
        Self::JsonSchema(value)
    }
}

literal_tag!(PredictionContentTag, Content, "content");

/// Static predicted output used to accelerate regeneration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatPredictionContent {
    #[serde(rename = "type")]
    kind: PredictionContentTag,
    /// Predicted text or explicit text parts.
    pub content: ChatPredictionValue,
}

impl ChatPredictionContent {
    /// Construct a plain predicted text value.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: PredictionContentTag::Content,
            content: ChatPredictionValue::Text(text.into()),
        }
    }

    /// Construct predicted output from text content parts.
    #[must_use]
    pub fn parts(parts: impl IntoIterator<Item = ChatTextContentPart>) -> Self {
        Self {
            kind: PredictionContentTag::Content,
            content: ChatPredictionValue::Parts(parts.into_iter().collect()),
        }
    }
}

/// Wire representation of predicted output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatPredictionValue {
    /// Plain text.
    Text(String),
    /// Text parts.
    Parts(Vec<ChatTextContentPart>),
}

/// Named or custom voice used for audio output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatVoice {
    /// Built-in voice name, retained as an open string.
    Named(String),
    /// Custom voice identifier.
    Custom(ChatCustomVoice),
}

impl From<String> for ChatVoice {
    fn from(value: String) -> Self {
        Self::Named(value)
    }
}

impl From<&str> for ChatVoice {
    fn from(value: &str) -> Self {
        Self::Named(value.to_owned())
    }
}

/// Custom voice reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCustomVoice {
    /// Voice identifier.
    pub id: String,
}

/// Requested audio-output configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatOutputAudio {
    /// Voice used by the model.
    pub voice: ChatVoice,
    /// Generated audio encoding.
    pub format: ChatOutputAudioFormat,
}

impl ChatOutputAudio {
    /// Construct an audio-output configuration.
    #[must_use]
    pub fn new(voice: impl Into<ChatVoice>, format: ChatOutputAudioFormat) -> Self {
        Self {
            voice: voice.into(),
            format,
        }
    }
}

/// Stop sequence configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatStop {
    /// One stop sequence.
    One(String),
    /// Between one and four stop sequences.
    Many(Vec<String>),
}

impl From<String> for ChatStop {
    fn from(value: String) -> Self {
        Self::One(value)
    }
}

impl From<&str> for ChatStop {
    fn from(value: &str) -> Self {
        Self::One(value.to_owned())
    }
}

impl From<Vec<String>> for ChatStop {
    fn from(value: Vec<String>) -> Self {
        Self::Many(value)
    }
}

/// Approximate user location for Chat web search.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWebSearchLocation {
    /// ISO 3166-1 alpha-2 country code.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub country: Omittable<String>,
    /// Region name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub region: Omittable<String>,
    /// City name.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub city: Omittable<String>,
    /// IANA timezone.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub timezone: Omittable<String>,
}

literal_tag!(WebSearchLocationTag, Approximate, "approximate");

/// Typed wrapper for an approximate web-search location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWebSearchUserLocation {
    #[serde(rename = "type")]
    kind: WebSearchLocationTag,
    /// Approximate geographic fields.
    pub approximate: ChatWebSearchLocation,
}

impl ChatWebSearchUserLocation {
    /// Construct an approximate location.
    #[must_use]
    pub const fn new(approximate: ChatWebSearchLocation) -> Self {
        Self {
            kind: WebSearchLocationTag::Approximate,
            approximate,
        }
    }
}

/// Chat web-search options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWebSearchOptions {
    /// Approximate user location, explicitly nullable.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user_location: Omittable<Nullable<ChatWebSearchUserLocation>>,
    /// Requested search context size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub search_context_size: Omittable<ChatWebSearchContextSize>,
}

/// Moderation configuration for a Chat request.
///
/// The pinned `CreateChatCompletionRequest.moderation` is the exact same
/// `ModerationParam` schema the Responses host uses, so `policy` reuses the
/// typed [`ModerationPolicy`] instead of a raw map (6-11);
/// [`ChatModerationConfig::with_policy`] remains for callers that already
/// hold a serializable policy of the same pinned shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatModerationConfig {
    /// Moderation model identifier.
    pub model: String,
    /// Input/output moderation policy, explicitly nullable.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub policy: Omittable<Nullable<ModerationPolicy>>,
}

impl ChatModerationConfig {
    /// Construct moderation configuration.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            policy: Omittable::Omitted,
        }
    }

    /// Sets the directional moderation policy.
    #[must_use]
    pub fn policy(mut self, policy: ModerationPolicy) -> Self {
        self.policy = Omittable::Value(Nullable::Value(policy));
        self
    }

    /// Sends `policy: null`.
    #[must_use]
    pub fn policy_null(mut self) -> Self {
        self.policy = Omittable::Value(Nullable::Null);
        self
    }

    /// Serialize a typed moderation policy through the pinned policy shape.
    ///
    /// Escape hatch for callers that already hold a policy as their own
    /// serializable type: the value must serialize exactly onto the pinned
    /// `ModerationPolicyParam` shape the Responses host shares (`input` /
    /// `output` directions, each carrying a `mode`). Members the typed
    /// [`ModerationPolicy`] cannot represent are rejected with an error
    /// instead of being silently dropped. New shapes should grow on
    /// [`ModerationPolicy`] itself, which keeps the wire form lossless.
    pub fn with_policy<T: Serialize>(mut self, policy: &T) -> Result<Self, serde_json::Error> {
        let object = serialize_object(policy, "moderation policy must serialize as a JSON object")?;
        let typed: ModerationPolicy = serde_json::from_value(Value::Object(object.clone()))?;
        if serde_json::to_value(&typed)? != Value::Object(object) {
            return Err(serde_json::Error::custom(
                "moderation policy must serialize exactly onto the pinned \
                 ModerationPolicyParam shape",
            ));
        }
        self.policy = Omittable::Value(Nullable::Value(typed));
        Ok(self)
    }
}

/// Inclusive Chat Completions bounds from the pinned OpenAPI.
pub const MIN_CHAT_CHOICES: u32 = 1;
/// Inclusive maximum for `n`.
pub const MAX_CHAT_CHOICES: u32 = 128;
/// Inclusive minimum stop-sequence array length.
pub const MIN_STOP_SEQUENCES: usize = 1;
/// Inclusive maximum stop-sequence array length.
pub const MAX_STOP_SEQUENCES: usize = 4;
/// Inclusive minimum logit-bias value.
pub const MIN_LOGIT_BIAS: i32 = -100;
/// Inclusive maximum logit-bias value.
pub const MAX_LOGIT_BIAS: i32 = 100;
/// Inclusive minimum deprecated `functions` array length.
pub const MIN_CHAT_FUNCTIONS: usize = 1;
/// Inclusive maximum deprecated `functions` array length.
pub const MAX_CHAT_FUNCTIONS: usize = 128;
/// Official Chat message / prediction content-array `minItems`.
pub const MIN_CHAT_CONTENT_PARTS: usize = 1;

/// A Chat create-request value that violates a pinned OpenAPI constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CreateChatCompletionConstraintError {
    /// `messages` is empty.
    #[error("messages must contain at least one item")]
    EmptyMessages,
    /// A message `content` array is empty (`minItems: 1`).
    #[error("message content must contain at least one part")]
    EmptyMessageContent,
    /// Predicted-output `content` array is empty (`minItems: 1`).
    #[error("prediction content must contain at least one part")]
    EmptyPredictionParts,
    /// `temperature` is non-finite or outside `0..=2`.
    #[error("temperature must be finite and within 0..=2, got {value}")]
    Temperature { value: String },
    /// `top_p` is non-finite or outside `0..=1`.
    #[error("top_p must be finite and within 0..=1, got {value}")]
    TopP { value: String },
    /// `frequency_penalty` is non-finite or outside `-2..=2`.
    #[error("frequency_penalty must be finite and within -2..=2, got {value}")]
    FrequencyPenalty { value: String },
    /// `presence_penalty` is non-finite or outside `-2..=2`.
    #[error("presence_penalty must be finite and within -2..=2, got {value}")]
    PresencePenalty { value: String },
    /// `top_logprobs` is outside `0..=20`.
    #[error("top_logprobs must be 0..={maximum}, got {actual}")]
    TopLogprobs { actual: u8, maximum: u32 },
    /// `n` is outside `1..=128`.
    #[error("n must be {minimum}..={maximum}, got {actual}")]
    Choices {
        actual: u32,
        minimum: u32,
        maximum: u32,
    },
    /// `safety_identifier` exceeds 64 characters.
    #[error("safety_identifier has {actual} characters; maximum is {maximum}")]
    SafetyIdentifier { actual: usize, maximum: usize },
    /// Metadata contains more than 16 pairs.
    #[error("metadata contains {actual} pairs; maximum is {maximum}")]
    MetadataPairCount { actual: usize, maximum: usize },
    /// A metadata key exceeds 64 characters.
    #[error("metadata key has {actual} characters; maximum is {maximum}")]
    MetadataKey { actual: usize, maximum: usize },
    /// A metadata value exceeds 512 characters.
    #[error("metadata value has {actual} characters; maximum is {maximum}")]
    MetadataValue { actual: usize, maximum: usize },
    /// `stop` array length is outside `1..=4`.
    #[error("stop must contain {minimum}..={maximum} sequences, got {actual}")]
    StopSequences {
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
    /// A `logit_bias` value is outside `-100..=100`.
    #[error("logit_bias[{token}] must be {minimum}..={maximum}, got {actual}")]
    LogitBias {
        token: String,
        actual: i32,
        minimum: i32,
        maximum: i32,
    },
    /// Deprecated `functions` array length is outside `1..=128`.
    #[error("functions must contain {minimum}..={maximum} entries, got {actual}")]
    Functions {
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
}

/// One successful classification inside a Chat moderation-results list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ChatModerationClassification {
    /// Successful classification for one input.
    #[serde(rename = "moderation_result")]
    Result {
        /// Category name to flagged boolean.
        categories: BTreeMap<String, bool>,
        /// Category name to the input modalities that contributed to the score.
        category_applied_input_types: BTreeMap<String, Vec<ModerationInputType>>,
        /// Category name to raw score.
        category_scores: BTreeMap<String, f64>,
        /// Whether any category flagged the content.
        flagged: bool,
        /// Moderation model that produced the result.
        model: String,
    },
}

/// Successful result list or error for one Chat moderation direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ChatCompletionModerationOutcome {
    /// Successful classifications for one direction.
    #[serde(rename = "moderation_results")]
    Results {
        /// Moderation model used for this direction.
        model: String,
        /// One result per moderated input.
        results: Vec<ChatModerationClassification>,
    },
    /// Failure while moderating one direction.
    #[serde(rename = "error")]
    Error {
        /// Service error code.
        code: String,
        /// Human-readable error message.
        message: String,
    },
}

/// Typed Chat Completions moderation echo (`moderation_results` list, not the
/// Responses single-result shape).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionModeration {
    input: ChatCompletionModerationOutcome,
    output: ChatCompletionModerationOutcome,
}

impl ChatCompletionModeration {
    /// Returns the input-side outcome.
    #[must_use]
    pub const fn input(&self) -> &ChatCompletionModerationOutcome {
        &self.input
    }

    /// Returns the output-side outcome.
    #[must_use]
    pub const fn output(&self) -> &ChatCompletionModerationOutcome {
        &self.output
    }
}

crate::open_string_enum! {
    /// Deprecated function-selection mode.
    pub enum ChatLegacyFunctionChoiceMode {
        None = "none",
        Auto = "auto"
    }
}

/// Deprecated named function choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLegacyNamedFunctionChoice {
    /// Function name.
    pub name: String,
}

/// Deprecated function selection policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatLegacyFunctionChoice {
    /// None, auto, or a future string value.
    Mode(ChatLegacyFunctionChoiceMode),
    /// Force a named function.
    Named(ChatLegacyNamedFunctionChoice),
}

/// Fields shared by streaming and non-streaming Chat create requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionRequestBody {
    /// Conversation messages. The typed create constructor always supplies at
    /// least one; wire deserialization also rejects an empty array.
    pub messages: Vec<ChatMessage>,
    /// Model identifier.
    pub model: ModelId,
    /// Audio output configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<Nullable<ChatOutputAudio>>,
    /// Frequency penalty.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub frequency_penalty: Omittable<Nullable<f64>>,
    /// Deprecated function selection.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function_call: Omittable<ChatLegacyFunctionChoice>,
    /// Deprecated function definitions. Legacy entries carry no `strict`
    /// field; use `tools[].function` for strict schema adherence.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub functions: Omittable<Vec<ChatCompletionFunction>>,
    /// Per-token logit biases.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logit_bias: Omittable<Nullable<BTreeMap<String, i32>>>,
    /// Whether token log probabilities are returned.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprobs: Omittable<Nullable<bool>>,
    /// Maximum completion tokens, including reasoning tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_completion_tokens: Omittable<Nullable<u64>>,
    /// Deprecated visible-token limit.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_tokens: Omittable<Nullable<u64>>,
    /// Stored-object metadata.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    /// Requested output modalities.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub modalities: Omittable<Nullable<Vec<ChatModality>>>,
    /// Moderation configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub moderation: Omittable<Nullable<ChatModerationConfig>>,
    /// Number of choices.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n: Omittable<Nullable<u32>>,
    /// Whether parallel tool calls are allowed.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub parallel_tool_calls: Omittable<bool>,
    /// Static predicted output.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prediction: Omittable<Nullable<ChatPredictionContent>>,
    /// Presence penalty.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub presence_penalty: Omittable<Nullable<f64>>,
    /// Prompt cache bucketing key.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_key: Omittable<Nullable<String>>,
    /// Prompt-cache options.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_options: Omittable<ChatPromptCacheOptions>,
    /// Deprecated prompt cache retention.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_retention: Omittable<Nullable<ChatPromptCacheRetention>>,
    /// Reasoning effort.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning_effort: Omittable<Nullable<ChatReasoningEffort>>,
    /// Output format.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub response_format: Omittable<ChatResponseFormat>,
    /// Stable abuse-prevention identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub safety_identifier: Omittable<Nullable<String>>,
    /// Deprecated deterministic seed.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub seed: Omittable<Nullable<i64>>,
    /// Processing tier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_tier: Omittable<Nullable<ChatServiceTier>>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub stop: Omittable<Nullable<ChatStop>>,
    /// Whether the result is stored.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub store: Omittable<Nullable<bool>>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub temperature: Omittable<Nullable<f64>>,
    /// Tool definitions.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tools: Omittable<Vec<ChatTool>>,
    /// Tool selection policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_choice: Omittable<ChatToolChoice>,
    /// Number of top logprobs to return.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub top_logprobs: Omittable<Nullable<u8>>,
    /// Nucleus sampling probability.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub top_p: Omittable<Nullable<f64>>,
    /// Deprecated stable end-user identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<String>,
    /// Requested answer verbosity.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub verbosity: Omittable<Nullable<ChatVerbosity>>,
    /// Chat web-search options.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub web_search_options: Omittable<ChatWebSearchOptions>,
}

impl ChatCompletionRequestBody {
    fn new(model: impl Into<ModelId>, message: impl Into<ChatMessage>) -> Self {
        Self {
            messages: vec![message.into()],
            model: model.into(),
            audio: Omittable::Omitted,
            frequency_penalty: Omittable::Omitted,
            function_call: Omittable::Omitted,
            functions: Omittable::Omitted,
            logit_bias: Omittable::Omitted,
            logprobs: Omittable::Omitted,
            max_completion_tokens: Omittable::Omitted,
            max_tokens: Omittable::Omitted,
            metadata: Omittable::Omitted,
            modalities: Omittable::Omitted,
            moderation: Omittable::Omitted,
            n: Omittable::Omitted,
            parallel_tool_calls: Omittable::Omitted,
            prediction: Omittable::Omitted,
            presence_penalty: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning_effort: Omittable::Omitted,
            response_format: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            seed: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            stop: Omittable::Omitted,
            store: Omittable::Omitted,
            temperature: Omittable::Omitted,
            tools: Omittable::Omitted,
            tool_choice: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            top_p: Omittable::Omitted,
            user: Omittable::Omitted,
            verbosity: Omittable::Omitted,
            web_search_options: Omittable::Omitted,
        }
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateChatCompletionConstraintError> {
        if self.messages.is_empty() {
            return Err(CreateChatCompletionConstraintError::EmptyMessages);
        }
        for message in &self.messages {
            validate_chat_message_content(message)?;
        }
        if let Omittable::Value(Nullable::Value(prediction)) = &self.prediction {
            if let ChatPredictionValue::Parts(parts) = &prediction.content
                && parts.len() < MIN_CHAT_CONTENT_PARTS
            {
                return Err(CreateChatCompletionConstraintError::EmptyPredictionParts);
            }
        }
        if let Omittable::Value(Nullable::Value(temperature)) = self.temperature
            && !(temperature.is_finite() && (0.0..=2.0).contains(&temperature))
        {
            return Err(CreateChatCompletionConstraintError::Temperature {
                value: temperature.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(top_p)) = self.top_p
            && !(top_p.is_finite() && (0.0..=1.0).contains(&top_p))
        {
            return Err(CreateChatCompletionConstraintError::TopP {
                value: top_p.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(penalty)) = self.frequency_penalty
            && !(penalty.is_finite() && (-2.0..=2.0).contains(&penalty))
        {
            return Err(CreateChatCompletionConstraintError::FrequencyPenalty {
                value: penalty.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(penalty)) = self.presence_penalty
            && !(penalty.is_finite() && (-2.0..=2.0).contains(&penalty))
        {
            return Err(CreateChatCompletionConstraintError::PresencePenalty {
                value: penalty.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(top_logprobs)) = self.top_logprobs
            && u32::from(top_logprobs) > MAX_TOP_LOGPROBS
        {
            return Err(CreateChatCompletionConstraintError::TopLogprobs {
                actual: top_logprobs,
                maximum: MAX_TOP_LOGPROBS,
            });
        }
        if let Omittable::Value(Nullable::Value(n)) = self.n
            && !(MIN_CHAT_CHOICES..=MAX_CHAT_CHOICES).contains(&n)
        {
            return Err(CreateChatCompletionConstraintError::Choices {
                actual: n,
                minimum: MIN_CHAT_CHOICES,
                maximum: MAX_CHAT_CHOICES,
            });
        }
        if let Omittable::Value(Nullable::Value(identifier)) = &self.safety_identifier {
            let actual = identifier.chars().count();
            if actual > MAX_SAFETY_IDENTIFIER_CHARS {
                return Err(CreateChatCompletionConstraintError::SafetyIdentifier {
                    actual,
                    maximum: MAX_SAFETY_IDENTIFIER_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(metadata)) = &self.metadata {
            if metadata.len() > MAX_RESPONSE_METADATA_PAIRS {
                return Err(CreateChatCompletionConstraintError::MetadataPairCount {
                    actual: metadata.len(),
                    maximum: MAX_RESPONSE_METADATA_PAIRS,
                });
            }
            for (key, value) in metadata {
                let key_chars = key.chars().count();
                if key_chars > MAX_RESPONSE_METADATA_KEY_CHARS {
                    return Err(CreateChatCompletionConstraintError::MetadataKey {
                        actual: key_chars,
                        maximum: MAX_RESPONSE_METADATA_KEY_CHARS,
                    });
                }
                let value_chars = value.chars().count();
                if value_chars > MAX_RESPONSE_METADATA_VALUE_CHARS {
                    return Err(CreateChatCompletionConstraintError::MetadataValue {
                        actual: value_chars,
                        maximum: MAX_RESPONSE_METADATA_VALUE_CHARS,
                    });
                }
            }
        }
        if let Omittable::Value(Nullable::Value(ChatStop::Many(stops))) = &self.stop {
            if !(MIN_STOP_SEQUENCES..=MAX_STOP_SEQUENCES).contains(&stops.len()) {
                return Err(CreateChatCompletionConstraintError::StopSequences {
                    actual: stops.len(),
                    minimum: MIN_STOP_SEQUENCES,
                    maximum: MAX_STOP_SEQUENCES,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(bias)) = &self.logit_bias {
            for (token, value) in bias {
                if !(MIN_LOGIT_BIAS..=MAX_LOGIT_BIAS).contains(value) {
                    return Err(CreateChatCompletionConstraintError::LogitBias {
                        token: token.clone(),
                        actual: *value,
                        minimum: MIN_LOGIT_BIAS,
                        maximum: MAX_LOGIT_BIAS,
                    });
                }
            }
        }
        if let Omittable::Value(functions) = &self.functions
            && !(MIN_CHAT_FUNCTIONS..=MAX_CHAT_FUNCTIONS).contains(&functions.len())
        {
            return Err(CreateChatCompletionConstraintError::Functions {
                actual: functions.len(),
                minimum: MIN_CHAT_FUNCTIONS,
                maximum: MAX_CHAT_FUNCTIONS,
            });
        }
        Ok(())
    }
}

fn validate_chat_message_content(
    message: &ChatMessage,
) -> Result<(), CreateChatCompletionConstraintError> {
    match message {
        ChatMessage::Developer(message) => validate_instruction_content(&message.content),
        ChatMessage::System(message) => validate_instruction_content(&message.content),
        ChatMessage::User(message) => match &message.content {
            ChatUserContent::Text(_) => Ok(()),
            ChatUserContent::Parts(parts) => validate_chat_content_part_count(parts.len()),
        },
        ChatMessage::Assistant(message) => {
            if let Omittable::Value(Nullable::Value(content)) = &message.content {
                match content {
                    ChatAssistantContent::Text(_) => Ok(()),
                    ChatAssistantContent::Parts(parts) => {
                        validate_chat_content_part_count(parts.len())
                    }
                }
            } else {
                Ok(())
            }
        }
        ChatMessage::Tool(message) => match &message.content {
            ChatToolContent::Text(_) => Ok(()),
            ChatToolContent::Parts(parts) => validate_chat_content_part_count(parts.len()),
        },
        ChatMessage::Function(_) | ChatMessage::Unknown(_) => Ok(()),
    }
}

fn validate_instruction_content(
    content: &ChatInstructionContent,
) -> Result<(), CreateChatCompletionConstraintError> {
    match content {
        ChatInstructionContent::Text(_) => Ok(()),
        ChatInstructionContent::Parts(parts) => validate_chat_content_part_count(parts.len()),
    }
}

fn validate_chat_content_part_count(
    actual: usize,
) -> Result<(), CreateChatCompletionConstraintError> {
    if actual < MIN_CHAT_CONTENT_PARTS {
        Err(CreateChatCompletionConstraintError::EmptyMessageContent)
    } else {
        Ok(())
    }
}

/// Options emitted only for a streaming Chat request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatStreamOptions {
    /// Include a final usage-only chunk.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include_usage: Omittable<bool>,
    /// Include chunk-size obfuscation.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include_obfuscation: Omittable<bool>,
}

/// Non-streaming Chat create typestate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatNonStreaming;

/// Streaming Chat create typestate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatStreaming;

mod request_mode_private {
    pub trait Sealed {}
    impl Sealed for super::ChatNonStreaming {}
    impl Sealed for super::ChatStreaming {}
}

/// Sealed typestate constraint for Chat create requests.
pub trait ChatCompletionRequestMode: request_mode_private::Sealed {
    /// Whether the request must carry `stream: true`.
    const STREAMING: bool;
}

impl ChatCompletionRequestMode for ChatNonStreaming {
    const STREAMING: bool = false;
}

impl ChatCompletionRequestMode for ChatStreaming {
    const STREAMING: bool = true;
}

/// Typed Chat completion create request.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(bound(serialize = ""))]
pub struct CreateChatCompletionRequest<M = ChatNonStreaming>
where
    M: ChatCompletionRequestMode,
{
    /// All fields shared by streaming and non-streaming requests.
    #[serde(flatten)]
    pub body: ChatCompletionRequestBody,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_options: Omittable<Nullable<ChatStreamOptions>>,
    #[serde(skip)]
    mode: PhantomData<fn() -> M>,
}

/// Ergonomic alias for a non-streaming Chat create request.
pub type ChatCompletionRequest = CreateChatCompletionRequest<ChatNonStreaming>;

/// Ergonomic alias for a streaming Chat create request.
pub type ChatCompletionStreamRequest = CreateChatCompletionRequest<ChatStreaming>;

#[derive(Deserialize)]
struct CreateChatCompletionRequestWire {
    #[serde(flatten)]
    body: ChatCompletionRequestBody,
    #[serde(default)]
    stream: Omittable<Nullable<bool>>,
    #[serde(default)]
    stream_options: Omittable<Nullable<ChatStreamOptions>>,
}

impl<'de, M> Deserialize<'de> for CreateChatCompletionRequest<M>
where
    M: ChatCompletionRequestMode,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateChatCompletionRequestWire::deserialize(deserializer)?;
        if wire.body.messages.is_empty() {
            return Err(D::Error::custom(
                "Chat completion request requires at least one message",
            ));
        }

        let wire_streaming = matches!(wire.stream, Omittable::Value(Nullable::Value(true)));
        if wire_streaming != M::STREAMING {
            return Err(D::Error::custom(if M::STREAMING {
                "streaming Chat request requires `stream: true`"
            } else {
                "non-streaming Chat request cannot carry `stream: true`"
            }));
        }
        if !M::STREAMING && wire.stream_options.is_value() {
            return Err(D::Error::custom(
                "non-streaming Chat request cannot carry `stream_options`",
            ));
        }

        Ok(Self {
            body: wire.body,
            stream: wire.stream,
            stream_options: wire.stream_options,
            mode: PhantomData,
        })
    }
}

impl CreateChatCompletionRequest<ChatNonStreaming> {
    /// Construct a minimal non-streaming request with one message.
    #[must_use]
    pub fn new(model: impl Into<ModelId>, message: impl Into<ChatMessage>) -> Self {
        Self {
            body: ChatCompletionRequestBody::new(model, message),
            stream: Omittable::Omitted,
            stream_options: Omittable::Omitted,
            mode: PhantomData,
        }
    }

    /// Switch to the streaming typestate and emit `stream: true`.
    #[must_use]
    pub fn into_streaming(self) -> CreateChatCompletionRequest<ChatStreaming> {
        CreateChatCompletionRequest {
            body: self.body,
            stream: Omittable::Value(Nullable::Value(true)),
            stream_options: Omittable::Omitted,
            mode: PhantomData,
        }
    }
}

impl CreateChatCompletionRequest<ChatStreaming> {
    /// Configure streaming-only options.
    #[must_use]
    pub fn with_stream_options(mut self, options: ChatStreamOptions) -> Self {
        self.stream_options = Omittable::Value(Nullable::Value(options));
        self
    }

    /// Sends `stream_options: null`.
    #[must_use]
    pub fn with_stream_options_null(mut self) -> Self {
        self.stream_options = Omittable::Value(Nullable::Null);
        self
    }

    /// Return to the non-streaming typestate, removing all stream fields.
    #[must_use]
    pub fn into_non_streaming(self) -> CreateChatCompletionRequest<ChatNonStreaming> {
        CreateChatCompletionRequest {
            body: self.body,
            stream: Omittable::Omitted,
            stream_options: Omittable::Omitted,
            mode: PhantomData,
        }
    }
}

impl<M> CreateChatCompletionRequest<M>
where
    M: ChatCompletionRequestMode,
{
    /// Append another conversation message.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<ChatMessage>) -> Self {
        self.body.messages.push(message.into());
        self
    }

    /// Append a tool definition.
    #[must_use]
    pub fn with_tool(mut self, tool: impl Into<ChatTool>) -> Self {
        match &mut self.body.tools {
            Omittable::Value(tools) => tools.push(tool.into()),
            Omittable::Omitted => self.body.tools = Omittable::Value(vec![tool.into()]),
        }
        self
    }

    /// Set the tool-selection policy.
    #[must_use]
    pub fn with_tool_choice(mut self, choice: impl Into<ChatToolChoice>) -> Self {
        self.body.tool_choice = Omittable::Value(choice.into());
        self
    }

    /// Set the output format.
    #[must_use]
    pub fn with_response_format(mut self, format: impl Into<ChatResponseFormat>) -> Self {
        self.body.response_format = Omittable::Value(format.into());
        self
    }

    /// Set the maximum number of generated completion tokens.
    #[must_use]
    pub fn with_max_completion_tokens(mut self, tokens: u64) -> Self {
        self.body.max_completion_tokens = Omittable::Value(Nullable::Value(tokens));
        self
    }

    /// Set sampling temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.body.temperature = Omittable::Value(Nullable::Value(temperature));
        self
    }

    /// Request token log probabilities and an optional top-logprob count.
    #[must_use]
    pub fn with_logprobs(mut self, top_logprobs: Option<u8>) -> Self {
        self.body.logprobs = Omittable::Value(Nullable::Value(true));
        if let Some(top_logprobs) = top_logprobs {
            self.body.top_logprobs = Omittable::Value(Nullable::Value(top_logprobs));
        }
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<&Self, CreateChatCompletionConstraintError> {
        self.body.validate()?;
        Ok(self)
    }
}

/// Audio generated by an assistant response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatResponseAudio {
    /// Audio response identifier.
    pub id: String,
    /// Expiration timestamp in Unix seconds.
    pub expires_at: u64,
    /// Base64-encoded generated audio.
    pub data: String,
    /// Text transcript of the audio.
    pub transcript: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatResponseAudio {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(UrlCitationTag, UrlCitation, "url_citation");

/// URL citation offsets and target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatUrlCitation {
    /// End character index.
    pub end_index: u64,
    /// Start character index.
    pub start_index: u64,
    /// Cited URL.
    pub url: String,
    /// Cited page title.
    pub title: String,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatUrlCitation {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Web-search URL citation annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatUrlCitationAnnotation {
    #[serde(rename = "type")]
    kind: UrlCitationTag,
    /// Citation details.
    pub url_citation: ChatUrlCitation,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatUrlCitationAnnotation {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// Annotation attached to generated Chat text.
    pub enum ChatAnnotation {
        UrlCitation(ChatUrlCitationAnnotation) = "url_citation"
    }
}

/// Assistant message returned in a non-streaming completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatResponseMessage {
    /// Generated text or explicit null.
    pub content: Nullable<String>,
    /// Refusal text, explicit null, or omitted by OpenAI-compatible providers.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub refusal: Omittable<Nullable<String>>,
    /// Tool calls generated by the model, or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_calls: Omittable<Nullable<Vec<ChatToolCall>>>,
    /// Search and other annotations.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub annotations: Omittable<Vec<ChatAnnotation>>,
    /// Role returned by the service. Future values are retained.
    pub role: ChatRole,
    /// Deprecated single function call, or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function_call: Omittable<Nullable<ChatLegacyFunctionCall>>,
    /// Generated audio or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<Nullable<ChatResponseAudio>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatResponseMessage {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One alternate token and its log probability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatTopLogprob {
    /// Token text.
    pub token: String,
    /// Natural-log probability.
    pub logprob: f64,
    /// UTF-8 bytes or explicit null.
    pub bytes: Nullable<Vec<u8>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatTopLogprob {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Log probability data for one generated token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatTokenLogprob {
    /// Token text.
    pub token: String,
    /// Natural-log probability.
    pub logprob: f64,
    /// UTF-8 bytes or explicit null.
    pub bytes: Nullable<Vec<u8>>,
    /// Most likely alternatives at this position.
    pub top_logprobs: Vec<ChatTopLogprob>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatTokenLogprob {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Log probabilities for content and refusal tokens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatChoiceLogprobs {
    /// Content token logprobs or explicit null.
    pub content: Nullable<Vec<ChatTokenLogprob>>,
    /// Refusal token logprobs or explicit null.
    pub refusal: Nullable<Vec<ChatTokenLogprob>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatChoiceLogprobs {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One non-streaming Chat completion choice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChoice {
    /// Stop reason.
    pub finish_reason: ChatFinishReason,
    /// Choice index.
    pub index: u32,
    /// Generated assistant message.
    pub message: ChatResponseMessage,
    /// Token log probabilities or explicit null.
    pub logprobs: Nullable<ChatChoiceLogprobs>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionChoice {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Detailed completion-token accounting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionTokenDetails {
    /// Accepted predicted tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub accepted_prediction_tokens: Omittable<u64>,
    /// Generated audio tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<u64>,
    /// Reasoning tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning_tokens: Omittable<u64>,
    /// Generated text tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<u64>,
    /// Rejected predicted tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub rejected_prediction_tokens: Omittable<u64>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionTokenDetails {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Detailed prompt-token accounting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatPromptTokenDetails {
    /// Input audio tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio_tokens: Omittable<u64>,
    /// Cache-hit tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub cached_tokens: Omittable<u64>,
    /// Input text tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub text_tokens: Omittable<u64>,
    /// Input image tokens.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_tokens: Omittable<u64>,
    /// Tokens written to cache.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub cache_write_tokens: Omittable<u64>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatPromptTokenDetails {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Usage statistics for a Chat completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionUsage {
    /// Generated tokens.
    pub completion_tokens: u64,
    /// Prompt tokens.
    pub prompt_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
    /// Compute units, currently often explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub compute_units: Omittable<Nullable<u64>>,
    /// Generated-token detail.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub completion_tokens_details: Omittable<ChatCompletionTokenDetails>,
    /// Prompt-token detail.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_tokens_details: Omittable<ChatPromptTokenDetails>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionUsage {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A non-streaming Chat completion response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletion {
    /// Completion identifier.
    pub id: String,
    /// Generated choices.
    pub choices: Vec<ChatCompletionChoice>,
    /// Creation timestamp in Unix seconds.
    pub created: u64,
    /// Model used by the service.
    pub model: ModelId,
    /// Object discriminator, open for forward compatibility.
    pub object: ChatCompletionObject,
    /// Stored-object metadata or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    /// Typed moderation outcomes when moderated completions were requested.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub moderation: Omittable<Nullable<ChatCompletionModeration>>,
    /// Effective service tier or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_tier: Omittable<Nullable<ChatServiceTier>>,
    /// Deprecated backend fingerprint.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub system_fingerprint: Omittable<String>,
    /// Usage statistics.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<ChatCompletionUsage>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletion {
    /// Returns typed moderation outcomes when present and non-null.
    #[must_use]
    pub fn moderation(&self) -> Option<&ChatCompletionModeration> {
        match &self.moderation {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Concatenate non-null text from all choices, separated by newlines.
    #[must_use]
    pub fn output_text(&self) -> String {
        self.choices
            .iter()
            .filter_map(|choice| match &choice.message.content {
                Nullable::Value(content) => Some(content.as_str()),
                Nullable::Null => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Deprecated partial single-function call in a stream delta.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatLegacyFunctionCallDelta {
    /// Partial argument JSON text. It need not parse until accumulated.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub arguments: Omittable<JsonText>,
    /// Function name when first announced.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatLegacyFunctionCallDelta {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Partial function payload in a streamed tool call.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionCallDelta {
    /// Function name when announced.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    /// Partial JSON argument text.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub arguments: Omittable<JsonText>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatFunctionCallDelta {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Indexed partial tool call in a stream delta.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatToolCallChunk {
    /// Position within the tool-call array.
    pub index: u32,
    /// Tool-call ID when announced.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    /// Tool kind when announced. Future strings are retained.
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<ChatToolKind>,
    /// Partial function details.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function: Omittable<ChatFunctionCallDelta>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatToolCallChunk {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Chat message delta carried by a stream chunk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionDelta {
    /// Partial generated content or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub content: Omittable<Nullable<String>>,
    /// Deprecated partial single-function call.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function_call: Omittable<ChatLegacyFunctionCallDelta>,
    /// Indexed partial tool calls.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_calls: Omittable<Vec<ChatToolCallChunk>>,
    /// Message role when first announced.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<ChatRole>,
    /// Partial refusal text or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub refusal: Omittable<Nullable<String>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionDelta {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One choice in a streamed Chat completion chunk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChunkChoice {
    /// Partial message update.
    pub delta: ChatCompletionDelta,
    /// Stop reason or explicit null while generation continues.
    pub finish_reason: Nullable<ChatFinishReason>,
    /// Choice index.
    pub index: u32,
    /// Token log probabilities or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub logprobs: Omittable<Nullable<ChatChoiceLogprobs>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionChunkChoice {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One decoded Chat Completions SSE chunk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    /// Completion identifier shared by all chunks.
    pub id: String,
    /// Choice deltas; the final usage chunk may use an empty array.
    pub choices: Vec<ChatCompletionChunkChoice>,
    /// Creation timestamp in Unix seconds.
    pub created: u64,
    /// Model used by the service.
    pub model: ModelId,
    /// Object discriminator, open for forward compatibility.
    pub object: ChatCompletionObject,
    /// Chunk-size obfuscation string.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub obfuscation: Omittable<String>,
    /// Effective service tier or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_tier: Omittable<Nullable<ChatServiceTier>>,
    /// Deprecated backend fingerprint.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub system_fingerprint: Omittable<String>,
    /// Usage is null on ordinary chunks and populated on the optional final
    /// usage chunk.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub usage: Omittable<Nullable<ChatCompletionUsage>>,
    /// Typed moderation outcomes when present on a stream chunk.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub moderation: Omittable<Nullable<ChatCompletionModeration>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionChunk {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Sort order for stored Chat completions and messages.
    pub enum ChatListOrder {
        Ascending = "asc",
        Descending = "desc"
    }
}

/// Query parameters for listing stored Chat completions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionListParams {
    /// Filter by model.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<ModelId>,
    /// Deep-object metadata filter, preserving missing versus explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    /// Cursor from the preceding page.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    /// Requested page size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    /// Timestamp sort order.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ChatListOrder>,
}

/// Required JSON body for updating stored completion metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateChatCompletionRequest {
    /// Replacement metadata, or explicit null to clear metadata.
    pub metadata: Nullable<BTreeMap<String, String>>,
}

impl UpdateChatCompletionRequest {
    /// Replace stored metadata.
    #[must_use]
    pub fn new(metadata: BTreeMap<String, String>) -> Self {
        Self {
            metadata: Nullable::Value(metadata),
        }
    }

    /// Clear stored metadata with an explicit JSON null.
    #[must_use]
    pub const fn clear() -> Self {
        Self {
            metadata: Nullable::Null,
        }
    }
}

crate::open_string_enum! {
    /// Object discriminator returned after deleting a stored Chat completion.
    pub enum ChatCompletionDeletedObject {
        Deleted = "chat.completion.deleted"
    }
}

/// Confirmation returned after deleting a stored Chat completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionDeleted {
    /// Deleted completion identifier.
    pub id: String,
    /// Whether deletion completed.
    pub deleted: bool,
    /// Object discriminator.
    pub object: ChatCompletionDeletedObject,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionDeleted {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Stored Chat collection discriminator.
    pub enum ChatCompletionListObject {
        List = "list"
    }
}

/// Page of stored Chat completions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionList {
    /// Collection discriminator.
    pub object: ChatCompletionListObject,
    /// Stored completions.
    pub data: Vec<ChatCompletion>,
    /// First completion identifier in this page.
    pub first_id: String,
    /// Last completion identifier in this page.
    pub last_id: String,
    /// Whether another page is available.
    pub has_more: bool,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolves the next-page cursor of a `first_id`/`last_id` list envelope.
///
/// Mirrors the shared auto-pagination rule (D0147): when `has_more` is set, a
/// non-empty envelope `last_id` wins, an empty one falls back to the id of the
/// page's final element, and pagination stops when neither names a cursor so
/// an empty `last_id` cannot silently refetch the first page.
fn list_next_after<'a>(
    has_more: bool,
    last_id: &'a str,
    last_item_id: Option<&'a str>,
) -> Option<&'a str> {
    if !has_more {
        return None;
    }
    if !last_id.is_empty() {
        return Some(last_id);
    }
    last_item_id.filter(|id| !id.is_empty())
}

impl ChatCompletionList {
    /// Cursor for the next page when `has_more` is true.
    ///
    /// A non-empty `last_id` wins; an empty one falls back to the id of the
    /// page's final completion (D0147).
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        list_next_after(
            self.has_more,
            &self.last_id,
            self.data.last().map(|completion| completion.id.as_str()),
        )
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query parameters for listing messages of a stored Chat completion.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionMessageListParams {
    /// Cursor from the preceding page.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    /// Requested page size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    /// Timestamp sort order.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ChatListOrder>,
}

/// Text content part retained on a stored message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionStoreTextContentPart {
    #[serde(rename = "type")]
    kind: TextContentTag,
    /// Stored text.
    pub text: String,
    /// Optional cache breakpoint from the original request.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionStoreTextContentPart {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Image URL retained on a stored message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionStoreImageUrl {
    /// Image URL or data URL.
    pub url: String,
    /// Requested detail.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub detail: Omittable<ChatImageDetail>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionStoreImageUrl {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Image content part retained on a stored message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionStoreImageContentPart {
    #[serde(rename = "type")]
    kind: ImageContentTag,
    /// Stored image URL and detail.
    pub image_url: ChatCompletionStoreImageUrl,
    /// Optional cache breakpoint from the original request.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub prompt_cache_breakpoint: Omittable<ChatPromptCacheBreakpoint>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionStoreImageContentPart {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// Content part retained on a stored Chat message.
    pub enum ChatCompletionStoreMessageContentPart {
        Text(ChatCompletionStoreTextContentPart) = "text",
        Image(ChatCompletionStoreImageContentPart) = "image_url"
    }
}

/// Assistant message returned from a stored completion's message collection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionStoreMessage {
    /// Stored message identifier.
    pub id: String,
    /// Generated text or explicit null.
    pub content: Nullable<String>,
    /// Refusal text, explicit null, or omitted by OpenAI-compatible providers.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub refusal: Omittable<Nullable<String>>,
    /// Tool calls generated by the model, or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tool_calls: Omittable<Nullable<Vec<ChatToolCall>>>,
    /// Search and other annotations.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub annotations: Omittable<Vec<ChatAnnotation>>,
    /// Message role. Future strings are retained.
    pub role: ChatRole,
    /// Deprecated function call, or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub function_call: Omittable<Nullable<ChatLegacyFunctionCall>>,
    /// Generated audio or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub audio: Omittable<Nullable<ChatResponseAudio>>,
    /// Original text/image content parts, or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub content_parts: Omittable<Nullable<Vec<ChatCompletionStoreMessageContentPart>>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionStoreMessage {
    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Page of messages belonging to a stored Chat completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionMessageList {
    /// Collection discriminator.
    pub object: ChatCompletionListObject,
    /// Stored assistant messages.
    pub data: Vec<ChatCompletionStoreMessage>,
    /// First message identifier in this page.
    pub first_id: String,
    /// Last message identifier in this page.
    pub last_id: String,
    /// Whether another page is available.
    pub has_more: bool,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatCompletionMessageList {
    /// Cursor for the next page when `has_more` is true.
    ///
    /// A non-empty `last_id` wins; an empty one falls back to the id of the
    /// page's final message (D0147).
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        list_next_after(
            self.has_more,
            &self.last_id,
            self.data.last().map(|message| message.id.as_str()),
        )
    }

    /// Future fields retained while decoding.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(ChatMessage: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatTool: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatToolCall: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletion: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionChunk: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionListParams: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(UpdateChatCompletionRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionDeleted: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionList: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionStoreMessage: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatCompletionMessageList: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(
        CreateChatCompletionRequest<ChatNonStreaming>:
            Serialize, DeserializeOwned, Send, Sync
    );
    assert_impl_all!(
        CreateChatCompletionRequest<ChatStreaming>:
            Serialize, DeserializeOwned, Send, Sync
    );

    fn ok<T, E: fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct WeatherArguments {
        city: String,
        units: String,
    }

    #[derive(Serialize)]
    struct StringSchema<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        description: &'a str,
    }

    #[derive(Serialize)]
    struct WeatherSchema<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        properties: BTreeMap<&'a str, StringSchema<'a>>,
        required: Vec<&'a str>,
        additional_properties: bool,
    }

    fn weather_schema() -> WeatherSchema<'static> {
        WeatherSchema {
            kind: "object",
            properties: BTreeMap::from([
                (
                    "city",
                    StringSchema {
                        kind: "string",
                        description: "City name",
                    },
                ),
                (
                    "units",
                    StringSchema {
                        kind: "string",
                        description: "Unit system",
                    },
                ),
            ]),
            required: vec!["city", "units"],
            additional_properties: false,
        }
    }

    #[test]
    fn multimodal_request_round_trips_without_hand_written_json() {
        let parts = vec![
            ChatTextContentPart::new("Describe these inputs")
                .with_cache_breakpoint()
                .into(),
            ChatImageContentPart::new("https://example.test/cat.png")
                .with_detail(ChatImageDetail::High)
                .into(),
            ChatAudioContentPart::from_bytes(b"RIFF", ChatInputAudioFormat::Wav).into(),
            ChatFileContentPart::new(ChatInputFile::from_bytes("notes.txt", b"hello")).into(),
        ];
        let request =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::parts(parts))
                .with_message(ChatDeveloperMessage::new("Be concise"))
                .with_temperature(0.2);

        let value = ok(serde_json::to_value(&request));
        assert_eq!(value["messages"][0]["content"][0]["type"], "text");
        assert_eq!(value["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(value["messages"][0]["content"][2]["type"], "input_audio");
        assert_eq!(value["messages"][0]["content"][3]["type"], "file");
        assert!(value.get("stream").is_none());

        let decoded = ok(serde_json::from_value::<CreateChatCompletionRequest>(
            value.clone(),
        ));
        assert_eq!(ok(serde_json::to_value(decoded)), value);
    }

    #[test]
    fn typed_function_schema_arguments_and_results_need_no_json_text() {
        let function = ok(ChatFunctionDefinition::new("weather")
            .with_description("Read weather")
            .with_parameters(&weather_schema()))
        .with_strict(true);
        let request = CreateChatCompletionRequest::new(
            "gpt-5.6-sol",
            ChatUserMessage::text("Weather in Shanghai?"),
        )
        .with_tool(ChatFunctionTool::new(function))
        .with_tool_choice(ChatNamedFunctionChoice::new("weather"));

        let value = ok(serde_json::to_value(request));
        assert_eq!(
            value["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
        assert_eq!(value["tool_choice"]["function"]["name"], "weather");

        let arguments = WeatherArguments {
            city: "上海".to_owned(),
            units: "metric".to_owned(),
        };
        let call = ok(ChatFunctionToolCall::from_serializable(
            "call_1", "weather", &arguments,
        ));
        assert_eq!(ok(call.function.arguments.parse())["city"], "上海");

        let result = ok(ChatToolMessage::from_serializable(
            "call_1",
            &json!({"temperature": 24}),
        ));
        let result_value = ok(serde_json::to_value(result));
        let content = match result_value["content"].as_str() {
            Some(content) => content,
            None => panic!("tool content must be a JSON string"),
        };
        assert_eq!(
            ok(serde_json::from_str::<Value>(content))["temperature"],
            24
        );
    }

    #[test]
    fn legacy_functions_entries_omit_strict_while_tools_function_keeps_it() {
        let mut legacy = CreateChatCompletionRequest::new(
            "gpt-5.6-sol",
            ChatUserMessage::text("Weather in Shanghai?"),
        );
        legacy.body.functions = Omittable::Value(vec![ok(ChatCompletionFunction::new("weather")
            .with_description("Read weather")
            .with_parameters(&weather_schema()))]);

        let value = ok(serde_json::to_value(&legacy));
        assert_eq!(value["functions"][0]["name"], "weather");
        assert_eq!(value["functions"][0]["description"], "Read weather");
        assert_eq!(value["functions"][0]["parameters"]["type"], "object");
        assert!(value["functions"][0].get("strict").is_none());

        let strict = ok(ChatFunctionDefinition::new("weather").with_parameters(&weather_schema()))
            .with_strict(true);
        let tools = CreateChatCompletionRequest::new(
            "gpt-5.6-sol",
            ChatUserMessage::text("Weather in Shanghai?"),
        )
        .with_tool(ChatFunctionTool::new(strict));
        let value = ok(serde_json::to_value(tools));
        assert_eq!(value["tools"][0]["function"]["strict"], true);
        assert!(value.get("functions").is_none());

        let decoded = ok(serde_json::from_value::<CreateChatCompletionRequest>(
            json!({
                "model": "gpt-5.6-sol",
                "messages": [{"role": "user", "content": "hi"}],
                "functions": [{
                    "name": "weather",
                    "description": "Read weather",
                    "parameters": {"type": "object"}
                }]
            }),
        ));
        assert_eq!(
            ok(serde_json::to_value(decoded))["functions"][0]["name"],
            "weather"
        );
    }

    #[test]
    fn known_content_tag_is_strict_and_future_tag_is_lossless() {
        let malformed = serde_json::from_value::<ChatUserContentPart>(json!({
            "type": "image_url",
            "image_url": {}
        }));
        assert!(malformed.is_err());

        let future = json!({
            "type": "input_video",
            "video": {"url": "https://example.test/video.mp4"},
            "future_flag": true
        });
        let decoded = ok(serde_json::from_value::<ChatUserContentPart>(
            future.clone(),
        ));
        match &decoded {
            ChatUserContentPart::Unknown(value) => {
                assert_eq!(value.discriminator(), "input_video");
                assert_eq!(value.raw().get("future_flag"), Some(&Value::Bool(true)));
            }
            _ => panic!("future content tag must remain unknown"),
        }
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn known_message_role_is_strict_and_future_role_is_lossless() {
        assert!(serde_json::from_value::<ChatMessage>(json!({"role": "user"})).is_err());

        let future = json!({
            "role": "critic",
            "content": "review",
            "confidence": 0.9
        });
        let decoded = ok(serde_json::from_value::<ChatMessage>(future.clone()));
        match &decoded {
            ChatMessage::Unknown(message) => {
                assert_eq!(message.role(), "critic");
                assert_eq!(message.raw().get("confidence"), Some(&json!(0.9)));
            }
            _ => panic!("future message role must remain unknown"),
        }
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn known_tool_tags_do_not_fall_back_to_unknown() {
        assert!(
            serde_json::from_value::<ChatTool>(json!({
                "type": "function",
                "function": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ChatToolChoice>(json!({
                "type": "function",
                "function": {}
            }))
            .is_err()
        );

        let future = json!({"type": "browser", "browser": {"name": "search"}});
        let tool = ok(serde_json::from_value::<ChatTool>(future.clone()));
        assert!(matches!(tool, ChatTool::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(tool)), future);

        let arbitrary_allowed = json!({"connector": "future-connector", "version": 2});
        let allowed = ok(serde_json::from_value::<ChatAllowedTool>(
            arbitrary_allowed.clone(),
        ));
        assert!(matches!(allowed, ChatAllowedTool::Arbitrary(_)));
        assert_eq!(ok(serde_json::to_value(allowed)), arbitrary_allowed);

        let empty_function = json!({"type": "function", "function": {}});
        let allowed = ok(serde_json::from_value::<ChatAllowedTool>(
            empty_function.clone(),
        ));
        assert!(matches!(allowed, ChatAllowedTool::Arbitrary(_)));
        assert_eq!(ok(serde_json::to_value(allowed)), empty_function);

        let named = json!({"type": "function", "function": {"name": "lookup"}});
        let allowed = ok(serde_json::from_value::<ChatAllowedTool>(named.clone()));
        assert!(matches!(allowed, ChatAllowedTool::Reference(_)));
        assert_eq!(ok(serde_json::to_value(allowed)), named);

        let full_definition = json!({
            "type": "function",
            "function": {
                "name": "lookup",
                "description": "city lookup",
                "parameters": {"type": "object"}
            }
        });
        let allowed = ok(serde_json::from_value::<ChatAllowedTool>(
            full_definition.clone(),
        ));
        assert!(matches!(allowed, ChatAllowedTool::Arbitrary(_)));
        assert_eq!(ok(serde_json::to_value(allowed)), full_definition);
    }

    #[test]
    fn schema_fields_require_json_objects() {
        assert!(
            ChatFunctionDefinition::new("bad")
                .with_parameters(&["not", "an", "object"])
                .is_err()
        );
        assert!(
            ChatJsonSchemaDefinition::new("bad")
                .with_schema(&["not", "an", "object"])
                .is_err()
        );
        assert!(
            serde_json::from_value::<ChatFunctionDefinition>(json!({
                "name": "bad",
                "parameters": []
            }))
            .is_err()
        );
    }

    #[test]
    fn request_typestate_controls_stream_wire_fields() {
        let non_streaming =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        let non_streaming_value = ok(serde_json::to_value(&non_streaming));
        assert!(non_streaming_value.get("stream").is_none());

        let streaming = non_streaming
            .into_streaming()
            .with_stream_options(ChatStreamOptions {
                include_usage: Omittable::Value(true),
                include_obfuscation: Omittable::Value(false),
            });
        let streaming_value = ok(serde_json::to_value(&streaming));
        assert_eq!(streaming_value["stream"], true);
        assert_eq!(streaming_value["stream_options"]["include_usage"], true);

        let null_options =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hi"))
                .into_streaming()
                .with_stream_options_null();
        let null_value = ok(serde_json::to_value(&null_options));
        assert_eq!(null_value["stream_options"], Value::Null);
        let decoded_null = ok(serde_json::from_value::<
            CreateChatCompletionRequest<ChatStreaming>,
        >(json!({
            "model": "gpt-5.6-sol",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true,
            "stream_options": null
        })));
        assert_eq!(
            ok(serde_json::to_value(decoded_null))["stream_options"],
            Value::Null
        );
        let decoded = ok(serde_json::from_value::<
            CreateChatCompletionRequest<ChatStreaming>,
        >(streaming_value.clone()));
        assert_eq!(ok(serde_json::to_value(decoded)), streaming_value);

        assert!(
            serde_json::from_value::<CreateChatCompletionRequest<ChatNonStreaming>>(
                streaming_value.clone()
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateChatCompletionRequest<ChatStreaming>>(
                non_streaming_value
            )
            .is_err()
        );
    }

    #[test]
    fn completion_response_preserves_extra_fields_at_each_level() {
        let fixture = json!({
            "id": "chatcmpl_123",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "gpt-future",
            "choices": [{
                "index": 0,
                "finish_reason": "future_stop",
                "message": {
                    "role": "assistant",
                    "content": "hello",
                    "refusal": null,
                    "message_future": {"a": 1}
                },
                "logprobs": null,
                "choice_future": true
            }],
            "usage": {
                "prompt_tokens": 2,
                "completion_tokens": 1,
                "total_tokens": 3,
                "usage_future": "kept"
            },
            "response_future": [1, 2, 3]
        });
        let completion = ok(serde_json::from_value::<ChatCompletion>(fixture.clone()));

        assert_eq!(completion.output_text(), "hello");
        assert_eq!(completion.choices[0].finish_reason.as_str(), "future_stop");
        assert!(completion.extra().contains_key("response_future"));
        assert!(completion.choices[0].extra().contains_key("choice_future"));
        assert!(
            completion.choices[0]
                .message
                .extra()
                .contains_key("message_future")
        );
        match &completion.usage {
            Omittable::Value(usage) => {
                assert!(usage.extra().contains_key("usage_future"));
            }
            _ => panic!("fixture must contain usage"),
        }
        assert_eq!(ok(serde_json::to_value(completion)), fixture);
    }

    #[test]
    fn stream_chunk_retains_partial_arguments_nulls_and_extras() {
        let fixture = json!({
            "id": "chatcmpl_123",
            "object": "chat.completion.chunk",
            "created": 1700000000,
            "model": "gpt-5.6-sol",
            "choices": [{
                "index": 0,
                "finish_reason": null,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "future_function_kind",
                        "function": {
                            "name": "weather",
                            "arguments": "{\"city\":"
                        },
                        "tool_delta_future": 7
                    }],
                    "delta_future": true
                },
                "logprobs": null
            }],
            "usage": null,
            "chunk_future": "kept"
        });
        let chunk = ok(serde_json::from_value::<ChatCompletionChunk>(
            fixture.clone(),
        ));
        let call = match &chunk.choices[0].delta.tool_calls {
            Omittable::Value(calls) => &calls[0],
            Omittable::Omitted => panic!("fixture must contain a tool-call delta"),
        };
        assert_eq!(
            call.kind.as_ref(),
            Omittable::Value(&ChatToolKind::Unknown("future_function_kind".into()))
        );
        let arguments = match &call.function {
            Omittable::Value(function) => match &function.arguments {
                Omittable::Value(arguments) => arguments,
                Omittable::Omitted => panic!("fixture must contain arguments"),
            },
            Omittable::Omitted => panic!("fixture must contain a function delta"),
        };
        assert_eq!(arguments.as_raw(), "{\"city\":");
        assert!(arguments.parse().is_err());
        assert!(call.extra().contains_key("tool_delta_future"));
        assert!(chunk.choices[0].delta.extra().contains_key("delta_future"));
        assert!(chunk.extra().contains_key("chunk_future"));
        assert_eq!(ok(serde_json::to_value(chunk)), fixture);
    }

    #[test]
    fn response_message_accepts_omitted_or_null_refusal() {
        let omitted = json!({"role": "assistant", "content": "hello"});
        let message = ok(serde_json::from_value::<ChatResponseMessage>(
            omitted.clone(),
        ));
        assert!(message.refusal.is_omitted());
        assert_eq!(ok(serde_json::to_value(message)), omitted);

        let valid = json!({"role": "assistant", "content": null, "refusal": null});
        let message = ok(serde_json::from_value::<ChatResponseMessage>(valid.clone()));
        assert!(message.content.is_null());
        assert!(matches!(message.refusal, Omittable::Value(Nullable::Null)));
        assert_eq!(ok(serde_json::to_value(message)), valid);
    }

    #[test]
    fn official_chat_completion_list_message_nulls_decode() {
        let message = ok(serde_json::from_value::<ChatResponseMessage>(json!({
            "role": "assistant",
            "content": "Mind of circuits hum",
            "tool_calls": null,
            "function_call": null
        })));
        assert_eq!(message.tool_calls, Omittable::Value(Nullable::Null));
        assert_eq!(message.function_call, Omittable::Value(Nullable::Null));
        assert!(
            serde_json::from_value::<ChatResponseMessage>(json!({
                "role": "assistant",
                "content": "hello",
                "tool_calls": null,
                "annotations": null
            }))
            .is_err(),
            "unofficial annotations null still fails"
        );

        let list = ok(serde_json::from_value::<ChatCompletionList>(json!({
            "object": "list",
            "data": [{
                "object": "chat.completion",
                "id": "chatcmpl-AyPNinnUqUDYo9SAdA52NobMflmj2",
                "model": "gpt-4o-2024-08-06",
                "created": 1_738_960_610_u64,
                "request_id": "req_ded8ab984ec4bf840f37566c1011c417",
                "tool_choice": null,
                "usage": {
                    "total_tokens": 31,
                    "completion_tokens": 18,
                    "prompt_tokens": 13
                },
                "system_fingerprint": "fp_50cad350e4",
                "input_user": null,
                "service_tier": "default",
                "tools": null,
                "metadata": {},
                "choices": [{
                    "index": 0,
                    "message": {
                        "content": "Mind of circuits hum",
                        "role": "assistant",
                        "tool_calls": null,
                        "function_call": null
                    },
                    "finish_reason": "stop",
                    "logprobs": null
                }],
                "response_format": null
            }],
            "first_id": "chatcmpl-AyPNinnUqUDYo9SAdA52NobMflmj2",
            "last_id": "chatcmpl-AyPNinnUqUDYo9SAdA52NobMflmj2",
            "has_more": false
        })));
        assert_eq!(
            list.data[0].choices[0].message.tool_calls,
            Omittable::Value(Nullable::Null)
        );
        assert_eq!(
            list.data[0].choices[0].message.function_call,
            Omittable::Value(Nullable::Null)
        );
        assert_eq!(list.data[0].extra().get("tools"), Some(&Value::Null));
        assert_eq!(list.data[0].extra().get("input_user"), Some(&Value::Null));
    }

    #[test]
    fn deepseek_chat_completion_omits_refusal_and_keeps_reasoning() {
        let fixture = json!({
            "id": "65d51090-b887-4d76-b4ec-799b87e1e413",
            "object": "chat.completion",
            "created": 1788099814,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "你好！",
                    "reasoning_content": "The user greeted me."
                },
                "logprobs": null,
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 96,
                "completion_tokens": 65,
                "total_tokens": 161,
                "prompt_tokens_details": { "cached_tokens": 0 },
                "completion_tokens_details": { "reasoning_tokens": 50 },
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 96
            },
            "system_fingerprint": "a26a7955944dc5c60445bff77fac9c8e"
        });
        let completion = ok(serde_json::from_value::<ChatCompletion>(fixture));
        assert_eq!(completion.output_text(), "你好！");
        assert!(completion.choices[0].message.refusal.is_omitted());
        assert_eq!(
            completion.choices[0]
                .message
                .extra()
                .get("reasoning_content")
                .and_then(Value::as_str),
            Some("The user greeted me.")
        );
    }

    #[test]
    fn stored_completion_params_and_update_preserve_nullability() {
        let missing = ok(serde_json::from_value::<ChatCompletionListParams>(
            json!({}),
        ));
        assert!(missing.metadata.is_omitted());

        let params_fixture = json!({
            "model": "gpt-future",
            "metadata": null,
            "after": "chatcmpl_prev",
            "limit": 20,
            "order": "future_order"
        });
        let params = ok(serde_json::from_value::<ChatCompletionListParams>(
            params_fixture.clone(),
        ));
        assert!(matches!(params.metadata, Omittable::Value(Nullable::Null)));
        match &params.order {
            Omittable::Value(order) => assert_eq!(order.as_str(), "future_order"),
            Omittable::Omitted => panic!("fixture must contain order"),
        }
        assert_eq!(ok(serde_json::to_value(params)), params_fixture);

        assert!(serde_json::from_value::<UpdateChatCompletionRequest>(json!({})).is_err());
        assert_eq!(
            ok(serde_json::to_value(UpdateChatCompletionRequest::clear())),
            json!({"metadata": null})
        );
        assert_eq!(
            ok(serde_json::to_value(UpdateChatCompletionRequest::new(
                BTreeMap::from([("team".to_owned(), "sdk".to_owned())])
            ))),
            json!({"metadata": {"team": "sdk"}})
        );
    }

    #[test]
    fn stored_completion_list_and_delete_are_lossless() {
        let completion = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "gpt-5.6-sol",
            "choices": [],
            "completion_future": true
        });
        let page_fixture = json!({
            "object": "list",
            "data": [completion],
            "first_id": "chatcmpl_1",
            "last_id": "chatcmpl_1",
            "has_more": true,
            "page_future": 7
        });
        let page = ok(serde_json::from_value::<ChatCompletionList>(
            page_fixture.clone(),
        ));
        assert_eq!(page.next_after(), Some("chatcmpl_1"));
        assert!(page.data[0].extra().contains_key("completion_future"));
        assert!(page.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(page)), page_fixture);

        let deleted_fixture = json!({
            "object": "chat.completion.deleted",
            "id": "chatcmpl_1",
            "deleted": true,
            "delete_future": "kept"
        });
        let deleted = ok(serde_json::from_value::<ChatCompletionDeleted>(
            deleted_fixture.clone(),
        ));
        assert!(deleted.extra().contains_key("delete_future"));
        assert_eq!(ok(serde_json::to_value(deleted)), deleted_fixture);
    }

    #[test]
    fn stored_completion_and_message_pages_fall_back_to_the_last_item_id() {
        // D0147: a page advertising more results with an empty last_id must
        // still name a cursor via data[-1].id instead of yielding an empty
        // cursor that silently refetches the first page.
        let completion = json!({
            "id": "chatcmpl_9",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "gpt-5.6-sol",
            "choices": []
        });
        let page = ok(serde_json::from_value::<ChatCompletionList>(json!({
            "object": "list",
            "data": [completion],
            "first_id": "chatcmpl_9",
            "last_id": "",
            "has_more": true
        })));
        assert_eq!(page.next_after(), Some("chatcmpl_9"));

        let message_page = ok(serde_json::from_value::<ChatCompletionMessageList>(json!({
            "object": "list",
            "data": [{"id": "msg_9", "role": "assistant", "content": null}],
            "first_id": "msg_9",
            "last_id": "",
            "has_more": true
        })));
        assert_eq!(message_page.next_after(), Some("msg_9"));

        // A non-empty server cursor still wins over the fallback, and neither
        // an empty cursor with empty data nor a terminal page advances.
        let server_cursor = ok(serde_json::from_value::<ChatCompletionList>(json!({
            "object": "list",
            "data": [completion],
            "first_id": "chatcmpl_9",
            "last_id": "chatcmpl_server",
            "has_more": true
        })));
        assert_eq!(server_cursor.next_after(), Some("chatcmpl_server"));

        let unresolved = ok(serde_json::from_value::<ChatCompletionList>(json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "",
            "has_more": true
        })));
        assert_eq!(unresolved.next_after(), None);

        let terminal = ok(serde_json::from_value::<ChatCompletionList>(json!({
            "object": "list",
            "data": [completion],
            "first_id": "chatcmpl_9",
            "last_id": "chatcmpl_9",
            "has_more": false
        })));
        assert_eq!(terminal.next_after(), None);
    }

    #[test]
    fn stored_message_list_content_parts_are_strict_and_lossless() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "id": "msg_1",
                "role": "assistant",
                "content": "A cat",
                "refusal": null,
                "content_parts": [
                    {"type": "text", "text": "A cat", "part_future": true},
                    {
                        "type": "image_url",
                        "image_url": {"url": "https://example.test/cat.png", "detail": "high"}
                    }
                ],
                "message_future": {"x": 1}
            }],
            "first_id": "msg_1",
            "last_id": "msg_1",
            "has_more": true,
            "list_future": true
        });
        let page = ok(serde_json::from_value::<ChatCompletionMessageList>(
            fixture.clone(),
        ));
        assert_eq!(page.next_after(), Some("msg_1"));
        assert!(page.data[0].extra().contains_key("message_future"));
        assert!(page.extra().contains_key("list_future"));
        match &page.data[0].content_parts {
            Omittable::Value(Nullable::Value(parts)) => match &parts[0] {
                ChatCompletionStoreMessageContentPart::Text(text) => {
                    assert_eq!(text.text, "A cat");
                    assert!(text.extra().contains_key("part_future"));
                }
                _ => panic!("expected text content part"),
            },
            _ => panic!("fixture must contain content parts"),
        }
        let encoded = ok(serde_json::to_value(page));
        assert_eq!(encoded, fixture);

        assert!(
            serde_json::from_value::<ChatCompletionStoreMessageContentPart>(json!({
                "type": "image_url",
                "image_url": {}
            }))
            .is_err()
        );

        let null_parts = json!({
            "object": "list",
            "data": [{
                "id": "msg_2",
                "role": "assistant",
                "content": null,
                "refusal": null,
                "content_parts": null
            }],
            "first_id": "msg_2",
            "last_id": "msg_2",
            "has_more": false
        });
        let decoded = ok(serde_json::from_value::<ChatCompletionMessageList>(
            null_parts.clone(),
        ));
        assert!(matches!(
            decoded.data[0].content_parts,
            Omittable::Value(Nullable::Null)
        ));
        assert_eq!(ok(serde_json::to_value(decoded)), null_parts);
    }

    #[test]
    fn prompt_cache_types_share_ga_responses_wire_json() {
        let breakpoint = serde_json::to_value(ChatPromptCacheBreakpoint::explicit()).expect("ser");
        assert_eq!(
            breakpoint,
            serde_json::to_value(crate::PromptCacheBreakpoint::explicit()).expect("ser")
        );
        assert_eq!(breakpoint, json!({ "mode": "explicit" }));

        let options = ChatPromptCacheOptions {
            mode: Omittable::Value(ChatPromptCacheMode::Implicit),
            ttl: Omittable::Value(ChatPromptCacheTtl::ThirtyMinutes),
        };
        assert_eq!(
            serde_json::to_value(&options).expect("ser"),
            json!({ "mode": "implicit", "ttl": "30m" })
        );
    }

    #[test]
    fn chat_completion_decodes_python_moderation_results_list() {
        let fixture = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-5.6-sol",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "logprobs": null,
                "finish_reason": "stop"
            }],
            "moderation": {
                "input": {
                    "type": "moderation_results",
                    "model": "omni-moderation-latest",
                    "results": [{
                        "type": "moderation_result",
                        "model": "omni-moderation-latest",
                        "flagged": false,
                        "categories": { "hate": false },
                        "category_scores": { "hate": 0.01 },
                        "category_applied_input_types": { "hate": ["text"] }
                    }]
                },
                "output": {
                    "type": "error",
                    "code": "moderation_unavailable",
                    "message": "output skipped"
                }
            }
        });
        let completion = ok(serde_json::from_value::<ChatCompletion>(fixture));
        let moderation = completion.moderation().expect("typed chat moderation");
        assert!(matches!(
            moderation.input(),
            ChatCompletionModerationOutcome::Results { results, .. } if results.len() == 1
        ));
        assert!(matches!(
            moderation.output(),
            ChatCompletionModerationOutcome::Error { code, .. } if code == "moderation_unavailable"
        ));
    }

    #[test]
    fn chat_moderation_policy_mirrors_the_pinned_moderation_param() {
        use crate::responses::{ModerationConfig, ModerationDirection, ModerationMode};

        // The pinned CreateChatCompletionRequest.moderation is the Responses
        // host's ModerationParam: {model, policy?} with ModerationPolicyParam
        // {input?: ModerationConfigParam|null, output?: ...}. Every mode of
        // both directions, the explicit nulls, and omission must mirror the
        // Responses wire byte-for-byte (6-11).
        for (input, output) in [
            (ModerationMode::Score, ModerationMode::Block),
            (ModerationMode::Block, ModerationMode::Score),
        ] {
            let (input_wire, output_wire) = (input.as_str().to_owned(), output.as_str().to_owned());
            let policy = ModerationPolicy::default()
                .input(ModerationDirection::new(input))
                .output(ModerationDirection::new(output));
            let chat = serde_json::to_value(
                ChatModerationConfig::new("omni-moderation-latest").policy(policy.clone()),
            )
            .expect("serialize chat moderation");
            assert_eq!(
                chat,
                json!({
                    "model": "omni-moderation-latest",
                    "policy": {
                        "input": {"mode": input_wire},
                        "output": {"mode": output_wire}
                    }
                })
            );
            let responses_host = serde_json::to_value(
                ModerationConfig::new("omni-moderation-latest").policy(policy),
            )
            .expect("serialize responses moderation");
            assert_eq!(chat, responses_host, "both hosts share the pinned param");
        }

        let nulls = serde_json::to_value(
            ChatModerationConfig::new("omni-moderation-latest")
                .policy(ModerationPolicy::default().input_null().output_null()),
        )
        .expect("serialize explicit policy nulls");
        assert_eq!(
            nulls,
            json!({
                "model": "omni-moderation-latest",
                "policy": {"input": null, "output": null}
            })
        );
        assert_eq!(
            serde_json::to_value(ChatModerationConfig::new("omni-moderation-latest").policy(
                ModerationPolicy::default().input(ModerationDirection::new(
                    ModerationMode::from_raw("future-mode")
                ))
            ))
            .expect("unknown modes stay lossless")["policy"]["input"]["mode"],
            "future-mode"
        );
        let omitted = serde_json::to_value(ChatModerationConfig::new("omni-moderation-latest"))
            .expect("serialize omitted policy");
        assert!(omitted.get("policy").is_none());
        let explicit_null =
            serde_json::to_value(ChatModerationConfig::new("omni-moderation-latest").policy_null())
                .expect("serialize policy null");
        assert_eq!(explicit_null["policy"], Value::Null);

        let decoded = serde_json::from_value::<ChatModerationConfig>(json!({
            "model": "omni-moderation-latest",
            "policy": {
                "input": {"mode": "score"},
                "output": null
            }
        }))
        .expect("decode typed policy");
        assert_eq!(
            decoded.policy,
            Omittable::Value(Nullable::Value(
                ModerationPolicy::default()
                    .input(ModerationDirection::new(ModerationMode::Score))
                    .output_null()
            ))
        );

        #[derive(Serialize)]
        struct PinnedShape {
            input: Option<PinnedDirection>,
            output: Option<PinnedDirection>,
        }
        #[derive(Serialize)]
        struct PinnedDirection {
            mode: &'static str,
        }
        let escaped = ChatModerationConfig::new("omni-moderation-latest")
            .with_policy(&PinnedShape {
                input: Some(PinnedDirection { mode: "block" }),
                output: None,
            })
            .expect("pinned-shaped serializable policy is accepted");
        assert_eq!(
            serde_json::to_value(escaped).expect("serialize escaped policy")["policy"],
            json!({"input": {"mode": "block"}, "output": null})
        );

        #[derive(Serialize)]
        struct FutureShape {
            input: PinnedDirection,
            retention: &'static str,
        }
        assert!(
            ChatModerationConfig::new("omni-moderation-latest")
                .with_policy(&FutureShape {
                    input: PinnedDirection { mode: "score" },
                    retention: "24h",
                })
                .is_err(),
            "members outside ModerationPolicyParam are rejected, not dropped"
        );
    }

    #[test]
    fn chat_create_validate_enforces_pinned_limits() {
        let ok_request =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"))
                .with_temperature(2.0)
                .with_logprobs(Some(20));
        ok_request.validate().expect("boundary values are accepted");

        let mut over =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        over.body.frequency_penalty = Omittable::Value(Nullable::Value(2.1));
        assert!(matches!(
            over.validate(),
            Err(CreateChatCompletionConstraintError::FrequencyPenalty { .. })
        ));

        let mut n = CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        n.body.n = Omittable::Value(Nullable::Value(129));
        assert!(matches!(
            n.validate(),
            Err(CreateChatCompletionConstraintError::Choices { actual: 129, .. })
        ));

        let mut stops =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        stops.body.stop = Omittable::Value(Nullable::Value(ChatStop::Many(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ])));
        assert!(matches!(
            stops.validate(),
            Err(CreateChatCompletionConstraintError::StopSequences { actual: 5, .. })
        ));

        let mut bias =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        bias.body.logit_bias =
            Omittable::Value(Nullable::Value(BTreeMap::from([("50256".into(), 101)])));
        assert!(matches!(
            bias.validate(),
            Err(CreateChatCompletionConstraintError::LogitBias { actual: 101, .. })
        ));

        let mut empty_functions =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        empty_functions.body.functions = Omittable::Value(Vec::new());
        assert!(matches!(
            empty_functions.validate(),
            Err(CreateChatCompletionConstraintError::Functions { actual: 0, .. })
        ));
        let mut too_many =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        too_many.body.functions = Omittable::Value(
            (0..=MAX_CHAT_FUNCTIONS)
                .map(|index| ChatCompletionFunction::new(format!("fn_{index}")))
                .collect(),
        );
        assert!(matches!(
            too_many.validate(),
            Err(CreateChatCompletionConstraintError::Functions { actual: 129, .. })
        ));
        let decoded = serde_json::from_value::<CreateChatCompletionRequest>(json!({
            "model": "gpt-5.6-sol",
            "messages": [{"role": "user", "content": "hello"}],
            "functions": []
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());

        let empty_user =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::parts(Vec::new()));
        assert!(matches!(
            empty_user.validate(),
            Err(CreateChatCompletionConstraintError::EmptyMessageContent)
        ));
        let empty_developer = CreateChatCompletionRequest::new(
            "gpt-5.6-sol",
            ChatDeveloperMessage::new(ChatInstructionContent::Parts(Vec::new())),
        );
        assert!(matches!(
            empty_developer.validate(),
            Err(CreateChatCompletionConstraintError::EmptyMessageContent)
        ));
        let mut empty_prediction =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        empty_prediction.body.prediction =
            Omittable::Value(Nullable::Value(ChatPredictionContent::parts(Vec::new())));
        assert!(matches!(
            empty_prediction.validate(),
            Err(CreateChatCompletionConstraintError::EmptyPredictionParts)
        ));
        let unofficial = serde_json::from_value::<CreateChatCompletionRequest>(json!({
            "model": "gpt-5.6-sol",
            "messages": [{"role": "user", "content": []}],
            "prediction": { "type": "content", "content": [] }
        }))
        .expect("serde remains lossless");
        assert!(matches!(
            unofficial.validate(),
            Err(CreateChatCompletionConstraintError::EmptyMessageContent)
        ));
    }

    #[test]
    fn stored_chat_update_and_list_match_openapi_inventory() {
        let update =
            UpdateChatCompletionRequest::new(BTreeMap::from([("topic".into(), "demo".into())]));
        let value = ok(serde_json::to_value(&update));
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["metadata"]);

        let list: ChatCompletionList = ok(serde_json::from_value(json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "",
            "has_more": false
        })));
        let encoded = ok(serde_json::to_value(&list));
        let mut keys: Vec<_> = encoded
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(keys, ["data", "first_id", "has_more", "last_id", "object"]);
    }

    /// The 33 optional properties pinned by `CreateChatCompletionRequest`,
    /// plus the required `messages` and `model`.
    const CHAT_CREATE_ALL_FIELDS: [&str; 35] = [
        "audio",
        "frequency_penalty",
        "function_call",
        "functions",
        "logit_bias",
        "logprobs",
        "max_completion_tokens",
        "max_tokens",
        "messages",
        "metadata",
        "modalities",
        "model",
        "moderation",
        "n",
        "parallel_tool_calls",
        "prediction",
        "presence_penalty",
        "prompt_cache_key",
        "prompt_cache_options",
        "prompt_cache_retention",
        "reasoning_effort",
        "response_format",
        "safety_identifier",
        "seed",
        "service_tier",
        "stop",
        "store",
        "temperature",
        "tool_choice",
        "tools",
        "top_logprobs",
        "top_p",
        "user",
        "verbosity",
        "web_search_options",
    ];

    /// Every `Omittable<Nullable<T>>` body property, each of which must keep
    /// an explicit wire `null` distinct from omission.
    const CHAT_CREATE_NULLABLE_FIELDS: [&str; 24] = [
        "audio",
        "frequency_penalty",
        "logit_bias",
        "logprobs",
        "max_completion_tokens",
        "max_tokens",
        "metadata",
        "modalities",
        "moderation",
        "n",
        "prediction",
        "presence_penalty",
        "prompt_cache_key",
        "prompt_cache_retention",
        "reasoning_effort",
        "safety_identifier",
        "seed",
        "service_tier",
        "stop",
        "store",
        "temperature",
        "top_logprobs",
        "top_p",
        "verbosity",
    ];

    #[test]
    fn chat_create_kitchen_sink_round_trips_every_field() {
        use crate::responses::{ModerationDirection, ModerationMode, ModerationPolicy};

        let grammar = ChatCustomToolDefinition::new("transcribe")
            .with_description("Free-form transcription")
            .with_format(ChatCustomToolFormat::Grammar(ChatCustomGrammarFormat::new(
                "start: WORD",
                ChatGrammarSyntax::Lark,
            )));
        let schema = ok(ChatJsonSchemaDefinition::new("weather")
            .with_description("Current weather")
            .with_schema(&weather_schema()))
        .with_strict(true);
        let mut request = CreateChatCompletionRequest::new(
            "gpt-5.6-sol",
            ChatUserMessage::text("Weather in Shanghai?"),
        )
        .with_tool(ChatCustomTool::new(grammar))
        .with_tool_choice(ChatToolChoice::Allowed(ChatAllowedToolsChoice::new(
            ChatAllowedTools::new(
                ChatAllowedToolsMode::Required,
                [ChatNamedCustomChoice::new("transcribe")],
            ),
        )))
        .with_response_format(ChatResponseFormat::JsonSchema(
            ChatResponseFormatJsonSchema::new(schema),
        ))
        .with_max_completion_tokens(1024)
        .with_temperature(1.5)
        .with_logprobs(Some(5));

        let body = &mut request.body;
        body.audio = Omittable::Value(Nullable::Value(ChatOutputAudio::new(
            ChatVoice::Named("coral".to_owned()),
            ChatOutputAudioFormat::Wav,
        )));
        body.frequency_penalty = Omittable::Value(Nullable::Value(0.5));
        body.function_call = Omittable::Value(ChatLegacyFunctionChoice::Named(
            ChatLegacyNamedFunctionChoice {
                name: "weather".to_owned(),
            },
        ));
        body.functions = Omittable::Value(vec![ChatCompletionFunction::new("weather")]);
        body.logit_bias =
            Omittable::Value(Nullable::Value(BTreeMap::from([("50256".to_owned(), 10)])));
        body.max_tokens = Omittable::Value(Nullable::Value(512));
        body.metadata = Omittable::Value(Nullable::Value(BTreeMap::from([(
            "team".to_owned(),
            "sdk".to_owned(),
        )])));
        body.modalities = Omittable::Value(Nullable::Value(vec![
            ChatModality::Text,
            ChatModality::Audio,
        ]));
        body.moderation = Omittable::Value(Nullable::Value(
            ChatModerationConfig::new("omni-moderation-latest").policy(
                ModerationPolicy::default()
                    .input(ModerationDirection::new(ModerationMode::Score))
                    .output(ModerationDirection::new(ModerationMode::Block)),
            ),
        ));
        body.n = Omittable::Value(Nullable::Value(1));
        body.parallel_tool_calls = Omittable::Value(false);
        body.prediction = Omittable::Value(Nullable::Value(ChatPredictionContent::parts([
            ChatTextContentPart::new("It is sunny"),
        ])));
        body.presence_penalty = Omittable::Value(Nullable::Value(0.25));
        body.prompt_cache_key = Omittable::Value(Nullable::Value("cache-key".to_owned()));
        body.prompt_cache_options = Omittable::Value(ChatPromptCacheOptions {
            ttl: Omittable::Value(ChatPromptCacheTtl::ThirtyMinutes),
            mode: Omittable::Value(ChatPromptCacheMode::Implicit),
        });
        body.prompt_cache_retention =
            Omittable::Value(Nullable::Value(ChatPromptCacheRetention::TwentyFourHours));
        body.reasoning_effort = Omittable::Value(Nullable::Value(ChatReasoningEffort::Medium));
        body.safety_identifier = Omittable::Value(Nullable::Value("user-1".to_owned()));
        body.seed = Omittable::Value(Nullable::Value(42));
        body.service_tier = Omittable::Value(Nullable::Value(ChatServiceTier::Flex));
        body.stop = Omittable::Value(Nullable::Value(ChatStop::Many(vec!["END".to_owned()])));
        body.store = Omittable::Value(Nullable::Value(true));
        body.top_p = Omittable::Value(Nullable::Value(0.9));
        body.user = Omittable::Value("user-1".to_owned());
        body.verbosity = Omittable::Value(Nullable::Value(ChatVerbosity::High));
        body.web_search_options = Omittable::Value(ChatWebSearchOptions {
            user_location: Omittable::Value(Nullable::Value(ChatWebSearchUserLocation::new(
                ChatWebSearchLocation {
                    country: Omittable::Value("US".to_owned()),
                    region: Omittable::Omitted,
                    city: Omittable::Value("Phoenix".to_owned()),
                    timezone: Omittable::Value("America/Phoenix".to_owned()),
                },
            ))),
            search_context_size: Omittable::Value(ChatWebSearchContextSize::Medium),
        });

        request
            .validate()
            .expect("kitchen-sink values stay in range");
        let value = ok(serde_json::to_value(&request));
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, CHAT_CREATE_ALL_FIELDS);

        let decoded = ok(serde_json::from_value::<CreateChatCompletionRequest>(
            value.clone(),
        ));
        assert_eq!(ok(serde_json::to_value(decoded)), value);
    }

    #[test]
    fn chat_create_explicit_nulls_round_trip_every_nullable_field() {
        // Chat request builders expose no per-field `*_null` constructors
        // (only `with_stream_options_null`), so explicit nulls are set
        // directly on the public body fields the way callers do.
        let mut request =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        let body = &mut request.body;
        body.audio = Omittable::Value(Nullable::Null);
        body.frequency_penalty = Omittable::Value(Nullable::Null);
        body.logit_bias = Omittable::Value(Nullable::Null);
        body.logprobs = Omittable::Value(Nullable::Null);
        body.max_completion_tokens = Omittable::Value(Nullable::Null);
        body.max_tokens = Omittable::Value(Nullable::Null);
        body.metadata = Omittable::Value(Nullable::Null);
        body.modalities = Omittable::Value(Nullable::Null);
        body.moderation = Omittable::Value(Nullable::Null);
        body.n = Omittable::Value(Nullable::Null);
        body.prediction = Omittable::Value(Nullable::Null);
        body.presence_penalty = Omittable::Value(Nullable::Null);
        body.prompt_cache_key = Omittable::Value(Nullable::Null);
        body.prompt_cache_retention = Omittable::Value(Nullable::Null);
        body.reasoning_effort = Omittable::Value(Nullable::Null);
        body.safety_identifier = Omittable::Value(Nullable::Null);
        body.seed = Omittable::Value(Nullable::Null);
        body.service_tier = Omittable::Value(Nullable::Null);
        body.stop = Omittable::Value(Nullable::Null);
        body.store = Omittable::Value(Nullable::Null);
        body.temperature = Omittable::Value(Nullable::Null);
        body.top_logprobs = Omittable::Value(Nullable::Null);
        body.top_p = Omittable::Value(Nullable::Null);
        body.verbosity = Omittable::Value(Nullable::Null);

        request
            .validate()
            .expect("official explicit nulls skip every range check");
        let value = ok(serde_json::to_value(&request));
        for key in CHAT_CREATE_NULLABLE_FIELDS {
            assert_eq!(value[key], Value::Null, "{key}");
        }
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        let mut expected: Vec<_> = CHAT_CREATE_NULLABLE_FIELDS.to_vec();
        expected.extend(["messages", "model"]);
        expected.sort();
        assert_eq!(keys, expected);

        let decoded = ok(serde_json::from_value::<CreateChatCompletionRequest>(
            value.clone(),
        ));
        assert_eq!(ok(serde_json::to_value(decoded)), value);
    }

    #[test]
    fn chat_create_prediction_keeps_all_three_wire_states() {
        // 10-06: `prediction` is `Omittable<Nullable<ChatPredictionContent>>`;
        // omission, explicit null, and a populated value must stay
        // distinguishable on the wire.
        let omitted =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        let omitted_value = ok(serde_json::to_value(&omitted));
        assert!(
            !omitted_value
                .as_object()
                .expect("object")
                .contains_key("prediction"),
            "omitted prediction must not serialize a key"
        );

        let mut nulled =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        nulled.body.prediction = Omittable::Value(Nullable::Null);
        let null_value = ok(serde_json::to_value(&nulled));
        assert_eq!(null_value["prediction"], Value::Null);

        let mut populated =
            CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hello"));
        populated.body.prediction =
            Omittable::Value(Nullable::Value(ChatPredictionContent::parts([
                ChatTextContentPart::new("It is sunny"),
            ])));
        let populated_value = ok(serde_json::to_value(&populated));
        assert_eq!(
            populated_value["prediction"]["content"][0]["text"],
            "It is sunny"
        );
    }

    #[test]
    fn final_usage_chunk_decodes_with_empty_choices_and_populated_usage() {
        let fixture = json!({
            "id": "chatcmpl_usage",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-5.6-sol",
            "choices": [],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 2,
                "total_tokens": 11,
                "compute_units": null,
                "usage_future": "kept"
            },
            "chunk_future": true
        });
        let chunk = ok(serde_json::from_value::<ChatCompletionChunk>(
            fixture.clone(),
        ));
        assert!(chunk.choices.is_empty());
        match &chunk.usage {
            Omittable::Value(Nullable::Value(usage)) => {
                assert_eq!(
                    (
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens
                    ),
                    (9, 2, 11)
                );
                assert_eq!(usage.compute_units, Omittable::Value(Nullable::Null));
                assert!(usage.extra().contains_key("usage_future"));
            }
            other => panic!("usage must decode as a non-null value, got {other:?}"),
        }
        assert!(chunk.extra().contains_key("chunk_future"));
        assert_eq!(ok(serde_json::to_value(chunk)), fixture);
    }

    #[test]
    fn populated_choice_logprobs_decode_and_round_trip() {
        let fixture = json!({
            "id": "chatcmpl_logprobs",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-5.6-sol",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "logprobs": {
                    "content": [
                        {
                            "token": "hel",
                            "logprob": -0.1,
                            "bytes": [104, 101, 108],
                            "top_logprobs": [
                                {"token": "hel", "logprob": -0.1, "bytes": null},
                                {"token": "lo", "logprob": -2.5, "bytes": [108, 111]}
                            ],
                            "token_future": 1
                        },
                        {"token": "!", "logprob": -0.05, "bytes": null, "top_logprobs": []}
                    ],
                    "refusal": [
                        {"token": "no", "logprob": -0.3, "bytes": null, "top_logprobs": []}
                    ],
                    "logprobs_future": "kept"
                },
                "finish_reason": "stop"
            }]
        });
        let completion = ok(serde_json::from_value::<ChatCompletion>(fixture.clone()));
        let logprobs = match &completion.choices[0].logprobs {
            Nullable::Value(logprobs) => logprobs,
            Nullable::Null => panic!("fixture must contain logprobs"),
        };
        match &logprobs.content {
            Nullable::Value(content) => {
                assert_eq!(content[0].bytes, Nullable::Value(vec![104, 101, 108]));
                match &content[0].top_logprobs[0].bytes {
                    Nullable::Null => {}
                    bytes => panic!("top_logprob bytes must stay null, got {bytes:?}"),
                }
                assert!(content[0].extra().contains_key("token_future"));
                assert_eq!(content[1].bytes, Nullable::Null);
            }
            Nullable::Null => panic!("content logprobs must decode"),
        }
        match &logprobs.refusal {
            Nullable::Value(refusal) => {
                assert_eq!(refusal[0].token, "no");
                assert_eq!(refusal[0].bytes, Nullable::Null);
            }
            Nullable::Null => panic!("refusal logprobs must decode"),
        }
        assert!(logprobs.extra().contains_key("logprobs_future"));
        assert_eq!(ok(serde_json::to_value(completion)), fixture);
    }

    #[test]
    fn chat_create_validate_enforces_the_remaining_pinned_limits() {
        let base = || CreateChatCompletionRequest::new("gpt-5.6-sol", ChatUserMessage::text("hi"));

        let mut boundary = base();
        boundary.body.presence_penalty = Omittable::Value(Nullable::Value(-2.0));
        boundary.body.top_p = Omittable::Value(Nullable::Value(0.0));
        boundary.body.safety_identifier =
            Omittable::Value(Nullable::Value("i".repeat(MAX_SAFETY_IDENTIFIER_CHARS)));
        boundary.body.metadata = Omittable::Value(Nullable::Value(BTreeMap::from([(
            "k".repeat(MAX_RESPONSE_METADATA_KEY_CHARS),
            "v".repeat(MAX_RESPONSE_METADATA_VALUE_CHARS),
        )])));
        boundary
            .validate()
            .expect("boundary presence_penalty/top_p/metadata/safety stay accepted");

        let mut temperature = base();
        temperature.body.temperature = Omittable::Value(Nullable::Value(2.1));
        assert!(matches!(
            temperature.validate(),
            Err(CreateChatCompletionConstraintError::Temperature { value }) if value == "2.1"
        ));

        let mut top_p = base();
        top_p.body.top_p = Omittable::Value(Nullable::Value(1.1));
        assert!(matches!(
            top_p.validate(),
            Err(CreateChatCompletionConstraintError::TopP { value }) if value == "1.1"
        ));

        let mut presence = base();
        presence.body.presence_penalty = Omittable::Value(Nullable::Value(-2.1));
        assert!(matches!(
            presence.validate(),
            Err(CreateChatCompletionConstraintError::PresencePenalty { value }) if value == "-2.1"
        ));

        let mut top_logprobs = base();
        top_logprobs.body.top_logprobs = Omittable::Value(Nullable::Value(21));
        assert!(matches!(
            top_logprobs.validate(),
            Err(CreateChatCompletionConstraintError::TopLogprobs {
                actual: 21,
                maximum: MAX_TOP_LOGPROBS
            })
        ));

        let mut pairs = base();
        pairs.body.metadata = Omittable::Value(Nullable::Value(
            (0..=MAX_RESPONSE_METADATA_PAIRS)
                .map(|index| (format!("key{index}"), "value".to_owned()))
                .collect(),
        ));
        assert!(matches!(
            pairs.validate(),
            Err(CreateChatCompletionConstraintError::MetadataPairCount {
                actual: 17,
                maximum: MAX_RESPONSE_METADATA_PAIRS
            })
        ));

        let mut key = base();
        key.body.metadata = Omittable::Value(Nullable::Value(BTreeMap::from([(
            "k".repeat(MAX_RESPONSE_METADATA_KEY_CHARS + 1),
            "v".to_owned(),
        )])));
        assert!(matches!(
            key.validate(),
            Err(CreateChatCompletionConstraintError::MetadataKey {
                actual: 65,
                maximum: MAX_RESPONSE_METADATA_KEY_CHARS
            })
        ));

        let mut value = base();
        value.body.metadata = Omittable::Value(Nullable::Value(BTreeMap::from([(
            "k".to_owned(),
            "v".repeat(MAX_RESPONSE_METADATA_VALUE_CHARS + 1),
        )])));
        assert!(matches!(
            value.validate(),
            Err(CreateChatCompletionConstraintError::MetadataValue {
                actual: 513,
                maximum: MAX_RESPONSE_METADATA_VALUE_CHARS
            })
        ));

        let mut safety = base();
        safety.body.safety_identifier =
            Omittable::Value(Nullable::Value("i".repeat(MAX_SAFETY_IDENTIFIER_CHARS + 1)));
        assert!(matches!(
            safety.validate(),
            Err(CreateChatCompletionConstraintError::SafetyIdentifier {
                actual: 65,
                maximum: MAX_SAFETY_IDENTIFIER_CHARS
            })
        ));

        let mut empty = base();
        empty.body.messages.clear();
        assert_eq!(
            empty.validate(),
            Err(CreateChatCompletionConstraintError::EmptyMessages)
        );
    }

    #[test]
    fn tool_choice_modes_named_custom_and_allowed_variants_round_trip() {
        for mode in ["auto", "none", "required"] {
            let wire = json!(mode);
            let decoded = ok(serde_json::from_value::<ChatToolChoice>(wire.clone()));
            assert!(
                matches!(&decoded, ChatToolChoice::Mode(value) if value.as_str() == mode),
                "{mode} must decode as a string mode"
            );
            assert_eq!(ok(serde_json::to_value(decoded)), wire);
        }

        let custom = ChatToolChoice::Custom(ChatNamedCustomChoice::new("synthesizer"));
        let custom_wire = json!({"type": "custom", "custom": {"name": "synthesizer"}});
        assert_eq!(ok(serde_json::to_value(&custom)), custom_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatToolChoice>(
                custom_wire.clone()
            )),
            custom
        );

        let allowed = ChatToolChoice::Allowed(ChatAllowedToolsChoice::new(ChatAllowedTools::new(
            ChatAllowedToolsMode::Required,
            [
                ChatAllowedTool::Reference(ChatNamedCustomChoice::new("synthesizer").into()),
                ChatAllowedTool::Arbitrary(
                    BTreeMap::from([
                        ("connector".to_owned(), json!("future-connector")),
                        ("version".to_owned(), json!(2)),
                    ])
                    .into_iter()
                    .collect::<serde_json::Map<_, _>>(),
                ),
            ],
        )));
        let allowed_wire = json!({
            "type": "allowed_tools",
            "allowed_tools": {
                "mode": "required",
                "tools": [
                    {"type": "custom", "custom": {"name": "synthesizer"}},
                    {"connector": "future-connector", "version": 2}
                ]
            }
        });
        assert_eq!(ok(serde_json::to_value(&allowed)), allowed_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatToolChoice>(
                allowed_wire.clone()
            )),
            allowed
        );

        let future = json!({"type": "future_policy", "strict": true});
        let decoded = ok(serde_json::from_value::<ChatToolChoice>(future.clone()));
        assert!(matches!(decoded, ChatToolChoice::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn custom_tool_round_trips_text_grammar_and_future_formats() {
        let text = ChatTool::Custom(ChatCustomTool::new(
            ChatCustomToolDefinition::new("validator")
                .with_description("Validates free text")
                .with_format(ChatCustomToolFormat::Text(ChatCustomTextFormat::new())),
        ));
        let text_wire = json!({
            "type": "custom",
            "custom": {
                "name": "validator",
                "description": "Validates free text",
                "format": {"type": "text"}
            }
        });
        assert_eq!(ok(serde_json::to_value(&text)), text_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatTool>(text_wire.clone())),
            text
        );

        let grammar = ChatTool::Custom(ChatCustomTool::new(
            ChatCustomToolDefinition::new("parser").with_format(ChatCustomToolFormat::Grammar(
                ChatCustomGrammarFormat::new("start: WORD", ChatGrammarSyntax::Regex),
            )),
        ));
        let grammar_wire = json!({
            "type": "custom",
            "custom": {
                "name": "parser",
                "format": {
                    "type": "grammar",
                    "grammar": {"definition": "start: WORD", "syntax": "regex"}
                }
            }
        });
        assert_eq!(ok(serde_json::to_value(&grammar)), grammar_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatTool>(grammar_wire.clone())),
            grammar
        );

        let future = json!({
            "type": "custom",
            "custom": {
                "name": "parser",
                "format": {"type": "future_format", "knobs": 3}
            }
        });
        let decoded = ok(serde_json::from_value::<ChatTool>(future.clone()));
        match &decoded {
            ChatTool::Custom(tool) => match &tool.custom.format {
                Omittable::Value(format) => {
                    assert!(matches!(format, ChatCustomToolFormat::Unknown(_)));
                }
                Omittable::Omitted => panic!("future format must be retained"),
            },
            other => panic!("expected a custom tool, got {other:?}"),
        }
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn response_format_three_variants_round_trip() {
        let text = ChatResponseFormat::Text(ChatResponseFormatText::new());
        let text_wire = json!({"type": "text"});
        assert_eq!(ok(serde_json::to_value(&text)), text_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatResponseFormat>(
                text_wire.clone()
            )),
            text
        );

        let object = ChatResponseFormat::JsonObject(ChatResponseFormatJsonObject::new());
        let object_wire = json!({"type": "json_object"});
        assert_eq!(ok(serde_json::to_value(&object)), object_wire);
        assert_eq!(
            ok(serde_json::from_value::<ChatResponseFormat>(
                object_wire.clone()
            )),
            object
        );

        let schema = ok(ChatJsonSchemaDefinition::new("weather")
            .with_description("Current weather")
            .with_schema(&weather_schema()))
        .with_strict(true);
        let json_schema = ChatResponseFormat::JsonSchema(ChatResponseFormatJsonSchema::new(schema));
        let encoded = ok(serde_json::to_value(&json_schema));
        assert_eq!(encoded["type"], "json_schema");
        assert_eq!(encoded["json_schema"]["name"], "weather");
        assert_eq!(encoded["json_schema"]["description"], "Current weather");
        assert_eq!(encoded["json_schema"]["strict"], true);
        assert_eq!(encoded["json_schema"]["schema"]["type"], "object");
        assert_eq!(
            ok(serde_json::from_value::<ChatResponseFormat>(
                encoded.clone()
            )),
            json_schema
        );

        let future = json!({"type": "future_format"});
        let decoded = ok(serde_json::from_value::<ChatResponseFormat>(future.clone()));
        assert!(matches!(decoded, ChatResponseFormat::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn assistant_refusal_content_part_and_future_tags_round_trip() {
        let mut assistant = ChatAssistantMessage::text("placeholder");
        assistant.content = Omittable::Value(Nullable::Value(ChatAssistantContent::Parts(vec![
            ChatTextContentPart::new("It is sunny").into(),
            ChatRefusalContentPart::new("cannot help with that").into(),
        ])));
        let message = ok(serde_json::to_value(&assistant));
        assert_eq!(
            message["content"],
            json!([
                {"type": "text", "text": "It is sunny"},
                {"type": "refusal", "refusal": "cannot help with that"}
            ])
        );
        let decoded = ok(serde_json::from_value::<ChatMessage>(message.clone()));
        match &decoded {
            ChatMessage::Assistant(message) => match &message.content {
                Omittable::Value(Nullable::Value(ChatAssistantContent::Parts(parts))) => {
                    assert!(matches!(
                        &parts[1],
                        ChatAssistantContentPart::Refusal(part) if part.refusal == "cannot help with that"
                    ));
                }
                other => panic!("assistant parts must decode, got {other:?}"),
            },
            other => panic!("expected an assistant message, got {other:?}"),
        }
        assert_eq!(ok(serde_json::to_value(decoded)), message);

        let future = json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "partial"},
                {"type": "future_part", "weight": 0.7}
            ]
        });
        let decoded = ok(serde_json::from_value::<ChatMessage>(future.clone()));
        match &decoded {
            ChatMessage::Assistant(message) => match &message.content {
                Omittable::Value(Nullable::Value(ChatAssistantContent::Parts(parts))) => {
                    assert!(matches!(
                        &parts[1],
                        ChatAssistantContentPart::Unknown(part) if part.discriminator() == "future_part"
                    ));
                }
                other => panic!("future part must stay retained, got {other:?}"),
            },
            other => panic!("expected an assistant message, got {other:?}"),
        }
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }
}
