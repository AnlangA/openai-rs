//! Bidirectional wire types for the Chat Completions API.
//!
//! Request builders cover text, image, audio, file, function-tool, custom-tool,
//! and structured-output inputs without requiring callers to format JSON text.
//! Tagged unions reject malformed payloads for known tags while retaining a
//! future tag and its complete semantic JSON object.

use std::{collections::BTreeMap, fmt, marker::PhantomData};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{
    ExtraFields, FileId, JsonText, ModelId, Nullable, Omittable,
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

crate::open_string_enum! {
    /// Prompt-cache mode.
    pub enum ChatPromptCacheMode {
        Implicit = "implicit",
        Explicit = "explicit"
    }
}

crate::open_string_enum! {
    /// Minimum prompt-cache lifetime.
    pub enum ChatPromptCacheTtl {
        ThirtyMinutes = "30m"
    }
}

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

literal_tag!(PromptCacheBreakpointTag, Explicit, "explicit");

/// Marks the end of a reusable prompt prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPromptCacheBreakpoint {
    mode: PromptCacheBreakpointTag,
}

impl ChatPromptCacheBreakpoint {
    /// Construct an explicit cache breakpoint.
    #[must_use]
    pub const fn explicit() -> Self {
        Self {
            mode: PromptCacheBreakpointTag::Explicit,
        }
    }
}

impl Default for ChatPromptCacheBreakpoint {
    fn default() -> Self {
        Self::explicit()
    }
}

/// Prompt-cache behavior for a Chat request.
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
    pub parameters: Omittable<Value>,
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
        self.parameters = Omittable::Value(serde_json::to_value(parameters)?);
        Ok(self)
    }

    /// Enable or disable strict schema adherence.
    #[must_use]
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Omittable::Value(Nullable::Value(strict));
        self
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

/// Predefined set of tools available to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatAllowedTools {
    /// Whether using an allowed tool is optional or required.
    pub mode: ChatAllowedToolsMode,
    /// Named tool references.
    pub tools: Vec<ChatToolReference>,
}

impl ChatAllowedTools {
    /// Construct an allowed-tools constraint.
    #[must_use]
    pub fn new(
        mode: ChatAllowedToolsMode,
        tools: impl IntoIterator<Item = ChatToolReference>,
    ) -> Self {
        Self {
            mode,
            tools: tools.into_iter().collect(),
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
    pub schema: Omittable<Value>,
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
        self.schema = Omittable::Value(serde_json::to_value(schema)?);
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatModerationConfig {
    /// Moderation model identifier.
    pub model: String,
    /// Policy object; represented semantically and constructible from any
    /// serializable typed policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub policy: Omittable<Nullable<Value>>,
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

    /// Serialize a typed moderation policy.
    pub fn with_policy<T: Serialize>(mut self, policy: &T) -> Result<Self, serde_json::Error> {
        self.policy = Omittable::Value(Nullable::Value(serde_json::to_value(policy)?));
        Ok(self)
    }
}

/// Deprecated function-selection mode.
crate::open_string_enum! {
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
    /// Deprecated function definitions.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub functions: Omittable<Vec<ChatFunctionDefinition>>,
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
}
