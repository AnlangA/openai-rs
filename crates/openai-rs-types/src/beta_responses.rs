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
        PromptReference, ResponseError, ResponseInputItem, ResponseInstructions,
        ResponseItemStatus, ResponseOutputItem, ResponseStatus, ResponseStreamEvent,
        ResponseStreamOptions, ResponseTextConfig, ResponseTool, ResponseUsage, ToolChoice,
        TruncationStrategy, UnknownTaggedObject,
    },
};

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

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

crate::open_string_enum! {
    /// Context policy for reasoning items on later turns.
    pub enum BetaReasoningContext {
        Auto = "auto",
        CurrentTurn = "current_turn",
        AllTurns = "all_turns",
    }
}

crate::open_string_enum! {
    /// Execution mode for reasoning.
    pub enum BetaReasoningMode {
        Standard = "standard",
        Pro = "pro",
    }
}

crate::open_string_enum! {
    /// Reasoning effort including the beta-only `max` setting.
    pub enum BetaReasoningEffort {
        None = "none",
        Minimal = "minimal",
        Low = "low",
        Medium = "medium",
        High = "high",
        XHigh = "xhigh",
        Max = "max",
    }
}

crate::open_string_enum! {
    /// Requested reasoning-summary style.
    pub enum BetaReasoningSummary {
        Auto = "auto",
        Concise = "concise",
        Detailed = "detailed",
    }
}

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

literal_tag!(PromptCacheBreakpointMode, Explicit, "explicit");

/// An exact, caller-selected prompt-cache boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetaPromptCacheBreakpoint {
    mode: PromptCacheBreakpointMode,
}

impl BetaPromptCacheBreakpoint {
    /// Creates the only breakpoint mode supported by the pinned schema.
    #[must_use]
    pub const fn explicit() -> Self {
        Self {
            mode: PromptCacheBreakpointMode::Explicit,
        }
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
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Value(
            BetaPromptCacheBreakpoint::explicit(),
        ));
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
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Value(
            BetaPromptCacheBreakpoint::explicit(),
        ));
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
            arguments: JsonText::from_serializable(arguments)?,
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

literal_tag!(MultiAgentCallOutputTag, MultiAgentCallOutput, "multi_agent_call_output");

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
    Stable(BetaStableInputItem),
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
            _ => serde_json::from_value(value)
                .map(Self::Stable)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ResponseInputItem> for BetaResponseInputItem {
    fn from(value: ResponseInputItem) -> Self {
        Self::Stable(BetaStableInputItem::new(value))
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
    Stable(BetaStableOutputItem),
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
                .map(Self::Stable)
                .map_err(D::Error::custom),
        }
    }
}

impl From<ResponseOutputItem> for BetaResponseOutputItem {
    fn from(value: ResponseOutputItem) -> Self {
        Self::Stable(BetaStableOutputItem::new(value))
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
}

crate::open_string_enum! {
    /// Prompt-cache breakpoint selection mode.
    pub enum BetaPromptCacheMode {
        Implicit = "implicit",
        Explicit = "explicit",
    }
}

literal_tag!(ThirtyMinuteTtlTag, ThirtyMinutes, "30m");

/// Prompt-cache options for `gpt-5.6` and later beta models.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaPromptCacheOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    mode: Omittable<BetaPromptCacheMode>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ttl: Omittable<ThirtyMinuteTtlTag>,
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
        self.ttl = Omittable::Value(ThirtyMinuteTtlTag::ThirtyMinutes);
        self
    }
}

crate::open_string_enum! {
    /// Moderation handling mode.
    pub enum BetaModerationMode {
        Score = "score",
        Block = "block",
    }
}

/// Input/output moderation policy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetaModerationPolicy {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<BetaModerationDirection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output: Omittable<Nullable<BetaModerationDirection>>,
}

/// Policy for one moderation direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaModerationDirection {
    mode: BetaModerationMode,
}

impl BetaModerationDirection {
    #[must_use]
    pub const fn new(mode: BetaModerationMode) -> Self {
        Self { mode }
    }
}

/// Moderation configuration on a create request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaModerationConfig {
    model: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    policy: Omittable<Nullable<BetaModerationPolicy>>,
}

impl BetaModerationConfig {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            policy: Omittable::Omitted,
        }
    }

    #[must_use]
    pub fn policy(mut self, policy: BetaModerationPolicy) -> Self {
        self.policy = Omittable::Value(Nullable::Value(policy));
        self
    }
}

/// One context-management compaction rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaContextManagement {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    compact_threshold: Omittable<Nullable<u64>>,
}

impl BetaContextManagement {
    /// Creates the currently supported `compaction` rule.
    #[must_use]
    pub fn compaction() -> Self {
        Self {
            kind: "compaction".to_owned(),
            compact_threshold: Omittable::Omitted,
        }
    }

    #[must_use]
    pub fn compact_threshold(mut self, threshold: u64) -> Self {
        self.compact_threshold = Omittable::Value(Nullable::Value(threshold));
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
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
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
        insert_omittable(&mut object, "input", &self.input)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "context_management", &self.context_management)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "moderation", &self.moderation)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "multi_agent", &self.multi_agent)
            .map_err(serde::ser::Error::custom)?;
        insert_omittable(&mut object, "prompt_cache_options", &self.prompt_cache_options)
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
            _ => return Err(D::Error::custom("beta streaming request requires `stream: true`")),
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

    /// Returns future fields retained during decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
