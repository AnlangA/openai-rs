//! Wire types for the preview Responses multi-agent API.
//!
//! The frozen beta schema intentionally reuses the stable Responses codecs for
//! branches whose wire contract is identical. Preview-only agent metadata,
//! multi-agent items, and WebSocket injection events remain explicit Rust
//! types instead of being hidden in untyped JSON values.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{
    ExtraFields, JsonText, Nullable, Omittable,
    responses::{
        ConversationObjectReference, ConversationReference, IncompleteDetails, InputContent,
        MessageRole, PromptReference, ResponseError, ResponseInputItem, ResponseInstructions,
        ResponseItemStatus, ResponseOutputItem, ResponseStatus, ResponseStreamEvent,
        ResponseStreamOptions, ResponseTextConfig, ResponseTool, ResponseUsage, ToolChoice,
        TruncationStrategy, UnknownTaggedObject,
    },
};

macro_rules! impl_tagged_content {
    ($name:ident { $($variant:ident($ty:ty) => $wire:literal),+ $(,)? }) => {
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
                match discriminator(&value).map_err(D::Error::custom)? {
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

crate::open_string_enum! {
    /// Action supported by the preview server-hosted multi-agent runtime.
    pub enum BetaMultiAgentAction {
        SpawnAgent = "spawn_agent",
        InterruptAgent = "interrupt_agent",
        ListAgents = "list_agents",
        SendMessage = "send_message",
        FollowupTask = "followup_task",
        WaitAgent = "wait_agent",
    }
}

crate::open_string_enum! {
    /// Phase assigned to a message produced by an agent.
    pub enum BetaMessagePhase {
        Commentary = "commentary",
        FinalAnswer = "final_answer",
    }
}

crate::open_string_enum! {
    /// Ordering for beta response input-item pages.
    pub enum BetaResponseItemOrder {
        Asc = "asc",
        Desc = "desc",
    }
}

crate::open_string_enum! {
    /// Response fields that can be explicitly expanded by the API.
    pub enum BetaResponseIncludable {
        WebSearchSources = "web_search_call.action.sources",
        CodeInterpreterOutputs = "code_interpreter_call.outputs",
        ComputerOutputImageUrl = "computer_call_output.output.image_url",
        FileSearchResults = "file_search_call.results",
        InputImageUrl = "message.input_image.image_url",
        OutputTextLogprobs = "message.output_text.logprobs",
        EncryptedReasoning = "reasoning.encrypted_content",
    }
}

crate::open_string_enum! {
    /// Prompt-cache retention policy accepted by the pinned beta schema.
    pub enum BetaPromptCacheRetention {
        InMemory = "in_memory",
        TwentyFourHours = "24h",
    }
}

crate::open_string_enum! {
    /// Processing tier accepted by beta Responses.
    pub enum BetaServiceTier {
        Auto = "auto",
        Default = "default",
        Flex = "flex",
        Scale = "scale",
        Priority = "priority",
        Fast = "fast",
        Ultrafast = "ultrafast",
    }
}

/// Shared with GA Responses.
pub use crate::responses::ReasoningContext as BetaReasoningContext;
/// Shared with GA Responses, including `max`.
pub use crate::responses::ReasoningEffort as BetaReasoningEffort;
/// Shared with GA Responses.
pub use crate::responses::ReasoningMode as BetaReasoningMode;
/// Shared with GA Responses.
pub use crate::responses::ReasoningSummary as BetaReasoningSummary;

/// Canonical identity attached to an item or streaming event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetaAgent {
    agent_name: String,
}

impl BetaAgent {
    /// Creates agent metadata from its canonical runtime name.
    #[must_use]
    pub fn new(agent_name: impl Into<String>) -> Self {
        Self {
            agent_name: agent_name.into(),
        }
    }

    /// Returns the canonical agent name.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }
}

literal_tag!(DirectCallerTag, Direct, "direct");
literal_tag!(ProgramCallerTag, Program, "program");

/// Typed execution context for a tool call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaCaller {
    /// The root model invoked the tool directly.
    Direct,
    /// A program item invoked the tool.
    Program { caller_id: String },
    /// A future caller kind retained losslessly.
    Unknown(UnknownTaggedObject),
}

impl Serialize for BetaCaller {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Direct => {
                #[derive(Serialize)]
                struct Direct {
                    #[serde(rename = "type")]
                    kind: DirectCallerTag,
                }
                Direct {
                    kind: DirectCallerTag::Direct,
                }
                .serialize(serializer)
            }
            Self::Program { caller_id } => {
                #[derive(Serialize)]
                struct Program<'a> {
                    #[serde(rename = "type")]
                    kind: ProgramCallerTag,
                    caller_id: &'a str,
                }
                Program {
                    kind: ProgramCallerTag::Program,
                    caller_id,
                }
                .serialize(serializer)
            }
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BetaCaller {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match discriminator(&value).map_err(D::Error::custom)? {
            "direct" => Ok(Self::Direct),
            "program" => value
                .get("caller_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .map(|caller_id| Self::Program { caller_id })
                .ok_or_else(|| D::Error::custom("program caller requires string `caller_id`")),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// Preview metadata carried by stable item branches.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaItemMetadata {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<BetaCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    phase: Omittable<Nullable<BetaMessagePhase>>,
}

impl BetaItemMetadata {
    /// Returns the non-null owning agent.
    #[must_use]
    pub fn agent(&self) -> Option<&BetaAgent> {
        non_null(&self.agent)
    }

    /// Returns the non-null tool caller.
    #[must_use]
    pub fn caller(&self) -> Option<&BetaCaller> {
        non_null(&self.caller)
    }

    /// Returns the entity that created the item.
    #[must_use]
    pub fn created_by(&self) -> Option<&str> {
        omitted_ref(&self.created_by).map(String::as_str)
    }

    /// Returns the message phase.
    #[must_use]
    pub fn phase(&self) -> Option<&BetaMessagePhase> {
        non_null(&self.phase)
    }

    fn with_agent(mut self, agent: BetaAgent) -> Self {
        self.agent = Omittable::Value(Nullable::Value(agent));
        self
    }

    fn with_caller(mut self, caller: BetaCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    fn with_created_by(mut self, created_by: impl Into<String>) -> Self {
        self.created_by = Omittable::Value(created_by.into());
        self
    }

    fn with_phase(mut self, phase: BetaMessagePhase) -> Self {
        self.phase = Omittable::Value(Nullable::Value(phase));
        self
    }
}

/// Configuration for server-hosted multi-agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetaMultiAgentConfig {
    enabled: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_concurrent_subagents: Omittable<u32>,
}

impl BetaMultiAgentConfig {
    /// Enables or disables multi-agent execution.
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            max_concurrent_subagents: Omittable::Omitted,
        }
    }

    /// Sets the maximum number of simultaneously active descendants.
    #[must_use]
    pub const fn max_concurrent_subagents(mut self, maximum: u32) -> Self {
        self.max_concurrent_subagents = Omittable::Value(maximum);
        self
    }

    /// Returns whether the server-hosted runtime is enabled.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Shared with GA Responses; the pinned wire object is `{ "mode": "explicit" }`.
pub use crate::responses::PromptCacheBreakpoint as BetaPromptCacheBreakpoint;

/// A stable input-content branch with a typed beta prompt-cache breakpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaPromptCachedInputContent {
    core: InputContent,
    prompt_cache_breakpoint: Omittable<Nullable<BetaPromptCacheBreakpoint>>,
}

impl BetaPromptCachedInputContent {
    /// Wraps any stable input content branch.
    #[must_use]
    pub fn new(core: impl Into<InputContent>) -> Self {
        Self {
            core: core.into(),
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Marks the exact end of a reusable prompt prefix.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(BetaPromptCacheBreakpoint::explicit()));
        self
    }

    /// Borrows the shared stable content codec.
    #[must_use]
    pub const fn core(&self) -> &InputContent {
        &self.core
    }

    /// Returns the non-null breakpoint when supplied.
    #[must_use]
    pub fn breakpoint(&self) -> Option<&BetaPromptCacheBreakpoint> {
        non_null(&self.prompt_cache_breakpoint)
    }
}

impl Serialize for BetaPromptCachedInputContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Overlay<'a> {
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            prompt_cache_breakpoint: &'a Omittable<Nullable<BetaPromptCacheBreakpoint>>,
        }
        merge_serialized(
            &self.core,
            &Overlay {
                prompt_cache_breakpoint: &self.prompt_cache_breakpoint,
            },
        )
        .map_err(serde::ser::Error::custom)?
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaPromptCachedInputContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let prompt_cache_breakpoint = decode_omittable(
            value
                .as_object()
                .and_then(|object| object.get("prompt_cache_breakpoint")),
        )
        .map_err(D::Error::custom)?;
        Ok(Self {
            core: serde_json::from_value(value).map_err(D::Error::custom)?,
            prompt_cache_breakpoint,
        })
    }
}

literal_tag!(PromptCachedMessageTag, Message, "message");

/// Message input whose content exposes beta prompt-cache boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaPromptCachedInputMessage {
    content: Vec<BetaPromptCachedInputContent>,
    role: MessageRole,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<PromptCachedMessageTag>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    phase: Omittable<Nullable<BetaMessagePhase>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
}

impl BetaPromptCachedInputMessage {
    /// Creates a message from typed content parts.
    #[must_use]
    pub fn new(
        role: MessageRole,
        content: impl IntoIterator<Item = BetaPromptCachedInputContent>,
    ) -> Self {
        Self {
            content: content.into_iter().collect(),
            role,
            kind: Omittable::Omitted,
            id: Omittable::Omitted,
            agent: Omittable::Omitted,
            phase: Omittable::Omitted,
            status: Omittable::Omitted,
        }
    }

    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl IntoIterator<Item = BetaPromptCachedInputContent>) -> Self {
        Self::new(MessageRole::User, content)
    }

    /// Creates a developer message.
    #[must_use]
    pub fn developer(content: impl IntoIterator<Item = BetaPromptCachedInputContent>) -> Self {
        Self::new(MessageRole::Developer, content)
    }

    /// Emits the optional `type: "message"` property.
    #[must_use]
    pub fn with_type(mut self) -> Self {
        self.kind = Omittable::Value(PromptCachedMessageTag::Message);
        self
    }

    /// Attaches owning-agent metadata.
    #[must_use]
    pub fn agent(mut self, agent: BetaAgent) -> Self {
        self.agent = Omittable::Value(Nullable::Value(agent));
        self
    }

    /// Labels an assistant message as commentary or final output.
    #[must_use]
    pub fn phase(mut self, phase: BetaMessagePhase) -> Self {
        self.phase = Omittable::Value(Nullable::Value(phase));
        self
    }

    /// Returns content parts in wire order.
    #[must_use]
    pub fn content(&self) -> &[BetaPromptCachedInputContent] {
        &self.content
    }
}

literal_tag!(InputTextTag, InputText, "input_text");
literal_tag!(InputImageTag, InputImage, "input_image");
literal_tag!(EncryptedContentTag, EncryptedContent, "encrypted_content");

/// Text sent inside an inter-agent message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaAgentInputText {
    #[serde(rename = "type")]
    kind: InputTextTag,
    text: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<BetaPromptCacheBreakpoint>>,
}

impl BetaAgentInputText {
    /// Creates inter-agent plaintext content.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: InputTextTag::InputText,
            text: text.into(),
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Marks the end of an explicitly reusable prefix.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(BetaPromptCacheBreakpoint::explicit()));
        self
    }

    /// Returns the plaintext content.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Image sent inside an inter-agent message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaAgentInputImage {
    #[serde(rename = "type")]
    kind: InputImageTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<Nullable<crate::responses::ImageDetail>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<BetaPromptCacheBreakpoint>>,
}

impl BetaAgentInputImage {
    /// Creates image content from a URL or data URL.
    #[must_use]
    pub fn from_url(image_url: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            file_id: Omittable::Omitted,
            image_url: Omittable::Value(Nullable::Value(image_url.into())),
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Creates image content from an uploaded file id.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            file_id: Omittable::Value(Nullable::Value(file_id.into())),
            image_url: Omittable::Omitted,
            prompt_cache_breakpoint: Omittable::Omitted,
        }
    }

    /// Sets requested image detail.
    #[must_use]
    pub fn detail(mut self, detail: crate::responses::ImageDetail) -> Self {
        self.detail = Omittable::Value(Nullable::Value(detail));
        self
    }

    /// Marks the end of an explicitly reusable prefix.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(BetaPromptCacheBreakpoint::explicit()));
        self
    }
}

/// Opaque encrypted content sent between agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetaAgentEncryptedContent {
    #[serde(rename = "type")]
    kind: EncryptedContentTag,
    encrypted_content: String,
}

impl BetaAgentEncryptedContent {
    /// Wraps an opaque encrypted payload without interpreting it.
    #[must_use]
    pub fn new(encrypted_content: impl Into<String>) -> Self {
        Self {
            kind: EncryptedContentTag::EncryptedContent,
            encrypted_content: encrypted_content.into(),
        }
    }
}

/// One typed content part sent between agents.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaAgentMessageContent {
    Text(BetaAgentInputText),
    Image(BetaAgentInputImage),
    Encrypted(BetaAgentEncryptedContent),
    Unknown(UnknownTaggedObject),
}

impl_tagged_content!(BetaAgentMessageContent {
    Text(BetaAgentInputText) => "input_text",
    Image(BetaAgentInputImage) => "input_image",
    Encrypted(BetaAgentEncryptedContent) => "encrypted_content",
});

impl From<BetaAgentInputText> for BetaAgentMessageContent {
    fn from(value: BetaAgentInputText) -> Self {
        Self::Text(value)
    }
}

impl From<BetaAgentInputImage> for BetaAgentMessageContent {
    fn from(value: BetaAgentInputImage) -> Self {
        Self::Image(value)
    }
}

impl From<BetaAgentEncryptedContent> for BetaAgentMessageContent {
    fn from(value: BetaAgentEncryptedContent) -> Self {
        Self::Encrypted(value)
    }
}

literal_tag!(AgentMessageTag, AgentMessage, "agent_message");

/// A message routed from one named agent to another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaAgentMessage {
    author: String,
    content: Vec<BetaAgentMessageContent>,
    recipient: String,
    #[serde(rename = "type")]
    kind: AgentMessageTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
}

impl BetaAgentMessage {
    /// Creates a typed inter-agent message.
    #[must_use]
    pub fn new(
        author: impl Into<String>,
        recipient: impl Into<String>,
        content: impl IntoIterator<Item = impl Into<BetaAgentMessageContent>>,
    ) -> Self {
        Self {
            author: author.into(),
            content: content.into_iter().map(Into::into).collect(),
            recipient: recipient.into(),
            kind: AgentMessageTag::AgentMessage,
            id: Omittable::Omitted,
            agent: Omittable::Omitted,
        }
    }

    /// Adds the platform item id when replaying a returned item.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Attaches owning-agent metadata.
    #[must_use]
    pub fn agent(mut self, agent: BetaAgent) -> Self {
        self.agent = Omittable::Value(Nullable::Value(agent));
        self
    }

    /// Returns the sending agent identity.
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Returns the destination agent identity.
    #[must_use]
    pub fn recipient(&self) -> &str {
        &self.recipient
    }

    /// Returns message content in wire order.
    #[must_use]
    pub fn content(&self) -> &[BetaAgentMessageContent] {
        &self.content
    }

    /// Returns the owning agent when present and non-null.
    #[must_use]
    pub fn owning_agent(&self) -> Option<&BetaAgent> {
        non_null(&self.agent)
    }
}

literal_tag!(MultiAgentCallTag, MultiAgentCall, "multi_agent_call");

/// A model request to perform one multi-agent runtime action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaMultiAgentCall {
    action: BetaMultiAgentAction,
    arguments: JsonText,
    call_id: String,
    #[serde(rename = "type")]
    kind: MultiAgentCallTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
}

impl BetaMultiAgentCall {
    /// Creates a call from automatically serialized action arguments.
    pub fn from_serializable<T: Serialize>(
        action: BetaMultiAgentAction,
        call_id: impl Into<String>,
        arguments: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            action,
            arguments: JsonText::from_serializable(arguments)?.cast(),
            call_id: call_id.into(),
            kind: MultiAgentCallTag::MultiAgentCall,
            id: Omittable::Omitted,
            agent: Omittable::Omitted,
        })
    }

    /// Retains already-encoded argument JSON without eagerly parsing it.
    #[must_use]
    pub fn from_raw(
        action: BetaMultiAgentAction,
        call_id: impl Into<String>,
        arguments: impl Into<Box<str>>,
    ) -> Self {
        Self {
            action,
            arguments: JsonText::from_raw(arguments),
            call_id: call_id.into(),
            kind: MultiAgentCallTag::MultiAgentCall,
            id: Omittable::Omitted,
            agent: Omittable::Omitted,
        }
    }

    /// Returns the lazily parsed action arguments.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }

    /// Returns the call id used to correlate the output.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the requested runtime action.
    #[must_use]
    pub const fn action(&self) -> &BetaMultiAgentAction {
        &self.action
    }

    /// Returns the owning agent when present and non-null.
    #[must_use]
    pub fn agent(&self) -> Option<&BetaAgent> {
        non_null(&self.agent)
    }
}

/// Citation attached to a multi-agent action output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum BetaMultiAgentAnnotation {
    #[serde(rename = "file_citation")]
    FileCitation {
        file_id: String,
        filename: String,
        index: u64,
    },
    #[serde(rename = "url_citation")]
    UrlCitation {
        end_index: u64,
        start_index: u64,
        title: String,
        url: String,
    },
    #[serde(rename = "container_file_citation")]
    ContainerFileCitation {
        container_id: String,
        end_index: u64,
        file_id: String,
        filename: String,
        start_index: u64,
    },
}

literal_tag!(OutputTextTag, OutputText, "output_text");

/// One text block returned by a multi-agent runtime action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaMultiAgentOutputText {
    text: String,
    #[serde(rename = "type")]
    kind: OutputTextTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    annotations: Omittable<Vec<BetaMultiAgentAnnotation>>,
}

impl BetaMultiAgentOutputText {
    /// Creates a plain output-text block.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: OutputTextTag::OutputText,
            annotations: Omittable::Omitted,
        }
    }

    /// Adds a typed citation.
    #[must_use]
    pub fn annotation(mut self, annotation: BetaMultiAgentAnnotation) -> Self {
        let annotations = match &mut self.annotations {
            Omittable::Value(annotations) => annotations,
            Omittable::Omitted => {
                self.annotations = Omittable::Value(Vec::new());
                match &mut self.annotations {
                    Omittable::Value(annotations) => annotations,
                    Omittable::Omitted => unreachable!(),
                }
            }
        };
        annotations.push(annotation);
        self
    }

    /// Returns the output text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

literal_tag!(
    MultiAgentCallOutputTag,
    MultiAgentCallOutput,
    "multi_agent_call_output"
);

/// Output correlated with a multi-agent runtime call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaMultiAgentCallOutput {
    action: BetaMultiAgentAction,
    call_id: String,
    output: Vec<BetaMultiAgentOutputText>,
    #[serde(rename = "type")]
    kind: MultiAgentCallOutputTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
}

impl BetaMultiAgentCallOutput {
    /// Creates an output for one multi-agent action.
    #[must_use]
    pub fn new(
        action: BetaMultiAgentAction,
        call_id: impl Into<String>,
        output: impl IntoIterator<Item = BetaMultiAgentOutputText>,
    ) -> Self {
        Self {
            action,
            call_id: call_id.into(),
            output: output.into_iter().collect(),
            kind: MultiAgentCallOutputTag::MultiAgentCallOutput,
            id: Omittable::Omitted,
            agent: Omittable::Omitted,
        }
    }

    /// Returns the call id being answered.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the ordered output blocks.
    #[must_use]
    pub fn output(&self) -> &[BetaMultiAgentOutputText] {
        &self.output
    }

    /// Returns the runtime action that produced this output.
    #[must_use]
    pub const fn action(&self) -> &BetaMultiAgentAction {
        &self.action
    }

    /// Returns the owning agent when present and non-null.
    #[must_use]
    pub fn agent(&self) -> Option<&BetaAgent> {
        non_null(&self.agent)
    }
}

/// One stable input branch enriched with preview-only metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaStableInputItem {
    core: ResponseInputItem,
    metadata: BetaItemMetadata,
}

impl BetaStableInputItem {
    /// Wraps a stable input item without adding preview metadata.
    #[must_use]
    pub fn new(core: ResponseInputItem) -> Self {
        Self {
            core,
            metadata: BetaItemMetadata::default(),
        }
    }

    /// Attaches the owning agent.
    #[must_use]
    pub fn agent(mut self, agent: BetaAgent) -> Self {
        self.metadata = self.metadata.with_agent(agent);
        self
    }

    /// Attaches typed tool-caller metadata.
    #[must_use]
    pub fn caller(mut self, caller: BetaCaller) -> Self {
        self.metadata = self.metadata.with_caller(caller);
        self
    }

    /// Attaches the creating entity id.
    #[must_use]
    pub fn created_by(mut self, created_by: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_created_by(created_by);
        self
    }

    /// Attaches a message phase.
    #[must_use]
    pub fn phase(mut self, phase: BetaMessagePhase) -> Self {
        self.metadata = self.metadata.with_phase(phase);
        self
    }

    /// Borrows the stable item codec.
    #[must_use]
    pub const fn core(&self) -> &ResponseInputItem {
        &self.core
    }

    /// Borrows preview-only metadata.
    #[must_use]
    pub const fn metadata(&self) -> &BetaItemMetadata {
        &self.metadata
    }
}

impl Serialize for BetaStableInputItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        merge_serialized(&self.core, &self.metadata)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaStableInputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(Self {
            core: serde_json::from_value(value.clone()).map_err(D::Error::custom)?,
            metadata: serde_json::from_value(value).map_err(D::Error::custom)?,
        })
    }
}

/// One beta input item.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaResponseInputItem {
    /// A branch shared with stable Responses plus typed beta metadata.
    Stable(Box<BetaStableInputItem>),
    /// A shared message branch with typed prompt-cache boundaries.
    PromptCachedMessage(Box<BetaPromptCachedInputMessage>),
    /// A message routed between named agents.
    AgentMessage(BetaAgentMessage),
    /// A request to the server-hosted agent runtime.
    MultiAgentCall(BetaMultiAgentCall),
    /// The result of a server-hosted agent action.
    MultiAgentCallOutput(BetaMultiAgentCallOutput),
}

impl Serialize for BetaResponseInputItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Stable(value) => value.serialize(serializer),
            Self::PromptCachedMessage(value) => value.serialize(serializer),
            Self::AgentMessage(value) => value.serialize(serializer),
            Self::MultiAgentCall(value) => value.serialize(serializer),
            Self::MultiAgentCallOutput(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BetaResponseInputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match optional_discriminator(&value).map_err(D::Error::custom)? {
            Some("agent_message") => serde_json::from_value(value)
                .map(Self::AgentMessage)
                .map_err(D::Error::custom),
            Some("multi_agent_call") => serde_json::from_value(value)
                .map(Self::MultiAgentCall)
                .map_err(D::Error::custom),
            Some("multi_agent_call_output") => serde_json::from_value(value)
                .map(Self::MultiAgentCallOutput)
                .map_err(D::Error::custom),
            None | Some("message") if contains_prompt_cache_breakpoint(&value) => {
                serde_json::from_value(value)
                    .map(Box::new)
                    .map(Self::PromptCachedMessage)
                    .map_err(D::Error::custom)
            }
            _ => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Stable)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ResponseInputItem> for BetaResponseInputItem {
    fn from(value: ResponseInputItem) -> Self {
        Self::Stable(Box::new(BetaStableInputItem::new(value)))
    }
}

impl From<BetaPromptCachedInputMessage> for BetaResponseInputItem {
    fn from(value: BetaPromptCachedInputMessage) -> Self {
        Self::PromptCachedMessage(Box::new(value))
    }
}

impl From<BetaAgentMessage> for BetaResponseInputItem {
    fn from(value: BetaAgentMessage) -> Self {
        Self::AgentMessage(value)
    }
}

impl From<BetaMultiAgentCall> for BetaResponseInputItem {
    fn from(value: BetaMultiAgentCall) -> Self {
        Self::MultiAgentCall(value)
    }
}

impl From<BetaMultiAgentCallOutput> for BetaResponseInputItem {
    fn from(value: BetaMultiAgentCallOutput) -> Self {
        Self::MultiAgentCallOutput(value)
    }
}

/// One stable output branch enriched with preview-only metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaStableOutputItem {
    core: ResponseOutputItem,
    metadata: BetaItemMetadata,
}

impl BetaStableOutputItem {
    /// Wraps a stable output item without adding preview metadata.
    #[must_use]
    pub fn new(core: ResponseOutputItem) -> Self {
        Self {
            core,
            metadata: BetaItemMetadata::default(),
        }
    }

    /// Borrows the stable output codec.
    #[must_use]
    pub const fn core(&self) -> &ResponseOutputItem {
        &self.core
    }

    /// Borrows preview-only metadata.
    #[must_use]
    pub const fn metadata(&self) -> &BetaItemMetadata {
        &self.metadata
    }
}

impl Serialize for BetaStableOutputItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        merge_serialized(&self.core, &self.metadata)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaStableOutputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(Self {
            core: serde_json::from_value(value.clone()).map_err(D::Error::custom)?,
            metadata: serde_json::from_value(value).map_err(D::Error::custom)?,
        })
    }
}

/// One beta response output item.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaResponseOutputItem {
    Stable(Box<BetaStableOutputItem>),
    AgentMessage(BetaAgentMessage),
    MultiAgentCall(BetaMultiAgentCall),
    MultiAgentCallOutput(BetaMultiAgentCallOutput),
}

impl Serialize for BetaResponseOutputItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Stable(value) => value.serialize(serializer),
            Self::AgentMessage(value) => value.serialize(serializer),
            Self::MultiAgentCall(value) => value.serialize(serializer),
            Self::MultiAgentCallOutput(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BetaResponseOutputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match optional_discriminator(&value).map_err(D::Error::custom)? {
            Some("agent_message") => serde_json::from_value(value)
                .map(Self::AgentMessage)
                .map_err(D::Error::custom),
            Some("multi_agent_call") => serde_json::from_value(value)
                .map(Self::MultiAgentCall)
                .map_err(D::Error::custom),
            Some("multi_agent_call_output") => serde_json::from_value(value)
                .map(Self::MultiAgentCallOutput)
                .map_err(D::Error::custom),
            _ => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Stable)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ResponseOutputItem> for BetaResponseOutputItem {
    fn from(value: ResponseOutputItem) -> Self {
        Self::Stable(Box::new(BetaStableOutputItem::new(value)))
    }
}

impl From<BetaAgentMessage> for BetaResponseOutputItem {
    fn from(value: BetaAgentMessage) -> Self {
        Self::AgentMessage(value)
    }
}

impl From<BetaMultiAgentCall> for BetaResponseOutputItem {
    fn from(value: BetaMultiAgentCall) -> Self {
        Self::MultiAgentCall(value)
    }
}

impl From<BetaMultiAgentCallOutput> for BetaResponseOutputItem {
    fn from(value: BetaMultiAgentCallOutput) -> Self {
        Self::MultiAgentCallOutput(value)
    }
}

/// Text or an ordered list of beta input items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BetaResponseInput {
    Text(String),
    Items(Vec<BetaResponseInputItem>),
}

impl From<String> for BetaResponseInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for BetaResponseInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<BetaResponseInputItem>> for BetaResponseInput {
    fn from(value: Vec<BetaResponseInputItem>) -> Self {
        Self::Items(value)
    }
}

/// Beta reasoning configuration, including preview-only context and mode.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaReasoningConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    context: Omittable<Nullable<BetaReasoningContext>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    effort: Omittable<Nullable<BetaReasoningEffort>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    generate_summary: Omittable<Nullable<BetaReasoningSummary>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    mode: Omittable<BetaReasoningMode>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    summary: Omittable<Nullable<BetaReasoningSummary>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl BetaReasoningConfig {
    /// Creates an empty reasoning configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn context(mut self, context: BetaReasoningContext) -> Self {
        self.context = Omittable::Value(Nullable::Value(context));
        self
    }

    #[must_use]
    pub fn effort(mut self, effort: BetaReasoningEffort) -> Self {
        self.effort = Omittable::Value(Nullable::Value(effort));
        self
    }

    #[must_use]
    pub fn mode(mut self, mode: BetaReasoningMode) -> Self {
        self.mode = Omittable::Value(mode);
        self
    }

    #[must_use]
    pub fn summary(mut self, summary: BetaReasoningSummary) -> Self {
        self.summary = Omittable::Value(Nullable::Value(summary));
        self
    }

    /// Returns the non-null reasoning context.
    #[must_use]
    pub fn context_ref(&self) -> Option<&BetaReasoningContext> {
        non_null(&self.context)
    }

    /// Returns the non-null reasoning effort.
    #[must_use]
    pub fn effort_ref(&self) -> Option<&BetaReasoningEffort> {
        non_null(&self.effort)
    }

    /// Returns the selected reasoning mode.
    #[must_use]
    pub fn mode_ref(&self) -> Option<&BetaReasoningMode> {
        omitted_ref(&self.mode)
    }

    /// Returns the non-null reasoning summary style.
    #[must_use]
    pub fn summary_ref(&self) -> Option<&BetaReasoningSummary> {
        non_null(&self.summary)
    }
}

/// Shared with GA Responses.
pub use crate::responses::ContextManagement as BetaContextManagement;
/// Shared with GA Responses.
pub use crate::responses::ModerationConfig as BetaModerationConfig;
/// Shared with GA Responses.
pub use crate::responses::ModerationDirection as BetaModerationDirection;
/// Shared with GA Responses.
pub use crate::responses::ModerationMode as BetaModerationMode;
/// Shared with GA Responses.
pub use crate::responses::ModerationPolicy as BetaModerationPolicy;
/// Shared with GA Responses.
pub use crate::responses::PromptCacheMode as BetaPromptCacheMode;
/// Shared with GA Responses.
pub use crate::responses::PromptCacheTtl as BetaPromptCacheTtl;

/// Prompt-cache options for `gpt-5.6` and later beta models.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaPromptCacheOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    mode: Omittable<BetaPromptCacheMode>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ttl: Omittable<BetaPromptCacheTtl>,
}

impl BetaPromptCacheOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn mode(mut self, mode: BetaPromptCacheMode) -> Self {
        self.mode = Omittable::Value(mode);
        self
    }

    /// Selects the only TTL supported by the pinned schema.
    #[must_use]
    pub fn thirty_minutes(mut self) -> Self {
        self.ttl = Omittable::Value(BetaPromptCacheTtl::ThirtyMinutes);
        self
    }
}

/// Non-streaming `POST /responses?beta=true` body.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaCreateResponseRequest {
    base: crate::responses::CreateResponseRequest,
    input: Omittable<BetaResponseInput>,
    context_management: Omittable<Nullable<Vec<BetaContextManagement>>>,
    moderation: Omittable<Nullable<BetaModerationConfig>>,
    multi_agent: Omittable<Nullable<BetaMultiAgentConfig>>,
    prompt_cache_options: Omittable<BetaPromptCacheOptions>,
    reasoning: Omittable<Nullable<BetaReasoningConfig>>,
}

impl Default for BetaCreateResponseRequest {
    fn default() -> Self {
        Self::empty()
    }
}

impl BetaCreateResponseRequest {
    /// Creates an empty beta request.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            base: crate::responses::CreateResponseRequest::empty(),
            input: Omittable::Omitted,
            context_management: Omittable::Omitted,
            moderation: Omittable::Omitted,
            multi_agent: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            reasoning: Omittable::Omitted,
        }
    }

    /// Creates a request with model and typed beta input.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<BetaResponseInput>) -> Self {
        Self::empty().model(model).input(input)
    }

    /// Sets the model id.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.base = self.base.model(model);
        self
    }

    /// Sets typed beta input.
    #[must_use]
    pub fn input(mut self, input: impl Into<BetaResponseInput>) -> Self {
        self.input = Omittable::Value(input.into());
        self
    }

    /// Sets system or developer instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<ResponseInstructions>) -> Self {
        self.base = self.base.instructions(instructions);
        self
    }

    /// Enables or disables background execution.
    #[must_use]
    pub fn background(mut self, background: bool) -> Self {
        self.base = self.base.background(background);
        self
    }

    /// Adds one typed context-management rule.
    #[must_use]
    pub fn context_management(mut self, rule: BetaContextManagement) -> Self {
        let rules = match &mut self.context_management {
            Omittable::Value(Nullable::Value(rules)) => rules,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => {
                self.context_management = Omittable::Value(Nullable::Value(Vec::new()));
                match &mut self.context_management {
                    Omittable::Value(Nullable::Value(rules)) => rules,
                    _ => unreachable!(),
                }
            }
        };
        rules.push(rule);
        self
    }

    /// Requests one optional expanded response field.
    #[must_use]
    pub fn include(mut self, include: BetaResponseIncludable) -> Self {
        self.base = self.base.include(include.as_str());
        self
    }

    /// Caps generated tokens.
    #[must_use]
    pub fn max_output_tokens(mut self, maximum: u32) -> Self {
        self.base = self.base.max_output_tokens(maximum);
        self
    }

    /// Caps total built-in tool calls.
    #[must_use]
    pub fn max_tool_calls(mut self, maximum: u32) -> Self {
        self.base = self.base.max_tool_calls(maximum);
        self
    }

    /// Inserts one metadata pair.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.base = self.base.metadata(key, value);
        self
    }

    /// Configures moderated completion handling.
    #[must_use]
    pub fn moderation(mut self, moderation: BetaModerationConfig) -> Self {
        self.moderation = Omittable::Value(Nullable::Value(moderation));
        self
    }

    /// Configures server-hosted multi-agent execution.
    #[must_use]
    pub fn multi_agent(mut self, multi_agent: BetaMultiAgentConfig) -> Self {
        self.multi_agent = Omittable::Value(Nullable::Value(multi_agent));
        self
    }

    /// Controls parallel tool calls.
    #[must_use]
    pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.base = self.base.parallel_tool_calls(enabled);
        self
    }

    /// Continues from a prior response.
    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.base = self.base.previous_response_id(id);
        self
    }

    /// Sets a prompt-cache key.
    #[must_use]
    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.base = self.base.prompt_cache_key(key);
        self
    }

    /// Sets typed prompt-cache options.
    #[must_use]
    pub fn prompt_cache_options(mut self, options: BetaPromptCacheOptions) -> Self {
        self.prompt_cache_options = Omittable::Value(options);
        self
    }

    /// Sets preview reasoning configuration.
    #[must_use]
    pub fn reasoning(mut self, reasoning: BetaReasoningConfig) -> Self {
        self.reasoning = Omittable::Value(Nullable::Value(reasoning));
        self
    }

    /// Controls response storage.
    #[must_use]
    pub fn store(mut self, store: bool) -> Self {
        self.base = self.base.store(store);
        self
    }

    /// Adds one stable or beta-compatible tool definition.
    #[must_use]
    pub fn tool(mut self, tool: impl Into<ResponseTool>) -> Self {
        self.base = self.base.tool(tool);
        self
    }

    /// Selects the tool-choice policy.
    #[must_use]
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.base = self.base.tool_choice(tool_choice);
        self
    }

    /// Sets typed text-output configuration.
    #[must_use]
    pub fn text(mut self, text: ResponseTextConfig) -> Self {
        self.base = self.base.text(text);
        self
    }

    /// Sets sampling temperature.
    #[must_use]
    pub fn temperature(mut self, temperature: f64) -> Self {
        self.base = self.base.temperature(temperature);
        self
    }

    /// Sets nucleus sampling probability.
    #[must_use]
    pub fn top_p(mut self, top_p: f64) -> Self {
        self.base = self.base.top_p(top_p);
        self
    }

    /// Sets the truncation policy.
    #[must_use]
    pub fn truncation(mut self, truncation: TruncationStrategy) -> Self {
        self.base = self.base.truncation(truncation);
        self
    }

    /// Converts the body to the streaming typestate.
    #[must_use]
    pub fn into_streaming(self) -> BetaCreateStreamingResponseRequest {
        BetaCreateStreamingResponseRequest {
            request: self,
            stream_options: Omittable::Omitted,
        }
    }
}

impl Serialize for BetaCreateResponseRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut object = serialized_object(&self.base).map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "input", &self.input).map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "context_management", &self.context_management)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "moderation", &self.moderation)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "multi_agent", &self.multi_agent)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(
            &mut object,
            "prompt_cache_options",
            &self.prompt_cache_options,
        )
        .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "reasoning", &self.reasoning)
            .map_err(serde::ser::Error::custom)?;
        Value::Object(object).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaCreateResponseRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = deserialize_object(deserializer)?;
        let input = take_omittable(&mut object, "input").map_err(D::Error::custom)?;
        let context_management =
            take_omittable(&mut object, "context_management").map_err(D::Error::custom)?;
        let moderation = take_omittable(&mut object, "moderation").map_err(D::Error::custom)?;
        let multi_agent = take_omittable(&mut object, "multi_agent").map_err(D::Error::custom)?;
        let prompt_cache_options =
            take_omittable(&mut object, "prompt_cache_options").map_err(D::Error::custom)?;
        let reasoning = take_omittable(&mut object, "reasoning").map_err(D::Error::custom)?;
        let base = serde_json::from_value(Value::Object(object)).map_err(D::Error::custom)?;
        Ok(Self {
            base,
            input,
            context_management,
            moderation,
            multi_agent,
            prompt_cache_options,
            reasoning,
        })
    }
}

/// Streaming `POST /responses?beta=true` body.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaCreateStreamingResponseRequest {
    request: BetaCreateResponseRequest,
    stream_options: Omittable<ResponseStreamOptions>,
}

impl BetaCreateStreamingResponseRequest {
    /// Creates a streaming request with model and input.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<BetaResponseInput>) -> Self {
        BetaCreateResponseRequest::new(model, input).into_streaming()
    }

    /// Sets SSE payload options.
    #[must_use]
    pub fn stream_options(mut self, options: ResponseStreamOptions) -> Self {
        self.stream_options = Omittable::Value(options);
        self
    }

    /// Converts back to the non-streaming typestate.
    #[must_use]
    pub fn into_non_streaming(self) -> BetaCreateResponseRequest {
        self.request
    }
}

impl Serialize for BetaCreateStreamingResponseRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut object = serialized_object(&self.request).map_err(serde::ser::Error::custom)?;
        object.insert("stream".to_owned(), Value::Bool(true));
        insert_omittable(&mut object, "stream_options", &self.stream_options)
            .map_err(serde::ser::Error::custom)?;
        Value::Object(object).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaCreateStreamingResponseRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = deserialize_object(deserializer)?;
        match object.remove("stream") {
            Some(Value::Bool(true)) => {}
            _ => {
                return Err(D::Error::custom(
                    "beta streaming request requires `stream: true`",
                ));
            }
        }
        let stream_options =
            take_omittable(&mut object, "stream_options").map_err(D::Error::custom)?;
        let request = serde_json::from_value(Value::Object(object)).map_err(D::Error::custom)?;
        Ok(Self {
            request,
            stream_options,
        })
    }
}

/// Moderation result returned by the beta response resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum BetaModerationOutcome {
    #[serde(rename = "moderation_result")]
    Result {
        categories: BTreeMap<String, bool>,
        category_applied_input_types: BTreeMap<String, Vec<BetaModerationInputType>>,
        category_scores: BTreeMap<String, f64>,
        flagged: bool,
        model: String,
    },
    #[serde(rename = "error")]
    Error { code: String, message: String },
}

crate::open_string_enum! {
    /// Modality reflected in one moderation category score.
    pub enum BetaModerationInputType {
        Text = "text",
        Image = "image",
    }
}

/// Moderation outcomes for response input and output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaModerationResult {
    input: BetaModerationOutcome,
    output: BetaModerationOutcome,
}

impl BetaModerationResult {
    /// Returns the input moderation outcome.
    #[must_use]
    pub const fn input(&self) -> &BetaModerationOutcome {
        &self.input
    }

    /// Returns the output moderation outcome.
    #[must_use]
    pub const fn output(&self) -> &BetaModerationOutcome {
        &self.output
    }
}

literal_tag!(BetaResponseObjectTag, Response, "response");

/// Complete beta Responses resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponse {
    id: String,
    created_at: i64,
    error: Nullable<ResponseError>,
    incomplete_details: Nullable<IncompleteDetails>,
    instructions: Nullable<ResponseInstructions>,
    metadata: Nullable<BTreeMap<String, String>>,
    model: String,
    #[serde(rename = "object")]
    object: BetaResponseObjectTag,
    output: Vec<BetaResponseOutputItem>,
    parallel_tool_calls: bool,
    temperature: Nullable<f64>,
    tool_choice: ToolChoice,
    tools: Vec<ResponseTool>,
    top_p: Nullable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    background: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    completed_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    conversation: Omittable<Nullable<ConversationObjectReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_output_tokens: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_tool_calls: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    moderation: Omittable<Nullable<BetaModerationResult>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<PromptReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<BetaPromptCacheOptions>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<BetaPromptCacheRetention>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<BetaReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    safety_identifier: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<Nullable<BetaServiceTier>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<ResponseTextConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_logprobs: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<Nullable<TruncationStrategy>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<ResponseUsage>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl BetaResponse {
    /// Returns the opaque response id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the Unix creation timestamp in seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns the model selected by the service.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns output items in wire order.
    #[must_use]
    pub fn output(&self) -> &[BetaResponseOutputItem] {
        &self.output
    }

    /// Returns the lifecycle status when present.
    #[must_use]
    pub fn status(&self) -> Option<&ResponseStatus> {
        omitted_ref(&self.status)
    }

    /// Returns usage when present.
    #[must_use]
    pub fn usage(&self) -> Option<&ResponseUsage> {
        omitted_ref(&self.usage)
    }

    /// Returns preview reasoning configuration when present and non-null.
    #[must_use]
    pub fn reasoning(&self) -> Option<&BetaReasoningConfig> {
        non_null(&self.reasoning)
    }

    /// Returns the maximum tool-call budget when present and non-null.
    #[must_use]
    pub fn max_tool_calls(&self) -> Option<u32> {
        non_null(&self.max_tool_calls).copied()
    }

    /// Returns typed moderation outcomes when present and non-null.
    #[must_use]
    pub fn moderation(&self) -> Option<&BetaModerationResult> {
        non_null(&self.moderation)
    }

    /// Returns future fields retained during decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request body for `POST /responses/compact?beta=true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaCompactResponseRequest {
    model: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<BetaResponseInput>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<Nullable<BetaPromptCacheOptions>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<BetaPromptCacheRetention>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<Nullable<BetaServiceTier>>,
}

impl BetaCompactResponseRequest {
    /// Creates the required model-only compact request.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: Omittable::Omitted,
            instructions: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            service_tier: Omittable::Omitted,
        }
    }

    /// Sets input to compact.
    #[must_use]
    pub fn input(mut self, input: impl Into<BetaResponseInput>) -> Self {
        self.input = Omittable::Value(Nullable::Value(input.into()));
        self
    }

    /// Sets compacting instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(Nullable::Value(instructions.into()));
        self
    }

    /// Continues from one stored response.
    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sets a prompt-cache key.
    #[must_use]
    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.prompt_cache_key = Omittable::Value(Nullable::Value(key.into()));
        self
    }

    /// Sets prompt-cache options.
    #[must_use]
    pub fn prompt_cache_options(mut self, options: BetaPromptCacheOptions) -> Self {
        self.prompt_cache_options = Omittable::Value(Nullable::Value(options));
        self
    }

    /// Sets the deprecated prompt-cache retention policy.
    #[must_use]
    pub fn prompt_cache_retention(mut self, retention: BetaPromptCacheRetention) -> Self {
        self.prompt_cache_retention = Omittable::Value(Nullable::Value(retention));
        self
    }

    /// Sets the requested service tier.
    #[must_use]
    pub fn service_tier(mut self, tier: BetaServiceTier) -> Self {
        self.service_tier = Omittable::Value(Nullable::Value(tier));
        self
    }
}

literal_tag!(BetaCompactedResponseTag, Compaction, "response.compaction");

/// Compacted beta response resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaCompactedResponse {
    id: String,
    created_at: i64,
    #[serde(rename = "object")]
    object: BetaCompactedResponseTag,
    output: Vec<BetaResponseOutputItem>,
    usage: ResponseUsage,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl BetaCompactedResponse {
    /// Returns the compacted response id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns compacted items in wire order.
    #[must_use]
    pub fn output(&self) -> &[BetaResponseOutputItem] {
        &self.output
    }

    /// Returns token usage for the compaction pass.
    #[must_use]
    pub const fn usage(&self) -> &ResponseUsage {
        &self.usage
    }
}

/// Query for `GET /responses/{id}?beta=true`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaRetrieveResponseParams {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<BetaResponseIncludable>,
}

impl BetaRetrieveResponseParams {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests one optional expanded response field.
    #[must_use]
    pub fn include(mut self, include: BetaResponseIncludable) -> Self {
        self.include.push(include);
        self
    }
}

/// Query for resuming a beta response SSE stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaRetrieveResponseStreamParams {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<BetaResponseIncludable>,
    stream: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include_obfuscation: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    starting_after: Omittable<u64>,
}

impl BetaRetrieveResponseStreamParams {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            include: Vec::new(),
            stream: true,
            include_obfuscation: Omittable::Omitted,
            starting_after: Omittable::Omitted,
        }
    }

    /// Requests one optional expanded response field.
    #[must_use]
    pub fn include(mut self, include: BetaResponseIncludable) -> Self {
        self.include.push(include);
        self
    }

    /// Controls streaming obfuscation fields.
    #[must_use]
    pub const fn include_obfuscation(mut self, include: bool) -> Self {
        self.include_obfuscation = Omittable::Value(include);
        self
    }

    /// Starts after a previously observed sequence number.
    #[must_use]
    pub const fn starting_after(mut self, sequence_number: u64) -> Self {
        self.starting_after = Omittable::Value(sequence_number);
        self
    }
}

impl Default for BetaRetrieveResponseStreamParams {
    fn default() -> Self {
        Self::new()
    }
}

/// Query for one beta input-item page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaListInputItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<BetaResponseIncludable>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<BetaResponseItemOrder>,
}

impl BetaListInputItemsParams {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn after(mut self, after: impl Into<String>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    #[must_use]
    pub fn include(mut self, include: BetaResponseIncludable) -> Self {
        self.include.push(include);
        self
    }

    #[must_use]
    pub const fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    #[must_use]
    pub fn order(mut self, order: BetaResponseItemOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Returns the opaque cursor when present.
    #[must_use]
    pub fn after_ref(&self) -> Option<&str> {
        match &self.after {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }
}

literal_tag!(ListObjectTag, List, "list");

/// One page of beta response input items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponseItemList {
    data: Vec<BetaResponseInputItem>,
    first_id: String,
    has_more: bool,
    last_id: String,
    #[serde(rename = "object")]
    object: ListObjectTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl BetaResponseItemList {
    #[must_use]
    pub fn data(&self) -> &[BetaResponseInputItem] {
        &self.data
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub fn first_id(&self) -> &str {
        &self.first_id
    }

    #[must_use]
    pub fn last_id(&self) -> &str {
        &self.last_id
    }
}

/// Body for `POST /responses/input_tokens?beta=true`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaCountInputTokensRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    conversation: Omittable<Nullable<ConversationReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<BetaResponseInput>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parallel_tool_calls: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    personality: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<BetaReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<Nullable<ResponseTextConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<Nullable<ToolChoice>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Nullable<Vec<ResponseTool>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<TruncationStrategy>,
}

impl BetaCountInputTokensRequest {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<BetaResponseInput>) -> Self {
        Self::empty().model(model).input(input)
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }

    #[must_use]
    pub fn input(mut self, input: impl Into<BetaResponseInput>) -> Self {
        self.input = Omittable::Value(Nullable::Value(input.into()));
        self
    }

    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(Nullable::Value(instructions.into()));
        self
    }

    #[must_use]
    pub fn conversation(mut self, conversation: impl Into<ConversationReference>) -> Self {
        self.conversation = Omittable::Value(Nullable::Value(conversation.into()));
        self
    }

    #[must_use]
    pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.parallel_tool_calls = Omittable::Value(Nullable::Value(enabled));
        self
    }

    #[must_use]
    pub fn personality(mut self, personality: impl Into<String>) -> Self {
        self.personality = Omittable::Value(personality.into());
        self
    }

    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    #[must_use]
    pub fn reasoning(mut self, reasoning: BetaReasoningConfig) -> Self {
        self.reasoning = Omittable::Value(Nullable::Value(reasoning));
        self
    }

    #[must_use]
    pub fn text(mut self, text: ResponseTextConfig) -> Self {
        self.text = Omittable::Value(Nullable::Value(text));
        self
    }

    #[must_use]
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Omittable::Value(Nullable::Value(choice));
        self
    }

    #[must_use]
    pub fn tool(mut self, tool: impl Into<ResponseTool>) -> Self {
        let tools = match &mut self.tools {
            Omittable::Value(Nullable::Value(tools)) => tools,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => {
                self.tools = Omittable::Value(Nullable::Value(Vec::new()));
                match &mut self.tools {
                    Omittable::Value(Nullable::Value(tools)) => tools,
                    _ => unreachable!(),
                }
            }
        };
        tools.push(tool.into());
        self
    }

    #[must_use]
    pub fn truncation(mut self, truncation: TruncationStrategy) -> Self {
        self.truncation = Omittable::Value(truncation);
        self
    }
}

/// The token-count response is identical to the stable Responses codec.
pub type BetaInputTokenCountResponse = crate::responses::InputTokenCountResponse;

/// One beta SSE event. The stable discriminator codec remains directly
/// accessible while beta-only ownership and nested item/resource views are
/// decoded explicitly.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaResponseStreamEvent {
    core: ResponseStreamEvent,
    agent: Omittable<Nullable<BetaAgent>>,
    stream_id: Omittable<String>,
    response: Option<BetaResponse>,
    item: Option<BetaResponseOutputItem>,
}

impl BetaResponseStreamEvent {
    /// Borrows the stable event discriminator and payload codec.
    #[must_use]
    pub const fn core(&self) -> &ResponseStreamEvent {
        &self.core
    }

    /// Returns the agent that owns this event.
    #[must_use]
    pub fn agent(&self) -> Option<&BetaAgent> {
        non_null(&self.agent)
    }

    /// Returns the WebSocket lane when the event came from a multiplexed socket.
    #[must_use]
    pub fn stream_id(&self) -> Option<&str> {
        omitted_ref(&self.stream_id).map(String::as_str)
    }

    /// Returns a typed beta response snapshot for lifecycle events.
    #[must_use]
    pub const fn response(&self) -> Option<&BetaResponse> {
        self.response.as_ref()
    }

    /// Returns a typed beta item for output-item events.
    #[must_use]
    pub const fn item(&self) -> Option<&BetaResponseOutputItem> {
        self.item.as_ref()
    }

    /// Returns the event sequence number.
    #[must_use]
    pub fn sequence_number(&self) -> Option<u64> {
        self.core.sequence_number()
    }

    /// Returns whether this event ends one response lifecycle.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.core.is_terminal()
    }
}

impl Serialize for BetaResponseStreamEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut object = serialized_object(&self.core).map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "agent", &self.agent).map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "stream_id", &self.stream_id)
            .map_err(serde::ser::Error::custom)?;
        if let Some(response) = &self.response {
            object.insert(
                "response".to_owned(),
                serde_json::to_value(response).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(item) = &self.item {
            object.insert(
                "item".to_owned(),
                serde_json::to_value(item).map_err(serde::ser::Error::custom)?,
            );
        }
        Value::Object(object).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaResponseStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("beta response event must be an object"))?;
        let agent = decode_omittable(object.get("agent")).map_err(D::Error::custom)?;
        let stream_id = decode_omittable(object.get("stream_id")).map_err(D::Error::custom)?;
        let response = object
            .get("response")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(D::Error::custom)?;
        let item = object
            .get("item")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(D::Error::custom)?;

        let mut stable_value = value;
        if let Some(response) = &response {
            let mut stable_response = serialized_object(response).map_err(D::Error::custom)?;
            for nullable_in_beta_only in ["background", "service_tier"] {
                if stable_response.get(nullable_in_beta_only) == Some(&Value::Null) {
                    stable_response.remove(nullable_in_beta_only);
                }
            }
            stable_value
                .as_object_mut()
                .ok_or_else(|| D::Error::custom("beta response event must be an object"))?
                .insert("response".to_owned(), Value::Object(stable_response));
        }
        let core = serde_json::from_value(stable_value).map_err(D::Error::custom)?;
        Ok(Self {
            core,
            agent,
            stream_id,
            response,
            item,
        })
    }
}

literal_tag!(ResponsesCreateEventTag, ResponseCreate, "response.create");

/// Client event that starts one beta response over a WebSocket.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaResponsesCreateEvent {
    request: BetaCreateResponseRequest,
    stream_id: Omittable<String>,
}

impl BetaResponsesCreateEvent {
    /// Converts a beta HTTP create body to a WebSocket create event.
    #[must_use]
    pub fn from_request(request: BetaCreateResponseRequest) -> Self {
        Self {
            request,
            stream_id: Omittable::Omitted,
        }
    }

    /// Routes the response through one FIFO WebSocket lane.
    #[must_use]
    pub fn stream_id(mut self, stream_id: impl Into<String>) -> Self {
        self.stream_id = Omittable::Value(stream_id.into());
        self
    }

    /// Returns the lane id when configured.
    #[must_use]
    pub fn stream_id_ref(&self) -> Option<&str> {
        omitted_ref(&self.stream_id).map(String::as_str)
    }
}

impl Serialize for BetaResponsesCreateEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut object = serialized_object(&self.request).map_err(serde::ser::Error::custom)?;
        object.remove("stream");
        object.remove("stream_options");
        object.insert(
            "type".to_owned(),
            serde_json::to_value(ResponsesCreateEventTag::ResponseCreate)
                .map_err(serde::ser::Error::custom)?,
        );
        insert_omittable(&mut object, "stream_id", &self.stream_id)
            .map_err(serde::ser::Error::custom)?;
        Value::Object(object).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BetaResponsesCreateEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = deserialize_object(deserializer)?;
        match object.remove("type") {
            Some(Value::String(value)) if value == "response.create" => {}
            _ => return Err(D::Error::custom("expected `response.create` client event")),
        }
        if object.contains_key("stream") {
            return Err(D::Error::custom(
                "WebSocket response.create must not include HTTP `stream`",
            ));
        }
        let stream_id = take_omittable(&mut object, "stream_id").map_err(D::Error::custom)?;
        let request = serde_json::from_value(Value::Object(object)).map_err(D::Error::custom)?;
        Ok(Self { request, stream_id })
    }
}

literal_tag!(ResponseInjectTag, ResponseInject, "response.inject");

/// Atomically injects client-owned tool outputs into an active response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponseInjectEvent {
    input: Vec<BetaResponseInputItem>,
    response_id: String,
    #[serde(rename = "type")]
    kind: ResponseInjectTag,
}

impl BetaResponseInjectEvent {
    /// Creates an injection event.
    #[must_use]
    pub fn new(
        response_id: impl Into<String>,
        input: impl IntoIterator<Item = impl Into<BetaResponseInputItem>>,
    ) -> Self {
        Self {
            input: input.into_iter().map(Into::into).collect(),
            response_id: response_id.into(),
            kind: ResponseInjectTag::ResponseInject,
        }
    }

    /// Returns the target active response id.
    #[must_use]
    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    /// Returns the atomically committed item set.
    #[must_use]
    pub fn input(&self) -> &[BetaResponseInputItem] {
        &self.input
    }
}

/// Client events accepted by the beta Responses WebSocket.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaResponsesClientEvent {
    Create(Box<BetaResponsesCreateEvent>),
    Inject(BetaResponseInjectEvent),
    Unknown(UnknownTaggedObject),
}

impl BetaResponsesClientEvent {
    /// Creates a `response.create` event.
    #[must_use]
    pub fn create(request: BetaCreateResponseRequest) -> Self {
        Self::Create(Box::new(BetaResponsesCreateEvent::from_request(request)))
    }

    /// Creates a lane-routed `response.create` event.
    #[must_use]
    pub fn create_on_stream(
        stream_id: impl Into<String>,
        request: BetaCreateResponseRequest,
    ) -> Self {
        Self::Create(Box::new(
            BetaResponsesCreateEvent::from_request(request).stream_id(stream_id),
        ))
    }

    /// Creates an atomic `response.inject` event.
    #[must_use]
    pub fn inject(event: BetaResponseInjectEvent) -> Self {
        Self::Inject(event)
    }
}

impl Serialize for BetaResponsesClientEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Create(value) => value.serialize(serializer),
            Self::Inject(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BetaResponsesClientEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match discriminator(&value).map_err(D::Error::custom)? {
            "response.create" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Create)
                .map_err(D::Error::custom),
            "response.inject" => serde_json::from_value(value)
                .map(Self::Inject)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

literal_tag!(
    ResponseInjectCreatedTag,
    ResponseInjectCreated,
    "response.inject.created"
);

/// Confirmation that an injected item set was committed atomically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponseInjectCreatedEvent {
    response_id: String,
    sequence_number: u64,
    #[serde(rename = "type")]
    kind: ResponseInjectCreatedTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_id: Omittable<String>,
}

impl BetaResponseInjectCreatedEvent {
    #[must_use]
    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    #[must_use]
    pub fn stream_id(&self) -> Option<&str> {
        omitted_ref(&self.stream_id).map(String::as_str)
    }
}

crate::open_string_enum! {
    /// Machine-readable rejection reason for `response.inject`.
    pub enum BetaResponseInjectErrorCode {
        ResponseAlreadyCompleted = "response_already_completed",
        ResponseNotFound = "response_not_found",
    }
}

/// Error returned when an injection could not be committed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponseInjectError {
    code: BetaResponseInjectErrorCode,
    message: String,
}

impl BetaResponseInjectError {
    #[must_use]
    pub const fn code(&self) -> &BetaResponseInjectErrorCode {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

literal_tag!(
    ResponseInjectFailedTag,
    ResponseInjectFailed,
    "response.inject.failed"
);

/// Event returning the uncommitted item set after injection failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaResponseInjectFailedEvent {
    error: BetaResponseInjectError,
    input: Vec<BetaResponseInputItem>,
    response_id: String,
    sequence_number: u64,
    #[serde(rename = "type")]
    kind: ResponseInjectFailedTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_id: Omittable<String>,
}

impl BetaResponseInjectFailedEvent {
    #[must_use]
    pub const fn error(&self) -> &BetaResponseInjectError {
        &self.error
    }

    #[must_use]
    pub fn input(&self) -> &[BetaResponseInputItem] {
        &self.input
    }

    #[must_use]
    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    #[must_use]
    pub fn stream_id(&self) -> Option<&str> {
        omitted_ref(&self.stream_id).map(String::as_str)
    }
}

/// Nested protocol error details used only by the beta WebSocket envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaWebSocketErrorDetails {
    code: Nullable<String>,
    message: String,
    param: Nullable<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    headers: Omittable<BTreeMap<String, String>>,
}

impl BetaWebSocketErrorDetails {
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn error_type(&self) -> &str {
        &self.kind
    }
}

literal_tag!(WebSocketErrorTag, Error, "error");

/// WebSocket-level error shape whose `error` discriminator collides with the
/// SSE streaming error and therefore requires structural routing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaWebSocketErrorEvent {
    error: BetaWebSocketErrorDetails,
    #[serde(rename = "type")]
    kind: WebSocketErrorTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    agent: Omittable<Nullable<BetaAgent>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sequence_number: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<u16>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_id: Omittable<String>,
}

impl BetaWebSocketErrorEvent {
    #[must_use]
    pub const fn error(&self) -> &BetaWebSocketErrorDetails {
        &self.error
    }
}

/// Server events emitted by the beta Responses WebSocket.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BetaResponsesServerEvent {
    Response(Box<BetaResponseStreamEvent>),
    InjectCreated(BetaResponseInjectCreatedEvent),
    InjectFailed(BetaResponseInjectFailedEvent),
    WebSocketError(BetaWebSocketErrorEvent),
}

impl BetaResponsesServerEvent {
    /// Returns the multiplexed lane id when present.
    #[must_use]
    pub fn stream_id(&self) -> Option<&str> {
        match self {
            Self::Response(event) => event.stream_id(),
            Self::InjectCreated(event) => event.stream_id(),
            Self::InjectFailed(event) => event.stream_id(),
            Self::WebSocketError(event) => omitted_ref(&event.stream_id).map(String::as_str),
        }
    }

    /// Returns a sequence number when this event carries one.
    #[must_use]
    pub fn sequence_number(&self) -> Option<u64> {
        match self {
            Self::Response(event) => event.sequence_number(),
            Self::InjectCreated(event) => Some(event.sequence_number()),
            Self::InjectFailed(event) => Some(event.sequence_number()),
            Self::WebSocketError(event) => omitted_ref(&event.sequence_number).copied(),
        }
    }

    /// Returns whether this event ends a model response lifecycle.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Response(event) if event.is_terminal())
    }
}

impl Serialize for BetaResponsesServerEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Response(value) => value.serialize(serializer),
            Self::InjectCreated(value) => value.serialize(serializer),
            Self::InjectFailed(value) => value.serialize(serializer),
            Self::WebSocketError(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BetaResponsesServerEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match discriminator(&value).map_err(D::Error::custom)? {
            "response.inject.created" => serde_json::from_value(value)
                .map(Self::InjectCreated)
                .map_err(D::Error::custom),
            "response.inject.failed" => serde_json::from_value(value)
                .map(Self::InjectFailed)
                .map_err(D::Error::custom),
            "error" if value.get("error").is_some() => serde_json::from_value(value)
                .map(Self::WebSocketError)
                .map_err(D::Error::custom),
            _ => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Response)
                .map_err(D::Error::custom),
        }
    }
}

/// Frozen schema manifest showing stable reuse and beta-only additions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BetaSchemaManifest {
    /// Stable schema names reused without weakening their validation.
    pub stable: &'static [&'static str],
    /// Preview-only schema names implemented in this module.
    pub beta_only: &'static [&'static str],
}

impl BetaSchemaManifest {
    /// Returns the total frozen branch count.
    #[must_use]
    pub const fn len(self) -> usize {
        self.stable.len() + self.beta_only.len()
    }

    /// Returns whether the manifest contains no schemas.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// Frozen 35-branch beta input union.
pub const BETA_RESPONSE_INPUT_MANIFEST: BetaSchemaManifest = BetaSchemaManifest {
    stable: &crate::responses::STABLE_RESPONSE_INPUT_SCHEMAS,
    beta_only: &["AgentMessage", "MultiAgentCall", "MultiAgentCallOutput"],
};

/// Frozen 31-branch beta output union.
pub const BETA_RESPONSE_OUTPUT_MANIFEST: BetaSchemaManifest = BetaSchemaManifest {
    stable: &crate::responses::STABLE_RESPONSE_OUTPUT_SCHEMAS,
    beta_only: &["AgentMessage", "MultiAgentCall", "MultiAgentCallOutput"],
};

/// Frozen 58-branch beta SSE union. Every discriminator is shared with the
/// stable stream, while the payload wrapper adds typed agent metadata.
pub const BETA_RESPONSE_STREAM_EVENT_MANIFEST: BetaSchemaManifest = BetaSchemaManifest {
    stable: &crate::responses::STABLE_RESPONSE_STREAM_EVENT_SCHEMAS,
    beta_only: &[],
};

/// Preview-only WebSocket server envelopes layered on the 58 SSE branches.
pub const BETA_RESPONSES_WEBSOCKET_ADDITIONAL_SCHEMAS: [&str; 3] = [
    "BetaResponseInjectCreatedEvent",
    "BetaResponseInjectFailedEvent",
    "BetaResponseWsError",
];

fn discriminator(value: &Value) -> Result<&str, &'static str> {
    optional_discriminator(value)?.ok_or("tagged object is missing string field `type`")
}

fn optional_discriminator(value: &Value) -> Result<Option<&str>, &'static str> {
    let object = value
        .as_object()
        .ok_or("tagged value must be a JSON object")?;
    match object.get("type") {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err("tagged object field `type` must be a string"),
        None => Ok(None),
    }
}

fn contains_prompt_cache_breakpoint(value: &Value) -> bool {
    value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|part| {
                part.as_object()
                    .is_some_and(|part| part.contains_key("prompt_cache_breakpoint"))
            })
        })
}

fn serialized_object<T: Serialize>(value: &T) -> serde_json::Result<Map<String, Value>> {
    match serde_json::to_value(value)? {
        Value::Object(object) => Ok(object),
        _ => Err(<serde_json::Error as serde::ser::Error>::custom(
            "wire value must serialize as an object",
        )),
    }
}

fn merge_serialized<A: Serialize, B: Serialize>(
    base: &A,
    overlay: &B,
) -> serde_json::Result<Value> {
    let mut object = serialized_object(base)?;
    object.extend(serialized_object(overlay)?);
    Ok(Value::Object(object))
}

fn insert_omittable<T: Serialize>(
    object: &mut Map<String, Value>,
    key: &str,
    value: &Omittable<T>,
) -> serde_json::Result<()> {
    if let Omittable::Value(value) = value {
        object.insert(key.to_owned(), serde_json::to_value(value)?);
    }
    Ok(())
}

fn take_omittable<T: for<'de> Deserialize<'de>>(
    object: &mut Map<String, Value>,
    key: &str,
) -> serde_json::Result<Omittable<T>> {
    object
        .remove(key)
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.map_or(Omittable::Omitted, Omittable::Value))
}

fn decode_omittable<T: for<'de> Deserialize<'de>>(
    value: Option<&Value>,
) -> serde_json::Result<Omittable<T>> {
    value
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.map_or(Omittable::Omitted, Omittable::Value))
}

fn deserialize_object<'de, D>(deserializer: D) -> Result<Map<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Object(object) => Ok(object),
        _ => Err(D::Error::custom("wire value must be a JSON object")),
    }
}

fn omitted_ref<T>(value: &Omittable<T>) -> Option<&T> {
    match value {
        Omittable::Value(value) => Some(value),
        Omittable::Omitted => None,
    }
}

fn non_null<T>(value: &Omittable<Nullable<T>>) -> Option<&T> {
    match value {
        Omittable::Value(Nullable::Value(value)) => Some(value),
        Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    fn response_fixture(output: Value) -> Value {
        json!({
            "id": "resp_beta_1",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-test",
            "object": "response",
            "output": output,
            "parallel_tool_calls": true,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null,
            "background": null,
            "max_tool_calls": 7,
            "reasoning": {
                "context": "all_turns",
                "effort": "max",
                "mode": "pro",
                "summary": "concise"
            },
            "service_tier": null,
            "status": "in_progress"
        })
    }

    fn assert_json_dto<T>()
    where
        T: Serialize + DeserializeOwned + Send + Sync,
    {
    }

    #[test]
    fn beta_manifests_pin_union_sizes_and_reuse() {
        assert_eq!(BETA_RESPONSE_INPUT_MANIFEST.len(), 35);
        assert_eq!(BETA_RESPONSE_OUTPUT_MANIFEST.len(), 31);
        assert_eq!(BETA_RESPONSE_STREAM_EVENT_MANIFEST.len(), 58);
        assert_eq!(BETA_RESPONSES_WEBSOCKET_ADDITIONAL_SCHEMAS.len(), 3);

        assert_json_dto::<BetaResponseInputItem>();
        assert_json_dto::<BetaResponseOutputItem>();
        assert_json_dto::<BetaResponseStreamEvent>();
        assert_json_dto::<BetaResponsesClientEvent>();
        assert_json_dto::<BetaResponsesServerEvent>();
        assert_impl_all!(BetaResponsesClientEvent: Send, Sync);
        assert_impl_all!(BetaResponsesServerEvent: Send, Sync);
    }

    #[test]
    fn preview_metadata_is_typed_on_stable_item_branches() {
        let fixture = json!({
            "type": "message",
            "role": "user",
            "content": "hello",
            "agent": {"agent_name": "root/research"},
            "phase": "commentary"
        });
        let item: BetaResponseInputItem =
            serde_json::from_value(fixture.clone()).expect("decode beta message");
        let BetaResponseInputItem::Stable(item) = &item else {
            panic!("message must reuse stable input codec");
        };
        assert_eq!(
            item.metadata().agent().map(BetaAgent::agent_name),
            Some("root/research")
        );
        assert_eq!(
            item.metadata().phase().map(BetaMessagePhase::as_str),
            Some("commentary")
        );
        assert_eq!(serde_json::to_value(item).expect("round trip"), fixture);
    }

    #[test]
    fn multi_agent_items_encode_arguments_without_manual_json_formatting() {
        #[derive(Serialize)]
        struct Spawn<'a> {
            task_name: &'a str,
            message: &'a str,
        }

        let call = BetaMultiAgentCall::from_serializable(
            BetaMultiAgentAction::SpawnAgent,
            "call_1",
            &Spawn {
                task_name: "research",
                message: "inspect schemas",
            },
        )
        .expect("serialize arguments");
        let item = BetaResponseInputItem::from(call);
        let value = serde_json::to_value(&item).expect("serialize call");
        assert_eq!(value["type"], "multi_agent_call");
        assert_eq!(value["action"], "spawn_agent");
        let arguments: Value =
            serde_json::from_str(value["arguments"].as_str().expect("argument string"))
                .expect("inner JSON");
        assert_eq!(arguments["task_name"], "research");

        let decoded: BetaResponseInputItem =
            serde_json::from_value(value.clone()).expect("decode call");
        assert_eq!(serde_json::to_value(decoded).expect("round trip"), value);
        assert!(
            serde_json::from_value::<BetaResponseInputItem>(json!({
                "type": "multi_agent_call",
                "action": "spawn_agent"
            }))
            .is_err(),
            "known beta tags must validate required fields"
        );
    }

    #[test]
    fn beta_response_exposes_multi_agent_output_and_reasoning_fields() {
        let fixture = response_fixture(json!([{
            "type": "multi_agent_call",
            "action": "list_agents",
            "arguments": "{}",
            "call_id": "call_2",
            "id": "item_2",
            "agent": {"agent_name": "root"}
        }]));
        let response: BetaResponse =
            serde_json::from_value(fixture.clone()).expect("decode beta response");
        assert_eq!(response.id(), "resp_beta_1");
        assert_eq!(response.max_tool_calls(), Some(7));
        assert_eq!(
            response
                .reasoning()
                .and_then(|reasoning| non_null(&reasoning.context))
                .map(BetaReasoningContext::as_str),
            Some("all_turns")
        );
        assert!(matches!(
            response.output(),
            [BetaResponseOutputItem::MultiAgentCall(_)]
        ));
        assert_eq!(
            serde_json::to_value(response).expect("round trip response"),
            fixture
        );
    }

    #[test]
    fn lifecycle_event_reuses_stable_discriminator_with_typed_agent_snapshot() {
        let fixture = json!({
            "type": "response.created",
            "sequence_number": 1,
            "agent": {"agent_name": "root/subagent"},
            "response": response_fixture(json!([]))
        });
        let event: BetaResponseStreamEvent =
            serde_json::from_value(fixture.clone()).expect("decode lifecycle event");
        assert_eq!(event.sequence_number(), Some(1));
        assert_eq!(
            event.agent().map(BetaAgent::agent_name),
            Some("root/subagent")
        );
        assert_eq!(event.response().map(BetaResponse::id), Some("resp_beta_1"));
        assert_eq!(serde_json::to_value(event).expect("round trip"), fixture);
    }

    #[test]
    fn create_and_count_requests_keep_beta_only_fields_typed() {
        let routed = BetaAgentMessage::new(
            "root",
            "root/research",
            [BetaAgentInputText::new("please inspect")],
        );
        let request =
            BetaCreateResponseRequest::new("gpt-test", vec![BetaResponseInputItem::from(routed)])
                .multi_agent(BetaMultiAgentConfig::new(true).max_concurrent_subagents(4))
                .reasoning(
                    BetaReasoningConfig::new()
                        .context(BetaReasoningContext::AllTurns)
                        .mode(BetaReasoningMode::Pro),
                );
        let value = serde_json::to_value(&request).expect("serialize create");
        assert_eq!(value["multi_agent"]["enabled"], true);
        assert_eq!(value["multi_agent"]["max_concurrent_subagents"], 4);
        assert_eq!(value["input"][0]["type"], "agent_message");
        assert!(value.get("stream").is_none());

        let streaming = request.into_streaming();
        let streaming_value = serde_json::to_value(streaming).expect("serialize stream create");
        assert_eq!(streaming_value["stream"], true);

        let count = BetaCountInputTokensRequest::new("gpt-test", "hello")
            .personality("friendly")
            .reasoning(BetaReasoningConfig::new().effort(BetaReasoningEffort::Max));
        let count_value = serde_json::to_value(count).expect("serialize count");
        assert_eq!(count_value["personality"], "friendly");
        assert_eq!(count_value["reasoning"]["effort"], "max");
    }

    #[test]
    fn websocket_inject_events_are_structurally_routed() {
        let inject = BetaResponsesClientEvent::inject(BetaResponseInjectEvent::new(
            "resp_beta_1",
            [BetaResponseInputItem::from(BetaMultiAgentCallOutput::new(
                BetaMultiAgentAction::WaitAgent,
                "call_3",
                [BetaMultiAgentOutputText::new("done")],
            ))],
        ));
        let inject_value = serde_json::to_value(&inject).expect("serialize inject");
        assert_eq!(inject_value["type"], "response.inject");
        assert_eq!(inject_value["input"][0]["type"], "multi_agent_call_output");

        let created: BetaResponsesServerEvent = serde_json::from_value(json!({
            "type": "response.inject.created",
            "response_id": "resp_beta_1",
            "sequence_number": 9,
            "stream_id": "lane.1"
        }))
        .expect("decode inject created");
        assert!(matches!(
            created,
            BetaResponsesServerEvent::InjectCreated(_)
        ));
        assert_eq!(created.stream_id(), Some("lane.1"));

        let websocket_error: BetaResponsesServerEvent = serde_json::from_value(json!({
            "type": "error",
            "error": {
                "code": "bad_event",
                "message": "invalid event",
                "param": null,
                "type": "invalid_request_error"
            },
            "status": 400
        }))
        .expect("decode structural WebSocket error");
        assert!(matches!(
            websocket_error,
            BetaResponsesServerEvent::WebSocketError(_)
        ));
    }

    #[test]
    fn future_stable_and_beta_tags_remain_lossless() {
        let fixture = json!({
            "type": "future_multi_agent_item",
            "id": "future_1",
            "agent": {"agent_name": "future-agent"},
            "payload": {"ok": true}
        });
        let item: BetaResponseInputItem =
            serde_json::from_value(fixture.clone()).expect("decode future input");
        assert_eq!(serde_json::to_value(item).expect("round trip"), fixture);
    }
}
