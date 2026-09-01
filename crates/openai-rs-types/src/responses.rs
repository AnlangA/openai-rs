//! Wire types for the OpenAI Responses API.
//!
//! The types in this module intentionally mirror the JSON protocol. Request
//! constructors and builders keep the common path free of hand-written JSON,
//! while response unions retain future tagged variants without hiding malformed
//! payloads for tags that this crate already knows.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::containers::{MAX_DOMAIN_SECRET_VALUE_CHARS, MIN_DOMAIN_SECRET_CHARS};
use crate::{ExtraFields, JsonText, Nullable, Omittable, WireSecret, open_string_enum};

pub use crate::kernel::{UnknownTaggedObject, UnknownTaggedObjectError};

fn object_discriminator(value: &Value) -> Result<String, &'static str> {
    crate::kernel::object_discriminator(value)
}

open_string_enum! {
    /// Lifecycle state of a response.
    pub enum ResponseStatus {
        Queued = "queued",
        InProgress = "in_progress",
        Completed = "completed",
        Failed = "failed",
        Incomplete = "incomplete",
        Cancelled = "cancelled"
    }
}

open_string_enum! {
    /// Lifecycle state of one response item.
    pub enum ResponseItemStatus {
        InProgress = "in_progress",
        Searching = "searching",
        Generating = "generating",
        Completed = "completed",
        Incomplete = "incomplete",
        Interpreting = "interpreting",
        Calling = "calling",
        Failed = "failed"
    }
}

open_string_enum! {
    /// Status of a message item, per the pinned `MessageStatus`.
    ///
    /// Construction domain for message items; the decode side keeps the
    /// shared open [`ResponseItemStatus`].
    pub enum MessageStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Incomplete = "incomplete"
    }
}

open_string_enum! {
    /// Status of a function-call item, per the pinned
    /// `FunctionCallItemStatus` / `FunctionCallOutputStatusEnum`.
    ///
    /// Construction domain for function-call items and their outputs; the
    /// decode side keeps the shared open [`ResponseItemStatus`].
    pub enum FunctionCallItemStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Incomplete = "incomplete"
    }
}

open_string_enum! {
    /// Status of an MCP tool-call item, per the pinned `MCPToolCallStatus`.
    ///
    /// Adds `calling` / `failed` on top of the message trio; the decode side
    /// keeps the shared open [`ResponseItemStatus`].
    pub enum McpToolCallStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Incomplete = "incomplete",
        Calling = "calling",
        Failed = "failed"
    }
}

open_string_enum! {
    /// Status of a web-search tool call, per the pinned `WebSearchToolCall.status`.
    ///
    /// Carries `searching` / `failed` but no `incomplete`; the decode side
    /// keeps the shared open [`ResponseItemStatus`].
    pub enum WebSearchToolCallStatus {
        InProgress = "in_progress",
        Searching = "searching",
        Completed = "completed",
        Failed = "failed"
    }
}

open_string_enum! {
    /// Status of a file-search tool call, per the pinned
    /// `FileSearchToolCall.status`.
    ///
    /// The full five-value domain (`in_progress`/`searching`/`completed`/
    /// `incomplete`/`failed`); the decode side keeps the shared open
    /// [`ResponseItemStatus`].
    pub enum FileSearchToolCallStatus {
        InProgress = "in_progress",
        Searching = "searching",
        Completed = "completed",
        Incomplete = "incomplete",
        Failed = "failed"
    }
}

open_string_enum! {
    /// Status of an image-generation call, per the pinned
    /// `ImageGenToolCall.status`.
    ///
    /// Carries `generating` / `failed` but no `incomplete` or `searching`;
    /// the decode side keeps the shared open [`ResponseItemStatus`].
    pub enum ImageGenToolCallStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Generating = "generating",
        Failed = "failed"
    }
}

open_string_enum! {
    /// Status of a code-interpreter call, per the pinned
    /// `CodeInterpreterToolCall.status`.
    ///
    /// Carries `interpreting` / `incomplete` / `failed`; the decode side
    /// keeps the shared open [`ResponseItemStatus`].
    pub enum CodeInterpreterToolCallStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Incomplete = "incomplete",
        Interpreting = "interpreting",
        Failed = "failed"
    }
}

impl From<MessageStatus> for ResponseItemStatus {
    fn from(value: MessageStatus) -> Self {
        match value {
            MessageStatus::InProgress => Self::InProgress,
            MessageStatus::Completed => Self::Completed,
            MessageStatus::Incomplete => Self::Incomplete,
            MessageStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<FunctionCallItemStatus> for ResponseItemStatus {
    fn from(value: FunctionCallItemStatus) -> Self {
        match value {
            FunctionCallItemStatus::InProgress => Self::InProgress,
            FunctionCallItemStatus::Completed => Self::Completed,
            FunctionCallItemStatus::Incomplete => Self::Incomplete,
            FunctionCallItemStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<McpToolCallStatus> for ResponseItemStatus {
    fn from(value: McpToolCallStatus) -> Self {
        match value {
            McpToolCallStatus::InProgress => Self::InProgress,
            McpToolCallStatus::Completed => Self::Completed,
            McpToolCallStatus::Incomplete => Self::Incomplete,
            McpToolCallStatus::Calling => Self::Calling,
            McpToolCallStatus::Failed => Self::Failed,
            McpToolCallStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<WebSearchToolCallStatus> for ResponseItemStatus {
    fn from(value: WebSearchToolCallStatus) -> Self {
        match value {
            WebSearchToolCallStatus::InProgress => Self::InProgress,
            WebSearchToolCallStatus::Searching => Self::Searching,
            WebSearchToolCallStatus::Completed => Self::Completed,
            WebSearchToolCallStatus::Failed => Self::Failed,
            WebSearchToolCallStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<FileSearchToolCallStatus> for ResponseItemStatus {
    fn from(value: FileSearchToolCallStatus) -> Self {
        match value {
            FileSearchToolCallStatus::InProgress => Self::InProgress,
            FileSearchToolCallStatus::Searching => Self::Searching,
            FileSearchToolCallStatus::Completed => Self::Completed,
            FileSearchToolCallStatus::Incomplete => Self::Incomplete,
            FileSearchToolCallStatus::Failed => Self::Failed,
            FileSearchToolCallStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<ImageGenToolCallStatus> for ResponseItemStatus {
    fn from(value: ImageGenToolCallStatus) -> Self {
        match value {
            ImageGenToolCallStatus::InProgress => Self::InProgress,
            ImageGenToolCallStatus::Completed => Self::Completed,
            ImageGenToolCallStatus::Generating => Self::Generating,
            ImageGenToolCallStatus::Failed => Self::Failed,
            ImageGenToolCallStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<CodeInterpreterToolCallStatus> for ResponseItemStatus {
    fn from(value: CodeInterpreterToolCallStatus) -> Self {
        match value {
            CodeInterpreterToolCallStatus::InProgress => Self::InProgress,
            CodeInterpreterToolCallStatus::Completed => Self::Completed,
            CodeInterpreterToolCallStatus::Incomplete => Self::Incomplete,
            CodeInterpreterToolCallStatus::Interpreting => Self::Interpreting,
            CodeInterpreterToolCallStatus::Failed => Self::Failed,
            CodeInterpreterToolCallStatus::Unknown(value) => Self::Unknown(value),
        }
    }
}

open_string_enum! {
    /// Role assigned to a Responses message.
    ///
    /// Members match official `MessageRole` / `BetaMessageRole` and the
    /// conversation sibling [`crate::conversations::ConversationMessageRole`].
    pub enum MessageRole {
        UnknownRole = "unknown",
        User = "user",
        Assistant = "assistant",
        System = "system",
        Critic = "critic",
        Discriminator = "discriminator",
        Developer = "developer",
        Tool = "tool"
    }
}

open_string_enum! {
    /// Assistant message phase used by Codex-class models.
    ///
    /// Official guidance requires resending this field on follow-up assistant
    /// messages. Dropping it can degrade later-turn performance.
    pub enum MessagePhase {
        Commentary = "commentary",
        FinalAnswer = "final_answer"
    }
}

open_string_enum! {
    /// Requested image fidelity.
    pub enum ImageDetail {
        Auto = "auto",
        Low = "low",
        High = "high",
        Original = "original"
    }
}

open_string_enum! {
    /// Official `FileInputDetail` / `FileDetailEnum` file rendering fidelity.
    ///
    /// Unlike [`ImageDetail`], this domain does not include `original`.
    pub enum FileDetail {
        Auto = "auto",
        Low = "low",
        High = "high"
    }
}

open_string_enum! {
    /// Official `ResponseErrorCode` machine-readable failure reason.
    ///
    /// Stream `ErrorPayload.code` stays an open string: that schema is
    /// `anyOf [string, null]`, not this enum.
    pub enum ResponseErrorCode {
        ServerError = "server_error",
        RateLimitExceeded = "rate_limit_exceeded",
        InvalidPrompt = "invalid_prompt",
        DataResidencyMismatch = "data_residency_mismatch",
        BioPolicy = "bio_policy",
        VectorStoreTimeout = "vector_store_timeout",
        InvalidImage = "invalid_image",
        InvalidImageFormat = "invalid_image_format",
        InvalidBase64Image = "invalid_base64_image",
        InvalidImageUrl = "invalid_image_url",
        ImageTooLarge = "image_too_large",
        ImageTooSmall = "image_too_small",
        ImageParseError = "image_parse_error",
        ImageContentPolicyViolation = "image_content_policy_violation",
        InvalidImageMode = "invalid_image_mode",
        ImageFileTooLarge = "image_file_too_large",
        UnsupportedImageMediaType = "unsupported_image_media_type",
        EmptyImageFile = "empty_image_file",
        FailedToDownloadImage = "failed_to_download_image",
        ImageFileNotFound = "image_file_not_found"
    }
}

open_string_enum! {
    /// Official `CallableToolAllowedCaller` invocation context.
    pub enum AllowedCaller {
        Direct = "direct",
        Programmatic = "programmatic"
    }
}

open_string_enum! {
    /// Official MCP `connector_id` service-connector identifiers.
    pub enum McpConnectorId {
        Dropbox = "connector_dropbox",
        Gmail = "connector_gmail",
        GoogleCalendar = "connector_googlecalendar",
        GoogleDrive = "connector_googledrive",
        MicrosoftTeams = "connector_microsoftteams",
        OutlookCalendar = "connector_outlookcalendar",
        OutlookEmail = "connector_outlookemail",
        SharePoint = "connector_sharepoint"
    }
}

open_string_enum! {
    /// Context truncation policy.
    pub enum TruncationStrategy {
        Auto = "auto",
        Disabled = "disabled"
    }
}

open_string_enum! {
    /// Amount of reasoning requested from a compatible model.
    pub enum ReasoningEffort {
        None = "none",
        Minimal = "minimal",
        Low = "low",
        Medium = "medium",
        High = "high",
        XHigh = "xhigh",
        Max = "max"
    }
}

open_string_enum! {
    /// Which prior reasoning items are rendered back to the model.
    pub enum ReasoningContext {
        Auto = "auto",
        CurrentTurn = "current_turn",
        AllTurns = "all_turns"
    }
}

open_string_enum! {
    /// Reasoning execution mode for GPT-5.6 and later models.
    pub enum ReasoningMode {
        Standard = "standard",
        Pro = "pro"
    }
}

open_string_enum! {
    /// Requested form of a reasoning summary.
    pub enum ReasoningSummary {
        Auto = "auto",
        Concise = "concise",
        Detailed = "detailed"
    }
}

open_string_enum! {
    /// Why a response stopped before completing.
    pub enum IncompleteReason {
        MaxOutputTokens = "max_output_tokens",
        ContentFilter = "content_filter"
    }
}

open_string_enum! {
    /// Completion status on a reasoning summary-part done event.
    ///
    /// Omitted when the part finished normally. The pin only enumerates
    /// `incomplete` for an interrupted part.
    pub enum ReasoningSummaryPartStatus {
        Incomplete = "incomplete"
    }
}

open_string_enum! {
    /// Optional fields that Responses endpoints may include.
    pub enum ResponseIncludable {
        FileSearchResults = "file_search_call.results",
        WebSearchResults = "web_search_call.results",
        WebSearchSources = "web_search_call.action.sources",
        InputImageUrl = "message.input_image.image_url",
        ComputerOutputImageUrl = "computer_call_output.output.image_url",
        CodeInterpreterOutputs = "code_interpreter_call.outputs",
        ReasoningEncryptedContent = "reasoning.encrypted_content",
        OutputTextLogprobs = "message.output_text.logprobs"
    }
}

open_string_enum! {
    /// Ordering for a response input-item page.
    ///
    /// The pinned `GET /responses/{id}/input_items` `order` query parameter
    /// enumerates `asc` / `desc` (default `desc`).
    pub enum ResponseItemOrder {
        Ascending = "asc",
        Descending = "desc"
    }
}

open_string_enum! {
    /// Retention policy for cached prompt prefixes.
    pub enum PromptCacheRetention {
        InMemory = "in_memory",
        TwentyFourHours = "24h"
    }
}

open_string_enum! {
    /// Processing tier requested for a response.
    pub enum ServiceTier {
        Auto = "auto",
        Default = "default",
        Flex = "flex",
        Scale = "scale",
        Priority = "priority",
        Fast = "fast",
        Ultrafast = "ultrafast"
    }
}

open_string_enum! {
    /// Processing tier accepted by the GA compact request.
    ///
    /// Members match the pinned `ServiceTierEnum` (auto/default/fast/flex/
    /// priority) exactly — the same domain the beta side models as
    /// [`crate::beta_responses::BetaCompactServiceTier`]. The create and
    /// response-echo sides keep the wider [`ServiceTier`] domain, which also
    /// accepts `scale` / `ultrafast`.
    pub enum CompactServiceTier {
        Auto = "auto",
        Default = "default",
        Fast = "fast",
        Flex = "flex",
        Priority = "priority"
    }
}

open_string_enum! {
    /// Requested verbosity of the generated answer.
    pub enum ResponseTextVerbosity {
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

open_string_enum! {
    /// Prompt-cache breakpoint selection mode.
    pub enum PromptCacheMode {
        Implicit = "implicit",
        Explicit = "explicit"
    }
}

open_string_enum! {
    /// Minimum lifetime applied to prompt-cache breakpoints.
    pub enum PromptCacheTtl {
        ThirtyMinutes = "30m"
    }
}

literal_tag!(PromptCacheBreakpointTag, Explicit, "explicit");

/// Explicit cache breakpoint attached to an input content part.
///
/// The pinned wire object is `{ "mode": "explicit" }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheBreakpoint {
    mode: PromptCacheBreakpointTag,
}

impl PromptCacheBreakpoint {
    /// Constructs an explicit cache breakpoint.
    #[must_use]
    pub const fn explicit() -> Self {
        Self {
            mode: PromptCacheBreakpointTag::Explicit,
        }
    }
}

impl Default for PromptCacheBreakpoint {
    fn default() -> Self {
        Self::explicit()
    }
}

/// Official create-request `PromptCacheOptionsParam`.
///
/// The pin lists no required properties; `ttl` and `mode` may be sent
/// independently. Response echo uses [`PromptCacheOptions`], which requires
/// both fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheOptionsParam {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    mode: Omittable<PromptCacheMode>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ttl: Omittable<PromptCacheTtl>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Official response-echo `PromptCacheOptions`.
///
/// The pin requires both `ttl` and `mode` when this object is present.
/// Create/compact requests use [`PromptCacheOptionsParam`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheOptions {
    mode: PromptCacheMode,
    ttl: PromptCacheTtl,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Pinned OpenAPI numeric and metadata limits for `POST /responses`.
pub const MAX_RESPONSE_METADATA_PAIRS: usize = 16;
/// Maximum Unicode scalar count for one metadata key.
pub const MAX_RESPONSE_METADATA_KEY_CHARS: usize = 64;
/// Maximum Unicode scalar count for one metadata value.
pub const MAX_RESPONSE_METADATA_VALUE_CHARS: usize = 512;
/// Maximum Unicode scalar count for `safety_identifier`.
pub const MAX_SAFETY_IDENTIFIER_CHARS: usize = 64;
/// Maximum Unicode scalar count for compact `prompt_cache_key`.
pub const MAX_PROMPT_CACHE_KEY_CHARS: usize = 64;
/// Inclusive maximum Unicode scalar count for compact/count-tokens string `input`.
pub const MAX_COMPACT_INPUT_CHARS: usize = 10_485_760;
/// Inclusive minimum for `max_output_tokens`.
pub const MIN_MAX_OUTPUT_TOKENS: u32 = 16;
/// Inclusive maximum for `top_logprobs`.
pub const MAX_TOP_LOGPROBS: u32 = 20;
/// Inclusive minimum for file-search `max_num_results`.
pub const MIN_FILE_SEARCH_RESULTS: u32 = 1;
/// Inclusive maximum for file-search `max_num_results`.
pub const MAX_FILE_SEARCH_RESULTS: u32 = 50;
/// Inclusive minimum for context-management `compact_threshold`.
pub const MIN_COMPACT_THRESHOLD: u64 = 1_000;
/// Inclusive maximum for image-generation tool `output_compression`.
pub const MAX_IMAGE_GENERATION_COMPRESSION: u8 = 100;
/// Inclusive maximum for image-generation tool `partial_images`.
pub const MAX_IMAGE_GENERATION_PARTIAL_IMAGES: u8 = 3;
/// Inclusive maximum for code-interpreter automatic-container `file_ids`.
pub const MAX_CODE_INTERPRETER_FILE_IDS: usize = 50;
/// Inclusive maximum for shell automatic-container `file_ids`.
pub const MAX_SHELL_CONTAINER_FILE_IDS: usize = 50;
/// Inclusive maximum for shell environment `skills`.
pub const MAX_SHELL_SKILLS: usize = 200;
/// Inclusive maximum for a referenced skill id.
pub const MAX_SKILL_ID_CHARS: usize = 64;
/// Inclusive minimum for a function-tool name.
pub const MIN_FUNCTION_TOOL_NAME_CHARS: usize = 1;
/// Inclusive maximum for a function-tool name.
pub const MAX_FUNCTION_TOOL_NAME_CHARS: usize = 128;
/// Inclusive minimum for function-shell `call_id`.
pub const MIN_FUNCTION_SHELL_CALL_ID_CHARS: usize = 1;
/// Inclusive maximum for function-shell `call_id`.
pub const MAX_FUNCTION_SHELL_CALL_ID_CHARS: usize = 64;
/// Inclusive maximum for function-shell stdout/stderr characters.
pub const MAX_FUNCTION_SHELL_OUTPUT_CHARS: usize = 10_485_760;
/// Inclusive minimum for function-call output `name`.
pub const MIN_FUNCTION_CALL_OUTPUT_NAME_CHARS: usize = 1;
/// Inclusive maximum for function-call output `name`.
pub const MAX_FUNCTION_CALL_OUTPUT_NAME_CHARS: usize = 128;
/// Inclusive minimum for function-call output `namespace`.
pub const MIN_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS: usize = 1;
/// Inclusive maximum for function-call output `namespace`.
pub const MAX_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS: usize = 64;
/// Inclusive maximum for function-call output string characters.
pub const MAX_FUNCTION_CALL_OUTPUT_CHARS: usize = 10_485_760;
/// Inclusive maximum for compaction `encrypted_content`.
pub const MAX_COMPACTION_ENCRYPTED_CHARS: usize = 20_971_520;
/// Inclusive maximum for input-text `text` characters.
pub const MAX_INPUT_TEXT_CHARS: usize = 10_485_760;
/// Inclusive minimum for apply_patch operation `path`.
pub const MIN_APPLY_PATCH_PATH_CHARS: usize = 1;
/// Inclusive maximum for apply_patch create/update `diff` characters.
pub const MAX_APPLY_PATCH_DIFF_CHARS: usize = 10_485_760;
/// Inclusive maximum for input-image `image_url` characters.
pub const MAX_INPUT_IMAGE_URL_CHARS: usize = 20_971_520;
/// Inclusive maximum for input-file `file_data` characters.
pub const MAX_INPUT_FILE_DATA_CHARS: usize = 73_400_320;
/// Inclusive minimum for inline skill source `data`.
pub const MIN_INLINE_SKILL_SOURCE_DATA_CHARS: usize = 1;
/// Inclusive maximum for inline skill source `data` characters.
pub const MAX_INLINE_SKILL_SOURCE_DATA_CHARS: usize = 70_254_592;
/// Inclusive minimum for WebSocket `stream_id`.
pub const MIN_STREAM_ID_CHARS: usize = 1;
/// Inclusive maximum for WebSocket `stream_id`.
pub const MAX_STREAM_ID_CHARS: usize = 256;

/// A create-request value that violates a pinned OpenAPI constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CreateResponseConstraintError {
    /// `temperature` is non-finite or outside `0..=2`.
    #[error("temperature must be finite and within 0..=2, got {value}")]
    Temperature {
        /// Rejected value rendered without retaining a floating-point field.
        value: String,
    },
    /// `top_p` is non-finite or outside `0..=1`.
    #[error("top_p must be finite and within 0..=1, got {value}")]
    TopP {
        /// Rejected value rendered without retaining a floating-point field.
        value: String,
    },
    /// `top_logprobs` is outside `0..=20`.
    #[error("top_logprobs must be 0..={maximum}, got {actual}")]
    TopLogprobs {
        /// Rejected value.
        actual: u32,
        /// Contract maximum.
        maximum: u32,
    },
    /// `max_output_tokens` is below the pinned minimum of 16.
    #[error("max_output_tokens must be at least {minimum}, got {actual}")]
    MaxOutputTokens {
        /// Rejected value.
        actual: u32,
        /// Contract minimum.
        minimum: u32,
    },
    /// `safety_identifier` exceeds 64 characters.
    #[error("safety_identifier has {actual} characters; maximum is {maximum}")]
    SafetyIdentifier {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Metadata contains more than 16 pairs.
    #[error("metadata contains {actual} pairs; maximum is {maximum}")]
    MetadataPairCount {
        /// Observed pair count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata key exceeds 64 characters.
    #[error("metadata key has {actual} characters; maximum is {maximum}")]
    MetadataKey {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata value exceeds 512 characters.
    #[error("metadata value has {actual} characters; maximum is {maximum}")]
    MetadataValue {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// `context_management` is present and empty.
    #[error("context_management must contain at least one entry when present")]
    EmptyContextManagement,
    /// A compaction `compact_threshold` is below the pinned minimum of 1000.
    #[error("context_management compact_threshold must be at least {minimum}, got {actual}")]
    CompactThreshold {
        /// Rejected value.
        actual: u64,
        /// Contract minimum.
        minimum: u64,
    },
    /// File-search `max_num_results` is outside `1..=50`.
    #[error("file_search max_num_results must be {minimum}..={maximum}, got {actual}")]
    FileSearchMaxResults {
        /// Rejected value.
        actual: u32,
        /// Contract minimum.
        minimum: u32,
        /// Contract maximum.
        maximum: u32,
    },
    /// File-search ranking `score_threshold` is non-finite or outside `0..=1`.
    #[error("file_search ranking score_threshold must be finite and within 0..=1, got {value}")]
    FileSearchScoreThreshold {
        /// Rejected value rendered without retaining a floating-point field.
        value: String,
    },
    /// Image-generation `output_compression` is outside `0..=100`.
    #[error("image_generation output_compression must be 0..={maximum}, got {actual}")]
    ImageGenerationCompression {
        /// Rejected value.
        actual: u8,
        /// Contract maximum.
        maximum: u8,
    },
    /// Image-generation `partial_images` is outside `0..=3`.
    #[error("image_generation partial_images must be 0..={maximum}, got {actual}")]
    ImageGenerationPartialImages {
        /// Rejected value.
        actual: u8,
        /// Contract maximum.
        maximum: u8,
    },
    /// Code-interpreter automatic container lists more than 50 `file_ids`.
    #[error("code_interpreter file_ids has {actual} entries; maximum is {maximum}")]
    CodeInterpreterFileIds {
        /// Observed file-id count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Code-interpreter allowlist `domain_secrets` is present and empty (`minItems: 1`).
    #[error(
        "code_interpreter allowlist domain_secrets must contain at least one secret when present"
    )]
    EmptyDomainSecrets,
    /// Code-interpreter allowlist `allowed_domains` is empty (`minItems: 1`).
    #[error("code_interpreter allowlist allowed_domains must contain at least one domain")]
    EmptyAllowedDomains,
    /// Code-interpreter domain-secret `domain` is empty (`minLength` 1).
    #[error("code_interpreter domain_secret domain has {actual} characters; minimum is {minimum}")]
    DomainSecretDomain {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
    },
    /// Code-interpreter domain-secret `name` is empty (`minLength` 1).
    #[error("code_interpreter domain_secret name has {actual} characters; minimum is {minimum}")]
    DomainSecretName {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
    },
    /// Code-interpreter domain-secret `value` is empty or longer than 10,485,760 characters.
    #[error(
        "code_interpreter domain_secret value has {actual} characters; must be {minimum}..={maximum}"
    )]
    DomainSecretValue {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Shell automatic container lists more than 50 `file_ids`.
    #[error("shell container file_ids has {actual} entries; maximum is {maximum}")]
    ShellContainerFileIds {
        /// Observed file-id count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Shell environment lists more than 200 `skills`.
    #[error("shell environment skills has {actual} entries; maximum is {maximum}")]
    ShellEnvironmentSkills {
        /// Observed skill count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A referenced skill id is empty or longer than 64 characters.
    #[error("skill_id has {actual} characters; must be 1..={maximum}")]
    SkillIdLength {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-tool `name` is empty, longer than 128 characters, or not
    /// `[A-Za-z0-9_-]`.
    #[error(
        "function tool name has {actual} characters; must be {minimum}..={maximum} and match [A-Za-z0-9_-]"
    )]
    FunctionToolName {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// `allowed_callers` is present and empty.
    #[error("allowed_callers must contain at least one entry when present")]
    EmptyAllowedCallers,
    /// Namespace `name` is empty.
    #[error("namespace name must be non-empty")]
    EmptyNamespaceName,
    /// Namespace `tools` is empty.
    #[error("namespace tools must contain at least one entry")]
    EmptyNamespaceTools,
    /// MCP `tunnel_id` does not match `tunnel_` plus 32 `[a-z0-9]` characters.
    #[error("mcp tunnel_id must match ^tunnel_[a-z0-9]{{32}}$")]
    McpTunnelId,
    /// Function-shell `call_id` is empty or longer than 64 characters.
    #[error("function-shell call_id has {actual} characters; must be {minimum}..={maximum}")]
    FunctionShellCallId {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-shell captured stdout exceeds 10,485,760 characters.
    #[error("function-shell stdout has {actual} characters; maximum is {maximum}")]
    FunctionShellStdout {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-shell captured stderr exceeds 10,485,760 characters.
    #[error("function-shell stderr has {actual} characters; maximum is {maximum}")]
    FunctionShellStderr {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A tool-item `call_id` is empty or longer than 64 characters.
    #[error("call_id has {actual} characters; must be {minimum}..={maximum}")]
    CallId {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-call output `name` is empty or longer than 128 characters.
    #[error("function_call_output name has {actual} characters; must be {minimum}..={maximum}")]
    FunctionCallOutputName {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-call output `namespace` is empty, longer than 64 characters, or
    /// not `[A-Za-z0-9_-]`.
    #[error(
        "function_call_output namespace has {actual} characters; must be {minimum}..={maximum} and match [A-Za-z0-9_-]"
    )]
    FunctionCallOutputNamespace {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Function-call output string exceeds 10,485,760 characters.
    #[error("function_call_output has {actual} characters; maximum is {maximum}")]
    FunctionCallOutputChars {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Compaction `encrypted_content` exceeds 20,971,520 characters.
    #[error("compaction encrypted_content has {actual} characters; maximum is {maximum}")]
    CompactionEncryptedContent {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Program `code` exceeds 10,485,760 characters.
    #[error("program code has {actual} characters; maximum is {maximum}")]
    ProgramCode {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Program `fingerprint` exceeds 10,485,760 characters.
    #[error("program fingerprint has {actual} characters; maximum is {maximum}")]
    ProgramFingerprint {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Program output `result` exceeds 10,485,760 characters.
    #[error("program_output result has {actual} characters; maximum is {maximum}")]
    ProgramResult {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Input-text `text` exceeds 10,485,760 characters.
    #[error("input_text text has {actual} characters; maximum is {maximum}")]
    InputText {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Apply-patch `path` is empty.
    #[error("apply_patch path has {actual} characters; minimum is {minimum}")]
    ApplyPatchPath {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
    },
    /// Apply-patch `diff` exceeds 10,485,760 characters.
    #[error("apply_patch diff has {actual} characters; maximum is {maximum}")]
    ApplyPatchDiff {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Inter-agent `encrypted_content` exceeds 10,485,760 characters.
    #[error("encrypted_content has {actual} characters; maximum is {maximum}")]
    AgentEncryptedContent {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Input-image `image_url` exceeds 20,971,520 characters.
    #[error("input_image image_url has {actual} characters; maximum is {maximum}")]
    InputImageUrl {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Input-file `file_data` exceeds 73,400,320 characters.
    #[error("input_file file_data has {actual} characters; maximum is {maximum}")]
    InputFileData {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Inline skill source `data` is empty or longer than 70,254,592 characters.
    #[error("inline skill source data has {actual} characters; must be {minimum}..={maximum}")]
    InlineSkillSourceData {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// WebSocket `stream_id` is empty, longer than 256 characters, or not
    /// `[A-Za-z0-9_.-]`.
    #[error(
        "stream_id has {actual} characters; must be {minimum}..={maximum} and match [A-Za-z0-9_.-]"
    )]
    StreamId {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// `max_concurrent_subagents` is below the pinned minimum of 1.
    #[error("max_concurrent_subagents must be at least {minimum}, got {actual}")]
    ConcurrentSubagents {
        /// Rejected value.
        actual: u32,
        /// Contract minimum.
        minimum: u32,
    },
}

impl PromptCacheOptionsParam {
    /// Creates empty prompt-cache options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates prompt-cache options with the given mode.
    #[must_use]
    pub fn with_mode(mode: PromptCacheMode) -> Self {
        Self {
            mode: Omittable::Value(mode),
            ttl: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the prompt-cache mode.
    #[must_use]
    pub fn mode(mut self, mode: PromptCacheMode) -> Self {
        self.mode = Omittable::Value(mode);
        self
    }

    /// Sets the only TTL currently accepted by the pinned schema.
    #[must_use]
    pub fn thirty_minutes(mut self) -> Self {
        self.ttl = Omittable::Value(PromptCacheTtl::ThirtyMinutes);
        self
    }

    /// Sets the prompt-cache TTL.
    #[must_use]
    pub fn ttl(mut self, ttl: PromptCacheTtl) -> Self {
        self.ttl = Omittable::Value(ttl);
        self
    }

    /// Returns the prompt-cache mode when set.
    #[must_use]
    pub fn mode_ref(&self) -> Option<&PromptCacheMode> {
        match &self.mode {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the prompt-cache TTL when set.
    #[must_use]
    pub fn ttl_ref(&self) -> Option<&PromptCacheTtl> {
        match &self.ttl {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns extra fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl PromptCacheOptions {
    /// Creates a complete official response-echo object.
    #[must_use]
    pub fn new(mode: PromptCacheMode, ttl: PromptCacheTtl) -> Self {
        Self {
            mode,
            ttl,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the applied prompt-cache mode.
    #[must_use]
    pub const fn mode(&self) -> &PromptCacheMode {
        &self.mode
    }

    /// Returns the applied prompt-cache TTL.
    #[must_use]
    pub const fn ttl(&self) -> &PromptCacheTtl {
        &self.ttl
    }

    /// Returns extra fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(InputTextTag, InputText, "input_text");
literal_tag!(InputImageTag, InputImage, "input_image");
literal_tag!(InputFileTag, InputFile, "input_file");
literal_tag!(InputMessageTag, Message, "message");
literal_tag!(
    FunctionCallOutputTag,
    FunctionCallOutput,
    "function_call_output"
);
literal_tag!(
    McpApprovalResponseTag,
    McpApprovalResponse,
    "mcp_approval_response"
);

/// A text content part supplied to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputText {
    #[serde(rename = "type")]
    kind: InputTextTag,
    text: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<PromptCacheBreakpoint>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputText {
    /// Creates a text input part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: InputTextTag::InputText,
            text: text.into(),
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Marks an explicit prompt-cache boundary after this part.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(PromptCacheBreakpoint::explicit()));
        self
    }

    /// Sends `prompt_cache_breakpoint: null`.
    #[must_use]
    pub fn prompt_cache_breakpoint_null(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the explicit prompt-cache breakpoint if set.
    #[must_use]
    pub fn prompt_cache_breakpoint_ref(&self) -> Option<&PromptCacheBreakpoint> {
        match &self.prompt_cache_breakpoint {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the input text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Checks pinned OpenAPI `text` `maxLength` without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_input_text_chars(self.text.chars().count())
    }
}

/// An image input addressed by URL or uploaded file id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputImage {
    #[serde(rename = "type")]
    kind: InputImageTag,
    detail: ImageDetail,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<PromptCacheBreakpoint>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputImage {
    /// Creates an image input from a URL or data URL.
    ///
    /// Official `InputImageContent` requires `detail`; constructors send the
    /// documented default `auto`.
    #[must_use]
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: ImageDetail::Auto,
            image_url: Omittable::Value(Nullable::Value(url.into())),
            file_id: Omittable::Omitted,
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates an image input from an uploaded file id.
    ///
    /// Official `InputImageContent` requires `detail`; constructors send the
    /// documented default `auto`.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: ImageDetail::Auto,
            image_url: Omittable::Omitted,
            file_id: Omittable::Value(Nullable::Value(file_id.into())),
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Marks an explicit prompt-cache boundary after this part.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(PromptCacheBreakpoint::explicit()));
        self
    }

    /// Sends `prompt_cache_breakpoint: null`.
    #[must_use]
    pub fn prompt_cache_breakpoint_null(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the explicit prompt-cache breakpoint if set.
    #[must_use]
    pub fn prompt_cache_breakpoint_ref(&self) -> Option<&PromptCacheBreakpoint> {
        match &self.prompt_cache_breakpoint {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Sets the requested fidelity.
    #[must_use]
    pub fn detail(mut self, detail: ImageDetail) -> Self {
        self.detail = detail;
        self
    }

    /// Returns the official required detail level.
    #[must_use]
    pub const fn detail_ref(&self) -> &ImageDetail {
        &self.detail
    }

    /// Sends official `image_url: null`.
    #[must_use]
    pub fn image_url_null(mut self) -> Self {
        self.image_url = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `file_id: null`.
    #[must_use]
    pub fn file_id_null(mut self) -> Self {
        self.file_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the image URL when present.
    #[must_use]
    pub fn image_url(&self) -> Option<&str> {
        match &self.image_url {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the uploaded file id when present.
    #[must_use]
    pub fn file_id(&self) -> Option<&str> {
        match &self.file_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Checks pinned OpenAPI `image_url` `maxLength` without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(Nullable::Value(image_url)) = &self.image_url {
            validate_input_image_url_chars(image_url.chars().count())?;
        }
        Ok(())
    }
}

/// Function-call output image part (`InputImageContentParamAutoParam`).
///
/// Official Param `required` is only `type`; `detail` is `anyOf` including
/// null. Message `InputContent` uses [`InputImage`] instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputImageParam {
    #[serde(rename = "type")]
    kind: InputImageTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<Nullable<ImageDetail>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<PromptCacheBreakpoint>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputImageParam {
    /// Creates a Param image from a URL without sending `detail`.
    #[must_use]
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            image_url: Omittable::Value(Nullable::Value(url.into())),
            file_id: Omittable::Omitted,
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a Param image from an uploaded file id without sending `detail`.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            image_url: Omittable::Omitted,
            file_id: Omittable::Value(Nullable::Value(file_id.into())),
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the requested fidelity.
    #[must_use]
    pub fn detail(mut self, detail: ImageDetail) -> Self {
        self.detail = Omittable::Value(Nullable::Value(detail));
        self
    }

    /// Sends official Param `detail: null`.
    #[must_use]
    pub fn detail_null(mut self) -> Self {
        self.detail = Omittable::Value(Nullable::Null);
        self
    }

    /// Marks an explicit prompt-cache boundary after this part.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(PromptCacheBreakpoint::explicit()));
        self
    }

    /// Sends official `prompt_cache_breakpoint: null`.
    #[must_use]
    pub fn prompt_cache_breakpoint_null(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `image_url: null`.
    #[must_use]
    pub fn image_url_null(mut self) -> Self {
        self.image_url = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `file_id: null`.
    #[must_use]
    pub fn file_id_null(mut self) -> Self {
        self.file_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the image URL when present.
    #[must_use]
    pub fn image_url(&self) -> Option<&str> {
        match &self.image_url {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the uploaded file id when present.
    #[must_use]
    pub fn file_id(&self) -> Option<&str> {
        match &self.file_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Checks pinned OpenAPI `image_url` `maxLength` without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(Nullable::Value(image_url)) = &self.image_url {
            validate_input_image_url_chars(image_url.chars().count())?;
        }
        Ok(())
    }
}

impl From<InputImage> for InputImageParam {
    fn from(value: InputImage) -> Self {
        Self {
            kind: value.kind,
            detail: Omittable::Value(Nullable::Value(value.detail)),
            image_url: value.image_url,
            file_id: value.file_id,
            prompt_cache_breakpoint: value.prompt_cache_breakpoint,
            extra: value.extra,
        }
    }
}

/// A file input addressed by URL, uploaded id, or base64 file data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputFile {
    #[serde(rename = "type")]
    kind: InputFileTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_data: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filename: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<FileDetail>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<Nullable<PromptCacheBreakpoint>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputFile {
    fn empty() -> Self {
        Self {
            kind: InputFileTag::InputFile,
            file_id: Omittable::Omitted,
            file_url: Omittable::Omitted,
            file_data: Omittable::Omitted,
            filename: Omittable::Omitted,
            detail: Omittable::Omitted,
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Marks an explicit prompt-cache boundary after this part.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self) -> Self {
        self.prompt_cache_breakpoint =
            Omittable::Value(Nullable::Value(PromptCacheBreakpoint::explicit()));
        self
    }

    /// Sends `prompt_cache_breakpoint: null`.
    #[must_use]
    pub fn prompt_cache_breakpoint_null(mut self) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the explicit prompt-cache breakpoint if set.
    #[must_use]
    pub fn prompt_cache_breakpoint_ref(&self) -> Option<&PromptCacheBreakpoint> {
        match &self.prompt_cache_breakpoint {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Creates a file input from an uploaded file id.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_id = Omittable::Value(Nullable::Value(file_id.into()));
        value
    }

    /// Creates a file input from a remote URL.
    #[must_use]
    pub fn from_url(file_url: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_url = Omittable::Value(Nullable::Value(file_url.into()));
        value
    }

    /// Creates a file input from base64 data and a filename.
    #[must_use]
    pub fn from_base64(file_data: impl Into<String>, filename: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_data = Omittable::Value(Nullable::Value(file_data.into()));
        value.filename = Omittable::Value(Nullable::Value(filename.into()));
        value
    }

    /// Sets the official file rendering detail.
    #[must_use]
    pub fn detail(mut self, detail: FileDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }

    /// Sends official `file_id: null`.
    #[must_use]
    pub fn file_id_null(mut self) -> Self {
        self.file_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `file_url: null`.
    #[must_use]
    pub fn file_url_null(mut self) -> Self {
        self.file_url = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `file_data: null`.
    #[must_use]
    pub fn file_data_null(mut self) -> Self {
        self.file_data = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `filename: null`.
    #[must_use]
    pub fn filename_null(mut self) -> Self {
        self.filename = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Checks pinned OpenAPI `file_data` `maxLength` without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(Nullable::Value(file_data)) = &self.file_data {
            validate_input_file_data_chars(file_data.chars().count())?;
        }
        Ok(())
    }
}

tagged_union! {
    /// One rich content part in an item-form input message.
    ///
    /// This is the widest input content union: the pinned item-form `Message`
    /// branch is the only request location where `computer_screenshot` is
    /// legal. The easy-form message and function-call outputs use the
    /// three-branch [`EasyInputContent`] instead.
    pub enum InputContent {
        Text(InputText) => "input_text",
        Image(InputImage) => "input_image",
        File(InputFile) => "input_file",
        ComputerScreenshot(ComputerScreenshot) => "computer_screenshot"
    }
}

impl From<InputText> for InputContent {
    fn from(value: InputText) -> Self {
        Self::Text(value)
    }
}

impl From<InputImage> for InputContent {
    fn from(value: InputImage) -> Self {
        Self::Image(value)
    }
}

impl From<InputFile> for InputContent {
    fn from(value: InputFile) -> Self {
        Self::File(value)
    }
}

impl From<ComputerScreenshot> for InputContent {
    fn from(value: ComputerScreenshot) -> Self {
        Self::ComputerScreenshot(value)
    }
}

tagged_union! {
    /// One rich content part accepted by easy-form message content.
    ///
    /// Members match the pinned `InputContent` request schema — the union
    /// behind `EasyInputMessage.content` / python's
    /// `ResponseInputMessageContentListParam` — exactly: `computer_screenshot`
    /// is legal only inside item-form messages ([`InputContent`]) and is not
    /// constructible here. The open `Unknown` variant keeps decoding lossless.
    pub enum EasyInputContent {
        Text(InputText) => "input_text",
        Image(InputImage) => "input_image",
        File(InputFile) => "input_file"
    }
}

impl From<InputText> for EasyInputContent {
    fn from(value: InputText) -> Self {
        Self::Text(value)
    }
}

impl From<InputImage> for EasyInputContent {
    fn from(value: InputImage) -> Self {
        Self::Image(value)
    }
}

impl From<InputFile> for EasyInputContent {
    fn from(value: InputFile) -> Self {
        Self::File(value)
    }
}

/// Text or an ordered list of rich message content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain text content.
    Text(String),
    /// Rich content parts.
    Parts(Vec<EasyInputContent>),
}

impl From<String> for MessageContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for MessageContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<EasyInputContent>> for MessageContent {
    fn from(value: Vec<EasyInputContent>) -> Self {
        Self::Parts(value)
    }
}

/// A Responses input message.
///
/// The service accepts request messages without an explicit `type`; the
/// constructor emits that compact shape. Decoding also accepts an explicit
/// `"type":"message"` and validates it when present. Construction pins the
/// four-role [`EasyInputMessageRole`] and the three-branch
/// [`EasyInputContent`] content union; decoding keeps both domains open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputMessage {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<InputMessageTag>,
    role: MessageRole,
    content: MessageContent,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    phase: Omittable<Nullable<MessagePhase>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputMessage {
    /// Creates a message for the supplied role.
    ///
    /// The constructor domain is the pinned four-value
    /// [`EasyInputMessageRole`]; decoding keeps the open [`MessageRole`] so
    /// multi-agent roles stay lossless.
    #[must_use]
    pub fn new(role: EasyInputMessageRole, content: impl Into<MessageContent>) -> Self {
        Self {
            kind: Omittable::Omitted,
            role: role.into(),
            content: content.into(),
            phase: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl Into<MessageContent>) -> Self {
        Self::new(EasyInputMessageRole::User, content)
    }

    /// Creates an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<MessageContent>) -> Self {
        Self::new(EasyInputMessageRole::Assistant, content)
    }

    /// Creates a developer message.
    #[must_use]
    pub fn developer(content: impl Into<MessageContent>) -> Self {
        Self::new(EasyInputMessageRole::Developer, content)
    }

    /// Creates a system message.
    #[must_use]
    pub fn system(content: impl Into<MessageContent>) -> Self {
        Self::new(EasyInputMessageRole::System, content)
    }

    /// Emits the optional `type: "message"` request property.
    #[must_use]
    pub fn with_type(mut self) -> Self {
        self.kind = Omittable::Value(InputMessageTag::Message);
        self
    }

    /// Labels an assistant message as commentary or the final answer.
    #[must_use]
    pub fn phase(mut self, phase: impl Into<MessagePhase>) -> Self {
        self.phase = Omittable::Value(Nullable::Value(phase.into()));
        self
    }

    /// Explicitly sends `phase: null`.
    #[must_use]
    pub fn phase_null(mut self) -> Self {
        self.phase = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the assistant message phase when present and non-null.
    #[must_use]
    pub fn phase_ref(&self) -> Option<&MessagePhase> {
        match &self.phase {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the message role.
    #[must_use]
    pub const fn role(&self) -> &MessageRole {
        &self.role
    }

    /// Returns the message content.
    #[must_use]
    pub const fn content(&self) -> &MessageContent {
        &self.content
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_message_content(&self.content)
    }
}

open_string_enum! {
    /// Role accepted by the easy-form input message when constructing requests.
    ///
    /// Members match the pinned `EasyInputMessage.role` / python
    /// `EasyInputMessageParam.role` Literal (user/assistant/system/developer)
    /// exactly. Multi-agent roles such as `critic` or `tool` decode through
    /// the open [`MessageRole`] but cannot be constructed here.
    pub enum EasyInputMessageRole {
        User = "user",
        Assistant = "assistant",
        System = "system",
        Developer = "developer"
    }
}

impl From<EasyInputMessageRole> for MessageRole {
    fn from(value: EasyInputMessageRole) -> Self {
        match value {
            EasyInputMessageRole::User => Self::User,
            EasyInputMessageRole::Assistant => Self::Assistant,
            EasyInputMessageRole::System => Self::System,
            EasyInputMessageRole::Developer => Self::Developer,
            EasyInputMessageRole::Unknown(value) => Self::Unknown(value),
        }
    }
}

/// Role accepted by the stored `InputMessage` schema when constructing
/// requests (`user` / `system` / `developer`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoredInputMessageRole {
    /// User input.
    User,
    /// System instruction.
    System,
    /// Developer instruction.
    Developer,
}

impl From<StoredInputMessageRole> for MessageRole {
    fn from(value: StoredInputMessageRole) -> Self {
        match value {
            StoredInputMessageRole::User => Self::User,
            StoredInputMessageRole::System => Self::System,
            StoredInputMessageRole::Developer => Self::Developer,
        }
    }
}

impl From<StoredInputMessageRole> for EasyInputMessageRole {
    fn from(value: StoredInputMessageRole) -> Self {
        match value {
            StoredInputMessageRole::User => Self::User,
            StoredInputMessageRole::System => Self::System,
            StoredInputMessageRole::Developer => Self::Developer,
        }
    }
}

/// The item-form input message used inside the expanded `Item` union.
///
/// This differs from [`InputMessage`]'s ergonomic schema: content is always an
/// array, assistant is not an accepted role, and a returned item may carry a
/// status. Decoding keeps the role as the open [`MessageRole`]: compacted and
/// listed items echo multi-agent roles such as `critic` or `tool` that the
/// request-side constructor intentionally cannot produce.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredInputMessage {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<InputMessageTag>,
    role: MessageRole,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseItemStatus>,
    content: Vec<InputContent>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl StoredInputMessage {
    /// Creates an item-form input message.
    #[must_use]
    pub fn new(
        role: StoredInputMessageRole,
        content: impl IntoIterator<Item = impl Into<InputContent>>,
    ) -> Self {
        Self {
            kind: Omittable::Value(InputMessageTag::Message),
            role: role.into(),
            status: Omittable::Omitted,
            content: content.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the role exactly as observed on the wire.
    ///
    /// Stored items decode through the open [`MessageRole`], so multi-agent
    /// roles such as `critic` or `tool` are preserved verbatim instead of
    /// failing the decode.
    #[must_use]
    pub const fn role(&self) -> &MessageRole {
        &self.role
    }

    /// Sets the returned item status.
    ///
    /// The pinned `MessageStatus` domain is the three message-trio values;
    /// decoded statuses replay through [`MessageStatus::from_raw`].
    #[must_use]
    pub fn status(mut self, status: MessageStatus) -> Self {
        self.status = Omittable::Value(status.into());
        self
    }

    /// Carries retained unknown properties through a replay conversion.
    ///
    /// Mirrors the JSON round-trip used for assistant messages, where
    /// source-only top-level fields survive inside `extra` instead of being
    /// dropped by the field-by-field rebuild.
    pub(crate) fn with_retained_extra(mut self, retained: &ExtraFields) -> Self {
        self.extra = merge_extra_fields(&self.extra, retained);
        self
    }

    /// Returns content parts.
    #[must_use]
    pub fn content(&self) -> &[InputContent] {
        &self.content
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_input_contents(&self.content)
    }
}

/// A plain string input or a sequence of typed input items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    /// A shorthand text input.
    Text(String),
    /// Fully typed input items.
    Items(Vec<ResponseInputItem>),
}

impl ResponseInput {
    /// Creates a shorthand text input.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Creates input from typed input items.
    #[must_use]
    pub fn items(items: impl IntoIterator<Item = impl Into<ResponseInputItem>>) -> Self {
        Self::Items(items.into_iter().map(Into::into).collect())
    }
}

impl From<String> for ResponseInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ResponseInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<ResponseInputItem>> for ResponseInput {
    fn from(value: Vec<ResponseInputItem>) -> Self {
        Self::Items(value)
    }
}

/// Instructions may use the same string or item representation as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInstructions {
    /// Plain instruction text.
    Text(String),
    /// Typed instruction items.
    Items(Vec<ResponseInputItem>),
}

impl From<String> for ResponseInstructions {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ResponseInstructions {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<ResponseInputItem>> for ResponseInstructions {
    fn from(value: Vec<ResponseInputItem>) -> Self {
        Self::Items(value)
    }
}

literal_tag!(FunctionToolTag, Function, "function");
literal_tag!(McpToolTag, Mcp, "mcp");
literal_tag!(FunctionCallTag, FunctionCall, "function_call");
literal_tag!(McpListToolsTag, McpListTools, "mcp_list_tools");
literal_tag!(McpCallTag, McpCall, "mcp_call");
literal_tag!(
    McpApprovalRequestTag,
    McpApprovalRequest,
    "mcp_approval_request"
);

/// A function tool available to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionTool {
    #[serde(rename = "type")]
    kind: FunctionToolTag,
    name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parameters: Omittable<Nullable<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_schema: Omittable<Nullable<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    strict: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    defer_loading: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
}

impl FunctionTool {
    /// Creates a strict function tool from `T`'s `schemars` JSON Schema definition.
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
        Ok(Self::new(name)
            .description(description)
            .parameters(schema)
            .strict(true))
    }

    /// Creates a function tool with a permissive empty object schema.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let mut parameters = Map::new();
        parameters.insert("type".to_owned(), Value::String("object".to_owned()));
        parameters.insert("properties".to_owned(), Value::Object(Map::new()));
        Self {
            kind: FunctionToolTag::Function,
            name: name.into(),
            description: Omittable::Omitted,
            parameters: Omittable::Value(Nullable::Value(Value::Object(parameters))),
            output_schema: Omittable::Omitted,
            strict: Omittable::Omitted,
            defer_loading: Omittable::Omitted,
            allowed_callers: Omittable::Omitted,
        }
    }

    /// Sets the human-readable tool description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(Nullable::Value(description.into()));
        self
    }

    /// Sets an already constructed JSON Schema value.
    #[must_use]
    pub fn parameters(mut self, parameters: Value) -> Self {
        self.parameters = Omittable::Value(Nullable::Value(parameters));
        self
    }

    /// Sends explicit `parameters: null`.
    #[must_use]
    pub fn parameters_null(mut self) -> Self {
        self.parameters = Omittable::Value(Nullable::Null);
        self
    }

    /// Serializes a schema representation without requiring JSON text.
    pub fn parameters_from<T: Serialize>(
        mut self,
        parameters: &T,
    ) -> Result<Self, serde_json::Error> {
        self.parameters = Omittable::Value(Nullable::Value(serde_json::to_value(parameters)?));
        Ok(self)
    }

    /// Sets a typed output JSON Schema.
    pub fn output_schema_from<T: Serialize>(
        mut self,
        output_schema: &T,
    ) -> Result<Self, serde_json::Error> {
        self.output_schema =
            Omittable::Value(Nullable::Value(serde_json::to_value(output_schema)?));
        Ok(self)
    }

    /// Enables or disables strict schema adherence.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = Omittable::Value(Nullable::Value(strict));
        self
    }

    /// Marks the tool for deferred loading by compatible models.
    #[must_use]
    pub fn defer_loading(mut self, defer_loading: bool) -> Self {
        self.defer_loading = Omittable::Value(defer_loading);
        self
    }

    /// Restricts which invocation contexts may call this function.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Sends official `description: null`.
    #[must_use]
    pub fn description_null(mut self) -> Self {
        self.description = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `output_schema: null`.
    #[must_use]
    pub fn output_schema_null(mut self) -> Self {
        self.output_schema = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `strict: null`.
    #[must_use]
    pub fn strict_null(mut self) -> Self {
        self.strict = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the description when present.
    #[must_use]
    pub fn description_ref(&self) -> Option<&str> {
        match &self.description {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the parameters JSON Schema when present.
    #[must_use]
    pub const fn parameters_ref(&self) -> Option<&Value> {
        match &self.parameters {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_function_tool_name(&self.name)?;
        validate_allowed_callers(&self.allowed_callers)
    }

    /// Returns the explicit strict flag when present.
    #[must_use]
    pub fn is_strict(&self) -> Option<bool> {
        match self.strict {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

pub(crate) fn validate_websocket_stream_id(
    value: &str,
) -> Result<(), CreateResponseConstraintError> {
    let actual = value.chars().count();
    let charset_ok = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !(MIN_STREAM_ID_CHARS..=MAX_STREAM_ID_CHARS).contains(&actual) || !charset_ok {
        return Err(CreateResponseConstraintError::StreamId {
            actual,
            minimum: MIN_STREAM_ID_CHARS,
            maximum: MAX_STREAM_ID_CHARS,
        });
    }
    Ok(())
}

fn validate_input_file_data_chars(actual: usize) -> Result<(), CreateResponseConstraintError> {
    if actual > MAX_INPUT_FILE_DATA_CHARS {
        return Err(CreateResponseConstraintError::InputFileData {
            actual,
            maximum: MAX_INPUT_FILE_DATA_CHARS,
        });
    }
    Ok(())
}

fn validate_inline_skill_source_data_chars(
    actual: usize,
) -> Result<(), CreateResponseConstraintError> {
    if !(MIN_INLINE_SKILL_SOURCE_DATA_CHARS..=MAX_INLINE_SKILL_SOURCE_DATA_CHARS).contains(&actual)
    {
        return Err(CreateResponseConstraintError::InlineSkillSourceData {
            actual,
            minimum: MIN_INLINE_SKILL_SOURCE_DATA_CHARS,
            maximum: MAX_INLINE_SKILL_SOURCE_DATA_CHARS,
        });
    }
    Ok(())
}

pub(crate) fn validate_input_text_chars(
    actual: usize,
) -> Result<(), CreateResponseConstraintError> {
    if actual > MAX_INPUT_TEXT_CHARS {
        return Err(CreateResponseConstraintError::InputText {
            actual,
            maximum: MAX_INPUT_TEXT_CHARS,
        });
    }
    Ok(())
}

pub(crate) fn validate_input_image_url_chars(
    actual: usize,
) -> Result<(), CreateResponseConstraintError> {
    if actual > MAX_INPUT_IMAGE_URL_CHARS {
        return Err(CreateResponseConstraintError::InputImageUrl {
            actual,
            maximum: MAX_INPUT_IMAGE_URL_CHARS,
        });
    }
    Ok(())
}

fn validate_apply_patch_path_chars(actual: usize) -> Result<(), CreateResponseConstraintError> {
    if actual < MIN_APPLY_PATCH_PATH_CHARS {
        return Err(CreateResponseConstraintError::ApplyPatchPath {
            actual,
            minimum: MIN_APPLY_PATCH_PATH_CHARS,
        });
    }
    Ok(())
}

fn validate_apply_patch_diff_chars(actual: usize) -> Result<(), CreateResponseConstraintError> {
    if actual > MAX_APPLY_PATCH_DIFF_CHARS {
        return Err(CreateResponseConstraintError::ApplyPatchDiff {
            actual,
            maximum: MAX_APPLY_PATCH_DIFF_CHARS,
        });
    }
    Ok(())
}

pub(crate) fn validate_input_content(
    content: &InputContent,
) -> Result<(), CreateResponseConstraintError> {
    match content {
        InputContent::File(file) => file.validate()?,
        InputContent::Image(image) => image.validate()?,
        InputContent::Text(text) => text.validate()?,
        InputContent::ComputerScreenshot(_) | InputContent::Unknown(_) => {}
    }
    Ok(())
}

pub(crate) fn validate_response_input_item(
    item: &ResponseInputItem,
) -> Result<(), CreateResponseConstraintError> {
    match item {
        ResponseInputItem::Message(item) => item.validate()?,
        ResponseInputItem::StoredMessage(item) => item.validate()?,
        ResponseInputItem::FunctionShellCall(item) => item.validate()?,
        ResponseInputItem::FunctionShellCallOutput(item) => item.validate()?,
        ResponseInputItem::FunctionCallOutput(item) => item.validate()?,
        ResponseInputItem::ComputerCallOutput(item) => item.validate()?,
        ResponseInputItem::ToolSearchCall(item) => item.validate()?,
        ResponseInputItem::ToolSearchOutput(item) => item.validate()?,
        ResponseInputItem::ApplyPatchCall(item) => item.validate()?,
        ResponseInputItem::ApplyPatchCallOutput(item) => item.validate()?,
        ResponseInputItem::Compaction(item) => item.validate()?,
        ResponseInputItem::Program(item) => item.validate()?,
        ResponseInputItem::ProgramOutput(item) => item.validate()?,
        ResponseInputItem::FunctionCall(item) => item.validate()?,
        ResponseInputItem::CustomToolCall(item) => item.validate()?,
        ResponseInputItem::CustomToolCallOutput(item) => item.validate()?,
        ResponseInputItem::AdditionalTools(item) => item.validate()?,
        _ => {}
    }
    Ok(())
}

pub(crate) fn validate_response_tool(
    tool: &ResponseTool,
) -> Result<(), CreateResponseConstraintError> {
    match tool {
        ResponseTool::Function(tool) => tool.validate(),
        ResponseTool::FileSearch(tool) => tool.validate(),
        ResponseTool::ImageGeneration(tool) => tool.validate(),
        ResponseTool::Mcp(tool) => tool.validate(),
        ResponseTool::CodeInterpreter(tool) => tool.validate(),
        ResponseTool::FunctionShell(tool) => tool.validate(),
        ResponseTool::Custom(tool) => tool.validate(),
        ResponseTool::Namespace(tool) => tool.validate(),
        ResponseTool::ApplyPatch(tool) => tool.validate(),
        _ => Ok(()),
    }
}

pub(crate) fn validate_response_tools(
    tools: &[ResponseTool],
) -> Result<(), CreateResponseConstraintError> {
    for tool in tools {
        validate_response_tool(tool)?;
    }
    Ok(())
}

fn validate_input_contents(parts: &[InputContent]) -> Result<(), CreateResponseConstraintError> {
    for part in parts {
        validate_input_content(part)?;
    }
    Ok(())
}

fn validate_easy_input_contents(
    parts: &[EasyInputContent],
) -> Result<(), CreateResponseConstraintError> {
    for part in parts {
        match part {
            EasyInputContent::File(file) => file.validate()?,
            EasyInputContent::Image(image) => image.validate()?,
            EasyInputContent::Text(text) => text.validate()?,
            EasyInputContent::Unknown(_) => {}
        }
    }
    Ok(())
}

fn validate_message_content(content: &MessageContent) -> Result<(), CreateResponseConstraintError> {
    if let MessageContent::Parts(parts) = content {
        validate_easy_input_contents(parts)?;
    }
    Ok(())
}

fn validate_function_call_output_value(
    output: &FunctionCallOutputValue,
) -> Result<(), CreateResponseConstraintError> {
    match output {
        FunctionCallOutputValue::Text(output) => {
            let actual = output.chars().count();
            if actual > MAX_FUNCTION_CALL_OUTPUT_CHARS {
                return Err(CreateResponseConstraintError::FunctionCallOutputChars {
                    actual,
                    maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
                });
            }
        }
        FunctionCallOutputValue::Content(parts) => validate_easy_input_contents(parts)?,
    }
    Ok(())
}

fn validate_function_call_output_param_value(
    output: &FunctionCallOutputParamValue,
) -> Result<(), CreateResponseConstraintError> {
    match output {
        FunctionCallOutputParamValue::Text(output) => {
            let actual = output.chars().count();
            if actual > MAX_FUNCTION_CALL_OUTPUT_CHARS {
                return Err(CreateResponseConstraintError::FunctionCallOutputChars {
                    actual,
                    maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
                });
            }
        }
        FunctionCallOutputParamValue::Content(parts) => {
            for part in parts {
                match part {
                    FunctionCallOutputContent::Text(text) => text.validate()?,
                    FunctionCallOutputContent::Image(image) => image.validate()?,
                    FunctionCallOutputContent::File(file) => file.validate()?,
                    FunctionCallOutputContent::Unknown(_) => {}
                }
            }
        }
    }
    Ok(())
}

fn validate_function_shell_call_id(call_id: &str) -> Result<(), CreateResponseConstraintError> {
    let actual = call_id.chars().count();
    if !(MIN_FUNCTION_SHELL_CALL_ID_CHARS..=MAX_FUNCTION_SHELL_CALL_ID_CHARS).contains(&actual) {
        return Err(CreateResponseConstraintError::FunctionShellCallId {
            actual,
            minimum: MIN_FUNCTION_SHELL_CALL_ID_CHARS,
            maximum: MAX_FUNCTION_SHELL_CALL_ID_CHARS,
        });
    }
    Ok(())
}

fn validate_call_id(call_id: &str) -> Result<(), CreateResponseConstraintError> {
    let actual = call_id.chars().count();
    if !(MIN_FUNCTION_SHELL_CALL_ID_CHARS..=MAX_FUNCTION_SHELL_CALL_ID_CHARS).contains(&actual) {
        return Err(CreateResponseConstraintError::CallId {
            actual,
            minimum: MIN_FUNCTION_SHELL_CALL_ID_CHARS,
            maximum: MAX_FUNCTION_SHELL_CALL_ID_CHARS,
        });
    }
    Ok(())
}

fn validate_omittable_caller(
    caller: &Omittable<Nullable<ToolCallCaller>>,
) -> Result<(), CreateResponseConstraintError> {
    if let Omittable::Value(Nullable::Value(ToolCallCaller::Program(program))) = caller {
        program.validate()?;
    }
    Ok(())
}

fn validate_omittable_call_id(
    call_id: &Omittable<Nullable<String>>,
) -> Result<(), CreateResponseConstraintError> {
    if let Omittable::Value(Nullable::Value(call_id)) = call_id {
        validate_call_id(call_id)?;
    }
    Ok(())
}

fn validate_function_tool_name(name: &str) -> Result<(), CreateResponseConstraintError> {
    let actual = name.chars().count();
    let charset_ok = name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if !(MIN_FUNCTION_TOOL_NAME_CHARS..=MAX_FUNCTION_TOOL_NAME_CHARS).contains(&actual)
        || !charset_ok
    {
        return Err(CreateResponseConstraintError::FunctionToolName {
            actual,
            minimum: MIN_FUNCTION_TOOL_NAME_CHARS,
            maximum: MAX_FUNCTION_TOOL_NAME_CHARS,
        });
    }
    Ok(())
}

fn validate_allowed_callers(
    allowed_callers: &Omittable<Nullable<Vec<AllowedCaller>>>,
) -> Result<(), CreateResponseConstraintError> {
    if let Omittable::Value(Nullable::Value(callers)) = allowed_callers
        && callers.is_empty()
    {
        return Err(CreateResponseConstraintError::EmptyAllowedCallers);
    }
    Ok(())
}

fn is_valid_mcp_tunnel_id(tunnel_id: &str) -> bool {
    let Some(rest) = tunnel_id.strip_prefix("tunnel_") else {
        return false;
    };
    rest.len() == 32
        && rest
            .bytes()
            .all(|byte: u8| byte.is_ascii_digit() || byte.is_ascii_lowercase())
}

/// Restricts an MCP tool set by name and/or read-only annotation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpToolFilter {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    read_only: Omittable<bool>,
}

impl McpToolFilter {
    /// Creates a filter for the supplied tool names.
    #[must_use]
    pub fn names(tool_names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            tool_names: tool_names.into_iter().map(Into::into).collect(),
            read_only: Omittable::Omitted,
        }
    }

    /// Restricts matches by the MCP read-only annotation.
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = Omittable::Value(read_only);
        self
    }

    /// Returns the selected tool names.
    #[must_use]
    pub fn tool_names(&self) -> &[String] {
        &self.tool_names
    }
}

/// Allowed MCP tools expressed as names or a structured filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpAllowedTools {
    /// Exact tool names.
    Names(Vec<String>),
    /// A structured filter.
    Filter(McpToolFilter),
}

/// Tool-name filters for conditional MCP approval.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpApprovalFilter {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    always: Omittable<McpToolFilter>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    never: Omittable<McpToolFilter>,
}

impl McpApprovalFilter {
    /// Requires approval for tools matching `filter`.
    #[must_use]
    pub fn always(filter: McpToolFilter) -> Self {
        Self {
            always: Omittable::Value(filter),
            never: Omittable::Omitted,
        }
    }

    /// Skips approval for tools matching `filter`.
    #[must_use]
    pub fn never(filter: McpToolFilter) -> Self {
        Self {
            always: Omittable::Omitted,
            never: Omittable::Value(filter),
        }
    }
}

/// Approval policy for a native remote MCP server.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum McpRequireApproval {
    /// Require approval for every call.
    Always,
    /// Do not require approval.
    Never,
    /// Use per-tool filters.
    Filter(McpApprovalFilter),
    /// A future string policy retained verbatim.
    Unknown(Box<str>),
}

impl Serialize for McpRequireApproval {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Always => serializer.serialize_str("always"),
            Self::Never => serializer.serialize_str("never"),
            Self::Unknown(value) => serializer.serialize_str(value),
            Self::Filter(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for McpRequireApproval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(value) => Ok(match value.as_str() {
                "always" => Self::Always,
                "never" => Self::Never,
                _ => Self::Unknown(value.into_boxed_str()),
            }),
            Value::Object(_) => serde_json::from_value(value)
                .map(Self::Filter)
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom(
                "MCP approval policy must be a string or object",
            )),
        }
    }
}

/// A native remote MCP server exposed to the OpenAI model.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    #[serde(rename = "type")]
    kind: McpToolTag,
    server_label: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    server_description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    server_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    connector_id: Omittable<McpConnectorId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tunnel_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    authorization: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    headers: Omittable<Nullable<BTreeMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_tools: Omittable<Nullable<McpAllowedTools>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    require_approval: Omittable<Nullable<McpRequireApproval>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    defer_loading: Omittable<bool>,
}

impl McpTool {
    fn empty(server_label: impl Into<String>) -> Self {
        Self {
            kind: McpToolTag::Mcp,
            server_label: server_label.into(),
            server_description: Omittable::Omitted,
            server_url: Omittable::Omitted,
            connector_id: Omittable::Omitted,
            tunnel_id: Omittable::Omitted,
            authorization: Omittable::Omitted,
            headers: Omittable::Omitted,
            allowed_tools: Omittable::Omitted,
            allowed_callers: Omittable::Omitted,
            require_approval: Omittable::Omitted,
            defer_loading: Omittable::Omitted,
        }
    }

    /// Creates a remote MCP tool backed by a server URL.
    #[must_use]
    pub fn remote(server_label: impl Into<String>, server_url: impl Into<String>) -> Self {
        let mut value = Self::empty(server_label);
        value.server_url = Omittable::Value(server_url.into());
        value
    }

    /// Creates an MCP tool backed by an OpenAI connector id.
    #[must_use]
    pub fn connector(
        server_label: impl Into<String>,
        connector_id: impl Into<McpConnectorId>,
    ) -> Self {
        let mut value = Self::empty(server_label);
        value.connector_id = Omittable::Value(connector_id.into());
        value
    }

    /// Creates an MCP tool backed by a secure tunnel id.
    #[must_use]
    pub fn tunnel(server_label: impl Into<String>, tunnel_id: impl Into<String>) -> Self {
        let mut value = Self::empty(server_label);
        value.tunnel_id = Omittable::Value(tunnel_id.into());
        value
    }

    /// Sets a server description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.server_description = Omittable::Value(description.into());
        self
    }

    /// Sets the server authorization value. Debug output remains redacted.
    #[must_use]
    pub fn authorization(mut self, authorization: impl Into<String>) -> Self {
        self.authorization = Omittable::Value(authorization.into());
        self
    }

    /// Adds a request header. Debug output never prints header values.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let mut headers = match std::mem::take(&mut self.headers) {
            Omittable::Value(Nullable::Value(headers)) => headers,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => BTreeMap::new(),
        };
        headers.insert(name.into(), value.into());
        self.headers = Omittable::Value(Nullable::Value(headers));
        self
    }

    /// Restricts tools made visible to the model.
    #[must_use]
    pub fn allowed_tools(mut self, allowed_tools: McpAllowedTools) -> Self {
        self.allowed_tools = Omittable::Value(Nullable::Value(allowed_tools));
        self
    }

    /// Restricts which invocation contexts may call this MCP tool.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Sets the approval policy.
    #[must_use]
    pub fn require_approval(mut self, policy: McpRequireApproval) -> Self {
        self.require_approval = Omittable::Value(Nullable::Value(policy));
        self
    }

    /// Sends `headers: null`.
    #[must_use]
    pub fn headers_null(mut self) -> Self {
        self.headers = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `allowed_tools: null`.
    #[must_use]
    pub fn allowed_tools_null(mut self) -> Self {
        self.allowed_tools = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `require_approval: null`.
    #[must_use]
    pub fn require_approval_null(mut self) -> Self {
        self.require_approval = Omittable::Value(Nullable::Null);
        self
    }

    /// Controls deferred loading for compatible models.
    #[must_use]
    pub fn defer_loading(mut self, defer_loading: bool) -> Self {
        self.defer_loading = Omittable::Value(defer_loading);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_allowed_callers(&self.allowed_callers)?;
        if let Omittable::Value(tunnel_id) = &self.tunnel_id
            && !is_valid_mcp_tunnel_id(tunnel_id)
        {
            return Err(CreateResponseConstraintError::McpTunnelId);
        }
        Ok(())
    }

    /// Returns the label used to correlate MCP call items.
    #[must_use]
    pub fn server_label(&self) -> &str {
        &self.server_label
    }
}

impl fmt::Debug for McpTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpTool")
            .field("server_label", &self.server_label)
            .field("server_description", &self.server_description)
            .field("server_url", &self.server_url)
            .field("connector_id", &self.connector_id)
            .field("tunnel_id", &self.tunnel_id)
            .field("authorization", &"[REDACTED]")
            .field(
                "header_count",
                &match &self.headers {
                    Omittable::Value(Nullable::Value(headers)) => headers.len(),
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => 0,
                },
            )
            .field("allowed_tools", &self.allowed_tools)
            .field("allowed_callers", &self.allowed_callers)
            .field("require_approval", &self.require_approval)
            .field("defer_loading", &self.defer_loading)
            .finish()
    }
}

tagged_union! {
    /// A tool definition accepted by Responses create.
    pub enum ResponseTool {
        Function(FunctionTool) => "function",
        FileSearch(FileSearchTool) => "file_search",
        Computer(ComputerTool) => "computer",
        ComputerUsePreview(ComputerUsePreviewTool) => "computer_use_preview",
        WebSearch(WebSearchTool) => "web_search" | "web_search_2025_08_26",
        Mcp(McpTool) => "mcp",
        CodeInterpreter(CodeInterpreterTool) => "code_interpreter",
        Programmatic(ProgrammaticTool) => "programmatic_tool_calling",
        ImageGeneration(ImageGenerationTool) => "image_generation",
        LocalShell(LocalShellTool) => "local_shell",
        FunctionShell(FunctionShellTool) => "shell",
        Custom(CustomTool) => "custom",
        Namespace(NamespaceTool) => "namespace",
        ToolSearch(ToolSearchTool) => "tool_search",
        WebSearchPreview(WebSearchPreviewTool) => "web_search_preview" | "web_search_preview_2025_03_11",
        ApplyPatch(ApplyPatchTool) => "apply_patch"
    }
}

impl From<FunctionTool> for ResponseTool {
    fn from(value: FunctionTool) -> Self {
        Self::Function(value)
    }
}

impl From<FileSearchTool> for ResponseTool {
    fn from(value: FileSearchTool) -> Self {
        Self::FileSearch(value)
    }
}

impl From<WebSearchTool> for ResponseTool {
    fn from(value: WebSearchTool) -> Self {
        Self::WebSearch(value)
    }
}

impl From<ImageGenerationTool> for ResponseTool {
    fn from(value: ImageGenerationTool) -> Self {
        Self::ImageGeneration(value)
    }
}

impl From<FunctionShellTool> for ResponseTool {
    fn from(value: FunctionShellTool) -> Self {
        Self::FunctionShell(value)
    }
}

impl From<ToolSearchTool> for ResponseTool {
    fn from(value: ToolSearchTool) -> Self {
        Self::ToolSearch(value)
    }
}

impl From<ApplyPatchTool> for ResponseTool {
    fn from(value: ApplyPatchTool) -> Self {
        Self::ApplyPatch(value)
    }
}

impl From<CustomTool> for ResponseTool {
    fn from(value: CustomTool) -> Self {
        Self::Custom(value)
    }
}

impl From<CodeInterpreterTool> for ResponseTool {
    fn from(value: CodeInterpreterTool) -> Self {
        Self::CodeInterpreter(value)
    }
}

impl From<McpTool> for ResponseTool {
    fn from(value: McpTool) -> Self {
        Self::Mcp(value)
    }
}

literal_tag!(DirectToolCallCallerTag, Direct, "direct");
literal_tag!(ProgramToolCallCallerTag, Program, "program");

/// A tool call produced by the model itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectToolCallCaller {
    #[serde(rename = "type")]
    kind: DirectToolCallCallerTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl DirectToolCallCaller {
    /// Creates a direct caller.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: DirectToolCallCallerTag::Direct,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for DirectToolCallCaller {
    fn default() -> Self {
        Self::new()
    }
}

/// A tool call produced by a programmatic caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramToolCallCaller {
    #[serde(rename = "type")]
    kind: ProgramToolCallCallerTag,
    caller_id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ProgramToolCallCaller {
    /// Creates a programmatic caller.
    #[must_use]
    pub fn new(caller_id: impl Into<String>) -> Self {
        Self {
            kind: ProgramToolCallCallerTag::Program,
            caller_id: caller_id.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the producing program call id.
    #[must_use]
    pub fn caller_id(&self) -> &str {
        &self.caller_id
    }

    /// Checks pinned `caller_id` `1..=64`.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.caller_id)
    }
}

/// Execution context that produced a tool call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToolCallCaller {
    /// Invoked directly by the model.
    Direct(DirectToolCallCaller),
    /// Invoked by a programmatic tool call.
    Program(ProgramToolCallCaller),
    /// Future caller retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl ToolCallCaller {
    /// Creates a direct caller.
    #[must_use]
    pub fn direct() -> Self {
        Self::Direct(DirectToolCallCaller::new())
    }

    /// Creates a programmatic caller.
    #[must_use]
    pub fn program(caller_id: impl Into<String>) -> Self {
        Self::Program(ProgramToolCallCaller::new(caller_id))
    }
}

impl Serialize for ToolCallCaller {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Direct(value) => value.serialize(serializer),
            Self::Program(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolCallCaller {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "direct" => serde_json::from_value(value)
                .map(Self::Direct)
                .map_err(D::Error::custom),
            "program" => serde_json::from_value(value)
                .map(Self::Program)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A model-produced function invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    #[serde(rename = "type")]
    kind: FunctionCallTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<String>,
    call_id: String,
    name: String,
    arguments: JsonText,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    namespace: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseItemStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCall {
    /// Creates a complete function call item.
    ///
    /// `status` still takes the shared open [`ResponseItemStatus`] because
    /// in-crate structured-output and RMCP adapters replay decoded statuses
    /// verbatim; the pinned construction domain is the narrower
    /// [`FunctionCallItemStatus`], accepted by [`Self::with_status`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: JsonText,
        status: FunctionCallItemStatus,
    ) -> Self {
        Self {
            kind: FunctionCallTag::FunctionCall,
            id: Omittable::Value(id.into()),
            call_id: call_id.into(),
            name: name.into(),
            arguments,
            namespace: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Value(status.into()),
            created_by: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a function-call input from the official required fields.
    #[must_use]
    pub fn call(call_id: impl Into<String>, name: impl Into<String>, arguments: JsonText) -> Self {
        Self {
            kind: FunctionCallTag::FunctionCall,
            id: Omittable::Omitted,
            call_id: call_id.into(),
            name: name.into(),
            arguments,
            namespace: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            created_by: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the stored item id when echoing a returned call.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(id.into());
        self
    }

    /// Sets the item status when echoing a returned call.
    ///
    /// The pinned `FunctionCallItemStatus` domain is the three message-trio
    /// values; tool-call-only states such as `searching` stay
    /// construction-invalid.
    #[must_use]
    pub fn with_status(mut self, status: FunctionCallItemStatus) -> Self {
        self.status = Omittable::Value(status.into());
        self
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match &self.id {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the call id used by the matching output item.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the lazily parsed JSON argument string.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }

    /// Parses the function arguments into a declared Rust type.
    pub fn arguments_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_str(self.arguments.as_str())
    }

    /// Returns the item status.
    #[must_use]
    pub fn status(&self) -> Option<&ResponseItemStatus> {
        match &self.status {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Sets the function namespace.
    #[must_use]
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Omittable::Value(namespace.into());
        self
    }

    /// Sets the execution context that produced this call.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned program-caller `caller_id` `1..=64`.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_omittable_caller(&self.caller)
    }

    /// Returns the execution context when present.
    #[must_use]
    pub const fn caller_ref(&self) -> Option<&ToolCallCaller> {
        match &self.caller {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// String or rich content supplied as a function or custom tool call output.
///
/// Content parts follow the pinned three-branch
/// `FunctionAndCustomToolCallOutput` union (text/image/file) modeled as
/// [`EasyInputContent`]; `computer_screenshot` is not a legal tool-output
/// part and decodes as the open `Unknown` retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FunctionCallOutputValue {
    /// An opaque text value, commonly a JSON string.
    Text(String),
    /// Typed text/image/file content parts.
    Content(Vec<EasyInputContent>),
}

impl From<String> for FunctionCallOutputValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for FunctionCallOutputValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<EasyInputContent>> for FunctionCallOutputValue {
    fn from(value: Vec<EasyInputContent>) -> Self {
        Self::Content(value)
    }
}

tagged_union! {
    /// One official `FunctionCallOutputItemParam` output content part.
    ///
    /// Members match the pinned three-branch request union (text/image/file)
    /// and the response-side `FunctionAndCustomToolCallOutput` exactly;
    /// `computer_screenshot` is not a legal function-output part and decodes
    /// as the open [`EasyInputContent`]-style `Unknown` retention instead.
    pub enum FunctionCallOutputContent {
        Text(InputText) => "input_text",
        Image(InputImageParam) => "input_image",
        File(InputFile) => "input_file"
    }
}

impl From<InputText> for FunctionCallOutputContent {
    fn from(value: InputText) -> Self {
        Self::Text(value)
    }
}

impl From<InputImageParam> for FunctionCallOutputContent {
    fn from(value: InputImageParam) -> Self {
        Self::Image(value)
    }
}

impl From<InputFile> for FunctionCallOutputContent {
    fn from(value: InputFile) -> Self {
        Self::File(value)
    }
}

impl From<EasyInputContent> for FunctionCallOutputContent {
    fn from(value: EasyInputContent) -> Self {
        match value {
            EasyInputContent::Text(text) => Self::Text(text),
            EasyInputContent::Image(image) => Self::Image(image.into()),
            EasyInputContent::File(file) => Self::File(file),
            EasyInputContent::Unknown(value) => Self::Unknown(value),
        }
    }
}

/// String or Param-shaped content for `FunctionCallOutputItemParam.output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FunctionCallOutputParamValue {
    /// An opaque text value, commonly a JSON string.
    Text(String),
    /// Typed text/image/file Param content parts.
    Content(Vec<FunctionCallOutputContent>),
}

impl From<String> for FunctionCallOutputParamValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for FunctionCallOutputParamValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<FunctionCallOutputContent>> for FunctionCallOutputParamValue {
    fn from(value: Vec<FunctionCallOutputContent>) -> Self {
        Self::Content(value)
    }
}

impl From<Vec<EasyInputContent>> for FunctionCallOutputParamValue {
    fn from(value: Vec<EasyInputContent>) -> Self {
        Self::Content(value.into_iter().map(Into::into).collect())
    }
}

impl From<FunctionCallOutputValue> for FunctionCallOutputParamValue {
    fn from(value: FunctionCallOutputValue) -> Self {
        match value {
            FunctionCallOutputValue::Text(text) => Self::Text(text),
            FunctionCallOutputValue::Content(parts) => parts.into(),
        }
    }
}

/// Output supplied for a preceding function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallOutput {
    #[serde(rename = "type")]
    kind: FunctionCallOutputTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<Nullable<String>>,
    output: FunctionCallOutputParamValue,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    namespace: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCallOutput {
    /// Creates a function output from an opaque string.
    #[must_use]
    pub fn new(
        call_id: impl Into<String>,
        output: impl Into<FunctionCallOutputParamValue>,
    ) -> Self {
        Self::from_output(output).with_call_id(call_id)
    }

    /// Creates a function-call output from the official required `output`.
    #[must_use]
    pub fn from_output(output: impl Into<FunctionCallOutputParamValue>) -> Self {
        Self {
            kind: FunctionCallOutputTag::FunctionCallOutput,
            call_id: Omittable::Omitted,
            output: output.into(),
            id: Omittable::Omitted,
            name: Omittable::Omitted,
            namespace: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Serializes a typed result into the output string.
    pub fn from_serializable<T: Serialize>(
        call_id: impl Into<String>,
        output: &T,
    ) -> Result<Self, serde_json::Error> {
        serde_json::to_string(output).map(|output| Self::new(call_id, output))
    }

    /// Serializes a typed result into JSON output for a function call.
    pub fn json<T: Serialize>(
        call_id: impl Into<String>,
        output: &T,
    ) -> Result<Self, serde_json::Error> {
        Self::from_serializable(call_id, output)
    }

    /// Sets the model-generated call id.
    #[must_use]
    pub fn with_call_id(mut self, call_id: impl Into<String>) -> Self {
        self.call_id = Omittable::Value(Nullable::Value(call_id.into()));
        self
    }

    /// Sets an item id for stored input items.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sets an item status for stored input items.
    ///
    /// The pinned `FunctionCallOutputStatusEnum` domain is the three
    /// message-trio values.
    #[must_use]
    pub fn status(mut self, status: FunctionCallItemStatus) -> Self {
        self.status = Omittable::Value(Nullable::Value(status.into()));
        self
    }

    /// Records the tool name that produced this output.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(Nullable::Value(name.into()));
        self
    }

    /// Sets the execution context that produced this output.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Records the namespace that produced this output.
    #[must_use]
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Omittable::Value(Nullable::Value(namespace.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `call_id: null`.
    #[must_use]
    pub fn call_id_null(mut self) -> Self {
        self.call_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `name: null`.
    #[must_use]
    pub fn name_null(mut self) -> Self {
        self.name = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `namespace: null`.
    #[must_use]
    pub fn namespace_null(mut self) -> Self {
        self.namespace = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_omittable_call_id(&self.call_id)?;
        if let Omittable::Value(Nullable::Value(name)) = &self.name {
            let actual = name.chars().count();
            if !(MIN_FUNCTION_CALL_OUTPUT_NAME_CHARS..=MAX_FUNCTION_CALL_OUTPUT_NAME_CHARS)
                .contains(&actual)
            {
                return Err(CreateResponseConstraintError::FunctionCallOutputName {
                    actual,
                    minimum: MIN_FUNCTION_CALL_OUTPUT_NAME_CHARS,
                    maximum: MAX_FUNCTION_CALL_OUTPUT_NAME_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(namespace)) = &self.namespace {
            let actual = namespace.chars().count();
            let charset_ok = namespace
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
            if !(MIN_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS
                ..=MAX_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS)
                .contains(&actual)
                || !charset_ok
            {
                return Err(CreateResponseConstraintError::FunctionCallOutputNamespace {
                    actual,
                    minimum: MIN_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS,
                    maximum: MAX_FUNCTION_CALL_OUTPUT_NAMESPACE_CHARS,
                });
            }
        }
        validate_function_call_output_param_value(&self.output)?;
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Returns the execution context when present.
    #[must_use]
    pub const fn caller_ref(&self) -> Option<&ToolCallCaller> {
        match &self.caller {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the related function call id.
    #[must_use]
    pub fn call_id(&self) -> Option<&str> {
        match &self.call_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the opaque output string.
    #[must_use]
    pub const fn output(&self) -> &FunctionCallOutputParamValue {
        &self.output
    }

    /// Parses a JSON output into a caller-selected type.
    pub fn deserialize_output<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<T, serde_json::Error> {
        match &self.output {
            FunctionCallOutputParamValue::Text(output) => serde_json::from_str(output),
            FunctionCallOutputParamValue::Content(output) => {
                serde_json::from_value(serde_json::to_value(output)?)
            }
        }
    }
}

/// One tool described by an `mcp_list_tools` output item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpListedTool {
    name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_schema: Omittable<Value>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    annotations: Omittable<Nullable<Value>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpListedTool {
    /// Creates a listed MCP tool from the official required fields.
    #[must_use]
    pub fn new(name: impl Into<String>, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            description: Omittable::Omitted,
            input_schema: Omittable::Value(input_schema),
            annotations: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the tool description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(Nullable::Value(description.into()));
        self
    }

    /// Sends official `description: null`.
    #[must_use]
    pub fn description_null(mut self) -> Self {
        self.description = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets additional tool annotations.
    #[must_use]
    pub fn annotations(mut self, annotations: Value) -> Self {
        self.annotations = Omittable::Value(Nullable::Value(annotations));
        self
    }

    /// Sends official `annotations: null`.
    #[must_use]
    pub fn annotations_null(mut self) -> Self {
        self.annotations = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the MCP tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the input schema when provided.
    #[must_use]
    pub fn input_schema(&self) -> Option<&Value> {
        match &self.input_schema {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }
}

/// The service's result of listing a native remote MCP server's tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpListTools {
    #[serde(rename = "type")]
    kind: McpListToolsTag,
    id: String,
    server_label: String,
    tools: Vec<McpListedTool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    error: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpListTools {
    /// Creates a list-tools item from the official required fields.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        server_label: impl Into<String>,
        tools: impl IntoIterator<Item = McpListedTool>,
    ) -> Self {
        Self {
            kind: McpListToolsTag::McpListTools,
            id: id.into(),
            server_label: server_label.into(),
            tools: tools.into_iter().collect(),
            error: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Records a list-tools error message.
    #[must_use]
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Omittable::Value(Nullable::Value(error.into()));
        self
    }

    /// Sends official `error: null`.
    #[must_use]
    pub fn error_null(mut self) -> Self {
        self.error = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the server label.
    #[must_use]
    pub fn server_label(&self) -> &str {
        &self.server_label
    }

    /// Returns the discovered tool definitions.
    #[must_use]
    pub fn tools(&self) -> &[McpListedTool] {
        &self.tools
    }

    /// Returns the list-tools error message when present and non-null.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match &self.error {
            Omittable::Value(Nullable::Value(error)) => Some(error),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

literal_tag!(McpProtocolErrorTag, McpProtocolError, "mcp_protocol_error");
literal_tag!(
    McpToolExecutionErrorTag,
    McpToolExecutionError,
    "mcp_tool_execution_error"
);
literal_tag!(McpHttpErrorTag, HttpError, "http_error");

/// An MCP protocol-level error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpProtocolError {
    #[serde(rename = "type")]
    kind: McpProtocolErrorTag,
    code: i64,
    message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpProtocolError {
    /// Creates a protocol error.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            kind: McpProtocolErrorTag::McpProtocolError,
            code,
            message: message.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the protocol error code.
    #[must_use]
    pub const fn code(&self) -> i64 {
        self.code
    }

    /// Returns the protocol error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// An error raised while executing an MCP tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolExecutionError {
    #[serde(rename = "type")]
    kind: McpToolExecutionErrorTag,
    content: Value,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpToolExecutionError {
    /// Creates an execution error.
    #[must_use]
    pub fn new(content: impl Into<Value>) -> Self {
        Self {
            kind: McpToolExecutionErrorTag::McpToolExecutionError,
            content: content.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the execution error content.
    #[must_use]
    pub const fn content(&self) -> &Value {
        &self.content
    }
}

/// An HTTP error from an MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpHttpError {
    #[serde(rename = "type")]
    kind: McpHttpErrorTag,
    code: i64,
    message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpHttpError {
    /// Creates an HTTP error.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            kind: McpHttpErrorTag::HttpError,
            code,
            message: message.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the HTTP status code.
    #[must_use]
    pub const fn code(&self) -> i64 {
        self.code
    }

    /// Returns the HTTP error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Official MCP tool-call error union.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum McpCallError {
    /// MCP protocol error.
    Protocol(McpProtocolError),
    /// Tool execution error.
    Execution(McpToolExecutionError),
    /// HTTP transport error.
    Http(McpHttpError),
    /// Future error retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for McpCallError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Protocol(value) => value.serialize(serializer),
            Self::Execution(value) => value.serialize(serializer),
            Self::Http(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for McpCallError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "mcp_protocol_error" => serde_json::from_value(value)
                .map(Self::Protocol)
                .map_err(D::Error::custom),
            "mcp_tool_execution_error" => serde_json::from_value(value)
                .map(Self::Execution)
                .map_err(D::Error::custom),
            "http_error" => serde_json::from_value(value)
                .map(Self::Http)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A native remote MCP tool invocation produced by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCall {
    #[serde(rename = "type")]
    kind: McpCallTag,
    id: String,
    server_label: String,
    name: String,
    arguments: JsonText,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseItemStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    approval_request_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    error: Omittable<Nullable<McpCallError>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpCall {
    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the server label.
    #[must_use]
    pub fn server_label(&self) -> &str {
        &self.server_label
    }

    /// Returns the MCP tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the lazily parsed argument string.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }

    /// Returns the item status when present.
    #[must_use]
    pub fn status(&self) -> Option<&ResponseItemStatus> {
        match &self.status {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the structured MCP error when present.
    #[must_use]
    pub const fn error(&self) -> Option<&McpCallError> {
        match &self.error {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Creates an MCP tool-call input from the official required fields.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        server_label: impl Into<String>,
        name: impl Into<String>,
        arguments: JsonText,
    ) -> Self {
        Self {
            kind: McpCallTag::McpCall,
            id: id.into(),
            server_label: server_label.into(),
            name: name.into(),
            arguments,
            status: Omittable::Omitted,
            approval_request_id: Omittable::Omitted,
            output: Omittable::Omitted,
            error: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the item status when echoing a returned call.
    ///
    /// The pinned `MCPToolCallStatus` domain adds `calling` / `failed` on
    /// top of the message trio.
    #[must_use]
    pub fn with_status(mut self, status: McpToolCallStatus) -> Self {
        self.status = Omittable::Value(status.into());
        self
    }

    /// Records the matching approval-request id.
    #[must_use]
    pub fn approval_request_id(mut self, approval_request_id: impl Into<String>) -> Self {
        self.approval_request_id = Omittable::Value(Nullable::Value(approval_request_id.into()));
        self
    }

    /// Sends official `approval_request_id: null`.
    #[must_use]
    pub fn approval_request_id_null(mut self) -> Self {
        self.approval_request_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Records the tool output string.
    #[must_use]
    pub fn output(mut self, output: impl Into<String>) -> Self {
        self.output = Omittable::Value(Nullable::Value(output.into()));
        self
    }

    /// Sends official `output: null`.
    #[must_use]
    pub fn output_null(mut self) -> Self {
        self.output = Omittable::Value(Nullable::Null);
        self
    }

    /// Records a structured MCP error.
    #[must_use]
    pub fn with_error(mut self, error: McpCallError) -> Self {
        self.error = Omittable::Value(Nullable::Value(error));
        self
    }

    /// Sends official `error: null`.
    #[must_use]
    pub fn error_null(mut self) -> Self {
        self.error = Omittable::Value(Nullable::Null);
        self
    }
}

/// A service request for user approval before an MCP call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpApprovalRequest {
    #[serde(rename = "type")]
    kind: McpApprovalRequestTag,
    id: String,
    server_label: String,
    name: String,
    arguments: JsonText,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpApprovalRequest {
    /// Returns the approval request id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the server label.
    #[must_use]
    pub fn server_label(&self) -> &str {
        &self.server_label
    }

    /// Returns the MCP tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the lazily parsed call arguments.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }

    /// Creates an approval-request input from the official required fields.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        server_label: impl Into<String>,
        name: impl Into<String>,
        arguments: JsonText,
    ) -> Self {
        Self {
            kind: McpApprovalRequestTag::McpApprovalRequest,
            id: id.into(),
            server_label: server_label.into(),
            name: name.into(),
            arguments,
            extra: ExtraFields::new(),
        }
    }
}

/// A user decision for a native remote MCP approval request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpApprovalResponse {
    #[serde(rename = "type")]
    kind: McpApprovalResponseTag,
    approval_request_id: String,
    approve: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reason: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpApprovalResponse {
    /// Approves a pending MCP call.
    #[must_use]
    pub fn approve(approval_request_id: impl Into<String>) -> Self {
        Self {
            kind: McpApprovalResponseTag::McpApprovalResponse,
            approval_request_id: approval_request_id.into(),
            approve: true,
            id: Omittable::Omitted,
            reason: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Rejects a pending MCP call with an optional reason.
    #[must_use]
    pub fn reject(approval_request_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: McpApprovalResponseTag::McpApprovalResponse,
            approval_request_id: approval_request_id.into(),
            approve: false,
            id: Omittable::Omitted,
            reason: Omittable::Value(Nullable::Value(reason.into())),
            extra: ExtraFields::new(),
        }
    }

    /// Sets the approval-response item id when echoing a stored item.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends explicit null for `reason`.
    #[must_use]
    pub fn reason_null(mut self) -> Self {
        self.reason = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the related approval request id.
    #[must_use]
    pub fn approval_request_id(&self) -> &str {
        &self.approval_request_id
    }

    /// Returns whether the call was approved.
    #[must_use]
    pub const fn is_approved(&self) -> bool {
        self.approve
    }

    /// Returns the item id when present.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match &self.id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the decision reason when present.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match &self.reason {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

literal_tag!(OutputTextTag, OutputText, "output_text");
literal_tag!(RefusalTag, Refusal, "refusal");
literal_tag!(OutputMessageTag, Message, "message");
literal_tag!(AssistantRoleTag, Assistant, "assistant");
literal_tag!(FileCitationTag, FileCitation, "file_citation");
literal_tag!(UrlCitationTag, UrlCitation, "url_citation");
literal_tag!(
    ContainerFileCitationTag,
    ContainerFileCitation,
    "container_file_citation"
);
literal_tag!(FilePathTag, FilePath, "file_path");

/// A citation to an uploaded file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileCitation {
    #[serde(rename = "type")]
    kind: FileCitationTag,
    file_id: String,
    index: u64,
    filename: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// A citation to a web resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlCitation {
    #[serde(rename = "type")]
    kind: UrlCitationTag,
    url: String,
    start_index: u64,
    end_index: u64,
    title: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// A citation to a file inside a container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerFileCitation {
    #[serde(rename = "type")]
    kind: ContainerFileCitationTag,
    container_id: String,
    file_id: String,
    start_index: u64,
    end_index: u64,
    filename: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// A path to a file referenced in output text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePathAnnotation {
    #[serde(rename = "type")]
    kind: FilePathTag,
    file_id: String,
    index: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

tagged_union! {
    /// An annotation that applies to a span of output text.
    pub enum Annotation {
        FileCitation(FileCitation) => "file_citation",
        UrlCitation(UrlCitation) => "url_citation",
        ContainerFileCitation(ContainerFileCitation) => "container_file_citation",
        FilePath(FilePathAnnotation) => "file_path"
    }
}

/// One alternative token at a logprob position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopLogProb {
    token: String,
    logprob: f64,
    bytes: Vec<i64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl TopLogProb {
    /// Returns the token string.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the token log probability.
    #[must_use]
    pub const fn logprob(&self) -> f64 {
        self.logprob
    }
}

/// Alternate token in an event-shaped logprob list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventTopLogProb {
    token: String,
    logprob: f64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EventTopLogProb {
    /// Returns the token string.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the token log probability.
    #[must_use]
    pub const fn logprob(&self) -> f64 {
        self.logprob
    }
}

/// Event-shaped log probability. Unlike [`LogProb`], `bytes` is absent and
/// `top_logprobs` is optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventLogProb {
    token: String,
    logprob: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    top_logprobs: Vec<EventTopLogProb>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EventLogProb {
    /// Returns the token string.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the token log probability.
    #[must_use]
    pub const fn logprob(&self) -> f64 {
        self.logprob
    }

    /// Returns alternative tokens at this position.
    #[must_use]
    pub fn top_logprobs(&self) -> &[EventTopLogProb] {
        &self.top_logprobs
    }
}

/// The log probability of one output token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogProb {
    token: String,
    logprob: f64,
    bytes: Vec<i64>,
    top_logprobs: Vec<TopLogProb>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LogProb {
    /// Returns the token string.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the token log probability.
    #[must_use]
    pub const fn logprob(&self) -> f64 {
        self.logprob
    }

    /// Returns alternative tokens at this position.
    #[must_use]
    pub fn top_logprobs(&self) -> &[TopLogProb] {
        &self.top_logprobs
    }
}

/// Text generated by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputText {
    #[serde(rename = "type")]
    kind: OutputTextTag,
    text: String,
    #[serde(default)]
    annotations: Vec<Annotation>,
    #[serde(default)]
    logprobs: Vec<LogProb>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputText {
    /// Creates an output text part, primarily for fixtures and adapters.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: OutputTextTag::OutputText,
            text: text.into(),
            annotations: Vec::new(),
            logprobs: Vec::new(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the generated text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns annotations in service order.
    #[must_use]
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Returns logprobs if included.
    #[must_use]
    pub fn logprobs(&self) -> &[LogProb] {
        &self.logprobs
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A safety refusal emitted instead of output text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    #[serde(rename = "type")]
    kind: RefusalTag,
    refusal: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Refusal {
    /// Creates a refusal part, primarily for fixtures and adapters.
    #[must_use]
    pub fn new(refusal: impl Into<String>) -> Self {
        Self {
            kind: RefusalTag::Refusal,
            refusal: refusal.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the refusal text.
    #[must_use]
    pub fn refusal(&self) -> &str {
        &self.refusal
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

tagged_union! {
    /// One content part in an assistant output message.
    pub enum OutputContent {
        Text(OutputText) => "output_text",
        Refusal(Refusal) => "refusal",
        ReasoningText(ReasoningTextContent) => "reasoning_text"
    }
}

impl From<OutputText> for OutputContent {
    fn from(value: OutputText) -> Self {
        Self::Text(value)
    }
}

impl From<Refusal> for OutputContent {
    fn from(value: Refusal) -> Self {
        Self::Refusal(value)
    }
}

impl From<ReasoningTextContent> for OutputContent {
    fn from(value: ReasoningTextContent) -> Self {
        Self::ReasoningText(value)
    }
}

/// A message produced by the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputMessage {
    #[serde(rename = "type")]
    kind: OutputMessageTag,
    id: String,
    status: ResponseItemStatus,
    role: AssistantRoleTag,
    content: Vec<OutputContent>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    phase: Omittable<Nullable<MessagePhase>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputMessage {
    /// Creates an assistant message, primarily for adapters and tests.
    ///
    /// `status` still takes the shared open [`ResponseItemStatus`] because
    /// in-crate conversation adapters replay decoded statuses verbatim; the
    /// pinned construction domain is the narrower [`MessageStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: MessageStatus,
        content: impl IntoIterator<Item = impl Into<OutputContent>>,
    ) -> Self {
        Self {
            kind: OutputMessageTag::Message,
            id: id.into(),
            status: status.into(),
            role: AssistantRoleTag::Assistant,
            content: content.into_iter().map(Into::into).collect(),
            phase: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Labels this assistant message as commentary or the final answer.
    #[must_use]
    pub fn phase(mut self, phase: impl Into<MessagePhase>) -> Self {
        self.phase = Omittable::Value(Nullable::Value(phase.into()));
        self
    }

    /// Explicitly sends official `phase: null`.
    #[must_use]
    pub fn phase_null(mut self) -> Self {
        self.phase = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the assistant message phase when present and non-null.
    #[must_use]
    pub fn phase_ref(&self) -> Option<&MessagePhase> {
        match &self.phase {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the message id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the item status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns output content in service order.
    #[must_use]
    pub fn content(&self) -> &[OutputContent] {
        &self.content
    }

    /// Returns all text parts in this message.
    pub fn text_parts(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|part| match part {
            OutputContent::Text(text) => Some(text.text()),
            OutputContent::Refusal(_)
            | OutputContent::ReasoningText(_)
            | OutputContent::Unknown(_) => None,
        })
    }

    /// Returns the refusal text if this message contains a refusal part.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.content.iter().find_map(|part| match part {
            OutputContent::Refusal(refusal) => Some(refusal.refusal()),
            _ => None,
        })
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A typed item accepted as Responses input.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
// Boxing the largest shell-call variants would be a breaking public-API
// refactor tracked separately from wire fixes.
#[allow(clippy::large_enum_variant)]
pub enum ResponseInputItem {
    /// A request message, whose `type` property may be omitted.
    Message(InputMessage),
    /// An item-form input message with array content.
    StoredMessage(StoredInputMessage),
    /// A prior assistant output message replayed as input.
    OutputMessage(OutputMessage),
    /// A prior function invocation replayed as input.
    FunctionCall(FunctionCall),
    /// Output for a prior function invocation.
    FunctionCallOutput(FunctionCallOutput),
    /// A prior MCP tool-list item replayed as input.
    McpListTools(McpListTools),
    /// A prior MCP call replayed as input.
    McpCall(McpCall),
    /// A prior MCP approval request replayed as input.
    McpApprovalRequest(McpApprovalRequest),
    /// A user's MCP approval decision.
    McpApprovalResponse(McpApprovalResponse),
    /// A compaction trigger control item.
    CompactionTrigger(CompactionTrigger),
    /// A reference to a stored item.
    ItemReference(ItemReference),
    /// A programmatic tool-calling program.
    Program(ProgramItem),
    /// Output from a programmatic tool-calling program.
    ProgramOutput(ProgramOutputItem),
    /// A file-search call replayed as input.
    FileSearchCall(FileSearchCall),
    /// A computer-use call replayed as input.
    ComputerCall(ComputerCall),
    /// Output for a computer-use call.
    ComputerCallOutput(ComputerCallOutput),
    /// A web-search call replayed as input.
    WebSearchCall(WebSearchCall),
    /// A tool-search request.
    ToolSearchCall(ToolSearchCallInput),
    /// Tools returned by tool search.
    ToolSearchOutput(ToolSearchOutputInput),
    /// Additional tools dynamically supplied to the model.
    AdditionalTools(AdditionalToolsInput),
    /// A reasoning item replayed as input.
    Reasoning(ReasoningItem),
    /// An encrypted compaction summary.
    Compaction(CompactionSummaryInput),
    /// An image-generation call replayed as input.
    ImageGenerationCall(ImageGenerationCall),
    /// A code-interpreter call replayed as input.
    CodeInterpreterCall(CodeInterpreterCall),
    /// A local-shell call replayed as input.
    LocalShellCall(LocalShellCall),
    /// Output from a local-shell call.
    LocalShellCallOutput(LocalShellCallOutput),
    /// A function-shell call.
    FunctionShellCall(FunctionShellCallInput),
    /// Output from a function-shell call.
    FunctionShellCallOutput(FunctionShellCallOutputInput),
    /// An apply-patch call.
    ApplyPatchCall(ApplyPatchCallInput),
    /// Output from an apply-patch call.
    ApplyPatchCallOutput(ApplyPatchCallOutputInput),
    /// Output from a custom tool call.
    CustomToolCallOutput(CustomToolCallOutput),
    /// A custom tool invocation.
    CustomToolCall(CustomToolCall),
    /// A future input item retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ResponseInputItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Message(value) => value.serialize(serializer),
            Self::StoredMessage(value) => value.serialize(serializer),
            Self::OutputMessage(value) => value.serialize(serializer),
            Self::FunctionCall(value) => value.serialize(serializer),
            Self::FunctionCallOutput(value) => value.serialize(serializer),
            Self::McpListTools(value) => value.serialize(serializer),
            Self::McpCall(value) => value.serialize(serializer),
            Self::McpApprovalRequest(value) => value.serialize(serializer),
            Self::McpApprovalResponse(value) => value.serialize(serializer),
            Self::CompactionTrigger(value) => value.serialize(serializer),
            Self::ItemReference(value) => value.serialize(serializer),
            Self::Program(value) => value.serialize(serializer),
            Self::ProgramOutput(value) => value.serialize(serializer),
            Self::FileSearchCall(value) => value.serialize(serializer),
            Self::ComputerCall(value) => value.serialize(serializer),
            Self::ComputerCallOutput(value) => value.serialize(serializer),
            Self::WebSearchCall(value) => value.serialize(serializer),
            Self::ToolSearchCall(value) => value.serialize(serializer),
            Self::ToolSearchOutput(value) => value.serialize(serializer),
            Self::AdditionalTools(value) => value.serialize(serializer),
            Self::Reasoning(value) => value.serialize(serializer),
            Self::Compaction(value) => value.serialize(serializer),
            Self::ImageGenerationCall(value) => value.serialize(serializer),
            Self::CodeInterpreterCall(value) => value.serialize(serializer),
            Self::LocalShellCall(value) => value.serialize(serializer),
            Self::LocalShellCallOutput(value) => value.serialize(serializer),
            Self::FunctionShellCall(value) => value.serialize(serializer),
            Self::FunctionShellCallOutput(value) => value.serialize(serializer),
            Self::ApplyPatchCall(value) => value.serialize(serializer),
            Self::ApplyPatchCallOutput(value) => value.serialize(serializer),
            Self::CustomToolCallOutput(value) => value.serialize(serializer),
            Self::CustomToolCall(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ResponseInputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("response input item must be an object"))?;

        let tag = match object.get("type") {
            Some(Value::String(tag)) => Some(tag.as_str()),
            Some(_) => return Err(D::Error::custom("input item `type` must be a string")),
            None if object.contains_key("role") || object.contains_key("id") => None,
            None => {
                return Err(D::Error::custom(
                    "input item is missing `type`, `role`, or `id`",
                ));
            }
        };

        match tag {
            None if object.contains_key("role") => serde_json::from_value(value)
                .map(Self::Message)
                .map_err(D::Error::custom),
            None => serde_json::from_value(value)
                .map(Self::ItemReference)
                .map_err(D::Error::custom),
            Some("message")
                if object.contains_key("id")
                    && object.get("role").and_then(Value::as_str) == Some("assistant") =>
            {
                serde_json::from_value(value)
                    .map(Self::OutputMessage)
                    .map_err(D::Error::custom)
            }
            Some("message")
                if object.contains_key("status")
                    || (object.get("content").is_some_and(Value::is_array)
                        && object.get("role").and_then(Value::as_str) != Some("assistant")) =>
            {
                serde_json::from_value(value)
                    .map(Self::StoredMessage)
                    .map_err(D::Error::custom)
            }
            Some("message") => serde_json::from_value(value)
                .map(Self::Message)
                .map_err(D::Error::custom),
            Some("function_call") => serde_json::from_value(value)
                .map(Self::FunctionCall)
                .map_err(D::Error::custom),
            Some("function_call_output") => serde_json::from_value(value)
                .map(Self::FunctionCallOutput)
                .map_err(D::Error::custom),
            Some("mcp_list_tools") => serde_json::from_value(value)
                .map(Self::McpListTools)
                .map_err(D::Error::custom),
            Some("mcp_call") => serde_json::from_value(value)
                .map(Self::McpCall)
                .map_err(D::Error::custom),
            Some("mcp_approval_request") => serde_json::from_value(value)
                .map(Self::McpApprovalRequest)
                .map_err(D::Error::custom),
            Some("mcp_approval_response") => serde_json::from_value(value)
                .map(Self::McpApprovalResponse)
                .map_err(D::Error::custom),
            Some("compaction_trigger") => serde_json::from_value(value)
                .map(Self::CompactionTrigger)
                .map_err(D::Error::custom),
            Some("program") => serde_json::from_value(value)
                .map(Self::Program)
                .map_err(D::Error::custom),
            Some("program_output") => serde_json::from_value(value)
                .map(Self::ProgramOutput)
                .map_err(D::Error::custom),
            Some("file_search_call") => serde_json::from_value(value)
                .map(Self::FileSearchCall)
                .map_err(D::Error::custom),
            Some("computer_call") => serde_json::from_value(value)
                .map(Self::ComputerCall)
                .map_err(D::Error::custom),
            Some("computer_call_output") => serde_json::from_value(value)
                .map(Self::ComputerCallOutput)
                .map_err(D::Error::custom),
            Some("web_search_call") => serde_json::from_value(value)
                .map(Self::WebSearchCall)
                .map_err(D::Error::custom),
            Some("tool_search_call") => serde_json::from_value(value)
                .map(Self::ToolSearchCall)
                .map_err(D::Error::custom),
            Some("tool_search_output") => serde_json::from_value(value)
                .map(Self::ToolSearchOutput)
                .map_err(D::Error::custom),
            Some("additional_tools") => serde_json::from_value(value)
                .map(Self::AdditionalTools)
                .map_err(D::Error::custom),
            Some("reasoning") => serde_json::from_value(value)
                .map(Self::Reasoning)
                .map_err(D::Error::custom),
            Some("compaction") => serde_json::from_value(value)
                .map(Self::Compaction)
                .map_err(D::Error::custom),
            Some("image_generation_call") => serde_json::from_value(value)
                .map(Self::ImageGenerationCall)
                .map_err(D::Error::custom),
            Some("code_interpreter_call") => serde_json::from_value(value)
                .map(Self::CodeInterpreterCall)
                .map_err(D::Error::custom),
            Some("local_shell_call") => serde_json::from_value(value)
                .map(Self::LocalShellCall)
                .map_err(D::Error::custom),
            Some("local_shell_call_output") => serde_json::from_value(value)
                .map(Self::LocalShellCallOutput)
                .map_err(D::Error::custom),
            Some("shell_call") => serde_json::from_value(value)
                .map(Self::FunctionShellCall)
                .map_err(D::Error::custom),
            Some("shell_call_output") => serde_json::from_value(value)
                .map(Self::FunctionShellCallOutput)
                .map_err(D::Error::custom),
            Some("apply_patch_call") => serde_json::from_value(value)
                .map(Self::ApplyPatchCall)
                .map_err(D::Error::custom),
            Some("apply_patch_call_output") => serde_json::from_value(value)
                .map(Self::ApplyPatchCallOutput)
                .map_err(D::Error::custom),
            Some("custom_tool_call_output") => serde_json::from_value(value)
                .map(Self::CustomToolCallOutput)
                .map_err(D::Error::custom),
            Some("custom_tool_call") => serde_json::from_value(value)
                .map(Self::CustomToolCall)
                .map_err(D::Error::custom),
            Some(_) => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<InputMessage> for ResponseInputItem {
    fn from(value: InputMessage) -> Self {
        Self::Message(value)
    }
}

impl From<StoredInputMessage> for ResponseInputItem {
    fn from(value: StoredInputMessage) -> Self {
        Self::StoredMessage(value)
    }
}

impl From<OutputMessage> for ResponseInputItem {
    fn from(value: OutputMessage) -> Self {
        Self::OutputMessage(value)
    }
}

impl From<FunctionCall> for ResponseInputItem {
    fn from(value: FunctionCall) -> Self {
        Self::FunctionCall(value)
    }
}

impl From<FunctionCallOutput> for ResponseInputItem {
    fn from(value: FunctionCallOutput) -> Self {
        Self::FunctionCallOutput(value)
    }
}

impl From<McpApprovalResponse> for ResponseInputItem {
    fn from(value: McpApprovalResponse) -> Self {
        Self::McpApprovalResponse(value)
    }
}

tagged_union! {
    /// One typed item generated by a response.
    pub enum ResponseOutputItem {
        Message(OutputMessage) => "message",
        FileSearchCall(FileSearchCall) => "file_search_call",
        FunctionCall(FunctionCall) => "function_call",
        FunctionCallOutput(FunctionCallOutputResource) => "function_call_output",
        WebSearchCall(WebSearchCall) => "web_search_call",
        ComputerCall(ComputerCall) => "computer_call",
        ComputerCallOutput(ComputerCallOutputResource) => "computer_call_output",
        Reasoning(ReasoningItem) => "reasoning",
        Program(ProgramItem) => "program",
        ProgramOutput(ProgramOutputItem) => "program_output",
        ToolSearchCall(ToolSearchCall) => "tool_search_call",
        ToolSearchOutput(ToolSearchOutput) => "tool_search_output",
        AdditionalTools(AdditionalTools) => "additional_tools",
        Compaction(CompactionItem) => "compaction",
        ImageGenerationCall(ImageGenerationCall) => "image_generation_call",
        CodeInterpreterCall(CodeInterpreterCall) => "code_interpreter_call",
        LocalShellCall(LocalShellCall) => "local_shell_call",
        LocalShellCallOutput(LocalShellCallOutput) => "local_shell_call_output",
        FunctionShellCall(FunctionShellCall) => "shell_call",
        FunctionShellCallOutput(FunctionShellCallOutput) => "shell_call_output",
        ApplyPatchCall(ApplyPatchCall) => "apply_patch_call",
        ApplyPatchCallOutput(ApplyPatchCallOutput) => "apply_patch_call_output",
        McpListTools(McpListTools) => "mcp_list_tools",
        McpCall(McpCall) => "mcp_call",
        McpApprovalRequest(McpApprovalRequest) => "mcp_approval_request",
        McpApprovalResponse(McpApprovalResponseResource) => "mcp_approval_response",
        CustomToolCall(CustomToolCall) => "custom_tool_call",
        CustomToolCallOutput(CustomToolCallOutputResource) => "custom_tool_call_output"
    }
}

impl From<OutputMessage> for ResponseOutputItem {
    fn from(value: OutputMessage) -> Self {
        Self::Message(value)
    }
}

impl From<FunctionCall> for ResponseOutputItem {
    fn from(value: FunctionCall) -> Self {
        Self::FunctionCall(value)
    }
}

literal_tag!(FunctionToolChoiceTag, Function, "function");
literal_tag!(McpToolChoiceTag, Mcp, "mcp");

/// Forces the model to call one named function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionToolChoice {
    #[serde(rename = "type")]
    kind: FunctionToolChoiceTag,
    name: String,
}

impl FunctionToolChoice {
    /// Creates a forced function choice.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: FunctionToolChoiceTag::Function,
            name: name.into(),
        }
    }

    /// Returns the selected function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Forces the model to call a native MCP server, optionally naming one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolChoice {
    #[serde(rename = "type")]
    kind: McpToolChoiceTag,
    server_label: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<Nullable<String>>,
}

impl McpToolChoice {
    /// Selects an MCP server.
    #[must_use]
    pub fn server(server_label: impl Into<String>) -> Self {
        Self {
            kind: McpToolChoiceTag::Mcp,
            server_label: server_label.into(),
            name: Omittable::Omitted,
        }
    }

    /// Selects one tool on the MCP server.
    #[must_use]
    pub fn tool(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(Nullable::Value(name.into()));
        self
    }

    /// Sends official `name: null` so the model may pick any tool on the server.
    #[must_use]
    pub fn name_null(mut self) -> Self {
        self.name = Omittable::Value(Nullable::Null);
        self
    }
}

/// Controls which tool the model may call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToolChoice {
    /// Let the model decide whether to call a tool.
    Auto,
    /// Disable tool calls.
    None,
    /// Require at least one tool call.
    Required,
    /// Force a named function.
    Function(FunctionToolChoice),
    /// Force a native MCP server or tool.
    Mcp(McpToolChoice),
    /// Restrict calls to an allowed set.
    AllowedTools(AllowedToolsChoice),
    /// Force one hosted tool type.
    Hosted(HostedToolChoice),
    /// Force one named custom tool.
    Custom(CustomToolChoice),
    /// Force programmatic tool calling.
    Programmatic(ProgrammaticToolChoice),
    /// Force apply-patch.
    ApplyPatch(ApplyPatchToolChoice),
    /// Force function-shell.
    FunctionShell(FunctionShellToolChoice),
    /// A future string mode retained verbatim.
    UnknownString(Box<str>),
    /// A future object choice retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::None => serializer.serialize_str("none"),
            Self::Required => serializer.serialize_str("required"),
            Self::UnknownString(value) => serializer.serialize_str(value),
            Self::Function(value) => value.serialize(serializer),
            Self::Mcp(value) => value.serialize(serializer),
            Self::AllowedTools(value) => value.serialize(serializer),
            Self::Hosted(value) => value.serialize(serializer),
            Self::Custom(value) => value.serialize(serializer),
            Self::Programmatic(value) => value.serialize(serializer),
            Self::ApplyPatch(value) => value.serialize(serializer),
            Self::FunctionShell(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if let Value::String(mode) = value {
            return Ok(match mode.as_str() {
                "auto" => Self::Auto,
                "none" => Self::None,
                "required" => Self::Required,
                _ => Self::UnknownString(mode.into_boxed_str()),
            });
        }

        let discriminator = object_discriminator(&value).map_err(D::Error::custom)?;
        match discriminator.as_str() {
            "function" => serde_json::from_value(value)
                .map(Self::Function)
                .map_err(D::Error::custom),
            "mcp" => serde_json::from_value(value)
                .map(Self::Mcp)
                .map_err(D::Error::custom),
            "allowed_tools" => serde_json::from_value(value)
                .map(Self::AllowedTools)
                .map_err(D::Error::custom),
            "file_search"
            | "web_search_preview"
            | "web_search"
            | "web_search_2025_08_26"
            | "computer"
            | "computer_use_preview"
            | "computer_use"
            | "web_search_preview_2025_03_11"
            | "image_generation"
            | "code_interpreter" => serde_json::from_value(value)
                .map(Self::Hosted)
                .map_err(D::Error::custom),
            "custom" => serde_json::from_value(value)
                .map(Self::Custom)
                .map_err(D::Error::custom),
            "programmatic_tool_calling" => serde_json::from_value(value)
                .map(Self::Programmatic)
                .map_err(D::Error::custom),
            "apply_patch" => serde_json::from_value(value)
                .map(Self::ApplyPatch)
                .map_err(D::Error::custom),
            "shell" => serde_json::from_value(value)
                .map(Self::FunctionShell)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

literal_tag!(TextFormatTag, Text, "text");
literal_tag!(JsonObjectFormatTag, JsonObject, "json_object");
literal_tag!(JsonSchemaFormatTag, JsonSchema, "json_schema");

/// Plain text output format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatText {
    #[serde(rename = "type")]
    kind: TextFormatTag,
}

impl Default for TextFormatText {
    fn default() -> Self {
        Self {
            kind: TextFormatTag::Text,
        }
    }
}

/// Legacy unconstrained JSON object output format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatJsonObject {
    #[serde(rename = "type")]
    kind: JsonObjectFormatTag,
}

impl Default for TextFormatJsonObject {
    fn default() -> Self {
        Self {
            kind: JsonObjectFormatTag::JsonObject,
        }
    }
}

/// JSON Schema-constrained structured output format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFormatJsonSchema {
    #[serde(rename = "type")]
    kind: JsonSchemaFormatTag,
    name: String,
    schema: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    strict: Omittable<Nullable<bool>>,
}

impl TextFormatJsonSchema {
    /// Creates a structured output format from a JSON Schema value.
    #[must_use]
    pub fn new(name: impl Into<String>, schema: Value) -> Self {
        Self {
            kind: JsonSchemaFormatTag::JsonSchema,
            name: name.into(),
            schema,
            description: Omittable::Omitted,
            strict: Omittable::Omitted,
        }
    }

    /// Serializes a schema representation without requiring JSON text.
    pub fn from_serializable<T: Serialize>(
        name: impl Into<String>,
        schema: &T,
    ) -> Result<Self, serde_json::Error> {
        serde_json::to_value(schema).map(|schema| Self::new(name, schema))
    }

    /// Sets the schema description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Sets strict schema adherence.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = Omittable::Value(Nullable::Value(strict));
        self
    }

    /// Sends official `strict: null`.
    #[must_use]
    pub fn strict_null(mut self) -> Self {
        self.strict = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the explicit strict flag when present and non-null.
    #[must_use]
    pub fn is_strict(&self) -> Option<bool> {
        match self.strict {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

#[cfg(feature = "structured-output")]
impl<T: schemars::JsonSchema> From<&crate::StructuredOutput<T>> for TextFormatJsonSchema {
    fn from(output: &crate::StructuredOutput<T>) -> Self {
        let mut schema = Self::new(output.name(), output.schema().clone()).strict(true);
        if let Some(description) = output.description() {
            schema = schema.description(description);
        }
        schema
    }
}

#[cfg(feature = "structured-output")]
impl<T: schemars::JsonSchema> From<crate::StructuredOutput<T>> for TextFormatJsonSchema {
    fn from(output: crate::StructuredOutput<T>) -> Self {
        Self::from(&output)
    }
}

tagged_union! {
    /// The requested model text output format.
    pub enum TextFormat {
        Text(TextFormatText) => "text",
        JsonObject(TextFormatJsonObject) => "json_object",
        JsonSchema(TextFormatJsonSchema) => "json_schema"
    }
}

#[cfg(feature = "structured-output")]
impl<T: schemars::JsonSchema> From<&crate::StructuredOutput<T>> for TextFormat {
    fn from(output: &crate::StructuredOutput<T>) -> Self {
        Self::JsonSchema(TextFormatJsonSchema::from(output))
    }
}

#[cfg(feature = "structured-output")]
impl<T: schemars::JsonSchema> From<crate::StructuredOutput<T>> for TextFormat {
    fn from(output: crate::StructuredOutput<T>) -> Self {
        Self::JsonSchema(TextFormatJsonSchema::from(output))
    }
}

impl Default for TextFormat {
    fn default() -> Self {
        Self::Text(TextFormatText::default())
    }
}

/// Text-generation configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseTextConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    format: Omittable<TextFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    verbosity: Omittable<Nullable<ResponseTextVerbosity>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseTextConfig {
    /// Creates text configuration for a format.
    #[must_use]
    pub fn new(format: TextFormat) -> Self {
        Self {
            format: Omittable::Value(format),
            verbosity: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets or updates the format.
    #[must_use]
    pub fn format(mut self, format: TextFormat) -> Self {
        self.format = Omittable::Value(format);
        self
    }

    /// Requests a verbosity value supported by the selected model.
    #[must_use]
    pub fn verbosity(mut self, verbosity: impl Into<ResponseTextVerbosity>) -> Self {
        self.verbosity = Omittable::Value(Nullable::Value(verbosity.into()));
        self
    }

    /// Sends official `verbosity: null`.
    #[must_use]
    pub fn verbosity_null(mut self) -> Self {
        self.verbosity = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the requested format when present.
    #[must_use]
    pub fn format_ref(&self) -> Option<&TextFormat> {
        match &self.format {
            Omittable::Value(format) => Some(format),
            Omittable::Omitted => None,
        }
    }

    /// Returns the requested verbosity when present and non-null.
    #[must_use]
    pub fn verbosity_ref(&self) -> Option<&ResponseTextVerbosity> {
        match &self.verbosity {
            Omittable::Value(Nullable::Value(verbosity)) => Some(verbosity),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

/// Reasoning configuration echoed by a response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    context: Omittable<Nullable<ReasoningContext>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    effort: Omittable<Nullable<ReasoningEffort>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    generate_summary: Omittable<Nullable<ReasoningSummary>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    mode: Omittable<ReasoningMode>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    summary: Omittable<Nullable<ReasoningSummary>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ReasoningConfig {
    /// Creates empty reasoning configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets which prior reasoning items are rendered back on later turns.
    #[must_use]
    pub fn context(mut self, context: ReasoningContext) -> Self {
        self.context = Omittable::Value(Nullable::Value(context));
        self
    }

    /// Sets the requested effort.
    #[must_use]
    pub fn effort(mut self, effort: ReasoningEffort) -> Self {
        self.effort = Omittable::Value(Nullable::Value(effort));
        self
    }

    /// Sets the deprecated `generate_summary` field. Prefer [`Self::summary`].
    #[must_use]
    pub fn generate_summary(mut self, summary: ReasoningSummary) -> Self {
        self.generate_summary = Omittable::Value(Nullable::Value(summary));
        self
    }

    /// Sets the reasoning execution mode (`standard` or `pro`).
    #[must_use]
    pub fn mode(mut self, mode: ReasoningMode) -> Self {
        self.mode = Omittable::Value(mode);
        self
    }

    /// Sets the requested summary style.
    #[must_use]
    pub fn summary(mut self, summary: ReasoningSummary) -> Self {
        self.summary = Omittable::Value(Nullable::Value(summary));
        self
    }

    /// Sends `context: null`.
    #[must_use]
    pub fn context_null(mut self) -> Self {
        self.context = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `effort: null`.
    #[must_use]
    pub fn effort_null(mut self) -> Self {
        self.effort = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `generate_summary: null`.
    #[must_use]
    pub fn generate_summary_null(mut self) -> Self {
        self.generate_summary = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `summary: null`.
    #[must_use]
    pub fn summary_null(mut self) -> Self {
        self.summary = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the non-null reasoning context when supplied.
    #[must_use]
    pub fn context_ref(&self) -> Option<&ReasoningContext> {
        match &self.context {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the non-null effort when supplied.
    #[must_use]
    pub fn effort_ref(&self) -> Option<&ReasoningEffort> {
        match &self.effort {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the reasoning mode when supplied.
    #[must_use]
    pub fn mode_ref(&self) -> Option<&ReasoningMode> {
        match &self.mode {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the non-null summary style when supplied.
    #[must_use]
    pub fn summary_ref(&self) -> Option<&ReasoningSummary> {
        match &self.summary {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

/// One context-management compaction rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManagement {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    compact_threshold: Omittable<Nullable<u64>>,
}

impl ContextManagement {
    /// Creates the currently supported `compaction` rule.
    #[must_use]
    pub fn compaction() -> Self {
        Self {
            kind: "compaction".to_owned(),
            compact_threshold: Omittable::Omitted,
        }
    }

    /// Sets the token threshold that triggers compaction.
    #[must_use]
    pub fn compact_threshold(mut self, threshold: u64) -> Self {
        self.compact_threshold = Omittable::Value(Nullable::Value(threshold));
        self
    }

    /// Sends `compact_threshold: null`.
    #[must_use]
    pub fn compact_threshold_null(mut self) -> Self {
        self.compact_threshold = Omittable::Value(Nullable::Null);
        self
    }
}

open_string_enum! {
    /// How Responses should treat flagged input or output.
    pub enum ModerationMode {
        Score = "score",
        Block = "block"
    }
}

/// Policy for one moderation direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModerationDirection {
    mode: ModerationMode,
}

impl ModerationDirection {
    /// Creates a direction policy.
    #[must_use]
    pub const fn new(mode: ModerationMode) -> Self {
        Self { mode }
    }
}

/// Input/output moderation policy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModerationPolicy {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<ModerationDirection>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output: Omittable<Nullable<ModerationDirection>>,
}

impl ModerationPolicy {
    /// Sets the input-side policy.
    #[must_use]
    pub fn input(mut self, direction: ModerationDirection) -> Self {
        self.input = Omittable::Value(Nullable::Value(direction));
        self
    }

    /// Sets the output-side policy.
    #[must_use]
    pub fn output(mut self, direction: ModerationDirection) -> Self {
        self.output = Omittable::Value(Nullable::Value(direction));
        self
    }

    /// Sends `input: null`.
    #[must_use]
    pub fn input_null(mut self) -> Self {
        self.input = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `output: null`.
    #[must_use]
    pub fn output_null(mut self) -> Self {
        self.output = Omittable::Value(Nullable::Null);
        self
    }
}

/// Moderation configuration on a create request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModerationConfig {
    model: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    policy: Omittable<Nullable<ModerationPolicy>>,
}

impl ModerationConfig {
    /// Creates a moderation config for the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            policy: Omittable::Omitted,
        }
    }

    /// Sets the directional policy.
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
}

open_string_enum! {
    /// Modality reflected in a response-side moderation category score.
    pub enum ModerationInputType {
        Text = "text",
        Image = "image"
    }
}

/// One successful or failed moderation outcome on a stored response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ResponseModerationOutcome {
    /// Successful classification for one direction.
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
    /// Failure while moderating one direction.
    #[serde(rename = "error")]
    Error {
        /// Service error code.
        code: String,
        /// Human-readable error message.
        message: String,
    },
}

/// Moderation results echoed on a stored response when requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseModeration {
    input: ResponseModerationOutcome,
    output: ResponseModerationOutcome,
}

impl ResponseModeration {
    /// Returns the input-side outcome.
    #[must_use]
    pub const fn input(&self) -> &ResponseModerationOutcome {
        &self.input
    }

    /// Returns the output-side outcome.
    #[must_use]
    pub const fn output(&self) -> &ResponseModerationOutcome {
        &self.output
    }
}

/// A stored conversation selected by id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationObjectReference {
    id: String,
}

impl ConversationObjectReference {
    /// Creates an object-form conversation reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// A conversation reference accepted as a string or object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConversationReference {
    /// Compact id form.
    Id(String),
    /// Object id form.
    Object(ConversationObjectReference),
}

impl From<String> for ConversationReference {
    fn from(value: String) -> Self {
        Self::Id(value)
    }
}

impl From<&str> for ConversationReference {
    fn from(value: &str) -> Self {
        Self::Id(value.to_owned())
    }
}

/// Reference to a reusable prompt template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptReference {
    id: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    version: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    variables: BTreeMap<String, Value>,
}

impl PromptReference {
    /// Creates a prompt reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: Omittable::Omitted,
            variables: BTreeMap::new(),
        }
    }

    /// Pins a prompt version.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Omittable::Value(Nullable::Value(version.into()));
        self
    }

    /// Sends `version: null`.
    #[must_use]
    pub fn version_null(mut self) -> Self {
        self.version = Omittable::Value(Nullable::Null);
        self
    }

    /// Serializes and inserts one typed prompt variable.
    pub fn variable<T: Serialize>(
        mut self,
        name: impl Into<String>,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        self.variables
            .insert(name.into(), serde_json::to_value(value)?);
        Ok(self)
    }
}

/// Options affecting SSE payloads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseStreamOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include_obfuscation: Omittable<bool>,
}

impl ResponseStreamOptions {
    /// Controls inclusion of padding/obfuscation fields.
    #[must_use]
    pub fn include_obfuscation(mut self, include: bool) -> Self {
        self.include_obfuscation = Omittable::Value(include);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct CreateResponseBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<ResponseInput>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    background: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    conversation: Omittable<Nullable<ConversationReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    context_management: Omittable<Nullable<Vec<ContextManagement>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Nullable<Vec<ResponseIncludable>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_output_tokens: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_tool_calls: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    moderation: Omittable<Nullable<ModerationConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parallel_tool_calls: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<PromptReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<PromptCacheOptionsParam>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<PromptCacheRetention>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<ReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    safety_identifier: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<Nullable<ServiceTier>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    store: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<ResponseTextConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<ToolChoice>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<ResponseTool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_logprobs: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_p: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<Nullable<TruncationStrategy>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<String>,
}

impl CreateResponseBody {
    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(Nullable::Value(temperature)) = self.temperature
            && !(temperature.is_finite() && (0.0..=2.0).contains(&temperature))
        {
            return Err(CreateResponseConstraintError::Temperature {
                value: temperature.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(top_p)) = self.top_p
            && !(top_p.is_finite() && (0.0..=1.0).contains(&top_p))
        {
            return Err(CreateResponseConstraintError::TopP {
                value: top_p.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(top_logprobs)) = self.top_logprobs
            && top_logprobs > MAX_TOP_LOGPROBS
        {
            return Err(CreateResponseConstraintError::TopLogprobs {
                actual: top_logprobs,
                maximum: MAX_TOP_LOGPROBS,
            });
        }
        if let Omittable::Value(Nullable::Value(max_output_tokens)) = self.max_output_tokens
            && max_output_tokens < MIN_MAX_OUTPUT_TOKENS
        {
            return Err(CreateResponseConstraintError::MaxOutputTokens {
                actual: max_output_tokens,
                minimum: MIN_MAX_OUTPUT_TOKENS,
            });
        }
        if let Omittable::Value(Nullable::Value(identifier)) = &self.safety_identifier {
            let actual = identifier.chars().count();
            if actual > MAX_SAFETY_IDENTIFIER_CHARS {
                return Err(CreateResponseConstraintError::SafetyIdentifier {
                    actual,
                    maximum: MAX_SAFETY_IDENTIFIER_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(metadata)) = &self.metadata {
            if metadata.len() > MAX_RESPONSE_METADATA_PAIRS {
                return Err(CreateResponseConstraintError::MetadataPairCount {
                    actual: metadata.len(),
                    maximum: MAX_RESPONSE_METADATA_PAIRS,
                });
            }
            for (key, value) in metadata {
                let key_chars = key.chars().count();
                if key_chars > MAX_RESPONSE_METADATA_KEY_CHARS {
                    return Err(CreateResponseConstraintError::MetadataKey {
                        actual: key_chars,
                        maximum: MAX_RESPONSE_METADATA_KEY_CHARS,
                    });
                }
                let value_chars = value.chars().count();
                if value_chars > MAX_RESPONSE_METADATA_VALUE_CHARS {
                    return Err(CreateResponseConstraintError::MetadataValue {
                        actual: value_chars,
                        maximum: MAX_RESPONSE_METADATA_VALUE_CHARS,
                    });
                }
            }
        }
        if let Omittable::Value(Nullable::Value(rules)) = &self.context_management {
            if rules.is_empty() {
                return Err(CreateResponseConstraintError::EmptyContextManagement);
            }
            for rule in rules {
                if let Omittable::Value(Nullable::Value(threshold)) = rule.compact_threshold
                    && threshold < MIN_COMPACT_THRESHOLD
                {
                    return Err(CreateResponseConstraintError::CompactThreshold {
                        actual: threshold,
                        minimum: MIN_COMPACT_THRESHOLD,
                    });
                }
            }
        }
        if let Omittable::Value(tools) = &self.tools {
            validate_response_tools(tools)?;
        }
        if let Omittable::Value(ResponseInput::Items(items)) = &self.input {
            for item in items {
                validate_response_input_item(item)?;
            }
        }
        Ok(())
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn deserialize_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match bool::deserialize(deserializer)? {
        false => Ok(false),
        true => Err(D::Error::custom(
            "CreateResponseRequest requires stream to be false",
        )),
    }
}

fn deserialize_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match bool::deserialize(deserializer)? {
        true => Ok(true),
        false => Err(D::Error::custom(
            "CreateStreamingResponseRequest requires stream to be true",
        )),
    }
}

/// A non-streaming `POST /responses` body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateResponseRequest {
    #[serde(flatten)]
    body: CreateResponseBody,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "deserialize_false"
    )]
    stream: bool,
}

/// A streaming `POST /responses` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateStreamingResponseRequest {
    #[serde(flatten)]
    body: CreateResponseBody,
    #[serde(deserialize_with = "deserialize_true")]
    stream: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_options: Omittable<Nullable<ResponseStreamOptions>>,
}

macro_rules! impl_create_response_builders {
    ($type:ty) => {
        impl $type {
            /// Creates a request with the ergonomic model-and-input pair.
            #[must_use]
            pub fn new(model: impl Into<String>, input: impl Into<ResponseInput>) -> Self {
                let mut value = Self::empty();
                value.body.model = Omittable::Value(model.into());
                value.body.input = Omittable::Value(input.into());
                value
            }

            /// Creates a follow-up request referencing a previous response id and model.
            ///
            /// `previous_response_id` does not carry the previous request's top-level
            /// `instructions` or `tools`. Resend those with [`Self::follow_up_from`]
            /// or the dedicated builders.
            #[must_use]
            pub fn follow_up(response: &Response, input: impl Into<ResponseInput>) -> Self {
                let mut value = Self::new(response.model(), input);
                value.body.previous_response_id =
                    Omittable::Value(Nullable::Value(response.id().to_owned()));
                value
            }

            /// Continues from `response` while copying stable prefix fields from
            /// the previous request (`instructions`, `tools`, `tool_choice`,
            /// `text`, `reasoning`, `prompt_cache_key`, and
            /// `prompt_cache_options`).
            ///
            /// `conversation` is deliberately not copied: the pinned contract
            /// states that `previous_response_id` "cannot be used in
            /// conjunction with `conversation`", so a follow-up request must
            /// not carry the previous request's conversation reference.
            #[must_use]
            pub fn follow_up_from(
                previous: &Self,
                response: &Response,
                input: impl Into<ResponseInput>,
            ) -> Self {
                let mut value = Self::follow_up(response, input);
                value.body.instructions = previous.body.instructions.clone();
                value.body.tools = previous.body.tools.clone();
                value.body.tool_choice = previous.body.tool_choice.clone();
                value.body.text = previous.body.text.clone();
                value.body.reasoning = previous.body.reasoning.clone();
                value.body.prompt_cache_key = previous.body.prompt_cache_key.clone();
                value.body.prompt_cache_options = previous.body.prompt_cache_options.clone();
                value
            }

            /// Sets the model id.
            #[must_use]
            pub fn model(mut self, model: impl Into<String>) -> Self {
                self.body.model = Omittable::Value(model.into());
                self
            }

            /// Sets model input.
            #[must_use]
            pub fn input(mut self, input: impl Into<ResponseInput>) -> Self {
                self.body.input = Omittable::Value(input.into());
                self
            }

            /// Sets instructions.
            #[must_use]
            pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
                self.body.instructions = Omittable::Value(Nullable::Value(instructions.into()));
                self
            }

            /// Explicitly sends `instructions: null`.
            #[must_use]
            pub fn instructions_null(mut self) -> Self {
                self.body.instructions = Omittable::Value(Nullable::Null);
                self
            }

            /// Enables or disables background execution.
            #[must_use]
            pub fn background(mut self, background: bool) -> Self {
                self.body.background = Omittable::Value(Nullable::Value(background));
                self
            }

            /// Sends `background: null`.
            #[must_use]
            pub fn background_null(mut self) -> Self {
                self.body.background = Omittable::Value(Nullable::Null);
                self
            }

            /// Associates the response with a conversation.
            ///
            /// Cannot be used in conjunction with `previous_response_id` (the
            /// pinned contract rejects the combination server-side).
            #[must_use]
            pub fn conversation(mut self, conversation: impl Into<ConversationReference>) -> Self {
                self.body.conversation = Omittable::Value(Nullable::Value(conversation.into()));
                self
            }

            /// Sends `conversation: null`.
            #[must_use]
            pub fn conversation_null(mut self) -> Self {
                self.body.conversation = Omittable::Value(Nullable::Null);
                self
            }

            /// Adds one context-management rule.
            #[must_use]
            pub fn context_management(mut self, rule: ContextManagement) -> Self {
                let mut rules = match std::mem::take(&mut self.body.context_management) {
                    Omittable::Value(Nullable::Value(rules)) => rules,
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
                };
                rules.push(rule);
                self.body.context_management = Omittable::Value(Nullable::Value(rules));
                self
            }

            /// Sends `context_management: null`.
            #[must_use]
            pub fn context_management_null(mut self) -> Self {
                self.body.context_management = Omittable::Value(Nullable::Null);
                self
            }

            /// Adds one optional response field to include.
            #[must_use]
            pub fn include(mut self, include: impl Into<ResponseIncludable>) -> Self {
                let mut includes = match std::mem::take(&mut self.body.include) {
                    Omittable::Value(Nullable::Value(includes)) => includes,
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
                };
                includes.push(include.into());
                self.body.include = Omittable::Value(Nullable::Value(includes));
                self
            }

            /// Sends `include: null`.
            #[must_use]
            pub fn include_null(mut self) -> Self {
                self.body.include = Omittable::Value(Nullable::Null);
                self
            }

            /// Caps generated tokens.
            #[must_use]
            pub fn max_output_tokens(mut self, max_output_tokens: u32) -> Self {
                self.body.max_output_tokens = Omittable::Value(Nullable::Value(max_output_tokens));
                self
            }

            /// Sends `max_output_tokens: null`.
            #[must_use]
            pub fn max_output_tokens_null(mut self) -> Self {
                self.body.max_output_tokens = Omittable::Value(Nullable::Null);
                self
            }

            /// Caps total built-in tool calls.
            #[must_use]
            pub fn max_tool_calls(mut self, max_tool_calls: u32) -> Self {
                self.body.max_tool_calls = Omittable::Value(Nullable::Value(max_tool_calls));
                self
            }

            /// Sends `max_tool_calls: null`.
            #[must_use]
            pub fn max_tool_calls_null(mut self) -> Self {
                self.body.max_tool_calls = Omittable::Value(Nullable::Null);
                self
            }

            /// Inserts one metadata pair.
            #[must_use]
            pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
                let mut metadata = match std::mem::take(&mut self.body.metadata) {
                    Omittable::Value(Nullable::Value(metadata)) => metadata,
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => BTreeMap::new(),
                };
                metadata.insert(key.into(), value.into());
                self.body.metadata = Omittable::Value(Nullable::Value(metadata));
                self
            }

            /// Sends `metadata: null`.
            #[must_use]
            pub fn metadata_null(mut self) -> Self {
                self.body.metadata = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets typed moderation configuration.
            #[must_use]
            pub fn moderation(mut self, moderation: ModerationConfig) -> Self {
                self.body.moderation = Omittable::Value(Nullable::Value(moderation));
                self
            }

            /// Sends `moderation: null`.
            #[must_use]
            pub fn moderation_null(mut self) -> Self {
                self.body.moderation = Omittable::Value(Nullable::Null);
                self
            }

            /// Controls parallel tool calls.
            #[must_use]
            pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
                self.body.parallel_tool_calls = Omittable::Value(Nullable::Value(enabled));
                self
            }

            /// Sends `parallel_tool_calls: null`.
            #[must_use]
            pub fn parallel_tool_calls_null(mut self) -> Self {
                self.body.parallel_tool_calls = Omittable::Value(Nullable::Null);
                self
            }

            /// Continues from a prior response id.
            ///
            /// Use this to create multi-turn conversations without resending
            /// prior items. Cannot be used in conjunction with `conversation`.
            #[must_use]
            pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
                self.body.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
                self
            }

            /// Sends `previous_response_id: null`.
            ///
            /// Clearing the field removes the pin's `previous_response_id`
            /// versus `conversation` exclusivity concern for this request.
            #[must_use]
            pub fn previous_response_id_null(mut self) -> Self {
                self.body.previous_response_id = Omittable::Value(Nullable::Null);
                self
            }

            /// Uses a reusable prompt template.
            #[must_use]
            pub fn prompt(mut self, prompt: PromptReference) -> Self {
                self.body.prompt = Omittable::Value(Nullable::Value(prompt));
                self
            }

            /// Sends `prompt: null`.
            #[must_use]
            pub fn prompt_null(mut self) -> Self {
                self.body.prompt = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets a prompt-cache key.
            #[must_use]
            pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
                self.body.prompt_cache_key = Omittable::Value(Nullable::Value(key.into()));
                self
            }

            /// Sends `prompt_cache_key: null`.
            #[must_use]
            pub fn prompt_cache_key_null(mut self) -> Self {
                self.body.prompt_cache_key = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets prompt-cache options.
            #[must_use]
            pub fn prompt_cache_options(mut self, options: PromptCacheOptionsParam) -> Self {
                self.body.prompt_cache_options = Omittable::Value(options);
                self
            }

            /// Sets the deprecated prompt-cache retention policy.
            ///
            /// Prefer [`Self::prompt_cache_options`] with [`PromptCacheOptionsParam::ttl`].
            /// The two fields are independent: retention is a maximum keep time,
            /// while `prompt_cache_options.ttl` is a minimum lifetime.
            #[must_use]
            pub fn prompt_cache_retention(
                mut self,
                retention: impl Into<PromptCacheRetention>,
            ) -> Self {
                self.body.prompt_cache_retention =
                    Omittable::Value(Nullable::Value(retention.into()));
                self
            }

            /// Sends `prompt_cache_retention: null`.
            #[must_use]
            pub fn prompt_cache_retention_null(mut self) -> Self {
                self.body.prompt_cache_retention = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets reasoning configuration.
            #[must_use]
            pub fn reasoning(mut self, reasoning: ReasoningConfig) -> Self {
                self.body.reasoning = Omittable::Value(Nullable::Value(reasoning));
                self
            }

            /// Sends `reasoning: null`.
            #[must_use]
            pub fn reasoning_null(mut self) -> Self {
                self.body.reasoning = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets an abuse-detection safety identifier.
            #[must_use]
            pub fn safety_identifier(mut self, identifier: impl Into<String>) -> Self {
                self.body.safety_identifier = Omittable::Value(Nullable::Value(identifier.into()));
                self
            }

            /// Sends `safety_identifier: null`.
            #[must_use]
            pub fn safety_identifier_null(mut self) -> Self {
                self.body.safety_identifier = Omittable::Value(Nullable::Null);
                self
            }

            /// Requests a service tier.
            #[must_use]
            pub fn service_tier(mut self, service_tier: impl Into<ServiceTier>) -> Self {
                self.body.service_tier = Omittable::Value(Nullable::Value(service_tier.into()));
                self
            }

            /// Sends `service_tier: null`.
            #[must_use]
            pub fn service_tier_null(mut self) -> Self {
                self.body.service_tier = Omittable::Value(Nullable::Null);
                self
            }

            /// Checks pinned OpenAPI field limits without sending the request.
            ///
            /// Builders remain lossless and do not reject out-of-range values so
            /// that captured wire fixtures can still roundtrip. Call this before
            /// submit when the application wants the documented Python/OpenAPI
            /// bounds (`temperature` 0..=2, `top_p` 0..=1, `top_logprobs` 0..=20,
            /// `max_output_tokens` >= 16, metadata 16×64/512, `safety_identifier`
            /// 64 characters, non-empty `context_management`).
            pub fn validate(&self) -> Result<&Self, CreateResponseConstraintError> {
                self.body.validate()?;
                Ok(self)
            }

            /// Controls response storage.
            #[must_use]
            pub fn store(mut self, store: bool) -> Self {
                self.body.store = Omittable::Value(Nullable::Value(store));
                self
            }

            /// Sends `store: null`.
            #[must_use]
            pub fn store_null(mut self) -> Self {
                self.body.store = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets sampling temperature.
            #[must_use]
            pub fn temperature(mut self, temperature: f64) -> Self {
                self.body.temperature = Omittable::Value(Nullable::Value(temperature));
                self
            }

            /// Sends `temperature: null`.
            #[must_use]
            pub fn temperature_null(mut self) -> Self {
                self.body.temperature = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets text output configuration.
            #[must_use]
            pub fn text(mut self, text: ResponseTextConfig) -> Self {
                self.body.text = Omittable::Value(text);
                self
            }

            /// Sets structured output configuration using a typed schema.
            #[cfg(feature = "structured-output")]
            #[must_use]
            pub fn text_format<T: schemars::JsonSchema>(
                mut self,
                output: &crate::StructuredOutput<T>,
            ) -> Self {
                let format = TextFormat::from(output);
                let text_config = match self.body.text {
                    Omittable::Value(config) => {
                        let mut next = ResponseTextConfig::new(format);
                        next = match config.verbosity {
                            Omittable::Value(Nullable::Value(verbosity)) => {
                                next.verbosity(verbosity)
                            }
                            Omittable::Value(Nullable::Null) => next.verbosity_null(),
                            Omittable::Omitted => next,
                        };
                        next
                    }
                    Omittable::Omitted => ResponseTextConfig::new(format),
                };
                self.body.text = Omittable::Value(text_config);
                self
            }

            /// Adds a tool.
            #[must_use]
            pub fn tool(mut self, tool: impl Into<ResponseTool>) -> Self {
                let mut tools = match std::mem::take(&mut self.body.tools) {
                    Omittable::Value(tools) => tools,
                    Omittable::Omitted => Vec::new(),
                };
                tools.push(tool.into());
                self.body.tools = Omittable::Value(tools);
                self
            }

            /// Adds a tool.
            #[must_use]
            pub fn with_tool(self, tool: impl Into<ResponseTool>) -> Self {
                self.tool(tool)
            }

            /// Adds multiple tools.
            #[must_use]
            pub fn tools(
                mut self,
                tools: impl IntoIterator<Item = impl Into<ResponseTool>>,
            ) -> Self {
                let mut configured = match std::mem::take(&mut self.body.tools) {
                    Omittable::Value(configured) => configured,
                    Omittable::Omitted => Vec::new(),
                };
                configured.extend(tools.into_iter().map(Into::into));
                self.body.tools = Omittable::Value(configured);
                self
            }

            /// Selects the tool-choice policy.
            #[must_use]
            pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
                self.body.tool_choice = Omittable::Value(tool_choice);
                self
            }

            /// Sets nucleus sampling probability.
            #[must_use]
            pub fn top_p(mut self, top_p: f64) -> Self {
                self.body.top_p = Omittable::Value(Nullable::Value(top_p));
                self
            }

            /// Sends `top_p: null`.
            #[must_use]
            pub fn top_p_null(mut self) -> Self {
                self.body.top_p = Omittable::Value(Nullable::Null);
                self
            }

            /// Requests token log probabilities at each output position.
            #[must_use]
            pub fn top_logprobs(mut self, top_logprobs: u32) -> Self {
                self.body.top_logprobs = Omittable::Value(Nullable::Value(top_logprobs));
                self
            }

            /// Sends `top_logprobs: null`.
            #[must_use]
            pub fn top_logprobs_null(mut self) -> Self {
                self.body.top_logprobs = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets the truncation strategy.
            #[must_use]
            pub fn truncation(mut self, truncation: TruncationStrategy) -> Self {
                self.body.truncation = Omittable::Value(Nullable::Value(truncation));
                self
            }

            /// Sends `truncation: null`.
            #[must_use]
            pub fn truncation_null(mut self) -> Self {
                self.body.truncation = Omittable::Value(Nullable::Null);
                self
            }

            /// Sets the deprecated end-user identifier when required.
            #[must_use]
            pub fn user(mut self, user: impl Into<String>) -> Self {
                self.body.user = Omittable::Value(user.into());
                self
            }

            /// Returns the model id when present.
            #[must_use]
            pub fn model_ref(&self) -> Option<&str> {
                match &self.body.model {
                    Omittable::Value(value) => Some(value),
                    Omittable::Omitted => None,
                }
            }

            /// Returns the request input when present.
            #[must_use]
            pub fn input_ref(&self) -> Option<&ResponseInput> {
                match &self.body.input {
                    Omittable::Value(value) => Some(value),
                    Omittable::Omitted => None,
                }
            }

            /// Returns configured tools.
            #[must_use]
            pub fn tools_ref(&self) -> &[ResponseTool] {
                match &self.body.tools {
                    Omittable::Value(tools) => tools,
                    Omittable::Omitted => &[],
                }
            }
        }
    };
}

impl CreateResponseRequest {
    /// Creates an empty request body. Every create property is optional in the
    /// frozen wire schema; most callers should prefer [`Self::new`].
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Converts this request to the streaming typestate.
    #[must_use]
    pub fn into_streaming(self) -> CreateStreamingResponseRequest {
        CreateStreamingResponseRequest {
            body: self.body,
            stream: true,
            stream_options: Omittable::Omitted,
        }
    }
}

impl_create_response_builders!(CreateResponseRequest);

impl CreateStreamingResponseRequest {
    /// Creates an empty streaming request body.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            body: CreateResponseBody::default(),
            stream: true,
            stream_options: Omittable::Omitted,
        }
    }

    /// Sets SSE payload options.
    #[must_use]
    pub fn stream_options(mut self, stream_options: ResponseStreamOptions) -> Self {
        self.stream_options = Omittable::Value(Nullable::Value(stream_options));
        self
    }

    /// Sends `stream_options: null`.
    #[must_use]
    pub fn stream_options_null(mut self) -> Self {
        self.stream_options = Omittable::Value(Nullable::Null);
        self
    }

    /// Converts this request back to non-streaming mode.
    #[must_use]
    pub fn into_non_streaming(self) -> CreateResponseRequest {
        CreateResponseRequest {
            body: self.body,
            stream: false,
        }
    }
}

impl_create_response_builders!(CreateStreamingResponseRequest);

literal_tag!(ResponsesCreateEventTag, ResponseCreate, "response.create");

/// Client event that starts inference on a Responses WebSocket connection.
///
/// The create parameters are flattened exactly as required by the wire
/// schema; the HTTP-only `stream` flag is not emitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesCreateEvent {
    #[serde(rename = "type")]
    kind: ResponsesCreateEventTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_id: Omittable<String>,
    #[serde(flatten)]
    body: CreateResponseBody,
}

impl ResponsesCreateEvent {
    /// Converts an HTTP create request into a WebSocket create event.
    #[must_use]
    pub fn from_request(request: CreateResponseRequest) -> Self {
        Self {
            kind: ResponsesCreateEventTag::ResponseCreate,
            stream_id: Omittable::Omitted,
            body: request.body,
        }
    }

    /// Creates a WebSocket event with the ergonomic model-and-input pair.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<ResponseInput>) -> Self {
        Self::from_request(CreateResponseRequest::new(model, input))
    }

    /// Routes the response through a named FIFO WebSocket lane.
    #[must_use]
    pub fn stream_id(mut self, stream_id: impl Into<String>) -> Self {
        self.stream_id = Omittable::Value(stream_id.into());
        self
    }

    /// Returns the lane id when supplied.
    #[must_use]
    pub fn stream_id_ref(&self) -> Option<&str> {
        match &self.stream_id {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Checks pinned OpenAPI `stream_id` and create-body limits.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(stream_id) = &self.stream_id {
            validate_websocket_stream_id(stream_id)?;
        }
        self.body.validate()
    }

    /// Converts this event back to a non-streaming HTTP create request.
    #[must_use]
    pub fn into_request(self) -> CreateResponseRequest {
        CreateResponseRequest {
            body: self.body,
            stream: false,
        }
    }
}

/// Event sent by a client over a Responses WebSocket connection.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ResponsesClientEvent {
    /// Starts one response; boxed to keep future/unknown variants compact.
    Create(Box<ResponsesCreateEvent>),
    /// A future client event retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ResponsesClientEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Create(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ResponsesClientEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "response.create" => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Create)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl ResponsesClientEvent {
    /// Creates a `response.create` client event.
    #[must_use]
    pub fn create(request: CreateResponseRequest) -> Self {
        Self::Create(Box::new(ResponsesCreateEvent::from_request(request)))
    }

    /// Creates a `response.create` event on a named lane.
    #[must_use]
    pub fn create_on_stream(stream_id: impl Into<String>, request: CreateResponseRequest) -> Self {
        Self::Create(Box::new(
            ResponsesCreateEvent::from_request(request).stream_id(stream_id),
        ))
    }
}

/// Official `ErrorPayload` nested on `ResponseWsError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesWebSocketErrorDetails {
    #[serde(rename = "type")]
    kind: String,
    code: Nullable<String>,
    message: String,
    param: Nullable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    headers: Omittable<BTreeMap<String, String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ResponsesWebSocketErrorDetails {
    /// Returns the protocol error type.
    #[must_use]
    pub fn error_type(&self) -> &str {
        &self.kind
    }

    /// Returns the official error code when the service sent a string.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match &self.code {
            Nullable::Value(code) => Some(code.as_str()),
            Nullable::Null => None,
        }
    }

    /// Returns the human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the associated parameter when present.
    #[must_use]
    pub fn param(&self) -> Option<&str> {
        match &self.param {
            Nullable::Value(param) => Some(param.as_str()),
            Nullable::Null => None,
        }
    }

    /// Returns official response headers when the service sent them.
    #[must_use]
    pub fn headers(&self) -> Option<&BTreeMap<String, String>> {
        match &self.headers {
            Omittable::Value(headers) => Some(headers),
            Omittable::Omitted => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(ResponsesWebSocketErrorTag, Error, "error");

/// Official `ResponseWsError` whose nested `error` collides with the SSE
/// `error` event and therefore requires structural routing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesWebSocketErrorEvent {
    error: ResponsesWebSocketErrorDetails,
    #[serde(rename = "type")]
    kind: ResponsesWebSocketErrorTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sequence_number: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<u16>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_id: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ResponsesWebSocketErrorEvent {
    /// Returns the nested official error payload.
    #[must_use]
    pub const fn error(&self) -> &ResponsesWebSocketErrorDetails {
        &self.error
    }

    /// Returns the HTTP status when the service sent one.
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self.status {
            Omittable::Value(status) => Some(status),
            Omittable::Omitted => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Event received from a Responses WebSocket connection.
///
/// Stable SSE-shaped events share [`ResponseStreamEvent`]. Official
/// `ResponseWsError` objects are routed separately because they nest
/// `ErrorPayload` under `error`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
// The Stream variant wraps the 58-branch ResponseStreamEvent union by
// value; boxing it would be a breaking public-API refactor tracked
// separately from wire fixes.
#[allow(clippy::large_enum_variant)]
pub enum ResponsesServerEvent {
    /// An SSE-shaped Responses event, including future unknown tags.
    Stream {
        /// Shared Responses stream event.
        event: ResponseStreamEvent,
        /// WebSocket lane echoed beside the stream event.
        stream_id: Option<String>,
    },
    /// Official WebSocket `error` envelope.
    WebSocketError(ResponsesWebSocketErrorEvent),
}

impl Serialize for ResponsesServerEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Stream { event, .. } => event.serialize(serializer),
            Self::WebSocketError(event) => event.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ResponsesServerEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "error")
            && value.get("error").is_some_and(Value::is_object)
        {
            return serde_json::from_value(value)
                .map(Self::WebSocketError)
                .map_err(D::Error::custom);
        }
        let stream_id = match value.as_object().and_then(|object| object.get("stream_id")) {
            Some(Value::String(stream_id)) => Some(stream_id.clone()),
            Some(_) => return Err(D::Error::custom("WebSocket `stream_id` must be a string")),
            None => None,
        };
        serde_json::from_value(value)
            .map(|event| Self::Stream { event, stream_id })
            .map_err(D::Error::custom)
    }
}

impl ResponsesServerEvent {
    /// Wraps a stable Responses event.
    #[must_use]
    pub fn new(event: ResponseStreamEvent) -> Self {
        Self::Stream {
            event,
            stream_id: None,
        }
    }

    /// Borrows the SSE-shaped event when this is not a WebSocket error.
    #[must_use]
    pub const fn event(&self) -> Option<&ResponseStreamEvent> {
        match self {
            Self::Stream { event, .. } => Some(event),
            Self::WebSocketError(_) => None,
        }
    }

    /// Consumes the SSE-shaped event when this is not a WebSocket error.
    #[must_use]
    pub fn into_event(self) -> Option<ResponseStreamEvent> {
        match self {
            Self::Stream { event, .. } => Some(event),
            Self::WebSocketError(_) => None,
        }
    }

    /// Returns whether this event terminates its response.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        match self {
            Self::Stream { event, .. } => event.is_terminal(),
            Self::WebSocketError(_) => false,
        }
    }

    /// Returns the shared sequence number when present.
    #[must_use]
    pub fn sequence_number(&self) -> Option<u64> {
        match self {
            Self::Stream { event, .. } => event.sequence_number(),
            Self::WebSocketError(event) => match event.sequence_number {
                Omittable::Value(sequence) => Some(sequence),
                Omittable::Omitted => None,
            },
        }
    }

    /// Returns the WebSocket lane echoed by the server.
    #[must_use]
    pub fn stream_id(&self) -> Option<&str> {
        match self {
            Self::Stream { stream_id, .. } => stream_id.as_deref(),
            Self::WebSocketError(event) => match &event.stream_id {
                Omittable::Value(stream_id) => Some(stream_id.as_str()),
                Omittable::Omitted => None,
            },
        }
    }
}

impl From<ResponseStreamEvent> for ResponsesServerEvent {
    fn from(value: ResponseStreamEvent) -> Self {
        Self::new(value)
    }
}

literal_tag!(ResponseObjectTag, Response, "response");

/// An error returned when the model could not generate a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    code: ResponseErrorCode,
    message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseError {
    /// Returns the official `ResponseErrorCode` when named, or the raw
    /// future value when unknown.
    #[must_use]
    pub const fn code(&self) -> &ResponseErrorCode {
        &self.code
    }

    /// Returns the human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Details explaining an incomplete response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncompleteDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reason: Omittable<IncompleteReason>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl IncompleteDetails {
    /// Returns the incomplete reason when present.
    #[must_use]
    pub fn reason(&self) -> Option<&IncompleteReason> {
        match &self.reason {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }
}

/// Token accounting for model input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputTokensDetails {
    cached_tokens: u64,
    cache_write_tokens: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputTokensDetails {
    /// Returns the number of cached input tokens.
    #[must_use]
    pub const fn cached_tokens(&self) -> u64 {
        self.cached_tokens
    }

    /// Returns the number of input tokens written to the cache.
    #[must_use]
    pub const fn cache_write_tokens(&self) -> u64 {
        self.cache_write_tokens
    }
}

/// Token accounting for model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    reasoning_tokens: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputTokensDetails {
    /// Returns the number of reasoning tokens.
    #[must_use]
    pub const fn reasoning_tokens(&self) -> u64 {
        self.reasoning_tokens
    }
}

/// Token usage reported for a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseUsage {
    input_tokens: u64,
    input_tokens_details: InputTokensDetails,
    output_tokens: u64,
    output_tokens_details: OutputTokensDetails,
    total_tokens: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    compute_units: Omittable<Nullable<u64>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseUsage {
    /// Returns total input tokens.
    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Returns the official input-token breakdown.
    #[must_use]
    pub const fn input_tokens_details(&self) -> &InputTokensDetails {
        &self.input_tokens_details
    }

    /// Returns total output tokens.
    #[must_use]
    pub const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Returns the official output-token breakdown.
    #[must_use]
    pub const fn output_tokens_details(&self) -> &OutputTokensDetails {
        &self.output_tokens_details
    }

    /// Returns input plus output tokens.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    /// Returns compute units when present and non-null.
    #[must_use]
    pub const fn compute_units(&self) -> Option<u64> {
        match self.compute_units {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Failures produced when parsing structured output from a Response.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OutputParseError {
    /// The response was incomplete.
    #[error(
        "response was incomplete: {reason}",
        reason = .0.as_deref().unwrap_or("reason not provided")
    )]
    Incomplete(Option<String>),
    /// The response failed with a service error.
    #[error("response failed: {}", .0.message())]
    Failed(ResponseError),
    /// The model refused to respond.
    #[error("model refused to respond: {0}")]
    Refusal(String),
    /// The response contains no text output.
    #[error("response contains no text output")]
    NoTextOutput,
    /// Failed to deserialize output text into the target type.
    #[error("failed to parse structured output: {0}")]
    Decode(#[source] serde_json::Error),
}

/// A complete Responses API resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    id: String,
    created_at: i64,
    error: Nullable<ResponseError>,
    incomplete_details: Nullable<IncompleteDetails>,
    instructions: Nullable<ResponseInstructions>,
    metadata: Nullable<BTreeMap<String, String>>,
    model: String,
    #[serde(rename = "object")]
    object: ResponseObjectTag,
    output: Vec<ResponseOutputItem>,
    parallel_tool_calls: bool,
    temperature: Nullable<f64>,
    tool_choice: ToolChoice,
    tools: Vec<ResponseTool>,
    top_p: Nullable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseStatus>,
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
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<PromptReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<Nullable<PromptCacheOptions>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<PromptCacheRetention>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<ReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    safety_identifier: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<Nullable<ServiceTier>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    store: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<ResponseTextConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_logprobs: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<Nullable<TruncationStrategy>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<Nullable<ResponseUsage>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    moderation: Omittable<Nullable<ResponseModeration>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Response {
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

    /// Returns the model id used by the service.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the response status when provided.
    #[must_use]
    pub fn status(&self) -> Option<&ResponseStatus> {
        match &self.status {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the service tier when provided and non-null.
    #[must_use]
    pub fn service_tier(&self) -> Option<&ServiceTier> {
        match &self.service_tier {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns echoed prompt-cache key when provided and non-null.
    #[must_use]
    pub fn prompt_cache_key(&self) -> Option<&str> {
        match &self.prompt_cache_key {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns echoed prompt-cache options when provided and non-null.
    #[must_use]
    pub fn prompt_cache_options(&self) -> Option<&PromptCacheOptions> {
        match &self.prompt_cache_options {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns echoed prompt-cache retention when provided and non-null.
    #[must_use]
    pub fn prompt_cache_retention(&self) -> Option<&PromptCacheRetention> {
        match &self.prompt_cache_retention {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns `top_logprobs` when provided and non-null.
    #[must_use]
    pub fn top_logprobs(&self) -> Option<u32> {
        match self.top_logprobs {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns typed moderation outcomes when provided and non-null.
    #[must_use]
    pub fn moderation(&self) -> Option<&ResponseModeration> {
        match &self.moderation {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns whether the response ran in the background when provided and non-null.
    #[must_use]
    pub fn background(&self) -> Option<bool> {
        match self.background {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the truncation strategy when provided and non-null.
    #[must_use]
    pub fn truncation(&self) -> Option<&TruncationStrategy> {
        match &self.truncation {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns output items in service order.
    #[must_use]
    pub fn output(&self) -> &[ResponseOutputItem] {
        &self.output
    }

    /// Concatenates every assistant output-text part in service order.
    #[must_use]
    pub fn output_text(&self) -> String {
        self.output
            .iter()
            .filter_map(|item| match item {
                ResponseOutputItem::Message(message) => Some(message.text_parts()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Iterates over every function call, not merely the first output item.
    pub fn function_calls(&self) -> impl Iterator<Item = &FunctionCall> {
        self.output.iter().filter_map(|item| match item {
            ResponseOutputItem::FunctionCall(call) => Some(call),
            _ => None,
        })
    }

    /// Returns the first refusal text if the response contains a refusal part.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.output.iter().find_map(|item| match item {
            ResponseOutputItem::Message(message) => message.refusal(),
            _ => None,
        })
    }

    /// Returns details explaining why the response was incomplete.
    #[must_use]
    pub fn incomplete_details(&self) -> Option<&IncompleteDetails> {
        match &self.incomplete_details {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }

    /// Parses assistant output text into a declared Rust type.
    ///
    /// Refusal, incomplete, and failed states are routed to dedicated error
    /// variants rather than being treated as malformed JSON.
    pub fn output_parsed<T: serde::de::DeserializeOwned>(&self) -> Result<T, OutputParseError> {
        if matches!(self.status(), Some(ResponseStatus::Incomplete)) {
            let reason = self
                .incomplete_details()
                .and_then(|details| details.reason().map(|reason| reason.as_str().to_owned()));
            return Err(OutputParseError::Incomplete(reason));
        }
        if matches!(self.status(), Some(ResponseStatus::Failed)) {
            let error = match &self.error {
                Nullable::Value(error) => error.clone(),
                // The pin pairs `status: "failed"` with a populated error
                // object; keep a readable fallback for the degenerate null.
                Nullable::Null => ResponseError {
                    code: ResponseErrorCode::from_raw("failed_without_error"),
                    message: "response failed without an error payload".to_owned(),
                    extra: ExtraFields::new(),
                },
            };
            return Err(OutputParseError::Failed(error));
        }
        if let Some(refusal) = self.refusal() {
            return Err(OutputParseError::Refusal(refusal.to_owned()));
        }
        let text = self.output_text();
        if text.is_empty() {
            return Err(OutputParseError::NoTextOutput);
        }
        serde_json::from_str(&text).map_err(OutputParseError::Decode)
    }

    /// Converts replayable output items into the corresponding input items.
    ///
    /// Resource-only fields such as `created_by` have no typed slot on the
    /// sendable input schemas and are deliberately not given builder copies
    /// (D0030-3). Replay stays lossless anyway: those fields are carried
    /// through the input item's `extra`, matching the bytes the conversation
    /// JSON round-trip path and the shared item structs retain.
    #[must_use]
    pub fn to_input_items(&self) -> Vec<ResponseInputItem> {
        self.output
            .iter()
            .map(|item| match item {
                ResponseOutputItem::Message(value) => {
                    ResponseInputItem::OutputMessage(value.clone())
                }
                ResponseOutputItem::FunctionCall(value) => {
                    ResponseInputItem::FunctionCall(value.clone())
                }
                ResponseOutputItem::FunctionCallOutput(value) => {
                    ResponseInputItem::FunctionCallOutput(FunctionCallOutput {
                        kind: FunctionCallOutputTag::FunctionCallOutput,
                        call_id: value.call_id.clone(),
                        output: value.output.clone().into(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        name: value.name.clone(),
                        namespace: value.namespace.clone(),
                        caller: value.caller.clone(),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::FileSearchCall(value) => {
                    ResponseInputItem::FileSearchCall(value.clone())
                }
                ResponseOutputItem::WebSearchCall(value) => {
                    ResponseInputItem::WebSearchCall(value.clone())
                }
                ResponseOutputItem::ComputerCall(value) => {
                    ResponseInputItem::ComputerCall(value.clone())
                }
                ResponseOutputItem::ComputerCallOutput(value) => {
                    ResponseInputItem::ComputerCallOutput(ComputerCallOutput {
                        kind: ComputerCallOutputTag::ComputerCallOutput,
                        call_id: value.call_id.clone(),
                        output: value.output.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        acknowledged_safety_checks: value.acknowledged_safety_checks.clone(),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::Reasoning(value) => ResponseInputItem::Reasoning(value.clone()),
                ResponseOutputItem::Program(value) => ResponseInputItem::Program(value.clone()),
                ResponseOutputItem::ProgramOutput(value) => {
                    ResponseInputItem::ProgramOutput(value.clone())
                }
                ResponseOutputItem::ToolSearchCall(value) => {
                    ResponseInputItem::ToolSearchCall(ToolSearchCallInput {
                        kind: ToolSearchCallInputTag::ToolSearchCall,
                        arguments: value.arguments.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        call_id: Omittable::Value(value.call_id.clone()),
                        execution: Omittable::Value(value.execution.clone()),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::ToolSearchOutput(value) => {
                    ResponseInputItem::ToolSearchOutput(ToolSearchOutputInput {
                        kind: ToolSearchOutputInputTag::ToolSearchOutput,
                        tools: value.tools.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        call_id: Omittable::Value(value.call_id.clone()),
                        execution: Omittable::Value(value.execution.clone()),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::AdditionalTools(value) => {
                    ResponseInputItem::AdditionalTools(AdditionalToolsInput {
                        kind: AdditionalToolsInputTag::AdditionalTools,
                        role: value.role.clone(),
                        tools: value.tools.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        extra: value.extra.clone(),
                    })
                }
                ResponseOutputItem::Compaction(value) => {
                    ResponseInputItem::Compaction(CompactionSummaryInput {
                        kind: CompactionSummaryInputTag::Compaction,
                        encrypted_content: value.encrypted_content.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::ImageGenerationCall(value) => {
                    ResponseInputItem::ImageGenerationCall(value.clone())
                }
                ResponseOutputItem::CodeInterpreterCall(value) => {
                    ResponseInputItem::CodeInterpreterCall(value.clone())
                }
                ResponseOutputItem::LocalShellCall(value) => {
                    ResponseInputItem::LocalShellCall(value.clone())
                }
                ResponseOutputItem::LocalShellCallOutput(value) => {
                    ResponseInputItem::LocalShellCallOutput(value.clone())
                }
                ResponseOutputItem::FunctionShellCall(value) => {
                    ResponseInputItem::FunctionShellCall(FunctionShellCallInput {
                        kind: FunctionShellCallInputTag::FunctionShellCall,
                        call_id: value.call_id.clone(),
                        action: FunctionShellActionParam::from(value.action.clone()),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        caller: value.caller.clone(),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        environment: Omittable::Value(value.environment.clone()),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::FunctionShellCallOutput(value) => {
                    ResponseInputItem::FunctionShellCallOutput(FunctionShellCallOutputInput {
                        kind: FunctionShellCallOutputInputTag::FunctionShellCallOutput,
                        call_id: value.call_id.clone(),
                        output: value.output.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        caller: value.caller.clone(),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        max_output_length: Omittable::Value(value.max_output_length),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::ApplyPatchCall(value) => {
                    ResponseInputItem::ApplyPatchCall(ApplyPatchCallInput {
                        kind: ApplyPatchCallInputTag::ApplyPatchCall,
                        call_id: value.call_id.clone(),
                        status: value.status.clone(),
                        operation: value.operation.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        caller: value.caller.clone(),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::ApplyPatchCallOutput(value) => {
                    ResponseInputItem::ApplyPatchCallOutput(ApplyPatchCallOutputInput {
                        kind: ApplyPatchCallOutputInputTag::ApplyPatchCallOutput,
                        call_id: value.call_id.clone(),
                        status: value.status.clone(),
                        output: value.output.clone(),
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        caller: value.caller.clone(),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::McpListTools(value) => {
                    ResponseInputItem::McpListTools(value.clone())
                }
                ResponseOutputItem::McpCall(value) => ResponseInputItem::McpCall(value.clone()),
                ResponseOutputItem::McpApprovalRequest(value) => {
                    ResponseInputItem::McpApprovalRequest(value.clone())
                }
                ResponseOutputItem::McpApprovalResponse(value) => {
                    ResponseInputItem::McpApprovalResponse(McpApprovalResponse {
                        kind: McpApprovalResponseTag::McpApprovalResponse,
                        approval_request_id: value.approval_request_id.clone(),
                        approve: value.approve,
                        id: Omittable::Value(Nullable::Value(value.id.clone())),
                        reason: value.reason.clone(),
                        extra: value.extra.clone(),
                    })
                }
                ResponseOutputItem::CustomToolCall(value) => {
                    ResponseInputItem::CustomToolCall(value.clone())
                }
                ResponseOutputItem::CustomToolCallOutput(value) => {
                    ResponseInputItem::CustomToolCallOutput(CustomToolCallOutput {
                        kind: CustomToolCallOutputTag::CustomToolCallOutput,
                        call_id: value.call_id.clone(),
                        output: value.output.clone(),
                        id: Omittable::Value(value.id.clone()),
                        caller: value.caller.clone(),
                        status: Omittable::Value(Nullable::Value(value.status.clone())),
                        extra: replay_created_by(&value.extra, &value.created_by),
                    })
                }
                ResponseOutputItem::Unknown(value) => ResponseInputItem::Unknown(value.clone()),
            })
            .collect()
    }

    /// Returns usage when the property was present and non-null.
    #[must_use]
    pub fn usage(&self) -> Option<&ResponseUsage> {
        match &self.usage {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns a model-generation error when non-null.
    #[must_use]
    pub fn error(&self) -> Option<&ResponseError> {
        match &self.error {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Merges one resource-only replay field into an input item's extra map.
///
/// Resource-only keys decode into named fields of the resource struct, so
/// they can never already exist in `extra`; the reserved-key check is a
/// structural assertion that never fires.
fn replay_resource_field(extra: &ExtraFields, key: &str, value: &str) -> ExtraFields {
    let mut fields = Map::with_capacity(extra.len() + 1);
    for (extra_key, extra_value) in extra.iter() {
        fields.insert(extra_key.to_owned(), extra_value.clone());
    }
    fields.insert(key.to_owned(), Value::String(value.to_owned()));
    ExtraFields::try_from_map(fields, std::iter::empty::<&str>()).unwrap_or_else(|_| extra.clone())
}

/// Replays a resource `created_by` through the input item's extra map.
///
/// D0030-3 keeps `created_by` resource-side without a sendable input copy;
/// replay conversions stay lossless by carrying it in `extra`, matching the
/// conversation JSON round-trip and the shared item structs.
fn replay_created_by(extra: &ExtraFields, created_by: &Omittable<String>) -> ExtraFields {
    match created_by {
        Omittable::Value(value) => replay_resource_field(extra, "created_by", value),
        Omittable::Omitted => extra.clone(),
    }
}

/// Merges retained unknown properties from a replay source into an input
/// item's extra map.
///
/// Used by cross-API replay conversions (for example the Conversations to
/// Responses message path) so top-level future fields survive the rebuild.
pub(crate) fn merge_extra_fields(base: &ExtraFields, retained: &ExtraFields) -> ExtraFields {
    if retained.is_empty() {
        return base.clone();
    }
    let mut fields = Map::with_capacity(base.len() + retained.len());
    for (key, value) in base.iter() {
        fields.insert(key.to_owned(), value.clone());
    }
    for (key, value) in retained.iter() {
        fields.insert(key.to_owned(), value.clone());
    }
    ExtraFields::try_from_map(fields, std::iter::empty::<&str>()).unwrap_or_else(|_| base.clone())
}

/// JSON body returned by deployments that represent response deletion.
///
/// Some official SDKs expose a successful delete as an empty body instead;
/// the client transport decides between those two success representations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedResponse {
    id: String,
    #[serde(rename = "object")]
    object: ResponseObjectTag,
    deleted: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl DeletedResponse {
    /// Returns the deleted response id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the service's deletion flag.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request body for `POST /responses/compact`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactResponseRequest {
    model: Nullable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<ResponseInput>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<Nullable<PromptCacheOptionsParam>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<PromptCacheRetention>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<Nullable<CompactServiceTier>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompactResponseRequest {
    /// Creates a compact request that sends official required `model: null`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            model: Nullable::Null,
            input: Omittable::Omitted,
            instructions: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a compact request with model and input.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<ResponseInput>) -> Self {
        Self {
            model: Nullable::Value(model.into()),
            input: Omittable::Value(Nullable::Value(input.into())),
            instructions: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the model id.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Nullable::Value(model.into());
        self
    }

    /// Sends `model: null`.
    #[must_use]
    pub fn model_null(mut self) -> Self {
        self.model = Nullable::Null;
        self
    }

    /// Sets the input to compact.
    #[must_use]
    pub fn input(mut self, input: impl Into<ResponseInput>) -> Self {
        self.input = Omittable::Value(Nullable::Value(input.into()));
        self
    }

    /// Sends `input: null`.
    #[must_use]
    pub fn input_null(mut self) -> Self {
        self.input = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets compaction instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(Nullable::Value(instructions.into()));
        self
    }

    /// Sends `instructions: null`.
    #[must_use]
    pub fn instructions_null(mut self) -> Self {
        self.instructions = Omittable::Value(Nullable::Null);
        self
    }

    /// Continues from a stored response.
    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends `previous_response_id: null`.
    #[must_use]
    pub fn previous_response_id_null(mut self) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets a prompt-cache key.
    #[must_use]
    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.prompt_cache_key = Omittable::Value(Nullable::Value(key.into()));
        self
    }

    /// Sends `prompt_cache_key: null`.
    #[must_use]
    pub fn prompt_cache_key_null(mut self) -> Self {
        self.prompt_cache_key = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets prompt-cache options.
    #[must_use]
    pub fn prompt_cache_options(mut self, options: PromptCacheOptionsParam) -> Self {
        self.prompt_cache_options = Omittable::Value(Nullable::Value(options));
        self
    }

    /// Sends `prompt_cache_options: null`.
    #[must_use]
    pub fn prompt_cache_options_null(mut self) -> Self {
        self.prompt_cache_options = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the deprecated prompt-cache retention policy.
    #[must_use]
    pub fn prompt_cache_retention(mut self, retention: PromptCacheRetention) -> Self {
        self.prompt_cache_retention = Omittable::Value(Nullable::Value(retention));
        self
    }

    /// Sends `prompt_cache_retention: null`.
    #[must_use]
    pub fn prompt_cache_retention_null(mut self) -> Self {
        self.prompt_cache_retention = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the requested service tier.
    ///
    /// The compact body pins the five-value `ServiceTierEnum`; the create
    /// side's wider [`ServiceTier`] domain (with `scale` / `ultrafast`) does
    /// not apply here.
    #[must_use]
    pub fn service_tier(mut self, service_tier: CompactServiceTier) -> Self {
        self.service_tier = Omittable::Value(Nullable::Value(service_tier));
        self
    }

    /// Sends `service_tier: null`.
    #[must_use]
    pub fn service_tier_null(mut self) -> Self {
        self.service_tier = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CompactResponseConstraintError> {
        if let Omittable::Value(Nullable::Value(key)) = &self.prompt_cache_key {
            let actual = key.chars().count();
            if actual > MAX_PROMPT_CACHE_KEY_CHARS {
                return Err(CompactResponseConstraintError::PromptCacheKey {
                    actual,
                    maximum: MAX_PROMPT_CACHE_KEY_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(ResponseInput::Text(input))) = &self.input {
            let actual = input.chars().count();
            if actual > MAX_COMPACT_INPUT_CHARS {
                return Err(CompactResponseConstraintError::InputLength {
                    actual,
                    maximum: MAX_COMPACT_INPUT_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(ResponseInput::Items(items))) = &self.input {
            for item in items {
                validate_response_input_item(item)?;
            }
        }
        Ok(())
    }
}

/// A compact-request value that violates a pinned OpenAPI constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CompactResponseConstraintError {
    /// `prompt_cache_key` exceeds 64 characters.
    #[error("prompt_cache_key has {actual} characters; maximum is {maximum}")]
    PromptCacheKey {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Compact `input` string exceeds the pinned 10 MiB character limit.
    #[error("compact input has {actual} characters; maximum is {maximum}")]
    InputLength {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Nested official `InputItem` / `Tool` constraint on compact `input`.
    #[error(transparent)]
    Input(#[from] CreateResponseConstraintError),
}

literal_tag!(CompactedResponseTag, Compaction, "response.compaction");

/// A compacted Responses resource.
///
/// `output` follows the pinned `CompactResource.output.items` → `ItemField`
/// union: compaction returns the conversation's user-role messages plus a
/// final compaction item, so items decode with the input-side codec where
/// user/system/developer roles and the `CompactionSummaryItemParam` shape
/// are accepted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactedResponse {
    id: String,
    created_at: i64,
    #[serde(rename = "object")]
    object: CompactedResponseTag,
    output: Vec<ResponseInputItem>,
    usage: ResponseUsage,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompactedResponse {
    /// Returns the compacted response id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns compacted output items in order.
    #[must_use]
    pub fn output(&self) -> &[ResponseInputItem] {
        &self.output
    }

    /// Returns official token accounting for the compaction pass.
    #[must_use]
    pub const fn usage(&self) -> &ResponseUsage {
        &self.usage
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A response input-item page size below the documented floor of 1.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("response input item list limit must be at least 1, got {actual}")]
pub struct ListResponseInputItemsLimitError {
    /// Rejected page size.
    actual: u32,
}

impl ListResponseInputItemsLimitError {
    /// Returns the rejected page size.
    #[must_use]
    pub const fn actual(self) -> u32 {
        self.actual
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct ListResponseInputItemsParamsWire {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<ResponseIncludable>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<ResponseItemOrder>,
}

/// Query parameters for a response's input-item page.
///
/// The pinned `limit` prose documents a 1..=100 range with a default of 20;
/// only the `>= 1` floor is enforced here because the ceiling exists solely
/// in that descriptive prose (see D0154/D0174 for the same stance elsewhere).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ListResponseInputItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<ResponseIncludable>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<ResponseItemOrder>,
}

impl<'de> Deserialize<'de> for ListResponseInputItemsParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ListResponseInputItemsParamsWire::deserialize(deserializer)?;
        if let Omittable::Value(limit) = wire.limit
            && limit == 0
        {
            return Err(D::Error::custom(ListResponseInputItemsLimitError {
                actual: limit,
            }));
        }
        Ok(Self {
            after: wire.after,
            include: wire.include,
            limit: wire.limit,
            order: wire.order,
        })
    }
}

impl ListResponseInputItemsParams {
    /// Creates empty list parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts after an opaque item id.
    #[must_use]
    pub fn after(mut self, after: impl Into<String>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Adds an optional response field to include.
    #[must_use]
    pub fn include(mut self, include: impl Into<ResponseIncludable>) -> Self {
        self.include.push(include.into());
        self
    }

    /// Returns response fields requested for inclusion.
    #[must_use]
    pub fn includes(&self) -> &[ResponseIncludable] {
        &self.include
    }

    /// Sets the requested page size.
    ///
    /// The pinned prose documents a 1..=100 range with a default of 20 when
    /// omitted; this builder rejects `0` and leaves the descriptive ceiling
    /// unenforced.
    pub fn limit(mut self, limit: u32) -> Result<Self, ListResponseInputItemsLimitError> {
        if limit == 0 {
            return Err(ListResponseInputItemsLimitError { actual: limit });
        }
        self.limit = Omittable::Value(limit);
        Ok(self)
    }

    /// Sets ascending or descending ordering.
    #[must_use]
    pub fn order(mut self, order: impl Into<ResponseItemOrder>) -> Self {
        self.order = Omittable::Value(order.into());
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

/// A cursor page of stored response input items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputItemList {
    #[serde(rename = "object")]
    object: ListObjectTag,
    data: Vec<ResponseInputItem>,
    first_id: String,
    last_id: String,
    has_more: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseInputItemList {
    /// Returns page items.
    #[must_use]
    pub fn data(&self) -> &[ResponseInputItem] {
        &self.data
    }

    /// Returns whether a later page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Returns the first item id on this page.
    #[must_use]
    pub fn first_id(&self) -> &str {
        &self.first_id
    }

    /// Returns the final item id for cursor pagination.
    #[must_use]
    pub fn last_id(&self) -> &str {
        &self.last_id
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request body for `POST /responses/input_tokens`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CountInputTokensRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    conversation: Omittable<Nullable<ConversationReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<Nullable<ResponseInput>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parallel_tool_calls: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    personality: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<ReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<Nullable<ResponseTextConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tool_choice: Omittable<Nullable<ToolChoice>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Nullable<Vec<ResponseTool>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<TruncationStrategy>,
}

impl CountInputTokensRequest {
    /// Creates an empty token-count request.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a token-count request with model and input.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<ResponseInput>) -> Self {
        Self::default().model(model).input(input)
    }

    /// Sets the model id.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }

    /// Sets the input to count.
    #[must_use]
    pub fn input(mut self, input: impl Into<ResponseInput>) -> Self {
        self.input = Omittable::Value(Nullable::Value(input.into()));
        self
    }

    /// Sets instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(Nullable::Value(instructions.into()));
        self
    }

    /// Associates a stored conversation.
    ///
    /// Cannot be used in conjunction with `previous_response_id`.
    #[must_use]
    pub fn conversation(mut self, conversation: impl Into<ConversationReference>) -> Self {
        self.conversation = Omittable::Value(Nullable::Value(conversation.into()));
        self
    }

    /// Sends `conversation: null`.
    #[must_use]
    pub fn conversation_null(mut self) -> Self {
        self.conversation = Omittable::Value(Nullable::Null);
        self
    }

    /// Selects a model-owned personality preset.
    #[must_use]
    pub fn personality(mut self, personality: impl Into<String>) -> Self {
        self.personality = Omittable::Value(personality.into());
        self
    }

    /// Adds a function or native MCP tool.
    #[must_use]
    pub fn tool(mut self, tool: impl Into<ResponseTool>) -> Self {
        let mut tools = match std::mem::take(&mut self.tools) {
            Omittable::Value(Nullable::Value(tools)) => tools,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
        };
        tools.push(tool.into());
        self.tools = Omittable::Value(Nullable::Value(tools));
        self
    }

    /// Sets tool choice.
    #[must_use]
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Omittable::Value(Nullable::Value(tool_choice));
        self
    }

    /// Sends `model: null`.
    #[must_use]
    pub fn model_null(mut self) -> Self {
        self.model = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `input: null`.
    #[must_use]
    pub fn input_null(mut self) -> Self {
        self.input = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `instructions: null`.
    #[must_use]
    pub fn instructions_null(mut self) -> Self {
        self.instructions = Omittable::Value(Nullable::Null);
        self
    }

    /// Enables or disables parallel tool calls.
    #[must_use]
    pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.parallel_tool_calls = Omittable::Value(Nullable::Value(enabled));
        self
    }

    /// Sends `parallel_tool_calls: null`.
    #[must_use]
    pub fn parallel_tool_calls_null(mut self) -> Self {
        self.parallel_tool_calls = Omittable::Value(Nullable::Null);
        self
    }

    /// Continues from a stored response.
    ///
    /// Cannot be used in conjunction with `conversation`.
    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends `previous_response_id: null`.
    #[must_use]
    pub fn previous_response_id_null(mut self) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets reasoning configuration.
    #[must_use]
    pub fn reasoning(mut self, reasoning: ReasoningConfig) -> Self {
        self.reasoning = Omittable::Value(Nullable::Value(reasoning));
        self
    }

    /// Sends `reasoning: null`.
    #[must_use]
    pub fn reasoning_null(mut self) -> Self {
        self.reasoning = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets text output configuration.
    #[must_use]
    pub fn text(mut self, text: ResponseTextConfig) -> Self {
        self.text = Omittable::Value(Nullable::Value(text));
        self
    }

    /// Sends `text: null`.
    #[must_use]
    pub fn text_null(mut self) -> Self {
        self.text = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `tool_choice: null`.
    #[must_use]
    pub fn tool_choice_null(mut self) -> Self {
        self.tool_choice = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends `tools: null`.
    #[must_use]
    pub fn tools_null(mut self) -> Self {
        self.tools = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets truncation strategy.
    #[must_use]
    pub fn truncation(mut self, truncation: TruncationStrategy) -> Self {
        self.truncation = Omittable::Value(truncation);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CountInputTokensConstraintError> {
        if let Omittable::Value(Nullable::Value(ResponseInput::Text(input))) = &self.input {
            let actual = input.chars().count();
            if actual > MAX_COMPACT_INPUT_CHARS {
                return Err(CountInputTokensConstraintError::InputLength {
                    actual,
                    maximum: MAX_COMPACT_INPUT_CHARS,
                });
            }
        }
        if let Omittable::Value(Nullable::Value(ResponseInput::Items(items))) = &self.input {
            for item in items {
                validate_response_input_item(item)?;
            }
        }
        if let Omittable::Value(Nullable::Value(tools)) = &self.tools {
            validate_response_tools(tools)?;
        }
        Ok(())
    }
}

/// A count-tokens request value that violates a pinned OpenAPI constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CountInputTokensConstraintError {
    /// Count-tokens `input` string exceeds the pinned 10 MiB character limit.
    #[error("count-tokens input has {actual} characters; maximum is {maximum}")]
    InputLength {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Nested official `InputItem` / `Tool` constraint on count-tokens `input` or `tools`.
    #[error(transparent)]
    Input(#[from] CreateResponseConstraintError),
}

literal_tag!(InputTokenCountTag, InputTokens, "response.input_tokens");

/// Input-token count returned by the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputTokenCountResponse {
    #[serde(rename = "object")]
    object: InputTokenCountTag,
    input_tokens: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputTokenCountResponse {
    /// Returns the counted input tokens.
    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(ResponseQueuedEventTag, ResponseQueued, "response.queued");
literal_tag!(ResponseCreatedEventTag, ResponseCreated, "response.created");
literal_tag!(
    ResponseInProgressEventTag,
    ResponseInProgress,
    "response.in_progress"
);
literal_tag!(
    ResponseCompletedEventTag,
    ResponseCompleted,
    "response.completed"
);
literal_tag!(ResponseFailedEventTag, ResponseFailed, "response.failed");
literal_tag!(
    ResponseIncompleteEventTag,
    ResponseIncomplete,
    "response.incomplete"
);

macro_rules! response_lifecycle_event {
    ($name:ident, $tag:ident, $variant:ident) => {
        /// A response lifecycle streaming event.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            sequence_number: u64,
            response: Response,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the monotonically increasing event sequence number.
            #[must_use]
            pub const fn sequence_number(&self) -> u64 {
                self.sequence_number
            }

            /// Returns the response snapshot carried by the event.
            #[must_use]
            pub const fn response(&self) -> &Response {
                &self.response
            }

            /// Returns future fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

response_lifecycle_event!(ResponseQueuedEvent, ResponseQueuedEventTag, ResponseQueued);
response_lifecycle_event!(
    ResponseCreatedEvent,
    ResponseCreatedEventTag,
    ResponseCreated
);
response_lifecycle_event!(
    ResponseInProgressEvent,
    ResponseInProgressEventTag,
    ResponseInProgress
);
response_lifecycle_event!(
    ResponseCompletedEvent,
    ResponseCompletedEventTag,
    ResponseCompleted
);
response_lifecycle_event!(ResponseFailedEvent, ResponseFailedEventTag, ResponseFailed);
response_lifecycle_event!(
    ResponseIncompleteEvent,
    ResponseIncompleteEventTag,
    ResponseIncomplete
);

literal_tag!(
    OutputItemAddedEventTag,
    OutputItemAdded,
    "response.output_item.added"
);
literal_tag!(
    OutputItemDoneEventTag,
    OutputItemDone,
    "response.output_item.done"
);

macro_rules! output_item_event {
    ($name:ident, $tag:ident, $variant:ident) => {
        /// A streaming event carrying a complete output item snapshot.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            output_index: u64,
            item: ResponseOutputItem,
            sequence_number: u64,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the output array index.
            #[must_use]
            pub const fn output_index(&self) -> u64 {
                self.output_index
            }

            /// Returns the output item snapshot.
            #[must_use]
            pub const fn item(&self) -> &ResponseOutputItem {
                &self.item
            }

            /// Returns the event sequence number.
            #[must_use]
            pub const fn sequence_number(&self) -> u64 {
                self.sequence_number
            }
        }
    };
}

output_item_event!(
    OutputItemAddedEvent,
    OutputItemAddedEventTag,
    OutputItemAdded
);
output_item_event!(OutputItemDoneEvent, OutputItemDoneEventTag, OutputItemDone);

literal_tag!(
    ContentPartAddedEventTag,
    ContentPartAdded,
    "response.content_part.added"
);
literal_tag!(
    ContentPartDoneEventTag,
    ContentPartDone,
    "response.content_part.done"
);

macro_rules! content_part_event {
    ($name:ident, $tag:ident, $variant:ident) => {
        /// A streaming event carrying one assistant content part.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            item_id: String,
            output_index: u64,
            content_index: u64,
            part: OutputContent,
            sequence_number: u64,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the containing item id.
            #[must_use]
            pub fn item_id(&self) -> &str {
                &self.item_id
            }

            /// Returns the output array index.
            #[must_use]
            pub const fn output_index(&self) -> u64 {
                self.output_index
            }

            /// Returns the content array index.
            #[must_use]
            pub const fn content_index(&self) -> u64 {
                self.content_index
            }

            /// Returns the content part snapshot.
            #[must_use]
            pub const fn part(&self) -> &OutputContent {
                &self.part
            }

            /// Returns the event sequence number.
            #[must_use]
            pub const fn sequence_number(&self) -> u64 {
                self.sequence_number
            }
        }
    };
}

content_part_event!(
    ContentPartAddedEvent,
    ContentPartAddedEventTag,
    ContentPartAdded
);
content_part_event!(
    ContentPartDoneEvent,
    ContentPartDoneEventTag,
    ContentPartDone
);

literal_tag!(
    OutputTextDeltaEventTag,
    OutputTextDelta,
    "response.output_text.delta"
);
literal_tag!(
    OutputTextDoneEventTag,
    OutputTextDone,
    "response.output_text.done"
);

/// Incremental assistant text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputTextDeltaEvent {
    #[serde(rename = "type")]
    kind: OutputTextDeltaEventTag,
    item_id: String,
    output_index: u64,
    content_index: u64,
    delta: String,
    sequence_number: u64,
    #[serde(default)]
    logprobs: Vec<EventLogProb>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputTextDeltaEvent {
    /// Returns the containing item id.
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Returns the output array index.
    #[must_use]
    pub const fn output_index(&self) -> u64 {
        self.output_index
    }

    /// Returns the content array index.
    #[must_use]
    pub const fn content_index(&self) -> u64 {
        self.content_index
    }

    /// Returns the incremental text.
    #[must_use]
    pub fn delta(&self) -> &str {
        &self.delta
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    /// Returns logprobs if included.
    #[must_use]
    pub fn logprobs(&self) -> &[EventLogProb] {
        &self.logprobs
    }
}

/// Final assistant text for one content part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputTextDoneEvent {
    #[serde(rename = "type")]
    kind: OutputTextDoneEventTag,
    item_id: String,
    output_index: u64,
    content_index: u64,
    text: String,
    sequence_number: u64,
    #[serde(default)]
    logprobs: Vec<EventLogProb>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputTextDoneEvent {
    /// Returns the completed text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    /// Returns logprobs if included.
    #[must_use]
    pub fn logprobs(&self) -> &[EventLogProb] {
        &self.logprobs
    }
}

literal_tag!(RefusalDeltaEventTag, RefusalDelta, "response.refusal.delta");
literal_tag!(RefusalDoneEventTag, RefusalDone, "response.refusal.done");

/// Incremental refusal text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefusalDeltaEvent {
    #[serde(rename = "type")]
    kind: RefusalDeltaEventTag,
    item_id: String,
    output_index: u64,
    content_index: u64,
    delta: String,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RefusalDeltaEvent {
    /// Returns the incremental refusal text.
    #[must_use]
    pub fn delta(&self) -> &str {
        &self.delta
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }
}

/// Final refusal text for one content part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefusalDoneEvent {
    #[serde(rename = "type")]
    kind: RefusalDoneEventTag,
    item_id: String,
    output_index: u64,
    content_index: u64,
    refusal: String,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RefusalDoneEvent {
    /// Returns the completed refusal text.
    #[must_use]
    pub fn refusal(&self) -> &str {
        &self.refusal
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }
}

literal_tag!(
    FunctionCallArgumentsDeltaEventTag,
    FunctionCallArgumentsDelta,
    "response.function_call_arguments.delta"
);
literal_tag!(
    FunctionCallArgumentsDoneEventTag,
    FunctionCallArgumentsDone,
    "response.function_call_arguments.done"
);

/// Incremental JSON text for a function call's arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallArgumentsDeltaEvent {
    #[serde(rename = "type")]
    kind: FunctionCallArgumentsDeltaEventTag,
    item_id: String,
    output_index: u64,
    delta: String,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCallArgumentsDeltaEvent {
    /// Returns the incremental, potentially incomplete JSON string.
    #[must_use]
    pub fn delta(&self) -> &str {
        &self.delta
    }

    /// Returns the containing item id.
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }
}

/// Final JSON argument string for a function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallArgumentsDoneEvent {
    #[serde(rename = "type")]
    kind: FunctionCallArgumentsDoneEventTag,
    item_id: String,
    output_index: u64,
    name: String,
    arguments: JsonText,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCallArgumentsDoneEvent {
    /// Returns the complete, lazily parsed JSON argument string.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }
}

literal_tag!(
    McpCallArgumentsDeltaEventTag,
    McpCallArgumentsDelta,
    "response.mcp_call_arguments.delta"
);
literal_tag!(
    McpCallArgumentsDoneEventTag,
    McpCallArgumentsDone,
    "response.mcp_call_arguments.done"
);

/// Incremental JSON text for a native remote MCP call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCallArgumentsDeltaEvent {
    #[serde(rename = "type")]
    kind: McpCallArgumentsDeltaEventTag,
    item_id: String,
    output_index: u64,
    delta: String,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpCallArgumentsDeltaEvent {
    /// Returns the incremental argument text.
    #[must_use]
    pub fn delta(&self) -> &str {
        &self.delta
    }
}

/// Final JSON argument string for a native remote MCP call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCallArgumentsDoneEvent {
    #[serde(rename = "type")]
    kind: McpCallArgumentsDoneEventTag,
    item_id: String,
    output_index: u64,
    arguments: JsonText,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpCallArgumentsDoneEvent {
    /// Returns the complete, lazily parsed MCP argument string.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }
}

literal_tag!(
    McpCallInProgressEventTag,
    McpCallInProgress,
    "response.mcp_call.in_progress"
);
literal_tag!(
    McpCallCompletedEventTag,
    McpCallCompleted,
    "response.mcp_call.completed"
);
literal_tag!(
    McpCallFailedEventTag,
    McpCallFailed,
    "response.mcp_call.failed"
);
literal_tag!(
    McpListToolsInProgressEventTag,
    McpListToolsInProgress,
    "response.mcp_list_tools.in_progress"
);
literal_tag!(
    McpListToolsCompletedEventTag,
    McpListToolsCompleted,
    "response.mcp_list_tools.completed"
);
literal_tag!(
    McpListToolsFailedEventTag,
    McpListToolsFailed,
    "response.mcp_list_tools.failed"
);

macro_rules! tool_status_event {
    ($name:ident, $tag:ident, $variant:ident) => {
        /// A native remote MCP tool lifecycle event.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            item_id: String,
            output_index: u64,
            sequence_number: u64,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the affected output item id.
            #[must_use]
            pub fn item_id(&self) -> &str {
                &self.item_id
            }

            /// Returns the output array index.
            #[must_use]
            pub const fn output_index(&self) -> u64 {
                self.output_index
            }

            /// Returns the event sequence number.
            #[must_use]
            pub const fn sequence_number(&self) -> u64 {
                self.sequence_number
            }
        }
    };
}

tool_status_event!(
    McpCallInProgressEvent,
    McpCallInProgressEventTag,
    McpCallInProgress
);
tool_status_event!(
    McpCallCompletedEvent,
    McpCallCompletedEventTag,
    McpCallCompleted
);
tool_status_event!(McpCallFailedEvent, McpCallFailedEventTag, McpCallFailed);
tool_status_event!(
    McpListToolsInProgressEvent,
    McpListToolsInProgressEventTag,
    McpListToolsInProgress
);
tool_status_event!(
    McpListToolsCompletedEvent,
    McpListToolsCompletedEventTag,
    McpListToolsCompleted
);
tool_status_event!(
    McpListToolsFailedEvent,
    McpListToolsFailedEventTag,
    McpListToolsFailed
);

literal_tag!(StreamErrorEventTag, Error, "error");

/// A standalone streaming error event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamErrorEvent {
    #[serde(rename = "type")]
    kind: StreamErrorEventTag,
    code: Nullable<String>,
    message: String,
    param: Nullable<String>,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl StreamErrorEvent {
    /// Returns the official error code when the service sent a string.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match &self.code {
            Nullable::Value(code) => Some(code.as_str()),
            Nullable::Null => None,
        }
    }

    /// Returns the offending request parameter when the service sent a string.
    #[must_use]
    pub fn param(&self) -> Option<&str> {
        match &self.param {
            Nullable::Value(param) => Some(param.as_str()),
            Nullable::Null => None,
        }
    }

    /// Returns the human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the event sequence number.
    #[must_use]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

tagged_union! {
    /// A major event emitted by the stable Responses SSE protocol.
    ///
    /// Unknown future event types remain usable through [`Self::Unknown`]. A
    /// malformed payload for any listed tag is a decoding error.
    pub enum ResponseStreamEvent {
        AudioDelta(AudioDeltaEvent) => "response.audio.delta",
        AudioDone(AudioDoneEvent) => "response.audio.done",
        AudioTranscriptDelta(AudioTranscriptDeltaEvent) => "response.audio.transcript.delta",
        AudioTranscriptDone(AudioTranscriptDoneEvent) => "response.audio.transcript.done",
        CodeInterpreterCodeDelta(CodeInterpreterCodeDeltaEvent) => "response.code_interpreter_call_code.delta",
        CodeInterpreterCodeDone(CodeInterpreterCodeDoneEvent) => "response.code_interpreter_call_code.done",
        CodeInterpreterCompleted(CodeInterpreterCompletedEvent) => "response.code_interpreter_call.completed",
        CodeInterpreterInProgress(CodeInterpreterInProgressEvent) => "response.code_interpreter_call.in_progress",
        CodeInterpreterInterpreting(CodeInterpreterInterpretingEvent) => "response.code_interpreter_call.interpreting",
        Queued(ResponseQueuedEvent) => "response.queued",
        Created(ResponseCreatedEvent) => "response.created",
        InProgress(ResponseInProgressEvent) => "response.in_progress",
        Completed(ResponseCompletedEvent) => "response.completed",
        Failed(ResponseFailedEvent) => "response.failed",
        Incomplete(ResponseIncompleteEvent) => "response.incomplete",
        OutputItemAdded(OutputItemAddedEvent) => "response.output_item.added",
        OutputItemDone(OutputItemDoneEvent) => "response.output_item.done",
        ContentPartAdded(ContentPartAddedEvent) => "response.content_part.added",
        ContentPartDone(ContentPartDoneEvent) => "response.content_part.done",
        OutputTextDelta(OutputTextDeltaEvent) => "response.output_text.delta",
        OutputTextDone(OutputTextDoneEvent) => "response.output_text.done",
        RefusalDelta(RefusalDeltaEvent) => "response.refusal.delta",
        RefusalDone(RefusalDoneEvent) => "response.refusal.done",
        FunctionCallArgumentsDelta(FunctionCallArgumentsDeltaEvent) => "response.function_call_arguments.delta",
        FunctionCallArgumentsDone(FunctionCallArgumentsDoneEvent) => "response.function_call_arguments.done",
        FileSearchCompleted(FileSearchCompletedEvent) => "response.file_search_call.completed",
        FileSearchInProgress(FileSearchInProgressEvent) => "response.file_search_call.in_progress",
        FileSearchSearching(FileSearchSearchingEvent) => "response.file_search_call.searching",
        ShellCommandAdded(ShellCommandAddedEvent) => "response.shell_call_command.added",
        ShellCommandDelta(ShellCommandDeltaEvent) => "response.shell_call_command.delta",
        ShellCommandDone(ShellCommandDoneEvent) => "response.shell_call_command.done",
        ShellOutputContentDelta(ShellOutputContentDeltaEvent) => "response.shell_call_output_content.delta",
        ShellOutputContentDone(ShellOutputContentDoneEvent) => "response.shell_call_output_content.done",
        ReasoningSummaryPartAdded(ReasoningSummaryPartAddedEvent) => "response.reasoning_summary_part.added",
        ReasoningSummaryPartDone(ReasoningSummaryPartDoneEvent) => "response.reasoning_summary_part.done",
        ReasoningSummaryTextDelta(ReasoningSummaryTextDeltaEvent) => "response.reasoning_summary_text.delta",
        ReasoningSummaryTextDone(ReasoningSummaryTextDoneEvent) => "response.reasoning_summary_text.done",
        ReasoningTextDelta(ReasoningTextDeltaEvent) => "response.reasoning_text.delta",
        ReasoningTextDone(ReasoningTextDoneEvent) => "response.reasoning_text.done",
        WebSearchCompleted(WebSearchCompletedEvent) => "response.web_search_call.completed",
        WebSearchInProgress(WebSearchInProgressEvent) => "response.web_search_call.in_progress",
        WebSearchSearching(WebSearchSearchingEvent) => "response.web_search_call.searching",
        ImageGenerationCompleted(ImageGenerationCompletedEvent) => "response.image_generation_call.completed",
        ImageGenerationGenerating(ImageGenerationGeneratingEvent) => "response.image_generation_call.generating",
        ImageGenerationInProgress(ImageGenerationInProgressEvent) => "response.image_generation_call.in_progress",
        ImageGenerationPartialImage(ImageGenerationPartialImageEvent) => "response.image_generation_call.partial_image",
        McpCallArgumentsDelta(McpCallArgumentsDeltaEvent) => "response.mcp_call_arguments.delta",
        McpCallArgumentsDone(McpCallArgumentsDoneEvent) => "response.mcp_call_arguments.done",
        McpCallInProgress(McpCallInProgressEvent) => "response.mcp_call.in_progress",
        McpCallCompleted(McpCallCompletedEvent) => "response.mcp_call.completed",
        McpCallFailed(McpCallFailedEvent) => "response.mcp_call.failed",
        McpListToolsInProgress(McpListToolsInProgressEvent) => "response.mcp_list_tools.in_progress",
        McpListToolsCompleted(McpListToolsCompletedEvent) => "response.mcp_list_tools.completed",
        McpListToolsFailed(McpListToolsFailedEvent) => "response.mcp_list_tools.failed",
        OutputTextAnnotationAdded(OutputTextAnnotationAddedEvent) => "response.output_text.annotation.added",
        CustomToolCallInputDelta(CustomToolCallInputDeltaEvent) => "response.custom_tool_call_input.delta",
        CustomToolCallInputDone(CustomToolCallInputDoneEvent) => "response.custom_tool_call_input.done",
        Error(StreamErrorEvent) => "error"
    }
}

impl ResponseStreamEvent {
    /// Returns whether this event ends a response lifecycle.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed(_) | Self::Failed(_) | Self::Incomplete(_) | Self::Error(_)
        )
    }

    /// Returns whether this is the standalone SSE error event.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Returns the event sequence number when it is known to this crate.
    #[must_use]
    pub fn sequence_number(&self) -> Option<u64> {
        match self {
            Self::AudioDelta(value) => Some(value.sequence_number()),
            Self::AudioDone(value) => Some(value.sequence_number()),
            Self::AudioTranscriptDelta(value) => Some(value.sequence_number()),
            Self::AudioTranscriptDone(value) => Some(value.sequence_number()),
            Self::CodeInterpreterCodeDelta(value) => Some(value.sequence_number()),
            Self::CodeInterpreterCodeDone(value) => Some(value.sequence_number()),
            Self::CodeInterpreterCompleted(value) => Some(value.sequence_number()),
            Self::CodeInterpreterInProgress(value) => Some(value.sequence_number()),
            Self::CodeInterpreterInterpreting(value) => Some(value.sequence_number()),
            Self::Queued(value) => Some(value.sequence_number()),
            Self::Created(value) => Some(value.sequence_number()),
            Self::InProgress(value) => Some(value.sequence_number()),
            Self::Completed(value) => Some(value.sequence_number()),
            Self::Failed(value) => Some(value.sequence_number()),
            Self::Incomplete(value) => Some(value.sequence_number()),
            Self::OutputItemAdded(value) => Some(value.sequence_number()),
            Self::OutputItemDone(value) => Some(value.sequence_number()),
            Self::ContentPartAdded(value) => Some(value.sequence_number()),
            Self::ContentPartDone(value) => Some(value.sequence_number()),
            Self::OutputTextDelta(value) => Some(value.sequence_number()),
            Self::OutputTextDone(value) => Some(value.sequence_number()),
            Self::RefusalDelta(value) => Some(value.sequence_number()),
            Self::RefusalDone(value) => Some(value.sequence_number()),
            Self::FunctionCallArgumentsDelta(value) => Some(value.sequence_number()),
            Self::FunctionCallArgumentsDone(value) => Some(value.sequence_number()),
            Self::FileSearchCompleted(value) => Some(value.sequence_number()),
            Self::FileSearchInProgress(value) => Some(value.sequence_number()),
            Self::FileSearchSearching(value) => Some(value.sequence_number()),
            Self::ShellCommandAdded(value) => Some(value.sequence_number()),
            Self::ShellCommandDelta(value) => Some(value.sequence_number()),
            Self::ShellCommandDone(value) => Some(value.sequence_number()),
            Self::ShellOutputContentDelta(value) => Some(value.sequence_number()),
            Self::ShellOutputContentDone(value) => Some(value.sequence_number()),
            Self::ReasoningSummaryPartAdded(value) => Some(value.sequence_number()),
            Self::ReasoningSummaryPartDone(value) => Some(value.sequence_number()),
            Self::ReasoningSummaryTextDelta(value) => Some(value.sequence_number()),
            Self::ReasoningSummaryTextDone(value) => Some(value.sequence_number()),
            Self::ReasoningTextDelta(value) => Some(value.sequence_number()),
            Self::ReasoningTextDone(value) => Some(value.sequence_number()),
            Self::WebSearchCompleted(value) => Some(value.sequence_number()),
            Self::WebSearchInProgress(value) => Some(value.sequence_number()),
            Self::WebSearchSearching(value) => Some(value.sequence_number()),
            Self::ImageGenerationCompleted(value) => Some(value.sequence_number()),
            Self::ImageGenerationGenerating(value) => Some(value.sequence_number()),
            Self::ImageGenerationInProgress(value) => Some(value.sequence_number()),
            Self::ImageGenerationPartialImage(value) => Some(value.sequence_number()),
            Self::McpCallArgumentsDelta(value) => Some(value.sequence_number),
            Self::McpCallArgumentsDone(value) => Some(value.sequence_number),
            Self::McpCallInProgress(value) => Some(value.sequence_number()),
            Self::McpCallCompleted(value) => Some(value.sequence_number()),
            Self::McpCallFailed(value) => Some(value.sequence_number()),
            Self::McpListToolsInProgress(value) => Some(value.sequence_number()),
            Self::McpListToolsCompleted(value) => Some(value.sequence_number()),
            Self::McpListToolsFailed(value) => Some(value.sequence_number()),
            Self::OutputTextAnnotationAdded(value) => Some(value.sequence_number()),
            Self::CustomToolCallInputDelta(value) => Some(value.sequence_number()),
            Self::CustomToolCallInputDone(value) => Some(value.sequence_number()),
            Self::Error(value) => Some(value.sequence_number()),
            Self::Unknown(value) => value.raw().get("sequence_number").and_then(Value::as_u64),
        }
    }
}

#[derive(Debug, Clone)]
struct AccumulatedText {
    item_id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct AccumulatedArguments {
    item_id: String,
    arguments: String,
}

/// Stateful reducer for a single ordered Responses event stream.
#[derive(Debug, Clone, Default)]
pub struct ResponseAccumulator {
    last_sequence_number: Option<u64>,
    item_ids: BTreeMap<u64, String>,
    text: BTreeMap<(u64, u64), AccumulatedText>,
    function_arguments: BTreeMap<u64, AccumulatedArguments>,
    snapshot: Option<Response>,
    terminal: bool,
}

impl ResponseAccumulator {
    /// Creates an empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes one event, enforcing sequence and item identity invariants.
    pub fn push(&mut self, event: ResponseStreamEvent) -> Result<(), ResponseAccumulatorError> {
        if self.terminal {
            return Err(ResponseAccumulatorError::EventAfterTerminal);
        }

        if let Some(sequence_number) = event.sequence_number() {
            match self.last_sequence_number {
                Some(previous) if sequence_number == previous => {
                    return Err(ResponseAccumulatorError::DuplicateSequence { sequence_number });
                }
                Some(previous) if sequence_number < previous => {
                    return Err(ResponseAccumulatorError::NonMonotonicSequence {
                        previous,
                        received: sequence_number,
                    });
                }
                _ => self.last_sequence_number = Some(sequence_number),
            }
        }

        match event {
            ResponseStreamEvent::Queued(event) => self.accept_response(event.response, false)?,
            ResponseStreamEvent::Created(event) => self.accept_response(event.response, false)?,
            ResponseStreamEvent::InProgress(event) => {
                self.accept_response(event.response, false)?
            }
            ResponseStreamEvent::Completed(event) => self.accept_response(event.response, true)?,
            ResponseStreamEvent::Failed(event) => self.accept_response(event.response, true)?,
            ResponseStreamEvent::Incomplete(event) => self.accept_response(event.response, true)?,
            ResponseStreamEvent::OutputItemAdded(event) => {
                self.observe_output_item(event.output_index, &event.item)?;
            }
            ResponseStreamEvent::OutputItemDone(event) => {
                self.observe_output_item(event.output_index, &event.item)?;
            }
            ResponseStreamEvent::ContentPartAdded(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                if let OutputContent::Text(text) = event.part {
                    self.set_text(
                        event.output_index,
                        event.content_index,
                        event.item_id,
                        text.text,
                    )?;
                }
            }
            ResponseStreamEvent::ContentPartDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                if let OutputContent::Text(text) = event.part {
                    self.set_text(
                        event.output_index,
                        event.content_index,
                        event.item_id,
                        text.text,
                    )?;
                }
            }
            ResponseStreamEvent::OutputTextDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                self.append_text(
                    event.output_index,
                    event.content_index,
                    event.item_id,
                    &event.delta,
                )?;
            }
            ResponseStreamEvent::OutputTextDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                self.set_text(
                    event.output_index,
                    event.content_index,
                    event.item_id,
                    event.text,
                )?;
            }
            ResponseStreamEvent::RefusalDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::RefusalDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::FunctionCallArgumentsDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                self.append_function_arguments(event.output_index, event.item_id, &event.delta)?;
            }
            ResponseStreamEvent::FunctionCallArgumentsDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
                self.set_function_arguments(
                    event.output_index,
                    event.item_id,
                    event.arguments.into_raw().into(),
                )?;
            }
            ResponseStreamEvent::McpCallArgumentsDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpCallArgumentsDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpCallInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpCallCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpCallFailed(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpListToolsInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpListToolsCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::McpListToolsFailed(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CodeInterpreterCodeDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CodeInterpreterCodeDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CodeInterpreterCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CodeInterpreterInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CodeInterpreterInterpreting(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::FileSearchCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::FileSearchInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::FileSearchSearching(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ShellOutputContentDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ShellOutputContentDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningSummaryPartAdded(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningSummaryPartDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningSummaryTextDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningSummaryTextDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningTextDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ReasoningTextDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::WebSearchCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::WebSearchInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::WebSearchSearching(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ImageGenerationCompleted(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ImageGenerationGenerating(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ImageGenerationInProgress(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::ImageGenerationPartialImage(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::OutputTextAnnotationAdded(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CustomToolCallInputDelta(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::CustomToolCallInputDone(event) => {
                self.bind_item(event.output_index, &event.item_id)?;
            }
            ResponseStreamEvent::Error(event) => {
                return Err(ResponseAccumulatorError::Stream {
                    code: event.code.into_option().map(|code| code.into_boxed_str()),
                    message: event.message,
                    param: event
                        .param
                        .into_option()
                        .map(|param| param.into_boxed_str()),
                    sequence_number: event.sequence_number,
                });
            }
            ResponseStreamEvent::AudioDelta(_)
            | ResponseStreamEvent::AudioDone(_)
            | ResponseStreamEvent::AudioTranscriptDelta(_)
            | ResponseStreamEvent::AudioTranscriptDone(_)
            | ResponseStreamEvent::ShellCommandAdded(_)
            | ResponseStreamEvent::ShellCommandDelta(_)
            | ResponseStreamEvent::ShellCommandDone(_)
            | ResponseStreamEvent::Unknown(_) => {}
        }
        Ok(())
    }

    /// Returns the most recent lifecycle response snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&Response> {
        self.snapshot.as_ref()
    }

    /// Returns the latest accepted sequence number.
    #[must_use]
    pub const fn last_sequence_number(&self) -> Option<u64> {
        self.last_sequence_number
    }

    /// Aggregates assistant output text in output/content index order.
    #[must_use]
    pub fn output_text(&self) -> String {
        if self.terminal {
            if let Some(response) = &self.snapshot {
                return response.output_text();
            }
        }
        self.text.values().map(|part| part.text.as_str()).collect()
    }

    /// Returns aggregated function arguments for an output item id.
    #[must_use]
    pub fn function_arguments(&self, item_id: &str) -> Option<&str> {
        self.function_arguments
            .values()
            .find(|arguments| arguments.item_id == item_id)
            .map(|arguments| arguments.arguments.as_str())
    }

    /// Returns the terminal response, or an error if the stream ended early.
    pub fn finish(self) -> Result<Response, ResponseAccumulatorError> {
        if !self.terminal {
            return Err(ResponseAccumulatorError::MissingTerminal);
        }
        self.snapshot
            .ok_or(ResponseAccumulatorError::MissingTerminal)
    }

    fn bind_item(
        &mut self,
        output_index: u64,
        item_id: &str,
    ) -> Result<(), ResponseAccumulatorError> {
        match self.item_ids.get(&output_index) {
            Some(expected) if expected != item_id => {
                Err(ResponseAccumulatorError::ItemIdentityMismatch {
                    output_index,
                    expected: expected.clone(),
                    received: item_id.to_owned(),
                })
            }
            Some(_) => Ok(()),
            None => {
                self.item_ids.insert(output_index, item_id.to_owned());
                Ok(())
            }
        }
    }

    fn append_text(
        &mut self,
        output_index: u64,
        content_index: u64,
        item_id: String,
        delta: &str,
    ) -> Result<(), ResponseAccumulatorError> {
        let key = (output_index, content_index);
        match self.text.get_mut(&key) {
            Some(part) if part.item_id != item_id => {
                Err(ResponseAccumulatorError::ContentIdentityMismatch {
                    output_index,
                    content_index,
                    expected: part.item_id.clone(),
                    received: item_id,
                })
            }
            Some(part) => {
                part.text.push_str(delta);
                Ok(())
            }
            None => {
                self.text.insert(
                    key,
                    AccumulatedText {
                        item_id,
                        text: delta.to_owned(),
                    },
                );
                Ok(())
            }
        }
    }

    fn set_text(
        &mut self,
        output_index: u64,
        content_index: u64,
        item_id: String,
        text: String,
    ) -> Result<(), ResponseAccumulatorError> {
        let key = (output_index, content_index);
        if let Some(part) = self.text.get(&key) {
            if part.item_id != item_id {
                return Err(ResponseAccumulatorError::ContentIdentityMismatch {
                    output_index,
                    content_index,
                    expected: part.item_id.clone(),
                    received: item_id,
                });
            }
        }
        self.text.insert(key, AccumulatedText { item_id, text });
        Ok(())
    }

    fn append_function_arguments(
        &mut self,
        output_index: u64,
        item_id: String,
        delta: &str,
    ) -> Result<(), ResponseAccumulatorError> {
        match self.function_arguments.get_mut(&output_index) {
            Some(arguments) if arguments.item_id != item_id => {
                Err(ResponseAccumulatorError::ItemIdentityMismatch {
                    output_index,
                    expected: arguments.item_id.clone(),
                    received: item_id,
                })
            }
            Some(arguments) => {
                arguments.arguments.push_str(delta);
                Ok(())
            }
            None => {
                self.function_arguments.insert(
                    output_index,
                    AccumulatedArguments {
                        item_id,
                        arguments: delta.to_owned(),
                    },
                );
                Ok(())
            }
        }
    }

    fn set_function_arguments(
        &mut self,
        output_index: u64,
        item_id: String,
        arguments: String,
    ) -> Result<(), ResponseAccumulatorError> {
        if let Some(current) = self.function_arguments.get(&output_index) {
            if current.item_id != item_id {
                return Err(ResponseAccumulatorError::ItemIdentityMismatch {
                    output_index,
                    expected: current.item_id.clone(),
                    received: item_id,
                });
            }
        }
        self.function_arguments
            .insert(output_index, AccumulatedArguments { item_id, arguments });
        Ok(())
    }

    fn observe_output_item(
        &mut self,
        output_index: u64,
        item: &ResponseOutputItem,
    ) -> Result<(), ResponseAccumulatorError> {
        if let Some(item_id) = response_output_item_id(item) {
            self.bind_item(output_index, item_id)?;
        }
        match item {
            ResponseOutputItem::Message(message) => {
                for (content_index, content) in message.content.iter().enumerate() {
                    if let OutputContent::Text(text) = content {
                        self.set_text(
                            output_index,
                            content_index as u64,
                            message.id.clone(),
                            text.text.clone(),
                        )?;
                    }
                }
            }
            ResponseOutputItem::FunctionCall(call) => {
                if let Some(item_id) = call.id() {
                    self.set_function_arguments(
                        output_index,
                        item_id.to_owned(),
                        call.arguments.as_raw().to_owned(),
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn accept_response(
        &mut self,
        response: Response,
        terminal: bool,
    ) -> Result<(), ResponseAccumulatorError> {
        for (output_index, item) in response.output.iter().enumerate() {
            self.observe_output_item(output_index as u64, item)?;
        }
        self.snapshot = Some(response);
        self.terminal = terminal;
        Ok(())
    }
}

fn response_output_item_id(item: &ResponseOutputItem) -> Option<&str> {
    match item {
        ResponseOutputItem::Message(value) => Some(&value.id),
        ResponseOutputItem::FileSearchCall(value) => Some(&value.id),
        ResponseOutputItem::FunctionCall(value) => value.id(),
        ResponseOutputItem::FunctionCallOutput(value) => Some(&value.id),
        ResponseOutputItem::WebSearchCall(value) => Some(&value.id),
        ResponseOutputItem::ComputerCall(value) => Some(&value.id),
        ResponseOutputItem::ComputerCallOutput(value) => Some(&value.id),
        ResponseOutputItem::Reasoning(value) => Some(&value.id),
        ResponseOutputItem::Program(value) => Some(&value.id),
        ResponseOutputItem::ProgramOutput(value) => Some(&value.id),
        ResponseOutputItem::ToolSearchCall(value) => Some(&value.id),
        ResponseOutputItem::ToolSearchOutput(value) => Some(&value.id),
        ResponseOutputItem::AdditionalTools(value) => Some(&value.id),
        ResponseOutputItem::Compaction(value) => Some(&value.id),
        ResponseOutputItem::ImageGenerationCall(value) => Some(&value.id),
        ResponseOutputItem::CodeInterpreterCall(value) => Some(&value.id),
        ResponseOutputItem::LocalShellCall(value) => Some(&value.id),
        ResponseOutputItem::LocalShellCallOutput(value) => Some(&value.id),
        ResponseOutputItem::FunctionShellCall(value) => Some(&value.id),
        ResponseOutputItem::FunctionShellCallOutput(value) => Some(&value.id),
        ResponseOutputItem::ApplyPatchCall(value) => Some(&value.id),
        ResponseOutputItem::ApplyPatchCallOutput(value) => Some(&value.id),
        ResponseOutputItem::McpCall(value) => Some(&value.id),
        ResponseOutputItem::McpListTools(value) => Some(&value.id),
        ResponseOutputItem::McpApprovalRequest(value) => Some(&value.id),
        ResponseOutputItem::McpApprovalResponse(value) => Some(&value.id),
        ResponseOutputItem::CustomToolCall(_) => None,
        ResponseOutputItem::CustomToolCallOutput(value) => Some(&value.id),
        ResponseOutputItem::Unknown(value) => value.raw().get("id").and_then(Value::as_str),
    }
}

/// Error produced while reducing a Responses event stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResponseAccumulatorError {
    /// The same sequence number was observed twice.
    #[error("duplicate response event sequence number {sequence_number}")]
    DuplicateSequence {
        /// Repeated sequence number.
        sequence_number: u64,
    },
    /// Sequence numbers moved backwards.
    #[error("response event sequence moved backwards from {previous} to {received}")]
    NonMonotonicSequence {
        /// Previously accepted sequence number.
        previous: u64,
        /// Newly received sequence number.
        received: u64,
    },
    /// An event was received after a terminal lifecycle event.
    #[error("received a response event after the terminal response")]
    EventAfterTerminal,
    /// An output index was reused for another item id.
    #[error(
        "response output index {output_index} changed item id from `{expected}` to `{received}`"
    )]
    ItemIdentityMismatch {
        /// Conflicting output index.
        output_index: u64,
        /// Item id first bound to the index.
        expected: String,
        /// Later conflicting item id.
        received: String,
    },
    /// A content index was reused for another item id.
    #[error(
        "response output/content index {output_index}/{content_index} changed item id from `{expected}` to `{received}`"
    )]
    ContentIdentityMismatch {
        /// Conflicting output index.
        output_index: u64,
        /// Conflicting content index.
        content_index: u64,
        /// Item id first bound to the index pair.
        expected: String,
        /// Later conflicting item id.
        received: String,
    },
    /// The SSE protocol emitted its standalone error event.
    #[error(
        "Responses stream error{code_clause}{param_clause} at sequence {sequence_number}: {message}",
        code_clause = .code.as_ref().map(|code| format!(" `{code}`")).unwrap_or_default(),
        param_clause = .param.as_ref().map(|param| format!(" on `{param}`")).unwrap_or_default(),
    )]
    Stream {
        /// Machine-readable service error code, or `None` when the service
        /// sent the official `code: null`.
        code: Option<Box<str>>,
        /// Human-readable service message.
        message: String,
        /// Offending request parameter, or `None` when absent or null.
        param: Option<Box<str>>,
        /// Sequence number carried by the error event.
        sequence_number: u64,
    },
    /// The stream ended without completed, failed, or incomplete response.
    #[error("Responses stream ended before a terminal response")]
    MissingTerminal,
}

// The following records complete the frozen stable union inventory. Complex
// nested payloads remain semantic `Value`s where their own schema family is
// outside this first Responses slice, but every required property and every
// discriminator is enforced here. Optional properties are retained through
// `ExtraFields`, so decoding and re-encoding stays lossless.

macro_rules! required_tagged_record {
    ($name:ident, $tag_name:ident, $tag_variant:ident, $wire:literal, {
        $($field:ident: $ty:ty),* $(,)?
    }) => {
        literal_tag!($tag_name, $tag_variant, $wire);

        #[doc = concat!("Wire record for the `", $wire, "` Responses item.")]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag_name,
            $($field: $ty,)*
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns future optional fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

literal_tag!(
    CompactionTriggerTag,
    CompactionTrigger,
    "compaction_trigger"
);

/// A request to start conversation compaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionTrigger {
    #[serde(rename = "type")]
    kind: CompactionTriggerTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompactionTrigger {
    /// Creates a tag-only compaction trigger.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: CompactionTriggerTag::CompactionTrigger,
            id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the stored item id when echoing a returned trigger.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl Default for CompactionTrigger {
    fn default() -> Self {
        Self::new()
    }
}

/// A stored-item reference accepted without a `type` discriminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemReference {
    id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ItemReference {
    /// Creates a reference to a stored item.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the stored item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

required_tagged_record!(ProgramItem, ProgramItemTag, Program, "program", {
    id: String,
    call_id: String,
    code: String,
    fingerprint: String
});

impl ProgramItem {
    /// Creates a program item.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        code: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            kind: ProgramItemTag::Program,
            id: id.into(),
            call_id: call_id.into(),
            code: code.into(),
            fingerprint: fingerprint.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Checks pinned `call_id` `1..=64` and `code` / `fingerprint` maxLength.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.call_id)?;
        let code_chars = self.code.chars().count();
        if code_chars > MAX_FUNCTION_CALL_OUTPUT_CHARS {
            return Err(CreateResponseConstraintError::ProgramCode {
                actual: code_chars,
                maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
            });
        }
        let fingerprint_chars = self.fingerprint.chars().count();
        if fingerprint_chars > MAX_FUNCTION_CALL_OUTPUT_CHARS {
            return Err(CreateResponseConstraintError::ProgramFingerprint {
                actual: fingerprint_chars,
                maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
            });
        }
        Ok(())
    }
}

open_string_enum! {
    /// Terminal status of a programmatic tool-calling program output.
    pub enum ProgramOutputStatus {
        Completed = "completed",
        Incomplete = "incomplete"
    }
}

required_tagged_record!(ProgramOutputItem, ProgramOutputItemTag, ProgramOutput, "program_output", {
    id: String,
    call_id: String,
    result: String,
    status: ProgramOutputStatus
});

impl ProgramOutputItem {
    /// Creates a program output item.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        result: impl Into<String>,
        status: impl Into<ProgramOutputStatus>,
    ) -> Self {
        Self {
            kind: ProgramOutputItemTag::ProgramOutput,
            id: id.into(),
            call_id: call_id.into(),
            result: result.into(),
            status: status.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Checks pinned `call_id` `1..=64` and `result` maxLength.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.call_id)?;
        let result_chars = self.result.chars().count();
        if result_chars > MAX_FUNCTION_CALL_OUTPUT_CHARS {
            return Err(CreateResponseConstraintError::ProgramResult {
                actual: result_chars,
                maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
            });
        }
        Ok(())
    }
}
literal_tag!(FileSearchCallTag, FileSearchCall, "file_search_call");

/// Scalar attribute attached to a file-search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum FileSearchAttributeValue {
    /// String attribute.
    String(String),
    /// Finite JSON number.
    Number(Number),
    /// Boolean attribute.
    Boolean(bool),
}

/// One retrieved file-search hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchResult {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filename: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    attributes: Omittable<Nullable<BTreeMap<String, FileSearchAttributeValue>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    score: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FileSearchResult {
    /// Creates an empty result.
    #[must_use]
    pub fn new() -> Self {
        Self {
            file_id: Omittable::Omitted,
            text: Omittable::Omitted,
            filename: Omittable::Omitted,
            attributes: Omittable::Omitted,
            score: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the file id.
    #[must_use]
    pub fn file_id(mut self, file_id: impl Into<String>) -> Self {
        self.file_id = Omittable::Value(file_id.into());
        self
    }

    /// Sets the retrieved text.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Omittable::Value(text.into());
        self
    }

    /// Sets the file name.
    #[must_use]
    pub fn filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Omittable::Value(filename.into());
        self
    }

    /// Sets file attributes.
    #[must_use]
    pub fn attributes(
        mut self,
        attributes: impl IntoIterator<Item = (impl Into<String>, FileSearchAttributeValue)>,
    ) -> Self {
        self.attributes = Omittable::Value(Nullable::Value(
            attributes.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        ));
        self
    }

    /// Sends official `attributes: null`.
    ///
    /// The pinned `VectorStoreFileAttributes` schema is
    /// `anyOf [{attribute map}, null]`, so an explicit null is an official
    /// echo form for a result without attributes.
    #[must_use]
    pub fn attributes_null(mut self) -> Self {
        self.attributes = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the relevance score.
    #[must_use]
    pub fn score(mut self, score: f64) -> Self {
        self.score = Omittable::Value(score);
        self
    }

    /// Returns the file id when present.
    #[must_use]
    pub fn file_id_ref(&self) -> Option<&str> {
        match &self.file_id {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns file attributes when present and non-null.
    #[must_use]
    pub fn attributes_ref(&self) -> Option<&BTreeMap<String, FileSearchAttributeValue>> {
        match &self.attributes {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the retrieved text when present.
    #[must_use]
    pub fn text_ref(&self) -> Option<&str> {
        match &self.text {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the relevance score when present.
    #[must_use]
    pub const fn score_ref(&self) -> Option<f64> {
        match self.score {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }
}

impl Default for FileSearchResult {
    fn default() -> Self {
        Self::new()
    }
}

/// A file-search tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchCall {
    #[serde(rename = "type")]
    kind: FileSearchCallTag,
    id: String,
    status: ResponseItemStatus,
    queries: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    results: Omittable<Nullable<Vec<FileSearchResult>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FileSearchCall {
    /// Creates a file-search call without results.
    ///
    /// `status` takes the pinned five-value [`FileSearchToolCallStatus`];
    /// decoding keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: FileSearchToolCallStatus,
        queries: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            kind: FileSearchCallTag::FileSearchCall,
            id: id.into(),
            status: status.into(),
            queries: queries.into_iter().map(Into::into).collect(),
            results: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets retrieved results.
    #[must_use]
    pub fn results(mut self, results: impl Into<Vec<FileSearchResult>>) -> Self {
        self.results = Omittable::Value(Nullable::Value(results.into()));
        self
    }

    /// Explicitly sends `results: null`.
    #[must_use]
    pub fn results_null(mut self) -> Self {
        self.results = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the search queries.
    #[must_use]
    pub fn queries(&self) -> &[String] {
        &self.queries
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns results when present.
    #[must_use]
    pub fn results_ref(&self) -> Option<&[FileSearchResult]> {
        match &self.results {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
open_string_enum! {
    /// Mouse button used by a computer-use click.
    pub enum ComputerClickButton {
        Left = "left",
        Right = "right",
        Wheel = "wheel",
        Back = "back",
        Forward = "forward"
    }
}

literal_tag!(ComputerClickTag, Click, "click");
literal_tag!(ComputerDoubleClickTag, DoubleClick, "double_click");
literal_tag!(ComputerDragTag, Drag, "drag");
literal_tag!(ComputerKeyPressTag, KeyPress, "keypress");
literal_tag!(ComputerMoveTag, Move, "move");
literal_tag!(ComputerScreenshotActionTag, Screenshot, "screenshot");
literal_tag!(ComputerScrollTag, Scroll, "scroll");
literal_tag!(ComputerTypeTag, Type, "type");
literal_tag!(ComputerWaitTag, Wait, "wait");
literal_tag!(
    ComputerScreenshotTag,
    ComputerScreenshot,
    "computer_screenshot"
);
literal_tag!(ComputerCallTag, ComputerCall, "computer_call");
literal_tag!(
    ComputerCallOutputTag,
    ComputerCallOutput,
    "computer_call_output"
);
literal_tag!(
    ComputerCallOutputResourceTag,
    ComputerCallOutputResource,
    "computer_call_output"
);

/// An x/y pair used by drag paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerCoordinate {
    x: i64,
    y: i64,
}

impl ComputerCoordinate {
    /// Creates a coordinate pair.
    #[must_use]
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    /// Returns the x coordinate.
    #[must_use]
    pub const fn x(&self) -> i64 {
        self.x
    }

    /// Returns the y coordinate.
    #[must_use]
    pub const fn y(&self) -> i64 {
        self.y
    }
}

/// A click action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerClickAction {
    #[serde(rename = "type")]
    kind: ComputerClickTag,
    button: ComputerClickButton,
    x: i64,
    y: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    keys: Omittable<Nullable<Vec<String>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerClickAction {
    /// Creates a click at `(x, y)`.
    #[must_use]
    pub fn new(button: ComputerClickButton, x: i64, y: i64) -> Self {
        Self {
            kind: ComputerClickTag::Click,
            button,
            x,
            y,
            keys: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Holds modifier keys during the click.
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys = Omittable::Value(Nullable::Value(keys.into_iter().map(Into::into).collect()));
        self
    }

    /// Sends official `keys: null`.
    #[must_use]
    pub fn keys_null(mut self) -> Self {
        self.keys = Omittable::Value(Nullable::Null);
        self
    }
}

/// A double-click action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerDoubleClickAction {
    #[serde(rename = "type")]
    kind: ComputerDoubleClickTag,
    x: i64,
    y: i64,
    keys: Nullable<Vec<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerDoubleClickAction {
    /// Creates a double-click at `(x, y)`.
    #[must_use]
    pub fn new(x: i64, y: i64) -> Self {
        Self {
            kind: ComputerDoubleClickTag::DoubleClick,
            x,
            y,
            keys: Nullable::Null,
            extra: ExtraFields::new(),
        }
    }

    /// Holds modifier keys during the double-click.
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys = Nullable::Value(keys.into_iter().map(Into::into).collect());
        self
    }

    /// Sends official required `keys: null`.
    #[must_use]
    pub fn keys_null(mut self) -> Self {
        self.keys = Nullable::Null;
        self
    }
}

/// A drag action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerDragAction {
    #[serde(rename = "type")]
    kind: ComputerDragTag,
    path: Vec<ComputerCoordinate>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    keys: Omittable<Nullable<Vec<String>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerDragAction {
    /// Creates a drag along `path`.
    #[must_use]
    pub fn new(path: impl Into<Vec<ComputerCoordinate>>) -> Self {
        Self {
            kind: ComputerDragTag::Drag,
            path: path.into(),
            keys: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Holds modifier keys during the drag.
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys = Omittable::Value(Nullable::Value(keys.into_iter().map(Into::into).collect()));
        self
    }

    /// Sends official `keys: null`.
    #[must_use]
    pub fn keys_null(mut self) -> Self {
        self.keys = Omittable::Value(Nullable::Null);
        self
    }
}

/// A keypress action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerKeyPressAction {
    #[serde(rename = "type")]
    kind: ComputerKeyPressTag,
    keys: Vec<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerKeyPressAction {
    /// Creates a keypress of the supplied keys.
    #[must_use]
    pub fn new(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind: ComputerKeyPressTag::KeyPress,
            keys: keys.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
        }
    }
}

/// A mouse-move action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerMoveAction {
    #[serde(rename = "type")]
    kind: ComputerMoveTag,
    x: i64,
    y: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    keys: Omittable<Nullable<Vec<String>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerMoveAction {
    /// Creates a move to `(x, y)`.
    #[must_use]
    pub fn new(x: i64, y: i64) -> Self {
        Self {
            kind: ComputerMoveTag::Move,
            x,
            y,
            keys: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Holds modifier keys during the move.
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys = Omittable::Value(Nullable::Value(keys.into_iter().map(Into::into).collect()));
        self
    }

    /// Sends official `keys: null`.
    #[must_use]
    pub fn keys_null(mut self) -> Self {
        self.keys = Omittable::Value(Nullable::Null);
        self
    }
}

/// A screenshot action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerScreenshotAction {
    #[serde(rename = "type")]
    kind: ComputerScreenshotActionTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerScreenshotAction {
    /// Creates a screenshot request.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ComputerScreenshotActionTag::Screenshot,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for ComputerScreenshotAction {
    fn default() -> Self {
        Self::new()
    }
}

/// A scroll action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerScrollAction {
    #[serde(rename = "type")]
    kind: ComputerScrollTag,
    x: i64,
    y: i64,
    scroll_x: i64,
    scroll_y: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    keys: Omittable<Nullable<Vec<String>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerScrollAction {
    /// Creates a scroll at `(x, y)`.
    #[must_use]
    pub fn new(x: i64, y: i64, scroll_x: i64, scroll_y: i64) -> Self {
        Self {
            kind: ComputerScrollTag::Scroll,
            x,
            y,
            scroll_x,
            scroll_y,
            keys: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Holds modifier keys during the scroll.
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys = Omittable::Value(Nullable::Value(keys.into_iter().map(Into::into).collect()));
        self
    }

    /// Sends official `keys: null`.
    #[must_use]
    pub fn keys_null(mut self) -> Self {
        self.keys = Omittable::Value(Nullable::Null);
        self
    }
}

/// A type-text action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerTypeAction {
    #[serde(rename = "type")]
    kind: ComputerTypeTag,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerTypeAction {
    /// Creates a type action.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: ComputerTypeTag::Type,
            text: text.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// A wait action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerWaitAction {
    #[serde(rename = "type")]
    kind: ComputerWaitTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerWaitAction {
    /// Creates a wait action.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ComputerWaitTag::Wait,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for ComputerWaitAction {
    fn default() -> Self {
        Self::new()
    }
}

/// A computer-use action requested by the model.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ComputerAction {
    /// Click.
    Click(ComputerClickAction),
    /// Double-click.
    DoubleClick(ComputerDoubleClickAction),
    /// Drag along a path.
    Drag(ComputerDragAction),
    /// Keypress.
    KeyPress(ComputerKeyPressAction),
    /// Mouse move.
    Move(ComputerMoveAction),
    /// Screenshot.
    Screenshot(ComputerScreenshotAction),
    /// Scroll.
    Scroll(ComputerScrollAction),
    /// Type text.
    Type(ComputerTypeAction),
    /// Wait.
    Wait(ComputerWaitAction),
    /// Future action retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ComputerAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Click(value) => value.serialize(serializer),
            Self::DoubleClick(value) => value.serialize(serializer),
            Self::Drag(value) => value.serialize(serializer),
            Self::KeyPress(value) => value.serialize(serializer),
            Self::Move(value) => value.serialize(serializer),
            Self::Screenshot(value) => value.serialize(serializer),
            Self::Scroll(value) => value.serialize(serializer),
            Self::Type(value) => value.serialize(serializer),
            Self::Wait(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ComputerAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "click" => serde_json::from_value(value)
                .map(Self::Click)
                .map_err(D::Error::custom),
            "double_click" => serde_json::from_value(value)
                .map(Self::DoubleClick)
                .map_err(D::Error::custom),
            "drag" => serde_json::from_value(value)
                .map(Self::Drag)
                .map_err(D::Error::custom),
            "keypress" => serde_json::from_value(value)
                .map(Self::KeyPress)
                .map_err(D::Error::custom),
            "move" => serde_json::from_value(value)
                .map(Self::Move)
                .map_err(D::Error::custom),
            "screenshot" => serde_json::from_value(value)
                .map(Self::Screenshot)
                .map_err(D::Error::custom),
            "scroll" => serde_json::from_value(value)
                .map(Self::Scroll)
                .map_err(D::Error::custom),
            "type" => serde_json::from_value(value)
                .map(Self::Type)
                .map_err(D::Error::custom),
            "wait" => serde_json::from_value(value)
                .map(Self::Wait)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A pending or acknowledged computer-use safety check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerSafetyCheck {
    id: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    code: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    message: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerSafetyCheck {
    /// Creates a safety check with the service-assigned id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            code: Omittable::Omitted,
            message: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the safety-check id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Sets the safety-check code.
    #[must_use]
    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Omittable::Value(Nullable::Value(code.into()));
        self
    }

    /// Sets the safety-check message.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Omittable::Value(Nullable::Value(message.into()));
        self
    }

    /// Sends official `code: null`.
    #[must_use]
    pub fn code_null(mut self) -> Self {
        self.code = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `message: null`.
    #[must_use]
    pub fn message_null(mut self) -> Self {
        self.message = Omittable::Value(Nullable::Null);
        self
    }
}

/// A computer-use screenshot sent as tool output or input content.
///
/// Tool-output `ComputerScreenshotImage` omits `detail` and treats locators as
/// optional strings. Input `ComputerScreenshotContent` requires nullable
/// locators plus `detail`, and may include `prompt_cache_breakpoint`. The
/// three-state locators plus omittable extras accept both official shapes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerScreenshot {
    #[serde(rename = "type")]
    kind: ComputerScreenshotTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<ImageDetail>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_breakpoint: Omittable<PromptCacheBreakpoint>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerScreenshot {
    /// Creates an empty screenshot payload.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ComputerScreenshotTag::ComputerScreenshot,
            image_url: Omittable::Omitted,
            file_id: Omittable::Omitted,
            detail: Omittable::Omitted,
            prompt_cache_breakpoint: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets a screenshot URL.
    #[must_use]
    pub fn image_url(mut self, image_url: impl Into<String>) -> Self {
        self.image_url = Omittable::Value(Nullable::Value(image_url.into()));
        self
    }

    /// Sets a screenshot file id.
    #[must_use]
    pub fn file_id(mut self, file_id: impl Into<String>) -> Self {
        self.file_id = Omittable::Value(Nullable::Value(file_id.into()));
        self
    }

    /// Sends official `image_url: null`.
    #[must_use]
    pub fn image_url_null(mut self) -> Self {
        self.image_url = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `file_id: null`.
    #[must_use]
    pub fn file_id_null(mut self) -> Self {
        self.file_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the official screenshot detail level.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<ImageDetail>) -> Self {
        self.detail = Omittable::Value(detail.into());
        self
    }

    /// Marks an explicit prompt-cache breakpoint on screenshot content.
    #[must_use]
    pub fn prompt_cache_breakpoint(mut self, breakpoint: PromptCacheBreakpoint) -> Self {
        self.prompt_cache_breakpoint = Omittable::Value(breakpoint);
        self
    }

    /// Returns the screenshot URL when present and non-null.
    #[must_use]
    pub fn image_url_value(&self) -> Option<&str> {
        match &self.image_url {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the screenshot file id when present and non-null.
    #[must_use]
    pub fn file_id_value(&self) -> Option<&str> {
        match &self.file_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }
}

impl Default for ComputerScreenshot {
    fn default() -> Self {
        Self::new()
    }
}

/// A computer-use call produced by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerCall {
    #[serde(rename = "type")]
    kind: ComputerCallTag,
    id: String,
    call_id: String,
    pending_safety_checks: Vec<ComputerSafetyCheck>,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    action: Omittable<ComputerAction>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    actions: Omittable<Vec<ComputerAction>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerCall {
    /// Creates a computer-use call from the official required fields.
    ///
    /// `status` takes the pinned three-value [`FunctionCallItemStatus`]
    /// domain, which matches the pinned `ComputerCall.status`; decoding
    /// keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        status: FunctionCallItemStatus,
    ) -> Self {
        Self {
            kind: ComputerCallTag::ComputerCall,
            id: id.into(),
            call_id: call_id.into(),
            pending_safety_checks: Vec::new(),
            status: status.into(),
            action: Omittable::Omitted,
            actions: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the single requested action.
    #[must_use]
    pub fn with_action(mut self, action: ComputerAction) -> Self {
        self.action = Omittable::Value(action);
        self
    }

    /// Sets batched actions.
    #[must_use]
    pub fn with_actions(mut self, actions: impl Into<Vec<ComputerAction>>) -> Self {
        self.actions = Omittable::Value(actions.into());
        self
    }

    /// Sets pending safety checks.
    #[must_use]
    pub fn with_pending_safety_checks(
        mut self,
        checks: impl IntoIterator<Item = ComputerSafetyCheck>,
    ) -> Self {
        self.pending_safety_checks = checks.into_iter().collect();
        self
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the model-generated call id.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns the single requested action when present.
    #[must_use]
    pub fn action(&self) -> Option<&ComputerAction> {
        match &self.action {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns batched actions when present.
    #[must_use]
    pub fn actions(&self) -> Option<&[ComputerAction]> {
        match &self.actions {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns pending safety checks.
    #[must_use]
    pub fn pending_safety_checks(&self) -> &[ComputerSafetyCheck] {
        &self.pending_safety_checks
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Output sent for a computer-use call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerCallOutput {
    #[serde(rename = "type")]
    kind: ComputerCallOutputTag,
    call_id: String,
    output: ComputerScreenshot,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    acknowledged_safety_checks: Omittable<Nullable<Vec<ComputerSafetyCheck>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerCallOutput {
    /// Creates a screenshot output for `call_id`.
    #[must_use]
    pub fn new(call_id: impl Into<String>, output: ComputerScreenshot) -> Self {
        Self {
            kind: ComputerCallOutputTag::ComputerCallOutput,
            call_id: call_id.into(),
            output,
            id: Omittable::Omitted,
            status: Omittable::Omitted,
            acknowledged_safety_checks: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Acknowledges pending safety checks.
    #[must_use]
    pub fn acknowledged_safety_checks(
        mut self,
        checks: impl IntoIterator<Item = ComputerSafetyCheck>,
    ) -> Self {
        self.acknowledged_safety_checks =
            Omittable::Value(Nullable::Value(checks.into_iter().collect()));
        self
    }

    /// Sends an explicit null acknowledgement list.
    #[must_use]
    pub fn acknowledged_safety_checks_null(mut self) -> Self {
        self.acknowledged_safety_checks = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.call_id)
    }

    /// Returns the related computer-call id.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the screenshot payload.
    #[must_use]
    pub const fn output(&self) -> &ComputerScreenshot {
        &self.output
    }

    /// Returns acknowledged safety checks when present.
    #[must_use]
    pub fn acknowledged_safety_checks_value(&self) -> Option<&[ComputerSafetyCheck]> {
        match &self.acknowledged_safety_checks {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A stored computer-use output item returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerCallOutputResource {
    #[serde(rename = "type")]
    kind: ComputerCallOutputResourceTag,
    id: String,
    call_id: String,
    output: ComputerScreenshot,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    acknowledged_safety_checks: Omittable<Nullable<Vec<ComputerSafetyCheck>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerCallOutputResource {
    /// Returns acknowledged safety checks when present.
    #[must_use]
    pub fn acknowledged_safety_checks(&self) -> Option<&[ComputerSafetyCheck]> {
        match &self.acknowledged_safety_checks {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(WebSearchActionSearchTag, Search, "search");
literal_tag!(WebSearchActionOpenPageTag, OpenPage, "open_page");
literal_tag!(WebSearchActionFindTag, FindInPage, "find_in_page");
literal_tag!(WebSearchSourceTag, Url, "url");
literal_tag!(WebSearchCallTag, WebSearchCall, "web_search_call");

/// A URL source cited by a web-search action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchSource {
    #[serde(rename = "type")]
    kind: WebSearchSourceTag,
    url: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchSource {
    /// Creates a URL source.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            kind: WebSearchSourceTag::Url,
            url: url.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the source URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// A `search` web-search action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchSearchAction {
    #[serde(rename = "type")]
    kind: WebSearchActionSearchTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    query: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    queries: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sources: Omittable<Vec<WebSearchSource>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchSearchAction {
    /// Creates a search action.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: WebSearchActionSearchTag::Search,
            query: Omittable::Omitted,
            queries: Omittable::Omitted,
            sources: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the deprecated single query.
    #[must_use]
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Omittable::Value(query.into());
        self
    }

    /// Sets the search queries.
    #[must_use]
    pub fn queries(mut self, queries: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.queries = Omittable::Value(queries.into_iter().map(Into::into).collect());
        self
    }

    /// Sets cited sources.
    #[must_use]
    pub fn sources(mut self, sources: impl IntoIterator<Item = WebSearchSource>) -> Self {
        self.sources = Omittable::Value(sources.into_iter().collect());
        self
    }
}

impl Default for WebSearchSearchAction {
    fn default() -> Self {
        Self::new()
    }
}

/// An `open_page` web-search action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchOpenPageAction {
    #[serde(rename = "type")]
    kind: WebSearchActionOpenPageTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    url: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchOpenPageAction {
    /// Creates an open-page action.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: WebSearchActionOpenPageTag::OpenPage,
            url: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the opened URL.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Omittable::Value(Nullable::Value(url.into()));
        self
    }

    /// Sends official `url: null`.
    #[must_use]
    pub fn url_null(mut self) -> Self {
        self.url = Omittable::Value(Nullable::Null);
        self
    }
}

impl Default for WebSearchOpenPageAction {
    fn default() -> Self {
        Self::new()
    }
}

/// A `find_in_page` web-search action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchFindAction {
    #[serde(rename = "type")]
    kind: WebSearchActionFindTag,
    url: String,
    pattern: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchFindAction {
    /// Creates a find-in-page action.
    #[must_use]
    pub fn new(url: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            kind: WebSearchActionFindTag::FindInPage,
            url: url.into(),
            pattern: pattern.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// An action recorded on a web-search tool call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WebSearchAction {
    /// A search query.
    Search(WebSearchSearchAction),
    /// Open a result URL.
    OpenPage(WebSearchOpenPageAction),
    /// Find text in an opened page.
    Find(WebSearchFindAction),
    /// Future action retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl From<WebSearchSearchAction> for WebSearchAction {
    fn from(value: WebSearchSearchAction) -> Self {
        Self::Search(value)
    }
}

impl From<WebSearchOpenPageAction> for WebSearchAction {
    fn from(value: WebSearchOpenPageAction) -> Self {
        Self::OpenPage(value)
    }
}

impl From<WebSearchFindAction> for WebSearchAction {
    fn from(value: WebSearchFindAction) -> Self {
        Self::Find(value)
    }
}

impl Serialize for WebSearchAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Search(value) => value.serialize(serializer),
            Self::OpenPage(value) => value.serialize(serializer),
            Self::Find(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for WebSearchAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "search" => serde_json::from_value(value)
                .map(Self::Search)
                .map_err(D::Error::custom),
            "open_page" => serde_json::from_value(value)
                .map(Self::OpenPage)
                .map_err(D::Error::custom),
            "find_in_page" => serde_json::from_value(value)
                .map(Self::Find)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A web-search call produced by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchCall {
    #[serde(rename = "type")]
    kind: WebSearchCallTag,
    id: String,
    status: ResponseItemStatus,
    action: WebSearchAction,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchCall {
    /// Creates a web-search call from the official required fields.
    ///
    /// `status` takes the pinned four-value [`WebSearchToolCallStatus`]
    /// (`in_progress`/`searching`/`completed`/`failed`); decoding keeps the
    /// shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: WebSearchToolCallStatus,
        action: impl Into<WebSearchAction>,
    ) -> Self {
        Self {
            kind: WebSearchCallTag::WebSearchCall,
            id: id.into(),
            status: status.into(),
            action: action.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns the recorded web-search action.
    #[must_use]
    pub const fn action(&self) -> &WebSearchAction {
        &self.action
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(
    FunctionCallOutputResourceTag,
    FunctionCallOutputResource,
    "function_call_output"
);

/// Function-call output returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallOutputResource {
    #[serde(rename = "type")]
    kind: FunctionCallOutputResourceTag,
    id: String,
    output: FunctionCallOutputValue,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    namespace: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCallOutputResource {
    /// Returns the function output.
    #[must_use]
    pub const fn output(&self) -> &FunctionCallOutputValue {
        &self.output
    }

    /// Returns the model-generated call id when present.
    #[must_use]
    pub fn call_id(&self) -> Option<&str> {
        match &self.call_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the creating actor id when present.
    #[must_use]
    pub fn created_by(&self) -> Option<&str> {
        match &self.created_by {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(ToolSearchCallInputTag, ToolSearchCall, "tool_search_call");

/// A tool-search call supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchCallInput {
    #[serde(rename = "type")]
    kind: ToolSearchCallInputTag,
    arguments: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    execution: Omittable<ToolSearchExecution>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ToolSearchCallInput {
    /// Creates a tool-search call input item.
    #[must_use]
    pub fn new(arguments: impl Into<Value>) -> Self {
        Self {
            kind: ToolSearchCallInputTag::ToolSearchCall,
            arguments: arguments.into(),
            id: Omittable::Omitted,
            call_id: Omittable::Omitted,
            execution: Omittable::Omitted,
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the item id when echoing a stored call.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sets the model-generated call id.
    #[must_use]
    pub fn call_id(mut self, call_id: impl Into<String>) -> Self {
        self.call_id = Omittable::Value(Nullable::Value(call_id.into()));
        self
    }

    /// Sets whether tool search ran on the server or client.
    #[must_use]
    pub fn execution(mut self, execution: impl Into<ToolSearchExecution>) -> Self {
        self.execution = Omittable::Value(execution.into());
        self
    }

    /// Sets the call status.
    ///
    /// `status` takes the pinned three-value [`FunctionCallItemStatus`]
    /// domain, which matches the pinned `ToolSearchCall.status`; decoding
    /// keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn status(mut self, status: FunctionCallItemStatus) -> Self {
        self.status = Omittable::Value(Nullable::Value(status.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `call_id: null`.
    #[must_use]
    pub fn call_id_null(mut self) -> Self {
        self.call_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_omittable_call_id(&self.call_id)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(ToolSearchCallTag, ToolSearchCall, "tool_search_call");

/// A tool-search call returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchCall {
    #[serde(rename = "type")]
    kind: ToolSearchCallTag,
    id: String,
    call_id: Nullable<String>,
    execution: ToolSearchExecution,
    arguments: Value,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ToolSearchCall {
    /// Returns the creating actor id when present.
    #[must_use]
    pub fn created_by(&self) -> Option<&str> {
        match &self.created_by {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(
    ToolSearchOutputInputTag,
    ToolSearchOutput,
    "tool_search_output"
);
literal_tag!(ToolSearchOutputTag, ToolSearchOutput, "tool_search_output");

/// Tool-search results supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchOutputInput {
    #[serde(rename = "type")]
    kind: ToolSearchOutputInputTag,
    tools: Vec<ResponseTool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    execution: Omittable<ToolSearchExecution>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ToolSearchOutputInput {
    /// Creates a tool-search output input item.
    #[must_use]
    pub fn new(tools: impl Into<Vec<ResponseTool>>) -> Self {
        Self {
            kind: ToolSearchOutputInputTag::ToolSearchOutput,
            tools: tools.into(),
            id: Omittable::Omitted,
            call_id: Omittable::Omitted,
            execution: Omittable::Omitted,
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the loaded tool definitions.
    #[must_use]
    pub fn tools(&self) -> &[ResponseTool] {
        &self.tools
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `call_id: null`.
    #[must_use]
    pub fn call_id_null(mut self) -> Self {
        self.call_id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_omittable_call_id(&self.call_id)?;
        validate_response_tools(&self.tools)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Tool-search results returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchOutput {
    #[serde(rename = "type")]
    kind: ToolSearchOutputTag,
    id: String,
    call_id: Nullable<String>,
    execution: ToolSearchExecution,
    tools: Vec<ResponseTool>,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ToolSearchOutput {
    /// Returns the loaded tool definitions.
    #[must_use]
    pub fn tools(&self) -> &[ResponseTool] {
        &self.tools
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(AdditionalToolsInputTag, AdditionalTools, "additional_tools");

/// Additional tools supplied as a developer input item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdditionalToolsInput {
    #[serde(rename = "type")]
    kind: AdditionalToolsInputTag,
    role: MessageRole,
    tools: Vec<ResponseTool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl AdditionalToolsInput {
    /// Creates an additional-tools input item.
    #[must_use]
    pub fn new(tools: impl Into<Vec<ResponseTool>>) -> Self {
        Self {
            kind: AdditionalToolsInputTag::AdditionalTools,
            role: MessageRole::Developer,
            tools: tools.into(),
            id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the item id when echoing a stored item.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_response_tools(&self.tools)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

required_tagged_record!(AdditionalTools, AdditionalToolsTag, AdditionalTools, "additional_tools", {
    id: String,
    role: MessageRole,
    tools: Vec<ResponseTool>
});
literal_tag!(SummaryTextContentTag, SummaryText, "summary_text");
literal_tag!(ReasoningTextContentTag, ReasoningText, "reasoning_text");
literal_tag!(ReasoningItemTag, Reasoning, "reasoning");

/// A reasoning summary paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryTextContent {
    #[serde(rename = "type")]
    kind: SummaryTextContentTag,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl SummaryTextContent {
    /// Creates a summary-text part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: SummaryTextContentTag::SummaryText,
            text: text.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the summary text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A reasoning-text paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningTextContent {
    #[serde(rename = "type")]
    kind: ReasoningTextContentTag,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ReasoningTextContent {
    /// Creates a reasoning-text part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: ReasoningTextContentTag::ReasoningText,
            text: text.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the reasoning text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A reasoning item returned by the API and replayed on follow-up turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningItem {
    #[serde(rename = "type")]
    kind: ReasoningItemTag,
    id: String,
    summary: Vec<SummaryTextContent>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    encrypted_content: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    content: Omittable<Vec<ReasoningTextContent>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseItemStatus>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ReasoningItem {
    /// Creates a reasoning item with the required summary list.
    #[must_use]
    pub fn new(id: impl Into<String>, summary: impl Into<Vec<SummaryTextContent>>) -> Self {
        Self {
            kind: ReasoningItemTag::Reasoning,
            id: id.into(),
            summary: summary.into(),
            encrypted_content: Omittable::Omitted,
            content: Omittable::Omitted,
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the summary parts.
    #[must_use]
    pub fn summary(&self) -> &[SummaryTextContent] {
        &self.summary
    }

    /// Sets encrypted reasoning content for ZDR / `store:false` follow-ups.
    #[must_use]
    pub fn encrypted_content(mut self, encrypted_content: impl Into<String>) -> Self {
        self.encrypted_content = Omittable::Value(Nullable::Value(encrypted_content.into()));
        self
    }

    /// Explicitly sends `encrypted_content: null`.
    #[must_use]
    pub fn encrypted_content_null(mut self) -> Self {
        self.encrypted_content = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns encrypted content when present.
    #[must_use]
    pub fn encrypted_content_ref(&self) -> Option<&str> {
        match &self.encrypted_content {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Sets reasoning text content.
    #[must_use]
    pub fn content(mut self, content: impl Into<Vec<ReasoningTextContent>>) -> Self {
        self.content = Omittable::Value(content.into());
        self
    }

    /// Returns reasoning text when present.
    #[must_use]
    pub fn content_ref(&self) -> Option<&[ReasoningTextContent]> {
        match &self.content {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Sets the item status.
    ///
    /// `status` takes the pinned three-value [`FunctionCallItemStatus`]
    /// domain, which matches the pinned `ReasoningItem.status`; decoding
    /// keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn status(mut self, status: FunctionCallItemStatus) -> Self {
        self.status = Omittable::Value(status.into());
        self
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(CompactionSummaryInputTag, Compaction, "compaction");

/// A compaction summary supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionSummaryInput {
    #[serde(rename = "type")]
    kind: CompactionSummaryInputTag,
    encrypted_content: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompactionSummaryInput {
    /// Creates a compaction input item.
    #[must_use]
    pub fn new(encrypted_content: impl Into<String>) -> Self {
        Self {
            kind: CompactionSummaryInputTag::Compaction,
            encrypted_content: encrypted_content.into(),
            id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the item id when echoing a stored compaction.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        let actual = self.encrypted_content.chars().count();
        if actual > MAX_COMPACTION_ENCRYPTED_CHARS {
            return Err(CreateResponseConstraintError::CompactionEncryptedContent {
                actual,
                maximum: MAX_COMPACTION_ENCRYPTED_CHARS,
            });
        }
        Ok(())
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(CompactionItemTag, Compaction, "compaction");

/// A compaction summary returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionItem {
    #[serde(rename = "type")]
    kind: CompactionItemTag,
    id: String,
    encrypted_content: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompactionItem {
    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
required_tagged_record!(
    ImageGenerationCall,
    ImageGenerationCallTag,
    ImageGenerationCall,
    "image_generation_call",
    {
        id: String,
        status: ResponseItemStatus,
        result: Nullable<String>
    }
);

impl ImageGenerationCall {
    /// Creates an image-generation call with official required `result: null`.
    ///
    /// `status` takes the pinned [`ImageGenToolCallStatus`] domain, which
    /// carries `generating` but not `incomplete`; decoding keeps the shared
    /// open [`ResponseItemStatus`].
    #[must_use]
    pub fn new(id: impl Into<String>, status: ImageGenToolCallStatus) -> Self {
        Self {
            kind: ImageGenerationCallTag::ImageGenerationCall,
            id: id.into(),
            status: status.into(),
            result: Nullable::Null,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the generated image payload.
    #[must_use]
    pub fn with_result(mut self, result: impl Into<String>) -> Self {
        self.result = Nullable::Value(result.into());
        self
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns the generated image payload when present and non-null.
    #[must_use]
    pub fn result(&self) -> Option<&str> {
        match &self.result {
            Nullable::Value(result) => Some(result),
            Nullable::Null => None,
        }
    }
}
literal_tag!(CodeInterpreterLogsTag, Logs, "logs");
literal_tag!(CodeInterpreterImageTag, Image, "image");
literal_tag!(
    CodeInterpreterCallTag,
    CodeInterpreterCall,
    "code_interpreter_call"
);

/// Logs emitted by the code interpreter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterLogs {
    #[serde(rename = "type")]
    kind: CodeInterpreterLogsTag,
    logs: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterLogs {
    /// Creates a logs output.
    #[must_use]
    pub fn new(logs: impl Into<String>) -> Self {
        Self {
            kind: CodeInterpreterLogsTag::Logs,
            logs: logs.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the captured logs.
    #[must_use]
    pub fn logs(&self) -> &str {
        &self.logs
    }
}

/// An image emitted by the code interpreter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterImage {
    #[serde(rename = "type")]
    kind: CodeInterpreterImageTag,
    url: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterImage {
    /// Creates an image output.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            kind: CodeInterpreterImageTag::Image,
            url: url.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the image URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// One code-interpreter output part.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CodeInterpreterOutput {
    /// Captured stdout/stderr logs.
    Logs(CodeInterpreterLogs),
    /// A generated image.
    Image(CodeInterpreterImage),
    /// Future output retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for CodeInterpreterOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Logs(value) => value.serialize(serializer),
            Self::Image(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CodeInterpreterOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "logs" => serde_json::from_value(value)
                .map(Self::Logs)
                .map_err(D::Error::custom),
            "image" => serde_json::from_value(value)
                .map(Self::Image)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A code-interpreter tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterCall {
    #[serde(rename = "type")]
    kind: CodeInterpreterCallTag,
    id: String,
    status: ResponseItemStatus,
    container_id: String,
    code: Nullable<String>,
    outputs: Nullable<Vec<CodeInterpreterOutput>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterCall {
    /// Creates a code-interpreter call with official required `code` / `outputs` nulls.
    ///
    /// `status` takes the pinned [`CodeInterpreterToolCallStatus`] domain,
    /// which carries `interpreting`; decoding keeps the shared open
    /// [`ResponseItemStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: CodeInterpreterToolCallStatus,
        container_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: CodeInterpreterCallTag::CodeInterpreterCall,
            id: id.into(),
            status: status.into(),
            container_id: container_id.into(),
            code: Nullable::Null,
            outputs: Nullable::Null,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the code to run.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Nullable::Value(code.into());
        self
    }

    /// Sets interpreter outputs.
    #[must_use]
    pub fn with_outputs(mut self, outputs: impl Into<Vec<CodeInterpreterOutput>>) -> Self {
        self.outputs = Nullable::Value(outputs.into());
        self
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns the container the interpreter ran in.
    #[must_use]
    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    /// Returns the code to run when present and non-null.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match &self.code {
            Nullable::Value(code) => Some(code),
            Nullable::Null => None,
        }
    }

    /// Returns the interpreter outputs when present.
    #[must_use]
    pub fn outputs(&self) -> Option<&[CodeInterpreterOutput]> {
        match &self.outputs {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(LocalShellExecTag, Exec, "exec");
literal_tag!(LocalShellCallTag, LocalShellCall, "local_shell_call");

/// A local-shell `exec` action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalShellExecAction {
    #[serde(rename = "type")]
    kind: LocalShellExecTag,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    timeout_ms: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    working_directory: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LocalShellExecAction {
    /// Creates an exec action.
    #[must_use]
    pub fn new(
        command: impl IntoIterator<Item = impl Into<String>>,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            kind: LocalShellExecTag::Exec,
            command: command.into_iter().map(Into::into).collect(),
            env: env.into_iter().map(|(k, v)| (k.into(), v.into())).collect(),
            timeout_ms: Omittable::Omitted,
            working_directory: Omittable::Omitted,
            user: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the command argv.
    #[must_use]
    pub fn command(&self) -> &[String] {
        &self.command
    }

    /// Returns environment variables.
    #[must_use]
    pub const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Sets an optional command timeout.
    #[must_use]
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Omittable::Value(Nullable::Value(timeout_ms));
        self
    }

    /// Sends official `timeout_ms: null`.
    #[must_use]
    pub fn timeout_ms_null(mut self) -> Self {
        self.timeout_ms = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets an optional working directory.
    #[must_use]
    pub fn working_directory(mut self, working_directory: impl Into<String>) -> Self {
        self.working_directory = Omittable::Value(Nullable::Value(working_directory.into()));
        self
    }

    /// Sends official `working_directory: null`.
    #[must_use]
    pub fn working_directory_null(mut self) -> Self {
        self.working_directory = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets an optional user to run the command as.
    #[must_use]
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Omittable::Value(Nullable::Value(user.into()));
        self
    }

    /// Sends official `user: null`.
    #[must_use]
    pub fn user_null(mut self) -> Self {
        self.user = Omittable::Value(Nullable::Null);
        self
    }
}

/// A local-shell action requested by the model.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LocalShellAction {
    /// Execute a command.
    Exec(LocalShellExecAction),
    /// Future action retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl From<LocalShellExecAction> for LocalShellAction {
    fn from(value: LocalShellExecAction) -> Self {
        Self::Exec(value)
    }
}

impl Serialize for LocalShellAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Exec(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for LocalShellAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "exec" => serde_json::from_value(value)
                .map(Self::Exec)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A local-shell call produced by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalShellCall {
    #[serde(rename = "type")]
    kind: LocalShellCallTag,
    id: String,
    call_id: String,
    action: LocalShellAction,
    status: ResponseItemStatus,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LocalShellCall {
    /// Creates a local-shell call from the official required fields.
    ///
    /// `status` takes the pinned three-value [`FunctionCallItemStatus`]
    /// domain, which matches the pinned `LocalShellCall.status`; decoding
    /// keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        action: impl Into<LocalShellAction>,
        status: FunctionCallItemStatus,
    ) -> Self {
        Self {
            kind: LocalShellCallTag::LocalShellCall,
            id: id.into(),
            call_id: call_id.into(),
            action: action.into(),
            status: status.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the item id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the model-generated call id.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the call status.
    #[must_use]
    pub const fn status(&self) -> &ResponseItemStatus {
        &self.status
    }

    /// Returns the requested action.
    #[must_use]
    pub const fn action(&self) -> &LocalShellAction {
        &self.action
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(
    LocalShellCallOutputTag,
    LocalShellCallOutput,
    "local_shell_call_output"
);

/// Output of a local-shell tool call.
///
/// Official output shape is `{id, output, status?}`. Ghost `call_id` is
/// retained in extra when present (D0110).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalShellCallOutput {
    #[serde(rename = "type")]
    kind: LocalShellCallOutputTag,
    id: String,
    output: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LocalShellCallOutput {
    /// Creates a local-shell output item.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        Self {
            kind: LocalShellCallOutputTag::LocalShellCallOutput,
            id: id.into(),
            call_id: Omittable::Value(call_id.into()),
            output: output.into(),
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the item status.
    ///
    /// `status` takes the pinned three-value [`FunctionCallItemStatus`]
    /// domain, which matches the pinned `LocalShellCallOutput.status`;
    /// decoding keeps the shared open [`ResponseItemStatus`].
    #[must_use]
    pub fn status(mut self, status: FunctionCallItemStatus) -> Self {
        self.status = Omittable::Value(Nullable::Value(status.into()));
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the captured output string.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(FunctionShellCallInputTag, FunctionShellCall, "shell_call");
literal_tag!(FunctionShellCallTag, FunctionShellCall, "shell_call");

/// Official input-item `FunctionShellActionParam`.
///
/// The pin requires only `commands`. `timeout_ms` and `max_output_length`
/// may be omitted or sent as `null`. Resource echo uses
/// [`FunctionShellAction`], which requires both nullable limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellActionParam {
    commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    timeout_ms: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_output_length: Omittable<Nullable<u64>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellActionParam {
    /// Creates a shell action that omits official optional limits.
    #[must_use]
    pub fn new(commands: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            commands: commands.into_iter().map(Into::into).collect(),
            timeout_ms: Omittable::Omitted,
            max_output_length: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the commands to run.
    #[must_use]
    pub fn commands(&self) -> &[String] {
        &self.commands
    }

    /// Sets a timeout in milliseconds.
    #[must_use]
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Omittable::Value(Nullable::Value(timeout_ms));
        self
    }

    /// Sets a per-command output character cap.
    #[must_use]
    pub fn max_output_length(mut self, max_output_length: u64) -> Self {
        self.max_output_length = Omittable::Value(Nullable::Value(max_output_length));
        self
    }

    /// Sends official `timeout_ms: null`.
    #[must_use]
    pub fn timeout_ms_null(mut self) -> Self {
        self.timeout_ms = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `max_output_length: null`.
    #[must_use]
    pub fn max_output_length_null(mut self) -> Self {
        self.max_output_length = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns the timeout when present and non-null.
    #[must_use]
    pub const fn timeout_ms_ref(&self) -> Option<u64> {
        match self.timeout_ms {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the output cap when present and non-null.
    #[must_use]
    pub const fn max_output_length_ref(&self) -> Option<u64> {
        match self.max_output_length {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl From<FunctionShellAction> for FunctionShellActionParam {
    fn from(value: FunctionShellAction) -> Self {
        Self {
            commands: value.commands,
            timeout_ms: Omittable::Value(value.timeout_ms),
            max_output_length: Omittable::Value(value.max_output_length),
            extra: value.extra,
        }
    }
}

/// Official resource `FunctionShellAction`.
///
/// The pin requires `commands`, `timeout_ms`, and `max_output_length`.
/// Input items use [`FunctionShellActionParam`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellAction {
    commands: Vec<String>,
    timeout_ms: Nullable<u64>,
    max_output_length: Nullable<u64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellAction {
    /// Creates a shell action with required nullable timeouts.
    #[must_use]
    pub fn new(commands: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            commands: commands.into_iter().map(Into::into).collect(),
            timeout_ms: Nullable::Null,
            max_output_length: Nullable::Null,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the commands to run.
    #[must_use]
    pub fn commands(&self) -> &[String] {
        &self.commands
    }

    /// Sets a timeout in milliseconds.
    #[must_use]
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Nullable::Value(timeout_ms);
        self
    }

    /// Sets a per-command output character cap.
    #[must_use]
    pub fn max_output_length(mut self, max_output_length: u64) -> Self {
        self.max_output_length = Nullable::Value(max_output_length);
        self
    }

    /// Sends official required `timeout_ms: null`.
    #[must_use]
    pub fn timeout_ms_null(mut self) -> Self {
        self.timeout_ms = Nullable::Null;
        self
    }

    /// Sends official required `max_output_length: null`.
    #[must_use]
    pub fn max_output_length_null(mut self) -> Self {
        self.max_output_length = Nullable::Null;
        self
    }
}

/// A function-shell call supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellCallInput {
    #[serde(rename = "type")]
    kind: FunctionShellCallInputTag,
    call_id: String,
    action: FunctionShellActionParam,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    environment: Omittable<Nullable<FunctionShellEnvironment>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellCallInput {
    /// Creates a shell-call input item.
    #[must_use]
    pub fn new(call_id: impl Into<String>, action: impl Into<FunctionShellActionParam>) -> Self {
        Self {
            kind: FunctionShellCallInputTag::FunctionShellCall,
            call_id: call_id.into(),
            action: action.into(),
            id: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            environment: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the requested action.
    #[must_use]
    pub const fn action(&self) -> &FunctionShellActionParam {
        &self.action
    }

    /// Sets the execution context that produced this call.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sets the execution environment.
    #[must_use]
    pub fn environment(mut self, environment: FunctionShellEnvironment) -> Self {
        self.environment = Omittable::Value(Nullable::Value(environment));
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `environment: null`.
    #[must_use]
    pub fn environment_null(mut self) -> Self {
        self.environment = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_function_shell_call_id(&self.call_id)?;
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A function-shell call returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellCall {
    #[serde(rename = "type")]
    kind: FunctionShellCallTag,
    id: String,
    call_id: String,
    action: FunctionShellAction,
    status: ResponseItemStatus,
    environment: Nullable<FunctionShellEnvironment>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellCall {
    /// Returns the requested action.
    #[must_use]
    pub const fn action(&self) -> &FunctionShellAction {
        &self.action
    }

    /// Returns the environment when present.
    #[must_use]
    pub const fn environment(&self) -> Option<&FunctionShellEnvironment> {
        match &self.environment {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }

    /// Returns the execution context when present.
    #[must_use]
    pub const fn caller_ref(&self) -> Option<&ToolCallCaller> {
        match &self.caller {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(FunctionShellTimeoutOutcomeTag, Timeout, "timeout");
literal_tag!(FunctionShellExitOutcomeTag, Exit, "exit");
literal_tag!(
    FunctionShellCallOutputInputTag,
    FunctionShellCallOutput,
    "shell_call_output"
);
literal_tag!(
    FunctionShellCallOutputTag,
    FunctionShellCallOutput,
    "shell_call_output"
);

/// A shell call that exceeded its time limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellTimeoutOutcome {
    #[serde(rename = "type")]
    kind: FunctionShellTimeoutOutcomeTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellTimeoutOutcome {
    /// Creates a timeout outcome.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: FunctionShellTimeoutOutcomeTag::Timeout,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for FunctionShellTimeoutOutcome {
    fn default() -> Self {
        Self::new()
    }
}

/// A shell call that returned an exit code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellExitOutcome {
    #[serde(rename = "type")]
    kind: FunctionShellExitOutcomeTag,
    exit_code: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellExitOutcome {
    /// Creates an exit outcome.
    #[must_use]
    pub fn new(exit_code: i64) -> Self {
        Self {
            kind: FunctionShellExitOutcomeTag::Exit,
            exit_code,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the process exit code.
    #[must_use]
    pub const fn exit_code(&self) -> i64 {
        self.exit_code
    }
}

/// Outcome of one shell-output chunk.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FunctionShellOutcome {
    /// The command timed out.
    Timeout(FunctionShellTimeoutOutcome),
    /// The command exited.
    Exit(FunctionShellExitOutcome),
    /// Future outcome retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl FunctionShellOutcome {
    /// Creates a timeout outcome.
    #[must_use]
    pub fn timeout() -> Self {
        Self::Timeout(FunctionShellTimeoutOutcome::new())
    }

    /// Creates an exit outcome.
    #[must_use]
    pub fn exit(exit_code: i64) -> Self {
        Self::Exit(FunctionShellExitOutcome::new(exit_code))
    }
}

impl Serialize for FunctionShellOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Timeout(value) => value.serialize(serializer),
            Self::Exit(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionShellOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "timeout" => serde_json::from_value(value)
                .map(Self::Timeout)
                .map_err(D::Error::custom),
            "exit" => serde_json::from_value(value)
                .map(Self::Exit)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// One captured shell-output chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellCallOutputContent {
    stdout: String,
    stderr: String,
    outcome: FunctionShellOutcome,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellCallOutputContent {
    /// Creates a captured output chunk.
    #[must_use]
    pub fn new(
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        outcome: FunctionShellOutcome,
    ) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            outcome,
            created_by: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns captured stdout.
    #[must_use]
    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    /// Returns captured stderr.
    #[must_use]
    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    /// Returns the chunk outcome.
    #[must_use]
    pub const fn outcome(&self) -> &FunctionShellOutcome {
        &self.outcome
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        let stdout = self.stdout.chars().count();
        if stdout > MAX_FUNCTION_SHELL_OUTPUT_CHARS {
            return Err(CreateResponseConstraintError::FunctionShellStdout {
                actual: stdout,
                maximum: MAX_FUNCTION_SHELL_OUTPUT_CHARS,
            });
        }
        let stderr = self.stderr.chars().count();
        if stderr > MAX_FUNCTION_SHELL_OUTPUT_CHARS {
            return Err(CreateResponseConstraintError::FunctionShellStderr {
                actual: stderr,
                maximum: MAX_FUNCTION_SHELL_OUTPUT_CHARS,
            });
        }
        Ok(())
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Shell output supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellCallOutputInput {
    #[serde(rename = "type")]
    kind: FunctionShellCallOutputInputTag,
    call_id: String,
    output: Vec<FunctionShellCallOutputContent>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_output_length: Omittable<Nullable<u64>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellCallOutputInput {
    /// Creates a shell-output input item.
    #[must_use]
    pub fn new(
        call_id: impl Into<String>,
        output: impl Into<Vec<FunctionShellCallOutputContent>>,
    ) -> Self {
        Self {
            kind: FunctionShellCallOutputInputTag::FunctionShellCallOutput,
            call_id: call_id.into(),
            output: output.into(),
            id: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            max_output_length: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns captured output chunks.
    #[must_use]
    pub fn output(&self) -> &[FunctionShellCallOutputContent] {
        &self.output
    }

    /// Sets the execution context that produced this output.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sets the output character cap.
    #[must_use]
    pub fn max_output_length(mut self, max_output_length: u64) -> Self {
        self.max_output_length = Omittable::Value(Nullable::Value(max_output_length));
        self
    }

    /// Sends official `max_output_length: null`.
    #[must_use]
    pub fn max_output_length_null(mut self) -> Self {
        self.max_output_length = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `status: null`.
    #[must_use]
    pub fn status_null(mut self) -> Self {
        self.status = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_function_shell_call_id(&self.call_id)?;
        validate_omittable_caller(&self.caller)?;
        for chunk in &self.output {
            chunk.validate()?;
        }
        Ok(())
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Shell output returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellCallOutput {
    #[serde(rename = "type")]
    kind: FunctionShellCallOutputTag,
    id: String,
    call_id: String,
    status: ResponseItemStatus,
    output: Vec<FunctionShellCallOutputContent>,
    max_output_length: Nullable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellCallOutput {
    /// Returns captured output chunks.
    #[must_use]
    pub fn output(&self) -> &[FunctionShellCallOutputContent] {
        &self.output
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(ApplyPatchCreateFileTag, CreateFile, "create_file");
literal_tag!(ApplyPatchDeleteFileTag, DeleteFile, "delete_file");
literal_tag!(ApplyPatchUpdateFileTag, UpdateFile, "update_file");
literal_tag!(ApplyPatchCallInputTag, ApplyPatchCall, "apply_patch_call");
literal_tag!(ApplyPatchCallTag, ApplyPatchCall, "apply_patch_call");
literal_tag!(
    ApplyPatchCallOutputInputTag,
    ApplyPatchCallOutput,
    "apply_patch_call_output"
);
literal_tag!(
    ApplyPatchCallOutputTag,
    ApplyPatchCallOutput,
    "apply_patch_call_output"
);

/// Create a file via apply_patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchCreateFile {
    #[serde(rename = "type")]
    kind: ApplyPatchCreateFileTag,
    path: String,
    diff: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchCreateFile {
    /// Creates a create-file operation.
    #[must_use]
    pub fn new(path: impl Into<String>, diff: impl Into<String>) -> Self {
        Self {
            kind: ApplyPatchCreateFileTag::CreateFile,
            path: path.into(),
            diff: diff.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the target path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the diff to apply.
    #[must_use]
    pub fn diff(&self) -> &str {
        &self.diff
    }
}

/// Delete a file via apply_patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchDeleteFile {
    #[serde(rename = "type")]
    kind: ApplyPatchDeleteFileTag,
    path: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchDeleteFile {
    /// Creates a delete-file operation.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            kind: ApplyPatchDeleteFileTag::DeleteFile,
            path: path.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the target path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Update a file via apply_patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchUpdateFile {
    #[serde(rename = "type")]
    kind: ApplyPatchUpdateFileTag,
    path: String,
    diff: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchUpdateFile {
    /// Creates an update-file operation.
    #[must_use]
    pub fn new(path: impl Into<String>, diff: impl Into<String>) -> Self {
        Self {
            kind: ApplyPatchUpdateFileTag::UpdateFile,
            path: path.into(),
            diff: diff.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the target path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the diff to apply.
    #[must_use]
    pub fn diff(&self) -> &str {
        &self.diff
    }
}

/// One create, delete, or update instruction for apply_patch.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ApplyPatchOperation {
    /// Create a new file.
    CreateFile(ApplyPatchCreateFile),
    /// Delete an existing file.
    DeleteFile(ApplyPatchDeleteFile),
    /// Update an existing file.
    UpdateFile(ApplyPatchUpdateFile),
    /// Future operation retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ApplyPatchOperation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::CreateFile(value) => value.serialize(serializer),
            Self::DeleteFile(value) => value.serialize(serializer),
            Self::UpdateFile(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ApplyPatchOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "create_file" => serde_json::from_value(value)
                .map(Self::CreateFile)
                .map_err(D::Error::custom),
            "delete_file" => serde_json::from_value(value)
                .map(Self::DeleteFile)
                .map_err(D::Error::custom),
            "update_file" => serde_json::from_value(value)
                .map(Self::UpdateFile)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

open_string_enum! {
    /// Lifecycle status of an apply-patch tool call.
    pub enum ApplyPatchCallStatus {
        InProgress = "in_progress",
        Completed = "completed"
    }
}

open_string_enum! {
    /// Terminal status of an apply-patch tool call output.
    pub enum ApplyPatchCallOutputStatus {
        Completed = "completed",
        Failed = "failed"
    }
}

/// An apply-patch call supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchCallInput {
    #[serde(rename = "type")]
    kind: ApplyPatchCallInputTag,
    call_id: String,
    status: ApplyPatchCallStatus,
    operation: ApplyPatchOperation,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchCallInput {
    /// Creates an apply-patch input item.
    #[must_use]
    pub fn new(
        call_id: impl Into<String>,
        status: impl Into<ApplyPatchCallStatus>,
        operation: ApplyPatchOperation,
    ) -> Self {
        Self {
            kind: ApplyPatchCallInputTag::ApplyPatchCall,
            call_id: call_id.into(),
            status: status.into(),
            operation,
            id: Omittable::Omitted,
            caller: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the requested operation.
    #[must_use]
    pub const fn operation(&self) -> &ApplyPatchOperation {
        &self.operation
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.call_id)?;
        match &self.operation {
            ApplyPatchOperation::CreateFile(operation) => {
                validate_apply_patch_path_chars(operation.path().chars().count())?;
                validate_apply_patch_diff_chars(operation.diff().chars().count())?;
            }
            ApplyPatchOperation::UpdateFile(operation) => {
                validate_apply_patch_path_chars(operation.path().chars().count())?;
                validate_apply_patch_diff_chars(operation.diff().chars().count())?;
            }
            ApplyPatchOperation::DeleteFile(operation) => {
                validate_apply_patch_path_chars(operation.path().chars().count())?;
            }
            ApplyPatchOperation::Unknown(_) => {}
        }
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// An apply-patch call returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchCall {
    #[serde(rename = "type")]
    kind: ApplyPatchCallTag,
    id: String,
    call_id: String,
    status: ApplyPatchCallStatus,
    operation: ApplyPatchOperation,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchCall {
    /// Returns the requested operation.
    #[must_use]
    pub const fn operation(&self) -> &ApplyPatchOperation {
        &self.operation
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Apply-patch output supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchCallOutputInput {
    #[serde(rename = "type")]
    kind: ApplyPatchCallOutputInputTag,
    call_id: String,
    status: ApplyPatchCallOutputStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchCallOutputInput {
    /// Creates an apply-patch output item.
    #[must_use]
    pub fn new(call_id: impl Into<String>, status: impl Into<ApplyPatchCallOutputStatus>) -> Self {
        Self {
            kind: ApplyPatchCallOutputInputTag::ApplyPatchCallOutput,
            call_id: call_id.into(),
            status: status.into(),
            output: Omittable::Omitted,
            id: Omittable::Omitted,
            caller: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the execution context that produced this output.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sets optional log text.
    #[must_use]
    pub fn output(mut self, output: impl Into<String>) -> Self {
        self.output = Omittable::Value(Nullable::Value(output.into()));
        self
    }

    /// Sends explicit null log text.
    #[must_use]
    pub fn output_null(mut self) -> Self {
        self.output = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `id: null`.
    #[must_use]
    pub fn id_null(mut self) -> Self {
        self.id = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_call_id(&self.call_id)?;
        if let Omittable::Value(Nullable::Value(output)) = &self.output {
            let actual = output.chars().count();
            if actual > MAX_FUNCTION_CALL_OUTPUT_CHARS {
                return Err(CreateResponseConstraintError::FunctionCallOutputChars {
                    actual,
                    maximum: MAX_FUNCTION_CALL_OUTPUT_CHARS,
                });
            }
        }
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Apply-patch output returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchCallOutput {
    #[serde(rename = "type")]
    kind: ApplyPatchCallOutputTag,
    id: String,
    call_id: String,
    status: ApplyPatchCallOutputStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchCallOutput {
    /// Returns optional log text.
    #[must_use]
    pub fn output(&self) -> Option<&str> {
        match &self.output {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns the execution context when present.
    #[must_use]
    pub const fn caller_ref(&self) -> Option<&ToolCallCaller> {
        match &self.caller {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(
    McpApprovalResponseResourceTag,
    McpApprovalResponse,
    "mcp_approval_response"
);

/// An MCP approval response returned by the API.
///
/// Ghost `request_id` is not a typed field (D0111). ExtraFields retains it
/// when present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpApprovalResponseResource {
    #[serde(rename = "type")]
    kind: McpApprovalResponseResourceTag,
    id: String,
    approval_request_id: String,
    approve: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reason: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpApprovalResponseResource {
    /// Returns the decision reason when present.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match &self.reason {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}
literal_tag!(CustomToolCallTag, CustomToolCall, "custom_tool_call");
literal_tag!(
    CustomToolCallOutputTag,
    CustomToolCallOutput,
    "custom_tool_call_output"
);
literal_tag!(
    CustomToolCallOutputResourceTag,
    CustomToolCallOutputResource,
    "custom_tool_call_output"
);

/// A custom-tool call produced by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolCall {
    #[serde(rename = "type")]
    kind: CustomToolCallTag,
    call_id: String,
    name: String,
    input: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    namespace: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<ResponseItemStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CustomToolCall {
    /// Creates a custom-tool call.
    #[must_use]
    pub fn new(
        call_id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            kind: CustomToolCallTag::CustomToolCall,
            call_id: call_id.into(),
            name: name.into(),
            input: input.into(),
            id: Omittable::Omitted,
            namespace: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            created_by: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the stored item id when echoing a returned call.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(id.into());
        self
    }

    /// Sets the custom-tool namespace.
    #[must_use]
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Omittable::Value(namespace.into());
        self
    }

    /// Sets the execution context that produced this call.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned program-caller `caller_id` `1..=64`.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Custom-tool output supplied as input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolCallOutput {
    #[serde(rename = "type")]
    kind: CustomToolCallOutputTag,
    call_id: String,
    output: FunctionCallOutputValue,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CustomToolCallOutput {
    /// Creates a custom-tool output.
    #[must_use]
    pub fn new(call_id: impl Into<String>, output: impl Into<FunctionCallOutputValue>) -> Self {
        Self {
            kind: CustomToolCallOutputTag::CustomToolCallOutput,
            call_id: call_id.into(),
            output: output.into(),
            id: Omittable::Omitted,
            caller: Omittable::Omitted,
            status: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the stored item id when echoing a returned output.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(id.into());
        self
    }

    /// Sets the execution context that produced this output.
    #[must_use]
    pub fn caller(mut self, caller: ToolCallCaller) -> Self {
        self.caller = Omittable::Value(Nullable::Value(caller));
        self
    }

    /// Sends official `caller: null`.
    #[must_use]
    pub fn caller_null(mut self) -> Self {
        self.caller = Omittable::Value(Nullable::Null);
        self
    }

    /// Checks pinned program-caller `caller_id` `1..=64` and output `file_data`.
    pub fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_function_call_output_value(&self.output)?;
        validate_omittable_caller(&self.caller)
    }

    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Custom-tool output returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolCallOutputResource {
    #[serde(rename = "type")]
    kind: CustomToolCallOutputResourceTag,
    id: String,
    call_id: String,
    output: FunctionCallOutputValue,
    status: ResponseItemStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<ToolCallCaller>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_by: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CustomToolCallOutputResource {
    /// Returns future optional fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Frozen schema-name inventory for the 32 expanded stable input branches.
pub const STABLE_RESPONSE_INPUT_SCHEMAS: [&str; 32] = [
    "EasyInputMessage",
    "CompactionTriggerItemParam",
    "ItemReferenceParam",
    "ProgramItemParam",
    "ProgramOutputItemParam",
    "InputMessage",
    "OutputMessage",
    "FileSearchToolCall",
    "ComputerToolCall",
    "ComputerCallOutputItemParam",
    "WebSearchToolCall",
    "FunctionToolCall",
    "FunctionCallOutputItemParam",
    "ToolSearchCallItemParam",
    "ToolSearchOutputItemParam",
    "AdditionalToolsItemParam",
    "ReasoningItem",
    "CompactionSummaryItemParam",
    "ImageGenToolCall",
    "CodeInterpreterToolCall",
    "LocalShellToolCall",
    "LocalShellToolCallOutput",
    "FunctionShellCallItemParam",
    "FunctionShellCallOutputItemParam",
    "ApplyPatchToolCallItemParam",
    "ApplyPatchToolCallOutputItemParam",
    "MCPListTools",
    "MCPApprovalRequest",
    "MCPApprovalResponse",
    "MCPToolCall",
    "CustomToolCallOutput",
    "CustomToolCall",
];

/// Discriminators aligned positionally with [`STABLE_RESPONSE_INPUT_SCHEMAS`].
/// `<absent:id>` denotes the untagged stored-item reference branch.
pub const STABLE_RESPONSE_INPUT_DISCRIMINATORS: [&str; 32] = [
    "message",
    "compaction_trigger",
    "<absent:id>",
    "program",
    "program_output",
    "message",
    "message",
    "file_search_call",
    "computer_call",
    "computer_call_output",
    "web_search_call",
    "function_call",
    "function_call_output",
    "tool_search_call",
    "tool_search_output",
    "additional_tools",
    "reasoning",
    "compaction",
    "image_generation_call",
    "code_interpreter_call",
    "local_shell_call",
    "local_shell_call_output",
    "shell_call",
    "shell_call_output",
    "apply_patch_call",
    "apply_patch_call_output",
    "mcp_list_tools",
    "mcp_approval_request",
    "mcp_approval_response",
    "mcp_call",
    "custom_tool_call_output",
    "custom_tool_call",
];

/// Frozen schema-name inventory for the 28 stable output branches.
pub const STABLE_RESPONSE_OUTPUT_SCHEMAS: [&str; 28] = [
    "OutputMessage",
    "FileSearchToolCall",
    "FunctionToolCall",
    "FunctionToolCallOutputResource",
    "WebSearchToolCall",
    "ComputerToolCall",
    "ComputerToolCallOutputResource",
    "ReasoningItem",
    "Program",
    "ProgramOutput",
    "ToolSearchCall",
    "ToolSearchOutput",
    "AdditionalTools",
    "CompactionBody",
    "ImageGenToolCall",
    "CodeInterpreterToolCall",
    "LocalShellToolCall",
    "LocalShellToolCallOutput",
    "FunctionShellCall",
    "FunctionShellCallOutput",
    "ApplyPatchToolCall",
    "ApplyPatchToolCallOutput",
    "MCPToolCall",
    "MCPListTools",
    "MCPApprovalRequest",
    "MCPApprovalResponseResource",
    "CustomToolCall",
    "CustomToolCallOutputResource",
];

/// Discriminators aligned positionally with [`STABLE_RESPONSE_OUTPUT_SCHEMAS`].
pub const STABLE_RESPONSE_OUTPUT_DISCRIMINATORS: [&str; 28] = [
    "message",
    "file_search_call",
    "function_call",
    "function_call_output",
    "web_search_call",
    "computer_call",
    "computer_call_output",
    "reasoning",
    "program",
    "program_output",
    "tool_search_call",
    "tool_search_output",
    "additional_tools",
    "compaction",
    "image_generation_call",
    "code_interpreter_call",
    "local_shell_call",
    "local_shell_call_output",
    "shell_call",
    "shell_call_output",
    "apply_patch_call",
    "apply_patch_call_output",
    "mcp_call",
    "mcp_list_tools",
    "mcp_approval_request",
    "mcp_approval_response",
    "custom_tool_call",
    "custom_tool_call_output",
];

macro_rules! tag_only_tool {
    ($name:ident, $tag_name:ident, $tag_variant:ident, $wire:literal) => {
        literal_tag!($tag_name, $tag_variant, $wire);

        #[doc = concat!("Responses `", $wire, "` tool definition.")]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag_name,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Creates the minimal valid tool definition.
            #[must_use]
            pub fn new() -> Self {
                Self {
                    kind: $tag_name::$tag_variant,
                    extra: ExtraFields::new(),
                }
            }

            /// Returns future optional fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

literal_tag!(FileSearchToolTag, FileSearch, "file_search");

open_string_enum! {
    /// Official `RankerVersionType` for Responses file-search ranking.
    ///
    /// Assistants `FileSearchRanker` (`default_2024_08_21`) is a different
    /// official schema and is not a named Responses ranker.
    pub enum FileSearchRanker {
        Auto = "auto",
        Default2024_11_15 = "default-2024-11-15"
    }
}

/// Reciprocal-rank-fusion weights for hybrid file search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchHybridSearch {
    /// Weight of embedding matches.
    pub embedding_weight: f64,
    /// Weight of sparse keyword matches.
    pub text_weight: f64,
}

impl FileSearchHybridSearch {
    /// Creates hybrid-search weights.
    #[must_use]
    pub const fn new(embedding_weight: f64, text_weight: f64) -> Self {
        Self {
            embedding_weight,
            text_weight,
        }
    }
}

/// Ranking options for the Responses file-search tool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileSearchRankingOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ranker: Omittable<FileSearchRanker>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    score_threshold: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    hybrid_search: Omittable<FileSearchHybridSearch>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FileSearchRankingOptions {
    /// Creates empty ranking options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a ranker.
    #[must_use]
    pub fn ranker(mut self, ranker: impl Into<FileSearchRanker>) -> Self {
        self.ranker = Omittable::Value(ranker.into());
        self
    }

    /// Sets the score threshold in `0..=1`.
    #[must_use]
    pub fn score_threshold(mut self, score_threshold: f64) -> Self {
        self.score_threshold = Omittable::Value(score_threshold);
        self
    }

    /// Sets hybrid-search fusion weights.
    #[must_use]
    pub fn hybrid_search(mut self, hybrid_search: FileSearchHybridSearch) -> Self {
        self.hybrid_search = Omittable::Value(hybrid_search);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(threshold) = self.score_threshold
            && !(threshold.is_finite() && (0.0..=1.0).contains(&threshold))
        {
            return Err(CreateResponseConstraintError::FileSearchScoreThreshold {
                value: threshold.to_string(),
            });
        }
        Ok(())
    }
}

/// File-search tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchTool {
    #[serde(rename = "type")]
    kind: FileSearchToolTag,
    vector_store_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_num_results: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ranking_options: Omittable<FileSearchRankingOptions>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filters: Omittable<Nullable<crate::vector_stores::VectorStoreFilter>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FileSearchTool {
    /// Creates a file-search tool over one or more vector stores.
    #[must_use]
    pub fn new(vector_store_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind: FileSearchToolTag::FileSearch,
            vector_store_ids: vector_store_ids.into_iter().map(Into::into).collect(),
            max_num_results: Omittable::Omitted,
            ranking_options: Omittable::Omitted,
            filters: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Caps returned results. The pin documents `1..=50`.
    #[must_use]
    pub fn max_num_results(mut self, max_num_results: u32) -> Self {
        self.max_num_results = Omittable::Value(max_num_results);
        self
    }

    /// Sets ranking options.
    #[must_use]
    pub fn ranking_options(mut self, options: FileSearchRankingOptions) -> Self {
        self.ranking_options = Omittable::Value(options);
        self
    }

    /// Applies an attribute filter.
    #[must_use]
    pub fn filters(mut self, filters: crate::vector_stores::VectorStoreFilter) -> Self {
        self.filters = Omittable::Value(Nullable::Value(filters));
        self
    }

    /// Explicitly sends `filters: null`.
    #[must_use]
    pub fn filters_null(mut self) -> Self {
        self.filters = Omittable::Value(Nullable::Null);
        self
    }

    /// Returns selected vector-store ids.
    #[must_use]
    pub fn vector_store_ids(&self) -> &[String] {
        &self.vector_store_ids
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(actual) = self.max_num_results
            && !(MIN_FILE_SEARCH_RESULTS..=MAX_FILE_SEARCH_RESULTS).contains(&actual)
        {
            return Err(CreateResponseConstraintError::FileSearchMaxResults {
                actual,
                minimum: MIN_FILE_SEARCH_RESULTS,
                maximum: MAX_FILE_SEARCH_RESULTS,
            });
        }
        if let Omittable::Value(options) = &self.ranking_options {
            options.validate()?;
        }
        Ok(())
    }
}

tag_only_tool!(ComputerTool, ComputerToolTag, Computer, "computer");
tag_only_tool!(
    ProgrammaticTool,
    ProgrammaticToolTag,
    ProgrammaticToolCalling,
    "programmatic_tool_calling"
);
tag_only_tool!(LocalShellTool, LocalShellToolTag, LocalShell, "local_shell");

open_string_enum! {
    /// Context window guidance for web search.
    pub enum WebSearchContextSize {
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

open_string_enum! {
    /// Content kinds accepted by web-search preview.
    pub enum WebSearchContentType {
        Text = "text",
        Image = "image"
    }
}

literal_tag!(WebSearchLocationTag, Approximate, "approximate");

/// Approximate user location for Responses web search.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WebSearchUserLocation {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<WebSearchLocationTag>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    country: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    region: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    city: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    timezone: Omittable<Nullable<String>>,
}

impl WebSearchUserLocation {
    /// Creates an empty approximate location.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: Omittable::Value(WebSearchLocationTag::Approximate),
            ..Self::default()
        }
    }

    /// Sets the ISO country code.
    #[must_use]
    pub fn country(mut self, country: impl Into<String>) -> Self {
        self.country = Omittable::Value(Nullable::Value(country.into()));
        self
    }

    /// Sets the region.
    #[must_use]
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Omittable::Value(Nullable::Value(region.into()));
        self
    }

    /// Sets the city.
    #[must_use]
    pub fn city(mut self, city: impl Into<String>) -> Self {
        self.city = Omittable::Value(Nullable::Value(city.into()));
        self
    }

    /// Sets the IANA timezone.
    #[must_use]
    pub fn timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = Omittable::Value(Nullable::Value(timezone.into()));
        self
    }

    /// Sends official `country: null`.
    #[must_use]
    pub fn country_null(mut self) -> Self {
        self.country = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `region: null`.
    #[must_use]
    pub fn region_null(mut self) -> Self {
        self.region = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `city: null`.
    #[must_use]
    pub fn city_null(mut self) -> Self {
        self.city = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `timezone: null`.
    #[must_use]
    pub fn timezone_null(mut self) -> Self {
        self.timezone = Omittable::Value(Nullable::Null);
        self
    }
}

/// Domain allowlist for live web search.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WebSearchFilters {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_domains: Omittable<Nullable<Vec<String>>>,
}

impl WebSearchFilters {
    /// Restricts search to the supplied domains.
    #[must_use]
    pub fn allowed_domains(domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed_domains: Omittable::Value(Nullable::Value(
                domains.into_iter().map(Into::into).collect(),
            )),
        }
    }

    /// Sends `allowed_domains: null`.
    #[must_use]
    pub fn allowed_domains_null(mut self) -> Self {
        self.allowed_domains = Omittable::Value(Nullable::Null);
        self
    }
}

open_string_enum! {
    /// Official web-search tool `type` values.
    pub enum WebSearchToolTag {
        WebSearch = "web_search",
        WebSearch20250826 = "web_search_2025_08_26"
    }
}

/// Web-search tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchTool {
    #[serde(rename = "type")]
    kind: WebSearchToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    external_web_access: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filters: Omittable<Nullable<WebSearchFilters>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user_location: Omittable<Nullable<WebSearchUserLocation>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    search_context_size: Omittable<WebSearchContextSize>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchTool {
    /// Creates a web-search tool using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: WebSearchToolTag::WebSearch,
            external_web_access: Omittable::Omitted,
            filters: Omittable::Omitted,
            user_location: Omittable::Omitted,
            search_context_size: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates the dated official `web_search_2025_08_26` tool.
    #[must_use]
    pub fn web_search_2025_08_26() -> Self {
        Self {
            kind: WebSearchToolTag::WebSearch20250826,
            ..Self::new()
        }
    }

    /// Returns the official tool `type`.
    #[must_use]
    pub const fn kind(&self) -> &WebSearchToolTag {
        &self.kind
    }

    /// Allows or disables live internet access.
    #[must_use]
    pub fn external_web_access(mut self, enabled: bool) -> Self {
        self.external_web_access = Omittable::Value(enabled);
        self
    }

    /// Sets domain filters.
    #[must_use]
    pub fn filters(mut self, filters: WebSearchFilters) -> Self {
        self.filters = Omittable::Value(Nullable::Value(filters));
        self
    }

    /// Explicitly sends `filters: null`.
    #[must_use]
    pub fn filters_null(mut self) -> Self {
        self.filters = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets an approximate user location.
    #[must_use]
    pub fn user_location(mut self, location: WebSearchUserLocation) -> Self {
        self.user_location = Omittable::Value(Nullable::Value(location));
        self
    }

    /// Sends `user_location: null`.
    #[must_use]
    pub fn user_location_null(mut self) -> Self {
        self.user_location = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets search context size.
    #[must_use]
    pub fn search_context_size(mut self, size: WebSearchContextSize) -> Self {
        self.search_context_size = Omittable::Value(size);
        self
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

open_string_enum! {
    /// Official preview web-search tool `type` values.
    pub enum WebSearchPreviewToolTag {
        WebSearchPreview = "web_search_preview",
        WebSearchPreview20250311 = "web_search_preview_2025_03_11"
    }
}

/// Preview web-search tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchPreviewTool {
    #[serde(rename = "type")]
    kind: WebSearchPreviewToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user_location: Omittable<Nullable<WebSearchUserLocation>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    search_context_size: Omittable<WebSearchContextSize>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    search_content_types: Omittable<Vec<WebSearchContentType>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl WebSearchPreviewTool {
    /// Creates a preview web-search tool using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: WebSearchPreviewToolTag::WebSearchPreview,
            user_location: Omittable::Omitted,
            search_context_size: Omittable::Omitted,
            search_content_types: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates the dated official `web_search_preview_2025_03_11` tool.
    #[must_use]
    pub fn web_search_preview_2025_03_11() -> Self {
        Self {
            kind: WebSearchPreviewToolTag::WebSearchPreview20250311,
            ..Self::new()
        }
    }

    /// Returns the official tool `type`.
    #[must_use]
    pub const fn kind(&self) -> &WebSearchPreviewToolTag {
        &self.kind
    }

    /// Sets an approximate user location.
    #[must_use]
    pub fn user_location(mut self, location: WebSearchUserLocation) -> Self {
        self.user_location = Omittable::Value(Nullable::Value(location));
        self
    }

    /// Sends `user_location: null`.
    #[must_use]
    pub fn user_location_null(mut self) -> Self {
        self.user_location = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets search context size.
    #[must_use]
    pub fn search_context_size(mut self, size: WebSearchContextSize) -> Self {
        self.search_context_size = Omittable::Value(size);
        self
    }

    /// Restricts preview results to the supplied content kinds.
    #[must_use]
    pub fn search_content_types(
        mut self,
        types: impl IntoIterator<Item = impl Into<WebSearchContentType>>,
    ) -> Self {
        self.search_content_types = Omittable::Value(types.into_iter().map(Into::into).collect());
        self
    }
}

impl Default for WebSearchPreviewTool {
    fn default() -> Self {
        Self::new()
    }
}

open_string_enum! {
    /// Quality setting for the image-generation tool.
    pub enum ImageGenerationQuality {
        Low = "low",
        Medium = "medium",
        High = "high",
        Auto = "auto"
    }
}

open_string_enum! {
    /// Encoded output format for the image-generation tool.
    pub enum ImageGenerationOutputFormat {
        Png = "png",
        Webp = "webp",
        Jpeg = "jpeg"
    }
}

open_string_enum! {
    /// Moderation setting for the image-generation tool.
    pub enum ImageGenerationModeration {
        Auto = "auto",
        Low = "low"
    }
}

open_string_enum! {
    /// Background setting for the image-generation tool.
    pub enum ImageGenerationBackground {
        Transparent = "transparent",
        Opaque = "opaque",
        Auto = "auto"
    }
}

open_string_enum! {
    /// Input-image fidelity for the image-generation tool.
    pub enum ImageGenerationInputFidelity {
        High = "high",
        Low = "low"
    }
}

open_string_enum! {
    /// Requested image-generation action.
    pub enum ImageGenerationAction {
        Generate = "generate",
        Edit = "edit",
        Auto = "auto"
    }
}

/// Optional inpainting mask for the image-generation tool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationInputMask {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<String>,
}

impl ImageGenerationInputMask {
    /// Creates an empty mask.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a base64 mask image.
    #[must_use]
    pub fn image_url(mut self, image_url: impl Into<String>) -> Self {
        self.image_url = Omittable::Value(image_url.into());
        self
    }

    /// Sets a mask file id.
    #[must_use]
    pub fn file_id(mut self, file_id: impl Into<String>) -> Self {
        self.file_id = Omittable::Value(file_id.into());
        self
    }
}

literal_tag!(ImageGenerationToolTag, ImageGeneration, "image_generation");

/// Image-generation tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationTool {
    #[serde(rename = "type")]
    kind: ImageGenerationToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    quality: Omittable<ImageGenerationQuality>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    size: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_format: Omittable<ImageGenerationOutputFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_compression: Omittable<u8>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    moderation: Omittable<ImageGenerationModeration>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    background: Omittable<ImageGenerationBackground>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_fidelity: Omittable<Nullable<ImageGenerationInputFidelity>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input_image_mask: Omittable<ImageGenerationInputMask>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    partial_images: Omittable<u8>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    action: Omittable<ImageGenerationAction>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ImageGenerationTool {
    /// Creates an image-generation tool using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ImageGenerationToolTag::ImageGeneration,
            model: Omittable::Omitted,
            quality: Omittable::Omitted,
            size: Omittable::Omitted,
            output_format: Omittable::Omitted,
            output_compression: Omittable::Omitted,
            moderation: Omittable::Omitted,
            background: Omittable::Omitted,
            input_fidelity: Omittable::Omitted,
            input_image_mask: Omittable::Omitted,
            partial_images: Omittable::Omitted,
            action: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Selects an image model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(model.into());
        self
    }

    /// Sets output quality.
    #[must_use]
    pub fn quality(mut self, quality: impl Into<ImageGenerationQuality>) -> Self {
        self.quality = Omittable::Value(quality.into());
        self
    }

    /// Sets output size, including open `WIDTHxHEIGHT` strings.
    #[must_use]
    pub fn size(mut self, size: impl Into<String>) -> Self {
        self.size = Omittable::Value(size.into());
        self
    }

    /// Sets encoded output format.
    #[must_use]
    pub fn output_format(mut self, format: impl Into<ImageGenerationOutputFormat>) -> Self {
        self.output_format = Omittable::Value(format.into());
        self
    }

    /// Sets JPEG/WebP compression (`0..=100`).
    #[must_use]
    pub fn output_compression(mut self, output_compression: u8) -> Self {
        self.output_compression = Omittable::Value(output_compression);
        self
    }

    /// Sets image-generation moderation.
    #[must_use]
    pub fn moderation(mut self, moderation: impl Into<ImageGenerationModeration>) -> Self {
        self.moderation = Omittable::Value(moderation.into());
        self
    }

    /// Sets background behavior.
    #[must_use]
    pub fn background(mut self, background: impl Into<ImageGenerationBackground>) -> Self {
        self.background = Omittable::Value(background.into());
        self
    }

    /// Sets input-image fidelity.
    #[must_use]
    pub fn input_fidelity(mut self, fidelity: impl Into<ImageGenerationInputFidelity>) -> Self {
        self.input_fidelity = Omittable::Value(Nullable::Value(fidelity.into()));
        self
    }

    /// Sends `input_fidelity: null`.
    #[must_use]
    pub fn input_fidelity_null(mut self) -> Self {
        self.input_fidelity = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets an inpainting mask.
    #[must_use]
    pub fn input_image_mask(mut self, mask: ImageGenerationInputMask) -> Self {
        self.input_image_mask = Omittable::Value(mask);
        self
    }

    /// Sets the number of partial images (`0..=3`).
    #[must_use]
    pub fn partial_images(mut self, partial_images: u8) -> Self {
        self.partial_images = Omittable::Value(partial_images);
        self
    }

    /// Sets generate/edit/auto action.
    #[must_use]
    pub fn action(mut self, action: impl Into<ImageGenerationAction>) -> Self {
        self.action = Omittable::Value(action.into());
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(actual) = self.output_compression
            && actual > MAX_IMAGE_GENERATION_COMPRESSION
        {
            return Err(CreateResponseConstraintError::ImageGenerationCompression {
                actual,
                maximum: MAX_IMAGE_GENERATION_COMPRESSION,
            });
        }
        if let Omittable::Value(actual) = self.partial_images
            && actual > MAX_IMAGE_GENERATION_PARTIAL_IMAGES
        {
            return Err(
                CreateResponseConstraintError::ImageGenerationPartialImages {
                    actual,
                    maximum: MAX_IMAGE_GENERATION_PARTIAL_IMAGES,
                },
            );
        }
        Ok(())
    }
}

impl Default for ImageGenerationTool {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(FunctionShellAutoTag, ContainerAuto, "container_auto");
literal_tag!(FunctionShellLocalTag, Local, "local");
literal_tag!(
    FunctionShellReferenceTag,
    ContainerReference,
    "container_reference"
);

literal_tag!(
    ContainerSkillReferenceTag,
    SkillReference,
    "skill_reference"
);
literal_tag!(InlineSkillTag, Inline, "inline");
literal_tag!(InlineSkillSourceTag, Base64, "base64");

open_string_enum! {
    /// Media type accepted by an inline skill zip payload.
    pub enum InlineSkillMediaType {
        ApplicationZip = "application/zip"
    }
}

/// Reference to a skill created through `/skills`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerSkillReference {
    #[serde(rename = "type")]
    kind: ContainerSkillReferenceTag,
    skill_id: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    version: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ContainerSkillReference {
    /// References a skill by id.
    #[must_use]
    pub fn new(skill_id: impl Into<String>) -> Self {
        Self {
            kind: ContainerSkillReferenceTag::SkillReference,
            skill_id: skill_id.into(),
            version: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the skill version (`latest` or a positive integer).
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Omittable::Value(version.into());
        self
    }

    /// Returns the skill id.
    #[must_use]
    pub fn skill_id(&self) -> &str {
        &self.skill_id
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        let actual = self.skill_id.chars().count();
        if actual == 0 || actual > MAX_SKILL_ID_CHARS {
            return Err(CreateResponseConstraintError::SkillIdLength {
                actual,
                maximum: MAX_SKILL_ID_CHARS,
            });
        }
        Ok(())
    }
}

/// Base64-encoded zip source for an inline skill.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineSkillSource {
    #[serde(rename = "type")]
    kind: InlineSkillSourceTag,
    media_type: InlineSkillMediaType,
    data: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InlineSkillSource {
    /// Creates a zip source.
    #[must_use]
    pub fn zip(data: impl Into<String>) -> Self {
        Self {
            kind: InlineSkillSourceTag::Base64,
            media_type: InlineSkillMediaType::ApplicationZip,
            data: data.into(),
            extra: ExtraFields::new(),
        }
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_inline_skill_source_data_chars(self.data.chars().count())
    }
}

impl fmt::Debug for InlineSkillSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InlineSkillSource")
            .field("media_type", &self.media_type)
            .field("data", &"[REDACTED]")
            .finish()
    }
}

/// Inline skill defined on the request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineSkill {
    #[serde(rename = "type")]
    kind: InlineSkillTag,
    name: String,
    description: String,
    source: InlineSkillSource,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InlineSkill {
    /// Creates an inline skill.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        source: InlineSkillSource,
    ) -> Self {
        Self {
            kind: InlineSkillTag::Inline,
            name: name.into(),
            description: description.into(),
            source,
            extra: ExtraFields::new(),
        }
    }
}

/// A skill attached to an automatic container.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ContainerSkill {
    /// Existing `/skills` id.
    Reference(ContainerSkillReference),
    /// Inline zip payload.
    Inline(InlineSkill),
    /// Future skill retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ContainerSkill {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Reference(value) => value.serialize(serializer),
            Self::Inline(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ContainerSkill {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "skill_reference" => serde_json::from_value(value)
                .map(Self::Reference)
                .map_err(D::Error::custom),
            "inline" => serde_json::from_value(value)
                .map(Self::Inline)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A skill available on a local computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalSkill {
    name: String,
    description: String,
    path: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LocalSkill {
    /// Creates a local skill.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            extra: ExtraFields::new(),
        }
    }
}

fn validate_skill_count(actual: usize) -> Result<(), CreateResponseConstraintError> {
    if actual > MAX_SHELL_SKILLS {
        return Err(CreateResponseConstraintError::ShellEnvironmentSkills {
            actual,
            maximum: MAX_SHELL_SKILLS,
        });
    }
    Ok(())
}

/// Automatically provisioned container environment for the shell tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellContainerAuto {
    #[serde(rename = "type")]
    kind: FunctionShellAutoTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    memory_limit: Omittable<Nullable<CodeInterpreterMemoryLimit>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    network_policy: Omittable<CodeInterpreterNetworkPolicy>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    skills: Omittable<Vec<ContainerSkill>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellContainerAuto {
    /// Creates an automatic container environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: FunctionShellAutoTag::ContainerAuto,
            file_ids: Omittable::Omitted,
            memory_limit: Omittable::Omitted,
            network_policy: Omittable::Omitted,
            skills: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Makes uploaded files available to the container.
    #[must_use]
    pub fn file_ids(mut self, file_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.file_ids = Omittable::Value(file_ids.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the container memory limit.
    #[must_use]
    pub fn memory_limit(mut self, memory_limit: impl Into<CodeInterpreterMemoryLimit>) -> Self {
        self.memory_limit = Omittable::Value(Nullable::Value(memory_limit.into()));
        self
    }

    /// Sends official `memory_limit: null`.
    #[must_use]
    pub fn memory_limit_null(mut self) -> Self {
        self.memory_limit = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the container network policy.
    #[must_use]
    pub fn network_policy(mut self, policy: CodeInterpreterNetworkPolicy) -> Self {
        self.network_policy = Omittable::Value(policy);
        self
    }

    /// Attaches container skills.
    #[must_use]
    pub fn skills(mut self, skills: impl Into<Vec<ContainerSkill>>) -> Self {
        self.skills = Omittable::Value(skills.into());
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(file_ids) = &self.file_ids
            && file_ids.len() > MAX_SHELL_CONTAINER_FILE_IDS
        {
            return Err(CreateResponseConstraintError::ShellContainerFileIds {
                actual: file_ids.len(),
                maximum: MAX_SHELL_CONTAINER_FILE_IDS,
            });
        }
        if let Omittable::Value(skills) = &self.skills {
            validate_skill_count(skills.len())?;
            for skill in skills {
                match skill {
                    ContainerSkill::Reference(reference) => reference.validate()?,
                    ContainerSkill::Inline(inline) => inline.source.validate()?,
                    ContainerSkill::Unknown(_) => {}
                }
            }
        }
        Ok(())
    }
}

impl Default for FunctionShellContainerAuto {
    fn default() -> Self {
        Self::new()
    }
}

/// Local computer environment for the shell tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellLocalEnvironment {
    #[serde(rename = "type")]
    kind: FunctionShellLocalTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    skills: Omittable<Vec<LocalSkill>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellLocalEnvironment {
    /// Creates a local environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: FunctionShellLocalTag::Local,
            skills: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Attaches local skills.
    #[must_use]
    pub fn skills(mut self, skills: impl Into<Vec<LocalSkill>>) -> Self {
        self.skills = Omittable::Value(skills.into());
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(skills) = &self.skills {
            validate_skill_count(skills.len())?;
        }
        Ok(())
    }
}

impl Default for FunctionShellLocalEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Existing container referenced by the shell tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellContainerReference {
    #[serde(rename = "type")]
    kind: FunctionShellReferenceTag,
    container_id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellContainerReference {
    /// References an existing container.
    #[must_use]
    pub fn new(container_id: impl Into<String>) -> Self {
        Self {
            kind: FunctionShellReferenceTag::ContainerReference,
            container_id: container_id.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// Environment accepted by the shell tool.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FunctionShellEnvironment {
    /// Automatically created container.
    ContainerAuto(FunctionShellContainerAuto),
    /// Local computer.
    Local(FunctionShellLocalEnvironment),
    /// Existing container id.
    ContainerReference(FunctionShellContainerReference),
    /// Future environment retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for FunctionShellEnvironment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::ContainerAuto(value) => value.serialize(serializer),
            Self::Local(value) => value.serialize(serializer),
            Self::ContainerReference(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionShellEnvironment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "container_auto" => serde_json::from_value(value)
                .map(Self::ContainerAuto)
                .map_err(D::Error::custom),
            "local" => serde_json::from_value(value)
                .map(Self::Local)
                .map_err(D::Error::custom),
            "container_reference" => serde_json::from_value(value)
                .map(Self::ContainerReference)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

literal_tag!(FunctionShellToolTag, Shell, "shell");

/// Shell tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionShellTool {
    #[serde(rename = "type")]
    kind: FunctionShellToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    environment: Omittable<Nullable<FunctionShellEnvironment>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionShellTool {
    /// Creates a shell tool using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: FunctionShellToolTag::Shell,
            environment: Omittable::Omitted,
            allowed_callers: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the execution environment.
    #[must_use]
    pub fn environment(mut self, environment: FunctionShellEnvironment) -> Self {
        self.environment = Omittable::Value(Nullable::Value(environment));
        self
    }

    /// Explicitly sends `environment: null`.
    #[must_use]
    pub fn environment_null(mut self) -> Self {
        self.environment = Omittable::Value(Nullable::Null);
        self
    }

    /// Restricts invocation contexts.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Sends official `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_allowed_callers(&self.allowed_callers)?;
        match &self.environment {
            Omittable::Value(Nullable::Value(FunctionShellEnvironment::ContainerAuto(
                container,
            ))) => container.validate(),
            Omittable::Value(Nullable::Value(FunctionShellEnvironment::Local(local))) => {
                local.validate()
            }
            _ => Ok(()),
        }
    }
}

impl Default for FunctionShellTool {
    fn default() -> Self {
        Self::new()
    }
}

open_string_enum! {
    /// Whether tool search runs on the server or the client.
    pub enum ToolSearchExecution {
        Server = "server",
        Client = "client"
    }
}

literal_tag!(ToolSearchToolTag, ToolSearch, "tool_search");

/// Tool-search configuration for deferred tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchTool {
    #[serde(rename = "type")]
    kind: ToolSearchToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    execution: Omittable<ToolSearchExecution>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parameters: Omittable<Nullable<Value>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ToolSearchTool {
    /// Creates a tool-search definition using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ToolSearchToolTag::ToolSearch,
            execution: Omittable::Omitted,
            description: Omittable::Omitted,
            parameters: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets server or client execution.
    #[must_use]
    pub fn execution(mut self, execution: impl Into<ToolSearchExecution>) -> Self {
        self.execution = Omittable::Value(execution.into());
        self
    }

    /// Sets a client-executed description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(Nullable::Value(description.into()));
        self
    }

    /// Sets a client-executed parameter schema.
    #[must_use]
    pub fn parameters(mut self, parameters: Value) -> Self {
        self.parameters = Omittable::Value(Nullable::Value(parameters));
        self
    }

    /// Sends official `description: null`.
    #[must_use]
    pub fn description_null(mut self) -> Self {
        self.description = Omittable::Value(Nullable::Null);
        self
    }

    /// Sends official `parameters: null`.
    #[must_use]
    pub fn parameters_null(mut self) -> Self {
        self.parameters = Omittable::Value(Nullable::Null);
        self
    }
}

impl Default for ToolSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(ApplyPatchToolTag, ApplyPatch, "apply_patch");

/// Apply-patch tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyPatchTool {
    #[serde(rename = "type")]
    kind: ApplyPatchToolTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ApplyPatchTool {
    /// Creates an apply-patch tool.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: ApplyPatchToolTag::ApplyPatch,
            allowed_callers: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Restricts invocation contexts.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Sends official `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_allowed_callers(&self.allowed_callers)
    }
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(
    ComputerUsePreviewToolTag,
    ComputerUsePreview,
    "computer_use_preview"
);

/// Preview computer-use tool with an explicit virtual display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerUsePreviewTool {
    #[serde(rename = "type")]
    kind: ComputerUsePreviewToolTag,
    environment: String,
    display_width: u32,
    display_height: u32,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ComputerUsePreviewTool {
    /// Creates a computer-use preview display.
    #[must_use]
    pub fn new(environment: impl Into<String>, display_width: u32, display_height: u32) -> Self {
        Self {
            kind: ComputerUsePreviewToolTag::ComputerUsePreview,
            environment: environment.into(),
            display_width,
            display_height,
            extra: ExtraFields::new(),
        }
    }
}

literal_tag!(CodeInterpreterToolTag, CodeInterpreter, "code_interpreter");
literal_tag!(AutoCodeInterpreterTag, Auto, "auto");
literal_tag!(CodeInterpreterNetworkDisabledTag, Disabled, "disabled");
literal_tag!(CodeInterpreterNetworkAllowlistTag, Allowlist, "allowlist");

open_string_enum! {
    /// Memory limit accepted by an automatic code-interpreter container.
    pub enum CodeInterpreterMemoryLimit {
        OneGiB = "1g",
        FourGiB = "4g",
        SixteenGiB = "16g",
        SixtyFourGiB = "64g"
    }
}

/// Disable outbound network access for a code-interpreter container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterNetworkDisabled {
    #[serde(rename = "type")]
    kind: CodeInterpreterNetworkDisabledTag,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterNetworkDisabled {
    /// Creates a disabled network policy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: CodeInterpreterNetworkDisabledTag::Disabled,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for CodeInterpreterNetworkDisabled {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain-scoped secret injected into a code-interpreter container.
///
/// Wire shape mirrors the pinned `ContainerNetworkPolicyDomainSecretParam`
/// (`domain`/`name` `minLength` 1, `value` `1..=10485760`) and reuses the
/// D0076 containers-side limits; the value stays redacted in `Debug`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeInterpreterDomainSecret {
    domain: String,
    name: String,
    value: WireSecret,
}

impl PartialEq for CodeInterpreterDomainSecret {
    fn eq(&self, other: &Self) -> bool {
        self.domain == other.domain
            && self.name == other.name
            && self
                .value
                .with_exposed(|left| other.value.with_exposed(|right| left == right))
    }
}

impl Eq for CodeInterpreterDomainSecret {}

impl CodeInterpreterDomainSecret {
    /// Constructs a domain-scoped secret.
    #[must_use]
    pub fn new(
        domain: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<WireSecret>,
    ) -> Self {
        Self {
            domain: domain.into(),
            name: name.into(),
            value: value.into(),
        }
    }

    /// Returns the associated domain.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Returns the injected secret name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Allow outbound access only to listed domains.
///
/// `allowed_domains` mirrors the pinned
/// `ContainerNetworkPolicyAllowlistParam.allowed_domains` (minItems 1) and
/// `domain_secrets` mirrors its `domain_secrets` (minItems 1, elements of
/// `ContainerNetworkPolicyDomainSecretParam`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterNetworkAllowlist {
    #[serde(rename = "type")]
    kind: CodeInterpreterNetworkAllowlistTag,
    allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    domain_secrets: Omittable<Vec<CodeInterpreterDomainSecret>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterNetworkAllowlist {
    /// Creates an allowlist policy.
    #[must_use]
    pub fn new(allowed_domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind: CodeInterpreterNetworkAllowlistTag::Allowlist,
            allowed_domains: allowed_domains.into_iter().map(Into::into).collect(),
            domain_secrets: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Adds one domain-scoped secret.
    #[must_use]
    pub fn with_secret(mut self, secret: CodeInterpreterDomainSecret) -> Self {
        match &mut self.domain_secrets {
            Omittable::Value(secrets) => secrets.push(secret),
            Omittable::Omitted => self.domain_secrets = Omittable::Value(vec![secret]),
        }
        self
    }

    /// Returns the allowed domains.
    #[must_use]
    pub fn allowed_domains(&self) -> &[String] {
        &self.allowed_domains
    }

    /// Returns domain-scoped secrets when present.
    #[must_use]
    pub fn domain_secrets(&self) -> &[CodeInterpreterDomainSecret] {
        match &self.domain_secrets {
            Omittable::Value(secrets) => secrets,
            Omittable::Omitted => &[],
        }
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if self.allowed_domains.is_empty() {
            return Err(CreateResponseConstraintError::EmptyAllowedDomains);
        }
        if let Omittable::Value(secrets) = &self.domain_secrets {
            if secrets.is_empty() {
                return Err(CreateResponseConstraintError::EmptyDomainSecrets);
            }
            for secret in secrets {
                validate_domain_secret(secret)?;
            }
        }
        Ok(())
    }
}

/// Network policy for an automatic code-interpreter container.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CodeInterpreterNetworkPolicy {
    /// No outbound access.
    Disabled(CodeInterpreterNetworkDisabled),
    /// Restricted outbound access.
    Allowlist(CodeInterpreterNetworkAllowlist),
    /// Future policy retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl CodeInterpreterNetworkPolicy {
    /// Creates a disabled policy.
    #[must_use]
    pub fn disabled() -> Self {
        Self::Disabled(CodeInterpreterNetworkDisabled::new())
    }

    /// Creates an allowlist policy.
    #[must_use]
    pub fn allowlist(allowed_domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Allowlist(CodeInterpreterNetworkAllowlist::new(allowed_domains))
    }
}

impl Serialize for CodeInterpreterNetworkPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Disabled(value) => value.serialize(serializer),
            Self::Allowlist(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CodeInterpreterNetworkPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "disabled" => serde_json::from_value(value)
                .map(Self::Disabled)
                .map_err(D::Error::custom),
            "allowlist" => serde_json::from_value(value)
                .map(Self::Allowlist)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// Automatic code-interpreter container configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoCodeInterpreterContainer {
    #[serde(rename = "type")]
    kind: AutoCodeInterpreterTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    memory_limit: Omittable<Nullable<CodeInterpreterMemoryLimit>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    network_policy: Omittable<CodeInterpreterNetworkPolicy>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl AutoCodeInterpreterContainer {
    /// Creates an automatic container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: AutoCodeInterpreterTag::Auto,
            file_ids: Omittable::Omitted,
            memory_limit: Omittable::Omitted,
            network_policy: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Makes uploaded files available to the container.
    #[must_use]
    pub fn file_ids(mut self, file_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.file_ids = Omittable::Value(file_ids.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the container memory limit.
    #[must_use]
    pub fn memory_limit(mut self, memory_limit: impl Into<CodeInterpreterMemoryLimit>) -> Self {
        self.memory_limit = Omittable::Value(Nullable::Value(memory_limit.into()));
        self
    }

    /// Sends official `memory_limit: null`.
    #[must_use]
    pub fn memory_limit_null(mut self) -> Self {
        self.memory_limit = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets the container network policy.
    #[must_use]
    pub fn network_policy(mut self, policy: CodeInterpreterNetworkPolicy) -> Self {
        self.network_policy = Omittable::Value(policy);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if let Omittable::Value(file_ids) = &self.file_ids
            && file_ids.len() > MAX_CODE_INTERPRETER_FILE_IDS
        {
            return Err(CreateResponseConstraintError::CodeInterpreterFileIds {
                actual: file_ids.len(),
                maximum: MAX_CODE_INTERPRETER_FILE_IDS,
            });
        }
        if let Omittable::Value(CodeInterpreterNetworkPolicy::Allowlist(policy)) =
            &self.network_policy
        {
            policy.validate()?;
        }
        Ok(())
    }
}

/// Checks the pinned domain-secret length limits shared with Containers.
fn validate_domain_secret(
    secret: &CodeInterpreterDomainSecret,
) -> Result<(), CreateResponseConstraintError> {
    let domain = secret.domain.chars().count();
    if domain < MIN_DOMAIN_SECRET_CHARS {
        return Err(CreateResponseConstraintError::DomainSecretDomain {
            actual: domain,
            minimum: MIN_DOMAIN_SECRET_CHARS,
        });
    }
    let name = secret.name.chars().count();
    if name < MIN_DOMAIN_SECRET_CHARS {
        return Err(CreateResponseConstraintError::DomainSecretName {
            actual: name,
            minimum: MIN_DOMAIN_SECRET_CHARS,
        });
    }
    secret.value.with_exposed(|value| {
        let actual = value.chars().count();
        if !(MIN_DOMAIN_SECRET_CHARS..=MAX_DOMAIN_SECRET_VALUE_CHARS).contains(&actual) {
            return Err(CreateResponseConstraintError::DomainSecretValue {
                actual,
                minimum: MIN_DOMAIN_SECRET_CHARS,
                maximum: MAX_DOMAIN_SECRET_VALUE_CHARS,
            });
        }
        Ok(())
    })
}

impl Default for AutoCodeInterpreterContainer {
    fn default() -> Self {
        Self::new()
    }
}

/// Code-interpreter container: an existing id or an automatic configuration.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CodeInterpreterContainer {
    /// Existing container id.
    Id(String),
    /// Automatically provisioned container.
    Auto(AutoCodeInterpreterContainer),
    /// Future object container retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for CodeInterpreterContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Id(value) => value.serialize(serializer),
            Self::Auto(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CodeInterpreterContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(id) => Ok(Self::Id(id)),
            Value::Object(_) => match object_discriminator(&value) {
                Ok(tag) if tag == "auto" => serde_json::from_value(value)
                    .map(Self::Auto)
                    .map_err(D::Error::custom),
                Ok(_) => UnknownTaggedObject::from_value(value)
                    .map(Self::Unknown)
                    .map_err(D::Error::custom),
                Err(error) => Err(D::Error::custom(error)),
            },
            _ => Err(D::Error::custom(
                "code interpreter container must be a string id or an object",
            )),
        }
    }
}

/// Code-interpreter tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterTool {
    #[serde(rename = "type")]
    kind: CodeInterpreterToolTag,
    container: CodeInterpreterContainer,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterTool {
    /// Selects an existing container by id.
    #[must_use]
    pub fn container_id(container_id: impl Into<String>) -> Self {
        Self {
            kind: CodeInterpreterToolTag::CodeInterpreter,
            container: CodeInterpreterContainer::Id(container_id.into()),
            allowed_callers: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Uses an automatic container with service defaults.
    #[must_use]
    pub fn automatic() -> Self {
        Self::auto(AutoCodeInterpreterContainer::new())
    }

    /// Uses a typed automatic-container configuration.
    #[must_use]
    pub fn auto(container: AutoCodeInterpreterContainer) -> Self {
        Self {
            kind: CodeInterpreterToolTag::CodeInterpreter,
            container: CodeInterpreterContainer::Auto(container),
            allowed_callers: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the selected container.
    #[must_use]
    pub const fn container(&self) -> &CodeInterpreterContainer {
        &self.container
    }

    /// Restricts which invocation contexts may call this tool.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Explicitly sends `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_allowed_callers(&self.allowed_callers)?;
        if let CodeInterpreterContainer::Auto(container) = &self.container {
            container.validate()?;
        }
        Ok(())
    }
}

literal_tag!(CustomToolTag, Custom, "custom");

open_string_enum! {
    /// Grammar syntax accepted by a custom tool.
    pub enum CustomToolGrammarSyntax {
        Lark = "lark",
        Regex = "regex"
    }
}

literal_tag!(CustomTextFormatTag, Text, "text");
literal_tag!(CustomGrammarFormatTag, Grammar, "grammar");

/// Unconstrained custom-tool text format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTextFormat {
    #[serde(rename = "type")]
    kind: CustomTextFormatTag,
}

impl CustomTextFormat {
    /// Creates unconstrained text format.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: CustomTextFormatTag::Text,
        }
    }
}

impl Default for CustomTextFormat {
    fn default() -> Self {
        Self::new()
    }
}

/// Grammar-constrained custom-tool format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomGrammarFormat {
    #[serde(rename = "type")]
    kind: CustomGrammarFormatTag,
    syntax: CustomToolGrammarSyntax,
    definition: String,
}

impl CustomGrammarFormat {
    /// Creates a grammar format.
    #[must_use]
    pub fn new(syntax: impl Into<CustomToolGrammarSyntax>, definition: impl Into<String>) -> Self {
        Self {
            kind: CustomGrammarFormatTag::Grammar,
            syntax: syntax.into(),
            definition: definition.into(),
        }
    }
}

/// Input format for a Responses custom tool.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CustomToolFormat {
    /// Unconstrained text.
    Text(CustomTextFormat),
    /// Lark or regex grammar.
    Grammar(CustomGrammarFormat),
    /// Future format retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for CustomToolFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Text(value) => value.serialize(serializer),
            Self::Grammar(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CustomToolFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match object_discriminator(&value)
            .map_err(D::Error::custom)?
            .as_str()
        {
            "text" => serde_json::from_value(value)
                .map(Self::Text)
                .map_err(D::Error::custom),
            "grammar" => serde_json::from_value(value)
                .map(Self::Grammar)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// A named custom free-form tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTool {
    #[serde(rename = "type")]
    kind: CustomToolTag,
    name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    format: Omittable<CustomToolFormat>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    defer_loading: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<AllowedCaller>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CustomTool {
    /// Creates a named custom tool.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: CustomToolTag::Custom,
            name: name.into(),
            description: Omittable::Omitted,
            format: Omittable::Omitted,
            defer_loading: Omittable::Omitted,
            allowed_callers: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets the model-facing description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Sets the input format.
    #[must_use]
    pub fn format(mut self, format: CustomToolFormat) -> Self {
        self.format = Omittable::Value(format);
        self
    }

    /// Marks the tool for deferred discovery.
    #[must_use]
    pub fn defer_loading(mut self, defer_loading: bool) -> Self {
        self.defer_loading = Omittable::Value(defer_loading);
        self
    }

    /// Restricts invocation contexts.
    #[must_use]
    pub fn allowed_callers(
        mut self,
        callers: impl IntoIterator<Item = impl Into<AllowedCaller>>,
    ) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Sends official `allowed_callers: null`.
    #[must_use]
    pub fn allowed_callers_null(mut self) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Null);
        self
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        validate_allowed_callers(&self.allowed_callers)
    }
}

literal_tag!(NamespaceToolTag, Namespace, "namespace");

tagged_union! {
    /// A function or custom tool nested inside a namespace.
    ///
    /// The pinned `NamespaceToolParam.tools.items` union is
    /// `oneOf [FunctionToolParam, CustomToolParam]`, so hosted tool types such
    /// as `web_search` cannot be constructed for this position. A genuinely
    /// future nested tool tag decodes losslessly as [`Unknown`].
    pub enum NamespaceToolEntry {
        Function(FunctionTool) => "function",
        Custom(CustomTool) => "custom"
    }
}

impl From<FunctionTool> for NamespaceToolEntry {
    fn from(value: FunctionTool) -> Self {
        Self::Function(value)
    }
}

impl From<CustomTool> for NamespaceToolEntry {
    fn from(value: CustomTool) -> Self {
        Self::Custom(value)
    }
}

/// A namespace that groups tools for deferred discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceTool {
    #[serde(rename = "type")]
    kind: NamespaceToolTag,
    name: String,
    description: String,
    tools: Vec<NamespaceToolEntry>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl NamespaceTool {
    /// Creates a namespace and its nested function/custom tools.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        tools: impl IntoIterator<Item = impl Into<NamespaceToolEntry>>,
    ) -> Self {
        Self {
            kind: NamespaceToolTag::Namespace,
            name: name.into(),
            description: description.into(),
            tools: tools.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
        }
    }

    /// Returns the nested function/custom tools.
    #[must_use]
    pub fn tools(&self) -> &[NamespaceToolEntry] {
        &self.tools
    }

    fn validate(&self) -> Result<(), CreateResponseConstraintError> {
        if self.name.is_empty() {
            return Err(CreateResponseConstraintError::EmptyNamespaceName);
        }
        if self.tools.is_empty() {
            return Err(CreateResponseConstraintError::EmptyNamespaceTools);
        }
        for tool in &self.tools {
            match tool {
                NamespaceToolEntry::Function(tool) => tool.validate()?,
                NamespaceToolEntry::Custom(tool) => tool.validate()?,
                NamespaceToolEntry::Unknown(_) => {}
            }
        }
        Ok(())
    }
}

/// Frozen schema-name inventory for the 16 stable tool branches.
pub const STABLE_RESPONSE_TOOL_SCHEMAS: [&str; 16] = [
    "FunctionTool",
    "FileSearchTool",
    "ComputerTool",
    "ComputerUsePreviewTool",
    "WebSearchTool",
    "MCPTool",
    "CodeInterpreterTool",
    "ProgrammaticToolCallingParam",
    "ImageGenTool",
    "LocalShellToolParam",
    "FunctionShellToolParam",
    "CustomToolParam",
    "NamespaceToolParam",
    "ToolSearchToolParam",
    "WebSearchPreviewTool",
    "ApplyPatchToolParam",
];

/// Discriminators aligned positionally with [`STABLE_RESPONSE_TOOL_SCHEMAS`].
pub const STABLE_RESPONSE_TOOL_DISCRIMINATORS: [&str; 16] = [
    "function",
    "file_search",
    "computer",
    "computer_use_preview",
    "web_search",
    "mcp",
    "code_interpreter",
    "programmatic_tool_calling",
    "image_generation",
    "local_shell",
    "shell",
    "custom",
    "namespace",
    "tool_search",
    "web_search_preview",
    "apply_patch",
];

open_string_enum! {
    /// Whether an allowed-tool set is optional or mandatory.
    pub enum AllowedToolsMode {
        Auto = "auto",
        Required = "required"
    }
}

literal_tag!(AllowedToolsChoiceTag, AllowedTools, "allowed_tools");

/// Restricts the model to a serialized set of tool selectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllowedToolsChoice {
    #[serde(rename = "type")]
    kind: AllowedToolsChoiceTag,
    mode: AllowedToolsMode,
    tools: Vec<Value>,
}

impl AllowedToolsChoice {
    /// Creates an empty allowed-tool set.
    #[must_use]
    pub fn new(mode: AllowedToolsMode) -> Self {
        Self {
            kind: AllowedToolsChoiceTag::AllowedTools,
            mode,
            tools: Vec::new(),
        }
    }

    /// Serializes and adds a typed tool selector.
    pub fn tool<T: Serialize>(mut self, tool: &T) -> Result<Self, serde_json::Error> {
        self.tools.push(serde_json::to_value(tool)?);
        Ok(self)
    }

    /// Returns serialized selectors in wire order.
    #[must_use]
    pub fn tools(&self) -> &[Value] {
        &self.tools
    }
}

open_string_enum! {
    /// Hosted tool types accepted by the tool-choice object branch.
    ///
    /// Members match the pinned `ToolChoiceTypes.type` domain exactly. The
    /// tool-type strings `web_search` / `web_search_2025_08_26` name request
    /// tools, not tool-choice values, and are therefore only reachable through
    /// the open-enum `Unknown` escape hatch, which still decodes any string.
    pub enum HostedToolType {
        FileSearch = "file_search",
        WebSearchPreview = "web_search_preview",
        Computer = "computer",
        ComputerUsePreview = "computer_use_preview",
        ComputerUse = "computer_use",
        WebSearchPreview20250311 = "web_search_preview_2025_03_11",
        ImageGeneration = "image_generation",
        CodeInterpreter = "code_interpreter"
    }
}

/// Forces one hosted tool type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedToolChoice {
    #[serde(rename = "type")]
    kind: HostedToolType,
}

impl HostedToolChoice {
    /// Creates a hosted-tool selector.
    #[must_use]
    pub fn new(kind: HostedToolType) -> Self {
        Self { kind }
    }

    /// Returns the hosted tool type.
    #[must_use]
    pub const fn kind(&self) -> &HostedToolType {
        &self.kind
    }
}

literal_tag!(CustomToolChoiceTag, Custom, "custom");

/// Forces one named custom tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolChoice {
    #[serde(rename = "type")]
    kind: CustomToolChoiceTag,
    name: String,
}

impl CustomToolChoice {
    /// Creates a custom-tool selector.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            kind: CustomToolChoiceTag::Custom,
            name: name.into(),
        }
    }
}

macro_rules! tag_only_choice {
    ($name:ident, $tag_name:ident, $tag_variant:ident, $wire:literal) => {
        literal_tag!($tag_name, $tag_variant, $wire);

        #[doc = concat!("Forces the Responses `", $wire, "` tool.")]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag_name,
        }

        impl $name {
            /// Creates this exact tool choice.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    kind: $tag_name::$tag_variant,
                }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

tag_only_choice!(
    ProgrammaticToolChoice,
    ProgrammaticToolChoiceTag,
    ProgrammaticToolCalling,
    "programmatic_tool_calling"
);
tag_only_choice!(
    ApplyPatchToolChoice,
    ApplyPatchToolChoiceTag,
    ApplyPatch,
    "apply_patch"
);
tag_only_choice!(
    FunctionShellToolChoice,
    FunctionShellToolChoiceTag,
    Shell,
    "shell"
);

/// Frozen schema-name inventory for the nine stable tool-choice branches.
pub const STABLE_RESPONSE_TOOL_CHOICE_SCHEMAS: [&str; 9] = [
    "ToolChoiceOptions",
    "ToolChoiceAllowed",
    "ToolChoiceTypes",
    "ToolChoiceFunction",
    "ToolChoiceMCP",
    "ToolChoiceCustom",
    "SpecificProgrammaticToolCallingParam",
    "SpecificApplyPatchParam",
    "SpecificFunctionShellParam",
];

/// Route discriminators aligned with [`STABLE_RESPONSE_TOOL_CHOICE_SCHEMAS`].
pub const STABLE_RESPONSE_TOOL_CHOICE_DISCRIMINATORS: [&str; 9] = [
    "<string:none|auto|required>",
    "allowed_tools",
    "<hosted-tool-type>",
    "function",
    "mcp",
    "custom",
    "programmatic_tool_calling",
    "apply_patch",
    "shell",
];

macro_rules! required_stream_event {
    ($name:ident, $tag_name:ident, $tag_variant:ident, $wire:literal, {
        $($field:ident: $ty:ty),* $(,)?
    }) => {
        required_stream_event!($name, $tag_name, $tag_variant, $wire, { $($field: $ty),* }, {});
    };
    ($name:ident, $tag_name:ident, $tag_variant:ident, $wire:literal, {
        $($field:ident: $ty:ty),* $(,)?
    }, {
        $($opt:ident: $oty:ty),* $(,)?
    }) => {
        literal_tag!($tag_name, $tag_variant, $wire);

        #[doc = concat!("Streaming event `", $wire, "`.")]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag_name,
            $($field: $ty,)*
            $(
                #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
                $opt: Omittable<$oty>,
            )*
            sequence_number: u64,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the monotonically increasing event sequence number.
            #[must_use]
            pub const fn sequence_number(&self) -> u64 {
                self.sequence_number
            }

            $(
                /// Returns the official optional field when the service sent it.
                #[must_use]
                pub const fn $opt(&self) -> Option<&$oty> {
                    match &self.$opt {
                        Omittable::Value(value) => Some(value),
                        Omittable::Omitted => None,
                    }
                }
            )*

            /// Returns future fields retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

required_stream_event!(AudioDeltaEvent, AudioDeltaEventTag, AudioDelta, "response.audio.delta", {
    delta: String
});
required_stream_event!(
    AudioDoneEvent,
    AudioDoneEventTag,
    AudioDone,
    "response.audio.done",
    {}
);
required_stream_event!(
    AudioTranscriptDeltaEvent,
    AudioTranscriptDeltaEventTag,
    AudioTranscriptDelta,
    "response.audio.transcript.delta",
    { delta: String }
);
required_stream_event!(
    AudioTranscriptDoneEvent,
    AudioTranscriptDoneEventTag,
    AudioTranscriptDone,
    "response.audio.transcript.done",
    {}
);
required_stream_event!(
    CodeInterpreterCodeDeltaEvent,
    CodeInterpreterCodeDeltaEventTag,
    CodeInterpreterCodeDelta,
    "response.code_interpreter_call_code.delta",
    {
        output_index: u64,
        item_id: String,
        delta: String
    }
);
required_stream_event!(
    CodeInterpreterCodeDoneEvent,
    CodeInterpreterCodeDoneEventTag,
    CodeInterpreterCodeDone,
    "response.code_interpreter_call_code.done",
    {
        output_index: u64,
        item_id: String,
        code: String
    }
);

literal_tag!(
    CodeInterpreterCompletedEventTag,
    CodeInterpreterCompleted,
    "response.code_interpreter_call.completed"
);
literal_tag!(
    CodeInterpreterInProgressEventTag,
    CodeInterpreterInProgress,
    "response.code_interpreter_call.in_progress"
);
literal_tag!(
    CodeInterpreterInterpretingEventTag,
    CodeInterpreterInterpreting,
    "response.code_interpreter_call.interpreting"
);
tool_status_event!(
    CodeInterpreterCompletedEvent,
    CodeInterpreterCompletedEventTag,
    CodeInterpreterCompleted
);
tool_status_event!(
    CodeInterpreterInProgressEvent,
    CodeInterpreterInProgressEventTag,
    CodeInterpreterInProgress
);
tool_status_event!(
    CodeInterpreterInterpretingEvent,
    CodeInterpreterInterpretingEventTag,
    CodeInterpreterInterpreting
);

literal_tag!(
    FileSearchCompletedEventTag,
    FileSearchCompleted,
    "response.file_search_call.completed"
);
literal_tag!(
    FileSearchInProgressEventTag,
    FileSearchInProgress,
    "response.file_search_call.in_progress"
);
literal_tag!(
    FileSearchSearchingEventTag,
    FileSearchSearching,
    "response.file_search_call.searching"
);
tool_status_event!(
    FileSearchCompletedEvent,
    FileSearchCompletedEventTag,
    FileSearchCompleted
);
tool_status_event!(
    FileSearchInProgressEvent,
    FileSearchInProgressEventTag,
    FileSearchInProgress
);
tool_status_event!(
    FileSearchSearchingEvent,
    FileSearchSearchingEventTag,
    FileSearchSearching
);

required_stream_event!(
    ShellCommandAddedEvent,
    ShellCommandAddedEventTag,
    ShellCommandAdded,
    "response.shell_call_command.added",
    {
        output_index: u64,
        command_index: u64,
        command: String
    }
);
required_stream_event!(
    ShellCommandDeltaEvent,
    ShellCommandDeltaEventTag,
    ShellCommandDelta,
    "response.shell_call_command.delta",
    {
        output_index: u64,
        command_index: u64,
        delta: String
    },
    {
        obfuscation: String
    }
);
required_stream_event!(
    ShellCommandDoneEvent,
    ShellCommandDoneEventTag,
    ShellCommandDone,
    "response.shell_call_command.done",
    {
        output_index: u64,
        command_index: u64,
        command: String
    }
);
/// Incremental stdout/stderr for `response.shell_call_output_content.delta`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShellCallOutputDelta {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stdout: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stderr: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ShellCallOutputDelta {
    /// Returns stdout when present.
    #[must_use]
    pub fn stdout(&self) -> Option<&str> {
        match &self.stdout {
            Omittable::Value(value) => Some(value.as_str()),
            Omittable::Omitted => None,
        }
    }

    /// Returns stderr when present.
    #[must_use]
    pub fn stderr(&self) -> Option<&str> {
        match &self.stderr {
            Omittable::Value(value) => Some(value.as_str()),
            Omittable::Omitted => None,
        }
    }
}

required_stream_event!(
    ShellOutputContentDeltaEvent,
    ShellOutputContentDeltaEventTag,
    ShellOutputContentDelta,
    "response.shell_call_output_content.delta",
    {
        item_id: String,
        output_index: u64,
        command_index: u64,
        delta: ShellCallOutputDelta
    }
);
required_stream_event!(
    ShellOutputContentDoneEvent,
    ShellOutputContentDoneEventTag,
    ShellOutputContentDone,
    "response.shell_call_output_content.done",
    {
        item_id: String,
        output_index: u64,
        command_index: u64,
        output: Vec<FunctionShellCallOutputContent>
    }
);

impl ShellOutputContentDeltaEvent {
    /// Returns the containing item id.
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Returns the incremental stdout/stderr payload.
    #[must_use]
    pub const fn delta(&self) -> &ShellCallOutputDelta {
        &self.delta
    }
}

impl ShellOutputContentDoneEvent {
    /// Returns the containing item id.
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Returns the finished output content items.
    #[must_use]
    pub fn output(&self) -> &[FunctionShellCallOutputContent] {
        &self.output
    }
}
required_stream_event!(
    ReasoningSummaryPartAddedEvent,
    ReasoningSummaryPartAddedEventTag,
    ReasoningSummaryPartAdded,
    "response.reasoning_summary_part.added",
    {
        item_id: String,
        output_index: u64,
        summary_index: u64,
        part: SummaryTextContent
    }
);
required_stream_event!(
    ReasoningSummaryPartDoneEvent,
    ReasoningSummaryPartDoneEventTag,
    ReasoningSummaryPartDone,
    "response.reasoning_summary_part.done",
    {
        item_id: String,
        output_index: u64,
        summary_index: u64,
        part: SummaryTextContent
    },
    {
        status: ReasoningSummaryPartStatus
    }
);
required_stream_event!(
    ReasoningSummaryTextDeltaEvent,
    ReasoningSummaryTextDeltaEventTag,
    ReasoningSummaryTextDelta,
    "response.reasoning_summary_text.delta",
    {
        item_id: String,
        output_index: u64,
        summary_index: u64,
        delta: String
    }
);
required_stream_event!(
    ReasoningSummaryTextDoneEvent,
    ReasoningSummaryTextDoneEventTag,
    ReasoningSummaryTextDone,
    "response.reasoning_summary_text.done",
    {
        item_id: String,
        output_index: u64,
        summary_index: u64,
        text: String
    }
);
required_stream_event!(
    ReasoningTextDeltaEvent,
    ReasoningTextDeltaEventTag,
    ReasoningTextDelta,
    "response.reasoning_text.delta",
    {
        item_id: String,
        output_index: u64,
        content_index: u64,
        delta: String
    }
);
required_stream_event!(
    ReasoningTextDoneEvent,
    ReasoningTextDoneEventTag,
    ReasoningTextDone,
    "response.reasoning_text.done",
    {
        item_id: String,
        output_index: u64,
        content_index: u64,
        text: String
    }
);

literal_tag!(
    WebSearchCompletedEventTag,
    WebSearchCompleted,
    "response.web_search_call.completed"
);
literal_tag!(
    WebSearchInProgressEventTag,
    WebSearchInProgress,
    "response.web_search_call.in_progress"
);
literal_tag!(
    WebSearchSearchingEventTag,
    WebSearchSearching,
    "response.web_search_call.searching"
);
tool_status_event!(
    WebSearchCompletedEvent,
    WebSearchCompletedEventTag,
    WebSearchCompleted
);
tool_status_event!(
    WebSearchInProgressEvent,
    WebSearchInProgressEventTag,
    WebSearchInProgress
);
tool_status_event!(
    WebSearchSearchingEvent,
    WebSearchSearchingEventTag,
    WebSearchSearching
);

literal_tag!(
    ImageGenerationCompletedEventTag,
    ImageGenerationCompleted,
    "response.image_generation_call.completed"
);
literal_tag!(
    ImageGenerationGeneratingEventTag,
    ImageGenerationGenerating,
    "response.image_generation_call.generating"
);
literal_tag!(
    ImageGenerationInProgressEventTag,
    ImageGenerationInProgress,
    "response.image_generation_call.in_progress"
);
tool_status_event!(
    ImageGenerationCompletedEvent,
    ImageGenerationCompletedEventTag,
    ImageGenerationCompleted
);
tool_status_event!(
    ImageGenerationGeneratingEvent,
    ImageGenerationGeneratingEventTag,
    ImageGenerationGenerating
);
tool_status_event!(
    ImageGenerationInProgressEvent,
    ImageGenerationInProgressEventTag,
    ImageGenerationInProgress
);
required_stream_event!(
    ImageGenerationPartialImageEvent,
    ImageGenerationPartialImageEventTag,
    ImageGenerationPartialImage,
    "response.image_generation_call.partial_image",
    {
        output_index: u64,
        item_id: String,
        partial_image_index: u64,
        partial_image_b64: String
    },
    {
        size: String,
        quality: ImageGenerationQuality,
        background: ImageGenerationBackground,
        output_format: ImageGenerationOutputFormat
    }
);
required_stream_event!(
    OutputTextAnnotationAddedEvent,
    OutputTextAnnotationAddedEventTag,
    OutputTextAnnotationAdded,
    "response.output_text.annotation.added",
    {
        item_id: String,
        output_index: u64,
        content_index: u64,
        annotation_index: u64,
        annotation: Nullable<Annotation>
    }
);
required_stream_event!(
    CustomToolCallInputDeltaEvent,
    CustomToolCallInputDeltaEventTag,
    CustomToolCallInputDelta,
    "response.custom_tool_call_input.delta",
    {
        output_index: u64,
        item_id: String,
        delta: String
    }
);
required_stream_event!(
    CustomToolCallInputDoneEvent,
    CustomToolCallInputDoneEventTag,
    CustomToolCallInputDone,
    "response.custom_tool_call_input.done",
    {
        output_index: u64,
        item_id: String,
        input: String
    }
);

/// Frozen schema-name inventory for all 58 stable Responses SSE branches.
pub const STABLE_RESPONSE_STREAM_EVENT_SCHEMAS: [&str; 58] = [
    "ResponseAudioDeltaEvent",
    "ResponseAudioDoneEvent",
    "ResponseAudioTranscriptDeltaEvent",
    "ResponseAudioTranscriptDoneEvent",
    "ResponseCodeInterpreterCallCodeDeltaEvent",
    "ResponseCodeInterpreterCallCodeDoneEvent",
    "ResponseCodeInterpreterCallCompletedEvent",
    "ResponseCodeInterpreterCallInProgressEvent",
    "ResponseCodeInterpreterCallInterpretingEvent",
    "ResponseCompletedEvent",
    "ResponseContentPartAddedEvent",
    "ResponseContentPartDoneEvent",
    "ResponseCreatedEvent",
    "ResponseErrorEvent",
    "ResponseFileSearchCallCompletedEvent",
    "ResponseFileSearchCallInProgressEvent",
    "ResponseFileSearchCallSearchingEvent",
    "ResponseFunctionCallArgumentsDeltaEvent",
    "ResponseFunctionCallArgumentsDoneEvent",
    "ResponseShellCallCommandAddedStreamingEvent",
    "ResponseShellCallCommandDeltaStreamingEvent",
    "ResponseShellCallCommandDoneStreamingEvent",
    "ResponseShellCallOutputContentDeltaStreamingEvent",
    "ResponseShellCallOutputContentDoneStreamingEvent",
    "ResponseInProgressEvent",
    "ResponseFailedEvent",
    "ResponseIncompleteEvent",
    "ResponseOutputItemAddedEvent",
    "ResponseOutputItemDoneEvent",
    "ResponseReasoningSummaryPartAddedEvent",
    "ResponseReasoningSummaryPartDoneEvent",
    "ResponseReasoningSummaryTextDeltaEvent",
    "ResponseReasoningSummaryTextDoneEvent",
    "ResponseReasoningTextDeltaEvent",
    "ResponseReasoningTextDoneEvent",
    "ResponseRefusalDeltaEvent",
    "ResponseRefusalDoneEvent",
    "ResponseTextDeltaEvent",
    "ResponseTextDoneEvent",
    "ResponseWebSearchCallCompletedEvent",
    "ResponseWebSearchCallInProgressEvent",
    "ResponseWebSearchCallSearchingEvent",
    "ResponseImageGenCallCompletedEvent",
    "ResponseImageGenCallGeneratingEvent",
    "ResponseImageGenCallInProgressEvent",
    "ResponseImageGenCallPartialImageEvent",
    "ResponseMCPCallArgumentsDeltaEvent",
    "ResponseMCPCallArgumentsDoneEvent",
    "ResponseMCPCallCompletedEvent",
    "ResponseMCPCallFailedEvent",
    "ResponseMCPCallInProgressEvent",
    "ResponseMCPListToolsCompletedEvent",
    "ResponseMCPListToolsFailedEvent",
    "ResponseMCPListToolsInProgressEvent",
    "ResponseOutputTextAnnotationAddedEvent",
    "ResponseQueuedEvent",
    "ResponseCustomToolCallInputDeltaEvent",
    "ResponseCustomToolCallInputDoneEvent",
];

/// Event discriminators aligned with [`STABLE_RESPONSE_STREAM_EVENT_SCHEMAS`].
pub const STABLE_RESPONSE_STREAM_EVENT_DISCRIMINATORS: [&str; 58] = [
    "response.audio.delta",
    "response.audio.done",
    "response.audio.transcript.delta",
    "response.audio.transcript.done",
    "response.code_interpreter_call_code.delta",
    "response.code_interpreter_call_code.done",
    "response.code_interpreter_call.completed",
    "response.code_interpreter_call.in_progress",
    "response.code_interpreter_call.interpreting",
    "response.completed",
    "response.content_part.added",
    "response.content_part.done",
    "response.created",
    "error",
    "response.file_search_call.completed",
    "response.file_search_call.in_progress",
    "response.file_search_call.searching",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "response.shell_call_command.added",
    "response.shell_call_command.delta",
    "response.shell_call_command.done",
    "response.shell_call_output_content.delta",
    "response.shell_call_output_content.done",
    "response.in_progress",
    "response.failed",
    "response.incomplete",
    "response.output_item.added",
    "response.output_item.done",
    "response.reasoning_summary_part.added",
    "response.reasoning_summary_part.done",
    "response.reasoning_summary_text.delta",
    "response.reasoning_summary_text.done",
    "response.reasoning_text.delta",
    "response.reasoning_text.done",
    "response.refusal.delta",
    "response.refusal.done",
    "response.output_text.delta",
    "response.output_text.done",
    "response.web_search_call.completed",
    "response.web_search_call.in_progress",
    "response.web_search_call.searching",
    "response.image_generation_call.completed",
    "response.image_generation_call.generating",
    "response.image_generation_call.in_progress",
    "response.image_generation_call.partial_image",
    "response.mcp_call_arguments.delta",
    "response.mcp_call_arguments.done",
    "response.mcp_call.completed",
    "response.mcp_call.failed",
    "response.mcp_call.in_progress",
    "response.mcp_list_tools.completed",
    "response.mcp_list_tools.failed",
    "response.mcp_list_tools.in_progress",
    "response.output_text.annotation.added",
    "response.queued",
    "response.custom_tool_call_input.delta",
    "response.custom_tool_call_input.done",
];

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};

    use super::*;

    // Frozen synthetic MCP-approval outputs (D0008/OVR-0007). Wiring them
    // through include_str keeps the ghost-`request_id` evidence live instead
    // of leaving dead files under testdata/.
    const MCP_APPROVAL_OUTPUT_FIXTURE: &str =
        include_str!("../../../testdata/fixtures/responses/mcp-approval/output.json");
    const MCP_APPROVAL_OUTPUT_WITHOUT_REQUEST_ID_FIXTURE: &str = include_str!(
        "../../../testdata/fixtures/responses/mcp-approval/output-without-request-id.json"
    );

    fn assert_json_dto<T>()
    where
        T: Serialize + DeserializeOwned + Send + Sync,
    {
    }

    #[test]
    fn every_public_wire_dto_is_owned_and_bidirectional() {
        assert_json_dto::<UnknownTaggedObject>();
        assert_json_dto::<ResponseStatus>();
        assert_json_dto::<ResponseItemStatus>();
        assert_json_dto::<MessageStatus>();
        assert_json_dto::<FunctionCallItemStatus>();
        assert_json_dto::<McpToolCallStatus>();
        assert_json_dto::<WebSearchToolCallStatus>();
        assert_json_dto::<FileSearchToolCallStatus>();
        assert_json_dto::<ImageGenToolCallStatus>();
        assert_json_dto::<CodeInterpreterToolCallStatus>();
        assert_json_dto::<CompactServiceTier>();
        assert_json_dto::<ProgramOutputStatus>();
        assert_json_dto::<ApplyPatchCallStatus>();
        assert_json_dto::<ApplyPatchCallOutputStatus>();
        assert_json_dto::<MessageRole>();
        assert_json_dto::<MessagePhase>();
        assert_json_dto::<ImageDetail>();
        assert_json_dto::<FileDetail>();
        assert_json_dto::<TruncationStrategy>();
        assert_json_dto::<ReasoningEffort>();
        assert_json_dto::<ReasoningSummary>();
        assert_json_dto::<IncompleteReason>();
        assert_json_dto::<ReasoningSummaryPartStatus>();
        assert_json_dto::<InputText>();
        assert_json_dto::<InputImage>();
        assert_json_dto::<InputImageParam>();
        assert_json_dto::<InputFile>();
        assert_json_dto::<InputContent>();
        assert_json_dto::<EasyInputContent>();
        assert_json_dto::<MessageContent>();
        assert_json_dto::<InputMessage>();
        assert_json_dto::<EasyInputMessageRole>();
        assert_json_dto::<StoredInputMessageRole>();
        assert_json_dto::<StoredInputMessage>();
        assert_json_dto::<ResponseInput>();
        assert_json_dto::<ResponseInstructions>();
        assert_json_dto::<FunctionTool>();
        assert_json_dto::<McpToolFilter>();
        assert_json_dto::<McpAllowedTools>();
        assert_json_dto::<McpApprovalFilter>();
        assert_json_dto::<McpRequireApproval>();
        assert_json_dto::<McpTool>();
        assert_json_dto::<ResponseTool>();
        assert_json_dto::<NamespaceToolEntry>();
        assert_json_dto::<FunctionCall>();
        assert_json_dto::<FunctionCallOutput>();
        assert_json_dto::<DirectToolCallCaller>();
        assert_json_dto::<ProgramToolCallCaller>();
        assert_json_dto::<ToolCallCaller>();
        assert_json_dto::<McpListedTool>();
        assert_json_dto::<McpListTools>();
        assert_json_dto::<McpCall>();
        assert_json_dto::<McpCallError>();
        assert_json_dto::<McpProtocolError>();
        assert_json_dto::<McpToolExecutionError>();
        assert_json_dto::<McpHttpError>();
        assert_json_dto::<McpApprovalRequest>();
        assert_json_dto::<McpApprovalResponse>();
        assert_json_dto::<FunctionCallOutputValue>();
        assert_json_dto::<FunctionCallOutputContent>();
        assert_json_dto::<FunctionCallOutputParamValue>();
        assert_json_dto::<OutputText>();
        assert_json_dto::<Refusal>();
        assert_json_dto::<OutputContent>();
        assert_json_dto::<OutputMessage>();
        assert_json_dto::<ResponseInputItem>();
        assert_json_dto::<ResponseOutputItem>();
        assert_json_dto::<FunctionToolChoice>();
        assert_json_dto::<McpToolChoice>();
        assert_json_dto::<ToolChoice>();
        assert_json_dto::<TextFormatText>();
        assert_json_dto::<TextFormatJsonObject>();
        assert_json_dto::<TextFormatJsonSchema>();
        assert_json_dto::<TextFormat>();
        assert_json_dto::<ResponseTextConfig>();
        assert_json_dto::<ResponseIncludable>();
        assert_json_dto::<ResponseItemOrder>();
        assert_json_dto::<CodeInterpreterDomainSecret>();
        assert_json_dto::<PromptCacheRetention>();
        assert_json_dto::<ServiceTier>();
        assert_json_dto::<ResponseTextVerbosity>();
        assert_json_dto::<PromptCacheMode>();
        assert_json_dto::<PromptCacheTtl>();
        assert_json_dto::<PromptCacheBreakpoint>();
        assert_json_dto::<PromptCacheOptionsParam>();
        assert_json_dto::<PromptCacheOptions>();
        assert_json_dto::<ReasoningEffort>();
        assert_json_dto::<ReasoningContext>();
        assert_json_dto::<ReasoningMode>();
        assert_json_dto::<ReasoningConfig>();
        assert_json_dto::<ContextManagement>();
        assert_json_dto::<ModerationConfig>();
        assert_json_dto::<Annotation>();
        assert_json_dto::<LogProb>();
        assert_json_dto::<TopLogProb>();
        assert_json_dto::<ConversationObjectReference>();
        assert_json_dto::<ConversationReference>();
        assert_json_dto::<PromptReference>();
        assert_json_dto::<ResponseStreamOptions>();
        assert_json_dto::<CreateResponseRequest>();
        assert_json_dto::<CreateStreamingResponseRequest>();
        assert_json_dto::<ResponsesCreateEvent>();
        assert_json_dto::<ResponsesClientEvent>();
        assert_json_dto::<ResponsesServerEvent>();
        assert_json_dto::<ResponseError>();
        assert_json_dto::<IncompleteDetails>();
        assert_json_dto::<InputTokensDetails>();
        assert_json_dto::<OutputTokensDetails>();
        assert_json_dto::<ResponseUsage>();
        assert_json_dto::<Response>();
        assert_json_dto::<DeletedResponse>();
        assert_json_dto::<CompactResponseRequest>();
        assert_json_dto::<CompactedResponse>();
        assert_json_dto::<ListResponseInputItemsParams>();
        assert_json_dto::<ResponseInputItemList>();
        assert_json_dto::<CountInputTokensRequest>();
        assert_json_dto::<InputTokenCountResponse>();
        assert_json_dto::<ResponseQueuedEvent>();
        assert_json_dto::<ResponseCreatedEvent>();
        assert_json_dto::<ResponseInProgressEvent>();
        assert_json_dto::<ResponseCompletedEvent>();
        assert_json_dto::<ResponseFailedEvent>();
        assert_json_dto::<ResponseIncompleteEvent>();
        assert_json_dto::<OutputItemAddedEvent>();
        assert_json_dto::<OutputItemDoneEvent>();
        assert_json_dto::<ContentPartAddedEvent>();
        assert_json_dto::<ContentPartDoneEvent>();
        assert_json_dto::<OutputTextDeltaEvent>();
        assert_json_dto::<OutputTextDoneEvent>();
        assert_json_dto::<RefusalDeltaEvent>();
        assert_json_dto::<RefusalDoneEvent>();
        assert_json_dto::<FunctionCallArgumentsDeltaEvent>();
        assert_json_dto::<FunctionCallArgumentsDoneEvent>();
        assert_json_dto::<McpCallArgumentsDeltaEvent>();
        assert_json_dto::<McpCallArgumentsDoneEvent>();
        assert_json_dto::<McpCallInProgressEvent>();
        assert_json_dto::<McpCallCompletedEvent>();
        assert_json_dto::<McpCallFailedEvent>();
        assert_json_dto::<McpListToolsInProgressEvent>();
        assert_json_dto::<McpListToolsCompletedEvent>();
        assert_json_dto::<McpListToolsFailedEvent>();
        assert_json_dto::<StreamErrorEvent>();
        assert_json_dto::<CompactionTrigger>();
        assert_json_dto::<ItemReference>();
        assert_json_dto::<ProgramItem>();
        assert_json_dto::<ProgramOutputItem>();
        assert_json_dto::<FileSearchCall>();
        assert_json_dto::<FileSearchResult>();
        assert_json_dto::<FileSearchAttributeValue>();
        assert_json_dto::<ComputerClickButton>();
        assert_json_dto::<ComputerCoordinate>();
        assert_json_dto::<ComputerClickAction>();
        assert_json_dto::<ComputerAction>();
        assert_json_dto::<ComputerSafetyCheck>();
        assert_json_dto::<ComputerScreenshot>();
        assert_json_dto::<ComputerCall>();
        assert_json_dto::<ComputerCallOutput>();
        assert_json_dto::<ComputerCallOutputResource>();
        assert_json_dto::<WebSearchSource>();
        assert_json_dto::<WebSearchSearchAction>();
        assert_json_dto::<WebSearchAction>();
        assert_json_dto::<WebSearchCall>();
        assert_json_dto::<FunctionCallOutputResource>();
        assert_json_dto::<ToolSearchCallInput>();
        assert_json_dto::<ToolSearchCall>();
        assert_json_dto::<ToolSearchOutputInput>();
        assert_json_dto::<ToolSearchOutput>();
        assert_json_dto::<AdditionalToolsInput>();
        assert_json_dto::<AdditionalTools>();
        assert_json_dto::<ReasoningItem>();
        assert_json_dto::<SummaryTextContent>();
        assert_json_dto::<ReasoningTextContent>();
        assert_json_dto::<CompactionSummaryInput>();
        assert_json_dto::<CompactionItem>();
        assert_json_dto::<ImageGenerationCall>();
        assert_json_dto::<CodeInterpreterCall>();
        assert_json_dto::<CodeInterpreterLogs>();
        assert_json_dto::<CodeInterpreterImage>();
        assert_json_dto::<CodeInterpreterOutput>();
        assert_json_dto::<LocalShellExecAction>();
        assert_json_dto::<LocalShellAction>();
        assert_json_dto::<LocalShellCall>();
        assert_json_dto::<LocalShellCallOutput>();
        assert_json_dto::<FunctionShellCallInput>();
        assert_json_dto::<FunctionShellActionParam>();
        assert_json_dto::<FunctionShellAction>();
        assert_json_dto::<FunctionShellCall>();
        assert_json_dto::<FunctionShellCallOutputInput>();
        assert_json_dto::<FunctionShellCallOutput>();
        assert_json_dto::<FunctionShellCallOutputContent>();
        assert_json_dto::<FunctionShellOutcome>();
        assert_json_dto::<FunctionShellTimeoutOutcome>();
        assert_json_dto::<FunctionShellExitOutcome>();
        assert_json_dto::<ApplyPatchCreateFile>();
        assert_json_dto::<ApplyPatchDeleteFile>();
        assert_json_dto::<ApplyPatchUpdateFile>();
        assert_json_dto::<ApplyPatchOperation>();
        assert_json_dto::<ApplyPatchCallInput>();
        assert_json_dto::<ApplyPatchCall>();
        assert_json_dto::<ApplyPatchCallOutputInput>();
        assert_json_dto::<ApplyPatchCallOutput>();
        assert_json_dto::<McpApprovalResponseResource>();
        assert_json_dto::<CustomToolCall>();
        assert_json_dto::<CustomToolCallOutput>();
        assert_json_dto::<CustomToolCallOutputResource>();
        assert_json_dto::<FileSearchRanker>();
        assert_json_dto::<FileSearchHybridSearch>();
        assert_json_dto::<FileSearchRankingOptions>();
        assert_json_dto::<FileSearchTool>();
        assert_json_dto::<WebSearchContextSize>();
        assert_json_dto::<WebSearchContentType>();
        assert_json_dto::<WebSearchUserLocation>();
        assert_json_dto::<WebSearchFilters>();
        assert_json_dto::<ImageGenerationQuality>();
        assert_json_dto::<ImageGenerationOutputFormat>();
        assert_json_dto::<ImageGenerationModeration>();
        assert_json_dto::<ImageGenerationBackground>();
        assert_json_dto::<ImageGenerationInputFidelity>();
        assert_json_dto::<ImageGenerationAction>();
        assert_json_dto::<ImageGenerationInputMask>();
        assert_json_dto::<FunctionShellEnvironment>();
        assert_json_dto::<ContainerSkill>();
        assert_json_dto::<ContainerSkillReference>();
        assert_json_dto::<InlineSkill>();
        assert_json_dto::<LocalSkill>();
        assert_json_dto::<ToolSearchExecution>();
        assert_json_dto::<CustomToolFormat>();
        assert_json_dto::<CustomToolGrammarSyntax>();
        assert_json_dto::<ComputerTool>();
        assert_json_dto::<ComputerUsePreviewTool>();
        assert_json_dto::<WebSearchTool>();
        assert_json_dto::<CodeInterpreterTool>();
        assert_json_dto::<CodeInterpreterContainer>();
        assert_json_dto::<AutoCodeInterpreterContainer>();
        assert_json_dto::<CodeInterpreterMemoryLimit>();
        assert_json_dto::<CodeInterpreterNetworkPolicy>();
        assert_json_dto::<ProgrammaticTool>();
        assert_json_dto::<ImageGenerationTool>();
        assert_json_dto::<LocalShellTool>();
        assert_json_dto::<FunctionShellTool>();
        assert_json_dto::<CustomTool>();
        assert_json_dto::<NamespaceTool>();
        assert_json_dto::<ToolSearchTool>();
        assert_json_dto::<WebSearchPreviewTool>();
        assert_json_dto::<ApplyPatchTool>();
        assert_json_dto::<AllowedToolsMode>();
        assert_json_dto::<AllowedToolsChoice>();
        assert_json_dto::<HostedToolType>();
        assert_json_dto::<HostedToolChoice>();
        assert_json_dto::<CustomToolChoice>();
        assert_json_dto::<ProgrammaticToolChoice>();
        assert_json_dto::<ApplyPatchToolChoice>();
        assert_json_dto::<FunctionShellToolChoice>();
        assert_json_dto::<AudioDeltaEvent>();
        assert_json_dto::<AudioDoneEvent>();
        assert_json_dto::<AudioTranscriptDeltaEvent>();
        assert_json_dto::<AudioTranscriptDoneEvent>();
        assert_json_dto::<CodeInterpreterCodeDeltaEvent>();
        assert_json_dto::<CodeInterpreterCodeDoneEvent>();
        assert_json_dto::<CodeInterpreterCompletedEvent>();
        assert_json_dto::<CodeInterpreterInProgressEvent>();
        assert_json_dto::<CodeInterpreterInterpretingEvent>();
        assert_json_dto::<FileSearchCompletedEvent>();
        assert_json_dto::<FileSearchInProgressEvent>();
        assert_json_dto::<FileSearchSearchingEvent>();
        assert_json_dto::<ShellCommandAddedEvent>();
        assert_json_dto::<ShellCommandDeltaEvent>();
        assert_json_dto::<ShellCommandDoneEvent>();
        assert_json_dto::<ShellOutputContentDeltaEvent>();
        assert_json_dto::<ShellOutputContentDoneEvent>();
        assert_json_dto::<ReasoningSummaryPartAddedEvent>();
        assert_json_dto::<ReasoningSummaryPartDoneEvent>();
        assert_json_dto::<ReasoningSummaryTextDeltaEvent>();
        assert_json_dto::<ReasoningSummaryTextDoneEvent>();
        assert_json_dto::<ReasoningTextDeltaEvent>();
        assert_json_dto::<ReasoningTextDoneEvent>();
        assert_json_dto::<WebSearchCompletedEvent>();
        assert_json_dto::<WebSearchInProgressEvent>();
        assert_json_dto::<WebSearchSearchingEvent>();
        assert_json_dto::<ImageGenerationCompletedEvent>();
        assert_json_dto::<ImageGenerationGeneratingEvent>();
        assert_json_dto::<ImageGenerationInProgressEvent>();
        assert_json_dto::<ImageGenerationPartialImageEvent>();
        assert_json_dto::<OutputTextAnnotationAddedEvent>();
        assert_json_dto::<CustomToolCallInputDeltaEvent>();
        assert_json_dto::<CustomToolCallInputDoneEvent>();
        assert_json_dto::<ResponseStreamEvent>();
    }

    #[test]
    fn frozen_union_manifests_route_every_known_discriminator_strictly() {
        assert_eq!(STABLE_RESPONSE_INPUT_SCHEMAS.len(), 32);
        assert_eq!(STABLE_RESPONSE_INPUT_DISCRIMINATORS.len(), 32);
        assert_eq!(STABLE_RESPONSE_OUTPUT_SCHEMAS.len(), 28);
        assert_eq!(STABLE_RESPONSE_OUTPUT_DISCRIMINATORS.len(), 28);
        assert_eq!(STABLE_RESPONSE_TOOL_SCHEMAS.len(), 16);
        assert_eq!(STABLE_RESPONSE_TOOL_DISCRIMINATORS.len(), 16);
        assert_eq!(STABLE_RESPONSE_TOOL_CHOICE_SCHEMAS.len(), 9);
        assert_eq!(STABLE_RESPONSE_TOOL_CHOICE_DISCRIMINATORS.len(), 9);
        assert_eq!(STABLE_RESPONSE_STREAM_EVENT_SCHEMAS.len(), 58);
        assert_eq!(STABLE_RESPONSE_STREAM_EVENT_DISCRIMINATORS.len(), 58);

        for discriminator in STABLE_RESPONSE_INPUT_DISCRIMINATORS {
            if matches!(discriminator, "compaction_trigger" | "<absent:id>") {
                continue;
            }
            let decoded = serde_json::from_value::<ResponseInputItem>(json!({
                "type": discriminator
            }));
            assert!(
                decoded.is_err(),
                "known input tag {discriminator} must validate its required payload"
            );
        }

        let item_reference: ResponseInputItem = serde_json::from_value(json!({"id": "item_1"}))
            .expect("decode untagged item reference");
        assert!(matches!(
            item_reference,
            ResponseInputItem::ItemReference(_)
        ));
        let trigger: ResponseInputItem =
            serde_json::from_value(json!({"type": "compaction_trigger"}))
                .expect("decode tag-only compaction trigger");
        assert!(matches!(trigger, ResponseInputItem::CompactionTrigger(_)));

        for discriminator in STABLE_RESPONSE_OUTPUT_DISCRIMINATORS {
            let decoded = serde_json::from_value::<ResponseOutputItem>(json!({
                "type": discriminator
            }));
            assert!(
                decoded.is_err(),
                "known output tag {discriminator} must validate its required payload"
            );
        }

        for discriminator in STABLE_RESPONSE_STREAM_EVENT_DISCRIMINATORS {
            let decoded = serde_json::from_value::<ResponseStreamEvent>(json!({
                "type": discriminator
            }));
            assert!(
                decoded.is_err(),
                "known event tag {discriminator} must validate its required payload"
            );
        }

        for discriminator in [
            "function",
            "file_search",
            "computer_use_preview",
            "mcp",
            "code_interpreter",
            "custom",
            "namespace",
        ] {
            let decoded = serde_json::from_value::<ResponseTool>(json!({"type": discriminator}));
            assert!(
                decoded.is_err(),
                "known tool tag {discriminator} must validate its required payload"
            );
        }

        for discriminator in [
            "computer",
            "web_search",
            "web_search_2025_08_26",
            "programmatic_tool_calling",
            "image_generation",
            "local_shell",
            "shell",
            "tool_search",
            "web_search_preview",
            "web_search_preview_2025_03_11",
            "apply_patch",
        ] {
            let decoded: ResponseTool = serde_json::from_value(json!({"type": discriminator}))
                .expect("decode tag-only known tool");
            assert!(!matches!(decoded, ResponseTool::Unknown(_)));
        }

        for discriminator in ["allowed_tools", "function", "mcp", "custom"] {
            assert!(
                serde_json::from_value::<ToolChoice>(json!({"type": discriminator})).is_err(),
                "known choice tag {discriminator} must validate its required payload"
            );
        }
        for discriminator in [
            "file_search",
            "programmatic_tool_calling",
            "apply_patch",
            "shell",
        ] {
            let decoded: ToolChoice = serde_json::from_value(json!({"type": discriminator}))
                .expect("decode tag-only known tool choice");
            assert!(!matches!(decoded, ToolChoice::Unknown(_)));
        }
        for mode in ["none", "auto", "required"] {
            serde_json::from_value::<ToolChoice>(json!(mode)).expect("decode string tool choice");
        }
    }

    #[test]
    fn request_builders_emit_typed_multimodal_and_tool_json() {
        let schema = json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": false
        });
        let tool = FunctionTool::new("weather")
            .description("Get weather")
            .parameters_from(&schema)
            .expect("serialize schema")
            .strict(true);
        let input = InputMessage::user(MessageContent::Parts(vec![
            InputText::new("What is shown?").into(),
            InputImage::from_url("https://example.test/image.png")
                .detail(ImageDetail::High)
                .into(),
            InputFile::from_file_id("file_123").into(),
        ]));

        let request =
            CreateResponseRequest::new("gpt-test", vec![ResponseInputItem::Message(input)])
                .tool(tool)
                .tool_choice(ToolChoice::Auto)
                .metadata("trace", "typed")
                .into_streaming()
                .stream_options(ResponseStreamOptions::default().include_obfuscation(false));

        let value = serde_json::to_value(&request).expect("serialize request");
        assert_eq!(value["model"], "gpt-test");
        assert_eq!(value["stream"], true);
        assert_eq!(value["input"][0]["role"], "user");
        assert!(value["input"][0].get("type").is_none());
        assert_eq!(value["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(value["tools"][0]["parameters"], schema);
        assert_eq!(value["tools"][0]["strict"], true);

        let decoded: CreateStreamingResponseRequest =
            serde_json::from_value(value).expect("deserialize request");
        assert_eq!(decoded.model_ref(), Some("gpt-test"));
        assert_eq!(decoded.tools_ref().len(), 1);

        let null_options = CreateStreamingResponseRequest::empty().stream_options_null();
        assert_eq!(
            serde_json::to_value(&null_options).expect("serialize")["stream_options"],
            Value::Null
        );
        let decoded_null = serde_json::from_value::<CreateStreamingResponseRequest>(json!({
            "model": "gpt-test",
            "stream": true,
            "stream_options": null
        }))
        .expect("official stream_options anyOf includes null");
        assert_eq!(
            serde_json::to_value(decoded_null).expect("re-encode")["stream_options"],
            Value::Null
        );
    }

    #[test]
    fn stream_typestate_rejects_conflicting_wire_flag() {
        let non_streaming = serde_json::from_value::<CreateResponseRequest>(json!({
            "model": "gpt-test",
            "stream": true
        }));
        assert!(non_streaming.is_err());

        let streaming = serde_json::from_value::<CreateStreamingResponseRequest>(json!({
            "model": "gpt-test",
            "stream": false
        }));
        assert!(streaming.is_err());

        let empty = serde_json::from_value::<CreateResponseRequest>(json!({}))
            .expect("all create properties are optional in the frozen schema");
        assert!(empty.model_ref().is_none());
        assert!(empty.input_ref().is_none());
    }

    #[test]
    fn create_and_token_count_requests_preserve_omitted_null_and_empty() {
        let create_fixture = json!({
            "background": null,
            "include": [],
            "metadata": {},
            "parallel_tool_calls": null,
            "store": null,
            "tools": [],
            "truncation": null
        });
        let create: CreateResponseRequest =
            serde_json::from_value(create_fixture.clone()).expect("decode tri-state create body");
        assert_eq!(
            serde_json::to_value(create).expect("encode tri-state create body"),
            create_fixture
        );

        let count_fixture = json!({
            "model": null,
            "input": null,
            "parallel_tool_calls": null,
            "text": null,
            "tool_choice": null,
            "tools": []
        });
        let count: CountInputTokensRequest = serde_json::from_value(count_fixture.clone())
            .expect("decode tri-state token-count body");
        assert_eq!(
            serde_json::to_value(count).expect("encode tri-state token-count body"),
            count_fixture
        );
    }

    #[test]
    fn websocket_events_reuse_create_and_stream_codecs_losslessly() {
        assert!(std::mem::size_of::<ResponsesClientEvent>() <= 128);

        let client = ResponsesClientEvent::create_on_stream(
            "agent.1",
            CreateResponseRequest::new("gpt-test", "hello")
                .tool(FunctionTool::new("lookup").strict(true)),
        );
        if let ResponsesClientEvent::Create(event) = &client {
            event
                .validate()
                .expect("official stream_id agent.1 is in range");
        }
        let value = serde_json::to_value(&client).expect("encode WS client event");
        assert_eq!(value["type"], "response.create");
        assert_eq!(value["stream_id"], "agent.1");
        assert_eq!(value["model"], "gpt-test");
        assert_eq!(value["input"], "hello");
        assert!(value.get("stream").is_none());
        let decoded: ResponsesClientEvent =
            serde_json::from_value(value.clone()).expect("decode WS client event");
        assert_eq!(
            serde_json::to_value(decoded).expect("round-trip WS client event"),
            value
        );

        let server_fixture = json!({
            "type": "response.output_text.delta",
            "stream_id": "agent.1",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hi",
            "sequence_number": 1,
            "logprobs": []
        });
        let server: ResponsesServerEvent =
            serde_json::from_value(server_fixture.clone()).expect("decode WS server event");
        assert_eq!(server.sequence_number(), Some(1));
        assert_eq!(server.stream_id(), Some("agent.1"));
        assert!(!server.is_terminal());
        assert_eq!(
            serde_json::to_value(server).expect("round-trip WS server event"),
            server_fixture
        );

        let future_fixture = json!({
            "type": "response.future_ws_event",
            "stream_id": "agent.1",
            "sequence_number": 2,
            "payload": true
        });
        let future: ResponsesServerEvent =
            serde_json::from_value(future_fixture.clone()).expect("decode future WS server event");
        assert!(matches!(
            future.event(),
            Some(ResponseStreamEvent::Unknown(_))
        ));
        assert_eq!(
            serde_json::to_value(future).expect("round-trip future WS event"),
            future_fixture
        );

        let ws_error_fixture = json!({
            "type": "error",
            "status": 400,
            "sequence_number": 3,
            "stream_id": "agent.1",
            "error": {
                "type": "invalid_request_error",
                "code": null,
                "message": "bad request",
                "param": null,
                "headers": {"x-request-id": "req_1"}
            }
        });
        let ws_error: ResponsesServerEvent = serde_json::from_value(ws_error_fixture.clone())
            .expect("decode official ResponseWsError");
        assert!(matches!(ws_error, ResponsesServerEvent::WebSocketError(_)));
        assert_eq!(ws_error.stream_id(), Some("agent.1"));
        assert_eq!(ws_error.sequence_number(), Some(3));
        let ResponsesServerEvent::WebSocketError(ws_error) = &ws_error else {
            panic!("expected websocket error");
        };
        assert_eq!(ws_error.status(), Some(400));
        assert_eq!(ws_error.error().code(), None);
        assert_eq!(ws_error.error().message(), "bad request");
        assert_eq!(
            ws_error
                .error()
                .headers()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_1".to_owned())
        );
        assert_eq!(
            serde_json::to_value(ws_error).expect("round-trip official ResponseWsError"),
            ws_error_fixture
        );
    }

    #[test]
    fn input_message_accepts_compact_and_explicit_discriminator_forms() {
        let compact: ResponseInputItem = serde_json::from_value(json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]
        }))
        .expect("decode compact input message");
        assert!(matches!(compact, ResponseInputItem::Message(_)));

        let explicit: ResponseInputItem = serde_json::from_value(json!({
            "type": "message",
            "role": "developer",
            "content": "be concise"
        }))
        .expect("decode explicitly tagged input message");
        assert!(matches!(explicit, ResponseInputItem::Message(_)));
    }

    #[test]
    fn official_response_item_list_user_message_resource_decodes() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "id": "msg_abc123",
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "Tell me a three sentence bedtime story about a unicorn."
                }]
            }],
            "first_id": "msg_abc123",
            "last_id": "msg_abc123",
            "has_more": false
        });
        let decoded: ResponseInputItemList =
            serde_json::from_value(fixture.clone()).expect("official InputMessageResource list");
        assert!(
            matches!(decoded.data()[0], ResponseInputItem::StoredMessage(_)),
            "user message resources must not route to assistant OutputMessage"
        );
        let encoded = serde_json::to_value(&decoded).expect("re-encode official item list");
        assert_eq!(encoded["data"][0]["id"], "msg_abc123");
        assert_eq!(encoded["data"][0]["role"], "user");
        assert_eq!(decoded.first_id(), "msg_abc123");
        assert_eq!(decoded.last_id(), "msg_abc123");

        let assistant: ResponseInputItem = serde_json::from_value(json!({
            "type": "message",
            "id": "msg_1",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "Once upon a time.",
                "annotations": []
            }]
        }))
        .expect("assistant OutputMessage still decodes");
        assert!(matches!(assistant, ResponseInputItem::OutputMessage(_)));

        assert!(
            serde_json::from_value::<ResponseInputItem>(json!({
                "id": "msg_abc123",
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "hello"
                }],
                "status": null
            }))
            .is_err(),
            "unofficial stored-message status null still fails"
        );
    }

    #[test]
    fn official_message_role_names_all_pin_members() {
        const OFFICIAL_MESSAGE_ROLE: [&str; 8] = [
            "unknown",
            "user",
            "assistant",
            "system",
            "critic",
            "discriminator",
            "developer",
            "tool",
        ];
        for value in OFFICIAL_MESSAGE_ROLE {
            let decoded = MessageRole::from_raw(value);
            assert!(
                decoded.is_known(),
                "official MessageRole value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
        }
        assert_eq!(MessageRole::UnknownRole.as_str(), "unknown");
        assert_eq!(MessageRole::Critic.as_str(), "critic");
        assert_eq!(MessageRole::Discriminator.as_str(), "discriminator");
        assert_eq!(MessageRole::Tool.as_str(), "tool");

        let additional: AdditionalTools = serde_json::from_value(json!({
            "type": "additional_tools",
            "id": "addtl_critic",
            "role": "critic",
            "tools": []
        }))
        .expect("official AdditionalTools MessageRole critic");
        assert_eq!(additional.role, MessageRole::Critic);
        assert_eq!(
            serde_json::to_value(&additional).expect("re-encode additional tools")["role"],
            "critic"
        );

        let input = InputMessage::new(EasyInputMessageRole::User, "lookup result");
        assert_eq!(
            serde_json::to_value(&input).expect("serialize easy input role")["role"],
            "user"
        );
        assert_eq!(
            InputMessage::assistant("draft").role(),
            &MessageRole::Assistant
        );
        assert_eq!(InputMessage::system("policy").role(), &MessageRole::System);
    }

    #[test]
    fn unknown_tag_retains_discriminator_and_complete_object() {
        let original = json!({
            "type": "future_quantum_call",
            "id": "q_1",
            "nested": {"answer": 42},
            "items": [true, null, "x"]
        });
        let decoded: ResponseOutputItem =
            serde_json::from_value(original.clone()).expect("decode future output item");
        let ResponseOutputItem::Unknown(unknown) = &decoded else {
            panic!("future tag must produce Unknown");
        };
        assert_eq!(unknown.discriminator(), "future_quantum_call");
        assert_eq!(unknown.raw().get("nested"), Some(&json!({"answer": 42})));
        assert_eq!(
            serde_json::to_value(decoded).expect("re-encode future output item"),
            original
        );
    }

    #[test]
    fn known_tag_with_malformed_payload_is_not_downgraded_to_unknown() {
        let malformed_call = serde_json::from_value::<ResponseOutputItem>(json!({
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "weather",
            "status": "completed"
        }));
        assert!(malformed_call.is_err(), "missing arguments must fail");

        let malformed_text = serde_json::from_value::<OutputContent>(json!({
            "type": "output_text",
            "annotations": []
        }));
        assert!(malformed_text.is_err(), "missing text must fail");

        let malformed_event = serde_json::from_value::<ResponseStreamEvent>(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 4
        }));
        assert!(malformed_event.is_err(), "missing delta must fail");
    }

    fn sample_response_value() -> Value {
        json!({
            "id": "resp_1",
            "created_at": 1_700_000_000,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": {"trace": "one"},
            "model": "gpt-test",
            "object": "response",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Hello ",
                            "annotations": [],
                            "logprobs": [],
                            "future_part_field": 1
                        },
                        {
                            "type": "output_text",
                            "text": "world",
                            "annotations": [],
                            "logprobs": []
                        }
                    ],
                    "future_message_field": {"ok": true}
                },
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "weather",
                    "arguments": "{\"city\":\"Paris\"}",
                    "status": "completed"
                }
            ],
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0,
            "status": "completed",
            "usage": {
                "input_tokens": 10,
                "input_tokens_details": {"cached_tokens": 2, "cache_write_tokens": 0},
                "output_tokens": 5,
                "output_tokens_details": {"reasoning_tokens": 1},
                "total_tokens": 15,
                "future_usage_field": "kept"
            },
            "future_response_field": {"nested": [1, 2, 3]}
        })
    }

    #[test]
    fn response_retains_extra_fields_and_exposes_safe_helpers() {
        let original = sample_response_value();
        let response: Response =
            serde_json::from_value(original.clone()).expect("decode response fixture");

        assert_eq!(response.id(), "resp_1");
        assert_eq!(response.status(), Some(&ResponseStatus::Completed));
        assert_eq!(response.output_text(), "Hello world");
        assert_eq!(response.function_calls().count(), 1);
        assert_eq!(response.to_input_items().len(), 2);
        assert_eq!(response.usage().map(ResponseUsage::total_tokens), Some(15));
        assert_eq!(
            response.extra_fields().get("future_response_field"),
            Some(&json!({"nested": [1, 2, 3]}))
        );
        assert_eq!(
            serde_json::to_value(response).expect("semantic response round trip"),
            original
        );
    }

    fn decode_stream_event(value: Value) -> ResponseStreamEvent {
        serde_json::from_value(value).expect("decode accumulator stream fixture")
    }

    #[test]
    fn accumulator_reduces_interleaved_text_and_function_arguments() {
        let mut accumulator = ResponseAccumulator::new();
        let fixtures = [
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": "msg_1",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": []
                },
                "sequence_number": 1
            }),
            json!({
                "type": "response.output_item.added",
                "output_index": 1,
                "item": {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "weather",
                    "arguments": "",
                    "status": "in_progress"
                },
                "sequence_number": 2
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "Hello ",
                "sequence_number": 3,
                "logprobs": []
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 1,
                "delta": "{\"city\":",
                "sequence_number": 4
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "world",
                "sequence_number": 5,
                "logprobs": []
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 1,
                "delta": "\"Paris\"}",
                "sequence_number": 6
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_1",
                "name": "weather",
                "output_index": 1,
                "arguments": "{\"city\":\"Paris\"}",
                "sequence_number": 7
            }),
            json!({
                "type": "response.output_text.done",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "text": "Hello world",
                "sequence_number": 8,
                "logprobs": []
            }),
        ];

        for fixture in fixtures {
            accumulator
                .push(decode_stream_event(fixture))
                .expect("accept interleaved event");
        }
        assert_eq!(accumulator.last_sequence_number(), Some(8));
        assert_eq!(accumulator.output_text(), "Hello world");
        assert_eq!(
            accumulator.function_arguments("fc_1"),
            Some("{\"city\":\"Paris\"}")
        );
        assert!(accumulator.snapshot().is_none());

        accumulator
            .push(decode_stream_event(json!({
                "type": "response.completed",
                "response": sample_response_value(),
                "sequence_number": 9
            })))
            .expect("accept terminal response");
        assert_eq!(accumulator.snapshot().map(Response::id), Some("resp_1"));
        assert_eq!(accumulator.output_text(), "Hello world");

        let response = accumulator.finish().expect("finish terminal response");
        assert_eq!(response.id(), "resp_1");
    }

    #[test]
    fn accumulator_reduces_part_level_lifecycle_across_content_indexes() {
        let mut accumulator = ResponseAccumulator::new();
        let fixtures = [
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": "msg_parts",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": []
                },
                "sequence_number": 1
            }),
            // content 0 opens as an empty output_text part.
            json!({
                "type": "response.content_part.added",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []},
                "sequence_number": 2
            }),
            // annotations arrive mid-part before any done snapshot.
            json!({
                "type": "response.output_text.annotation.added",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "annotation_index": 0,
                "annotation": {
                    "type": "url_citation",
                    "url": "https://example.com",
                    "start_index": 0,
                    "end_index": 4,
                    "title": "Example"
                },
                "sequence_number": 3
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "delta": "Anch",
                "sequence_number": 4,
                "logprobs": []
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "delta": "ored",
                "sequence_number": 5,
                "logprobs": []
            }),
            // The part-level done snapshot must replace, not duplicate, the
            // accumulated deltas.
            json!({
                "type": "response.content_part.done",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "text": "Anchored",
                    "annotations": [{
                        "type": "url_citation",
                        "url": "https://example.com",
                        "start_index": 0,
                        "end_index": 4,
                        "title": "Example"
                    }],
                    "logprobs": []
                },
                "sequence_number": 6
            }),
            json!({
                "type": "response.output_text.done",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 0,
                "text": "Anchored",
                "sequence_number": 7,
                "logprobs": []
            }),
            // content 1 is a reasoning_text part: it binds the item but must
            // never contribute to output_text().
            json!({
                "type": "response.content_part.added",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 1,
                "part": {"type": "reasoning_text", "text": "thinking"},
                "sequence_number": 8
            }),
            json!({
                "type": "response.content_part.done",
                "item_id": "msg_parts",
                "output_index": 0,
                "content_index": 1,
                "part": {"type": "reasoning_text", "text": "thinking"},
                "sequence_number": 9
            }),
            // A second output item keeps its own content 0 aggregated after
            // the first item's parts in output index order.
            json!({
                "type": "response.output_item.added",
                "output_index": 1,
                "item": {
                    "type": "message",
                    "id": "msg_after",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": []
                },
                "sequence_number": 10
            }),
            json!({
                "type": "response.content_part.added",
                "item_id": "msg_after",
                "output_index": 1,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []},
                "sequence_number": 11
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_after",
                "output_index": 1,
                "content_index": 0,
                "delta": " tail",
                "sequence_number": 12,
                "logprobs": []
            }),
            json!({
                "type": "response.output_text.done",
                "item_id": "msg_after",
                "output_index": 1,
                "content_index": 0,
                "text": " tail",
                "sequence_number": 13,
                "logprobs": []
            }),
        ];

        for fixture in fixtures {
            accumulator
                .push(decode_stream_event(fixture))
                .expect("accept part-level lifecycle event");
        }
        assert_eq!(accumulator.last_sequence_number(), Some(13));
        assert_eq!(
            accumulator.output_text(),
            "Anchored tail",
            "part snapshots replace deltas and reasoning_text parts stay out of output_text()"
        );

        accumulator
            .push(decode_stream_event(json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_parts",
                    "created_at": 2,
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "metadata": null,
                    "model": "gpt-test",
                    "object": "response",
                    "output": [
                        {
                            "type": "message",
                            "id": "msg_parts",
                            "status": "completed",
                            "role": "assistant",
                            "content": [
                                {
                                    "type": "output_text",
                                    "text": "Anchored",
                                    "annotations": [],
                                    "logprobs": []
                                },
                                {"type": "reasoning_text", "text": "thinking"}
                            ]
                        },
                        {
                            "type": "message",
                            "id": "msg_after",
                            "status": "completed",
                            "role": "assistant",
                            "content": [
                                {
                                    "type": "output_text",
                                    "text": " tail",
                                    "annotations": [],
                                    "logprobs": []
                                }
                            ]
                        }
                    ],
                    "parallel_tool_calls": false,
                    "temperature": null,
                    "tool_choice": "auto",
                    "tools": [],
                    "top_p": null
                },
                "sequence_number": 14
            })))
            .expect("accept terminal response");
        assert_eq!(
            accumulator.output_text(),
            "Anchored tail",
            "terminal snapshot keeps reasoning_text parts out of the final text"
        );
        let response = accumulator.finish().expect("finish terminal response");
        assert_eq!(response.id(), "resp_parts");
    }

    #[test]
    fn accumulator_rejects_duplicate_sequence_and_item_identity_changes() {
        let first = json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "a",
            "sequence_number": 1,
            "logprobs": []
        });
        let mut duplicate = ResponseAccumulator::new();
        duplicate
            .push(decode_stream_event(first.clone()))
            .expect("accept first sequence");
        let error = duplicate
            .push(decode_stream_event(first))
            .expect_err("duplicate sequence must fail");
        assert_eq!(
            error,
            ResponseAccumulatorError::DuplicateSequence { sequence_number: 1 }
        );

        let mut mismatch = ResponseAccumulator::new();
        mismatch
            .push(decode_stream_event(json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": "msg_1",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": []
                },
                "sequence_number": 1
            })))
            .expect("bind output index");
        let error = mismatch
            .push(decode_stream_event(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_other",
                "output_index": 0,
                "content_index": 0,
                "delta": "x",
                "sequence_number": 2,
                "logprobs": []
            })))
            .expect_err("item identity mismatch must fail");
        assert!(matches!(
            error,
            ResponseAccumulatorError::ItemIdentityMismatch {
                output_index: 0,
                ..
            }
        ));

        assert_eq!(
            ResponseAccumulator::new().finish(),
            Err(ResponseAccumulatorError::MissingTerminal)
        );
    }

    #[test]
    fn typed_function_arguments_and_outputs_need_no_json_text() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct WeatherArgs {
            city: String,
        }

        let arguments: JsonText = JsonText::from_serializable(&json!({"city": "Paris"}))
            .expect("serialize typed arguments");
        let call = FunctionCall::new(
            "fc_1",
            "call_1",
            "weather",
            arguments,
            FunctionCallItemStatus::Completed,
        );
        let parsed: WeatherArgs = call
            .arguments()
            .deserialize_as()
            .expect("parse typed arguments");
        assert_eq!(parsed.city, "Paris");

        let output =
            FunctionCallOutput::from_serializable(call.call_id(), &json!({"temperature": 21}))
                .expect("serialize typed output");
        let parsed: Value = output.deserialize_output().expect("parse typed output");
        assert_eq!(parsed, json!({"temperature": 21}));
    }

    #[test]
    fn tool_choice_and_open_status_values_round_trip() {
        let choice = ToolChoice::Function(FunctionToolChoice::new("weather"));
        let encoded = serde_json::to_value(&choice).expect("encode function choice");
        assert_eq!(encoded, json!({"type": "function", "name": "weather"}));
        let decoded: ToolChoice = serde_json::from_value(encoded).expect("decode function choice");
        assert_eq!(decoded, choice);

        let status: ResponseStatus =
            serde_json::from_value(json!("paused_by_future_server")).expect("decode future status");
        assert_eq!(status.as_str(), "paused_by_future_server");
        assert_eq!(
            serde_json::to_value(status).expect("encode future status"),
            json!("paused_by_future_server")
        );
    }

    #[test]
    fn stream_events_distinguish_terminal_and_unknown_events() {
        let delta: ResponseStreamEvent = serde_json::from_value(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hi",
            "sequence_number": 7,
            "logprobs": [],
            "future_delta_field": true
        }))
        .expect("decode text delta");
        assert!(!delta.is_terminal());
        assert_eq!(delta.sequence_number(), Some(7));
        let ResponseStreamEvent::OutputTextDelta(delta) = delta else {
            panic!("expected output text delta");
        };
        assert_eq!(delta.delta(), "Hi");

        let future = json!({
            "type": "response.future_progress",
            "sequence_number": 8,
            "payload": {"percent": 50}
        });
        let event: ResponseStreamEvent =
            serde_json::from_value(future.clone()).expect("decode future event");
        assert_eq!(event.sequence_number(), Some(8));
        let ResponseStreamEvent::Unknown(unknown) = &event else {
            panic!("future event must stay unknown");
        };
        assert_eq!(unknown.discriminator(), "response.future_progress");
        assert_eq!(
            serde_json::to_value(event).expect("re-encode future event"),
            future
        );

        let error: ResponseStreamEvent = serde_json::from_value(json!({
            "type": "error",
            "code": "server_error",
            "message": "retry later",
            "param": null,
            "sequence_number": 9
        }))
        .expect("decode stream error");
        assert!(error.is_terminal());
        let ResponseStreamEvent::Error(error) = error else {
            panic!("expected stream error");
        };
        assert_eq!(error.code(), Some("server_error"));
        assert_eq!(error.param(), None);
        assert_eq!(error.message(), "retry later");
        assert_eq!(error.sequence_number(), 9);

        let param_error: ResponseStreamEvent = serde_json::from_value(json!({
            "type": "error",
            "code": "invalid_prompt",
            "message": "unknown field",
            "param": "input_text.text",
            "sequence_number": 11
        }))
        .expect("decode stream error param");
        let ResponseStreamEvent::Error(param_error) = param_error else {
            panic!("expected stream error with param");
        };
        assert_eq!(param_error.param(), Some("input_text.text"));

        let null_code: ResponseStreamEvent = serde_json::from_value(json!({
            "type": "error",
            "code": null,
            "message": "retry later",
            "param": null,
            "sequence_number": 10
        }))
        .expect("decode official null stream error code");
        let ResponseStreamEvent::Error(null_code) = null_code else {
            panic!("expected stream error");
        };
        assert_eq!(null_code.code(), None);
        assert_eq!(null_code.param(), None);
    }

    #[test]
    fn accumulator_stream_error_keeps_code_param_and_sequence_number() {
        // The standalone SSE error event carries the offending parameter and
        // its sequence number; a null code stays `None` instead of an empty
        // string (4-05).
        let mut accumulator = ResponseAccumulator::new();
        let error = ResponseStreamEvent::Error(StreamErrorEvent {
            kind: StreamErrorEventTag::Error,
            code: Nullable::Value("invalid_prompt".into()),
            message: "unknown field".into(),
            param: Nullable::Value("input_text.text".into()),
            sequence_number: 4,
            extra: ExtraFields::new(),
        });
        let err = accumulator
            .push(error)
            .expect_err("stream error must surface");
        assert_eq!(
            err,
            ResponseAccumulatorError::Stream {
                code: Some("invalid_prompt".into()),
                message: "unknown field".into(),
                param: Some("input_text.text".into()),
                sequence_number: 4,
            }
        );
        assert_eq!(
            err.to_string(),
            "Responses stream error `invalid_prompt` on `input_text.text` at sequence 4: unknown field"
        );

        let mut null_accumulator = ResponseAccumulator::new();
        let null_error = ResponseStreamEvent::Error(StreamErrorEvent {
            kind: StreamErrorEventTag::Error,
            code: Nullable::Null,
            message: "retry later".into(),
            param: Nullable::Null,
            sequence_number: 5,
            extra: ExtraFields::new(),
        });
        let null_err = null_accumulator
            .push(null_error)
            .expect_err("null-code stream error must still surface");
        assert!(
            matches!(
                &null_err,
                ResponseAccumulatorError::Stream {
                    code: None,
                    param: None,
                    sequence_number: 5,
                    ..
                }
            ),
            "null code and param must round-trip as None: {null_err:?}"
        );
        assert_eq!(
            null_err.to_string(),
            "Responses stream error at sequence 5: retry later"
        );
    }

    #[test]
    fn native_mcp_secrets_are_redacted_from_debug() {
        let tool = McpTool::remote("calendar", "https://mcp.example.test")
            .authorization("Bearer extremely-secret")
            .header("X-Secret", "also-secret")
            .allowed_tools(McpAllowedTools::Names(vec!["read_events".to_owned()]))
            .require_approval(McpRequireApproval::Always);

        let debug = format!("{tool:?}");
        assert!(!debug.contains("extremely-secret"));
        assert!(!debug.contains("also-secret"));

        let value = serde_json::to_value(tool).expect("serialize MCP tool");
        assert_eq!(value["type"], "mcp");
        assert_eq!(value["server_label"], "calendar");
        assert_eq!(value["require_approval"], "always");
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
    struct WeatherQuery {
        city: String,
        country: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
    struct WeatherReport {
        temperature: f64,
        summary: String,
    }

    #[test]
    fn function_tool_for_type_and_arguments_as() {
        let tool = FunctionTool::for_type::<WeatherQuery>("get_weather", "Get weather for a city")
            .expect("build tool");
        let serialized = serde_json::to_value(&tool).expect("serialize tool");
        assert_eq!(serialized["type"], "function");
        assert_eq!(serialized["name"], "get_weather");
        assert_eq!(serialized["strict"], true);
        assert_eq!(serialized["parameters"]["type"], "object");
        assert_eq!(serialized["parameters"]["additionalProperties"], false);

        let call = FunctionCall::new(
            "item_1",
            "call_123",
            "get_weather",
            serde_json::json!({ "city": "Hangzhou", "country": null })
                .to_string()
                .into(),
            FunctionCallItemStatus::Completed,
        );
        let parsed: WeatherQuery = call.arguments_as().expect("parse arguments");
        assert_eq!(
            parsed,
            WeatherQuery {
                city: "Hangzhou".into(),
                country: None,
            }
        );

        let output = FunctionCallOutput::json(
            call.call_id(),
            &WeatherReport {
                temperature: 26.0,
                summary: "Sunny".into(),
            },
        )
        .expect("build output");
        let val = serde_json::to_value(&output).expect("serialize output");
        assert_eq!(val["type"], "function_call_output");
        assert_eq!(val["call_id"], "call_123");

        let default_tool = FunctionTool::new("basic_tool");
        let default_val = serde_json::to_value(&default_tool).expect("serialize default tool");
        assert!(
            default_val.get("strict").is_none(),
            "default tool must omit strict field"
        );
        assert_eq!(default_tool.is_strict(), None);

        let decoded_null_strict: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "from_api",
            "parameters": {"type": "object"},
            "strict": null
        }))
        .expect("decode function tool with strict: null");
        assert_eq!(decoded_null_strict.is_strict(), None);

        let decoded_bool_strict: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "from_api",
            "parameters": {"type": "object"},
            "strict": true
        }))
        .expect("decode function tool with strict: true");
        assert_eq!(decoded_bool_strict.is_strict(), Some(true));
        assert_eq!(
            val["output"],
            "{\"temperature\":26.0,\"summary\":\"Sunny\"}"
        );
    }

    #[test]
    fn response_output_parsed_branches() {
        // Success case
        let success_response = Response {
            id: "resp_1".into(),
            created_at: 1000,
            error: Nullable::Null,
            incomplete_details: Nullable::Null,
            instructions: Nullable::Null,
            metadata: Nullable::Null,
            model: "gpt-5.6-sol".into(),
            object: ResponseObjectTag::Response,
            output: vec![ResponseOutputItem::Message(OutputMessage::new(
                "msg_1",
                MessageStatus::Completed,
                vec![OutputContent::Text(OutputText::new(
                    "{\"temperature\":20.5,\"summary\":\"Cloudy\"}",
                ))],
            ))],
            parallel_tool_calls: false,
            temperature: Nullable::Null,
            tool_choice: ToolChoice::Auto,
            tools: vec![],
            top_p: Nullable::Null,
            status: Omittable::Value(ResponseStatus::Completed),
            background: Omittable::Omitted,
            completed_at: Omittable::Omitted,
            conversation: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            max_tool_calls: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            store: Omittable::Omitted,
            text: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            truncation: Omittable::Omitted,
            usage: Omittable::Omitted,
            user: Omittable::Omitted,
            moderation: Omittable::Omitted,
            extra: ExtraFields::new(),
        };
        let parsed: WeatherReport = success_response
            .output_parsed()
            .expect("parse successful output");
        assert_eq!(parsed.temperature, 20.5);

        // Refusal case
        let refusal_response = Response {
            id: "resp_2".into(),
            created_at: 1000,
            error: Nullable::Null,
            incomplete_details: Nullable::Null,
            instructions: Nullable::Null,
            metadata: Nullable::Null,
            model: "gpt-5.6-sol".into(),
            object: ResponseObjectTag::Response,
            output: vec![ResponseOutputItem::Message(OutputMessage::new(
                "msg_2",
                MessageStatus::Completed,
                vec![OutputContent::Refusal(Refusal::new(
                    "Cannot assist with that request",
                ))],
            ))],
            parallel_tool_calls: false,
            temperature: Nullable::Null,
            tool_choice: ToolChoice::Auto,
            tools: vec![],
            top_p: Nullable::Null,
            status: Omittable::Value(ResponseStatus::Completed),
            background: Omittable::Omitted,
            completed_at: Omittable::Omitted,
            conversation: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            max_tool_calls: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            store: Omittable::Omitted,
            text: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            truncation: Omittable::Omitted,
            usage: Omittable::Omitted,
            user: Omittable::Omitted,
            moderation: Omittable::Omitted,
            extra: ExtraFields::new(),
        };
        let err = refusal_response
            .output_parsed::<WeatherReport>()
            .expect_err("refusal must error");
        assert!(matches!(err, OutputParseError::Refusal(r) if r.contains("Cannot assist")));

        // Incomplete case
        let incomplete_response = Response {
            id: "resp_3".into(),
            created_at: 1000,
            error: Nullable::Null,
            incomplete_details: Nullable::Value(IncompleteDetails {
                reason: Omittable::Value(IncompleteReason::MaxOutputTokens),
                extra: ExtraFields::new(),
            }),
            instructions: Nullable::Null,
            metadata: Nullable::Null,
            model: "gpt-5.6-sol".into(),
            object: ResponseObjectTag::Response,
            output: vec![],
            parallel_tool_calls: false,
            temperature: Nullable::Null,
            tool_choice: ToolChoice::Auto,
            tools: vec![],
            top_p: Nullable::Null,
            status: Omittable::Value(ResponseStatus::Incomplete),
            background: Omittable::Omitted,
            completed_at: Omittable::Omitted,
            conversation: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            max_tool_calls: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            store: Omittable::Omitted,
            text: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            truncation: Omittable::Omitted,
            usage: Omittable::Omitted,
            user: Omittable::Omitted,
            moderation: Omittable::Omitted,
            extra: ExtraFields::new(),
        };
        let inc_err = incomplete_response
            .output_parsed::<WeatherReport>()
            .expect_err("incomplete must error");
        assert!(
            matches!(&inc_err, OutputParseError::Incomplete(Some(reason)) if reason == "max_output_tokens")
        );
        assert_eq!(
            inc_err.to_string(),
            "response was incomplete: max_output_tokens"
        );

        // Failed case routes the service error payload instead of losing it.
        let failed_response = Response {
            error: Nullable::Value(ResponseError {
                code: ResponseErrorCode::RateLimitExceeded,
                message: "Rate limit reached".into(),
                extra: ExtraFields::new(),
            }),
            status: Omittable::Value(ResponseStatus::Failed),
            ..incomplete_response
        };
        let failed_err = failed_response
            .output_parsed::<WeatherReport>()
            .expect_err("failed must error");
        match &failed_err {
            OutputParseError::Failed(error) => {
                assert_eq!(error.code(), &ResponseErrorCode::RateLimitExceeded);
                assert_eq!(error.message(), "Rate limit reached");
            }
            other => panic!("expected a failed error, got {other:?}"),
        }
        assert_eq!(
            failed_err.to_string(),
            "response failed: Rate limit reached"
        );

        // A failed status without an error object still reports readably.
        let null_error_response = Response {
            error: Nullable::Null,
            status: Omittable::Value(ResponseStatus::Failed),
            ..failed_response
        };
        let null_err = null_error_response
            .output_parsed::<WeatherReport>()
            .expect_err("failed without error must still error");
        assert!(
            matches!(&null_err, OutputParseError::Failed(error) if error.message() == "response failed without an error payload")
        );
        assert_eq!(
            null_err.to_string(),
            "response failed: response failed without an error payload"
        );
    }

    #[test]
    fn output_text_and_events_decode_without_logprobs_and_annotations() {
        let text_json = json!({
            "type": "output_text",
            "text": "Hello, world!"
        });
        let output_text: OutputText =
            serde_json::from_value(text_json).expect("decode text without logprobs/annotations");
        assert_eq!(output_text.text(), "Hello, world!");
        assert!(output_text.annotations().is_empty());
        assert!(output_text.logprobs().is_empty());

        let delta_json = json!({
            "type": "response.output_text.delta",
            "item_id": "item_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hello",
            "sequence_number": 1
        });
        let delta_event: OutputTextDeltaEvent =
            serde_json::from_value(delta_json).expect("decode delta without logprobs");
        assert_eq!(delta_event.delta(), "Hello");
        assert!(delta_event.logprobs().is_empty());

        let done_json = json!({
            "type": "response.output_text.done",
            "item_id": "item_1",
            "output_index": 0,
            "content_index": 0,
            "text": "Hello, world!",
            "sequence_number": 2
        });
        let done_event: OutputTextDoneEvent =
            serde_json::from_value(done_json).expect("decode done without logprobs");
        assert_eq!(done_event.text(), "Hello, world!");
        assert!(done_event.logprobs().is_empty());
    }

    #[test]
    fn output_text_required_empty_arrays_survive_decode_encode_round_trip() {
        // Pinned OutputTextContent lists annotations and logprobs in
        // `required` (the service always emits `[]`), so re-encoding a
        // decoded value must keep both keys even when they are empty. A
        // missing key still decodes through `#[serde(default)]` tolerance.
        let official = json!({
            "type": "output_text",
            "text": "Hello, world!",
            "annotations": [],
            "logprobs": []
        });
        let decoded: OutputText =
            serde_json::from_value(official.clone()).expect("decode official empty arrays");
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode keeps required keys"),
            official
        );

        let tolerant: OutputText = serde_json::from_value(json!({
            "type": "output_text",
            "text": "Hello, world!"
        }))
        .expect("missing keys still decode");
        let encoded = serde_json::to_value(&tolerant).expect("re-encode fills required keys");
        assert_eq!(encoded["annotations"], json!([]));
        assert_eq!(encoded["logprobs"], json!([]));

        let delta: OutputTextDeltaEvent = serde_json::from_value(json!({
            "type": "response.output_text.delta",
            "item_id": "item_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hello",
            "sequence_number": 1,
            "logprobs": []
        }))
        .expect("decode official delta empty logprobs");
        assert_eq!(
            serde_json::to_value(&delta).expect("re-encode delta")["logprobs"],
            json!([])
        );

        let done: OutputTextDoneEvent = serde_json::from_value(json!({
            "type": "response.output_text.done",
            "item_id": "item_1",
            "output_index": 0,
            "content_index": 0,
            "text": "Hello, world!",
            "sequence_number": 2,
            "logprobs": []
        }))
        .expect("decode official done empty logprobs");
        assert_eq!(
            serde_json::to_value(&done).expect("re-encode done")["logprobs"],
            json!([])
        );
    }

    #[test]
    fn to_input_items_converts_all_output_items() {
        let output_json = json!([
            {"type": "message", "id": "m1", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "hi"}]},
            {"type": "function_call", "id": "fc1", "call_id": "c1", "name": "fn1", "arguments": "{}", "status": "completed"},
            {"type": "function_call_output", "id": "fco1", "output": "result", "status": "completed"},
            {"type": "file_search_call", "id": "fs1", "status": "completed", "queries": ["test"]},
            {"type": "web_search_call", "id": "ws1", "status": "completed", "action": {"type": "search", "query": "openai"}},
            {"type": "computer_call", "id": "cc1", "call_id": "c_call", "action": {"type": "screenshot"}, "pending_safety_checks": [], "status": "completed"},
            {"type": "computer_call_output", "id": "cco1", "call_id": "c_call", "output": {"type": "computer_screenshot", "image_url": "https://example.com/screen.png"}, "status": "completed"},
            {"type": "reasoning", "id": "r1", "summary": []},
            {"type": "program", "id": "p1", "call_id": "c_p1", "code": "print(1)", "fingerprint": "fp1"},
            {"type": "program_output", "id": "po1", "call_id": "c_p1", "result": "1\n", "status": "completed"},
            {"type": "tool_search_call", "id": "tsc1", "call_id": null, "execution": "sync", "arguments": {}, "status": "completed"},
            {"type": "tool_search_output", "id": "tso1", "call_id": null, "execution": "sync", "tools": [], "status": "completed"},
            {"type": "additional_tools", "id": "at1", "role": "assistant", "tools": []},
            {"type": "compaction", "id": "cmp1", "encrypted_content": "enc_data"},
            {"type": "image_generation_call", "id": "ig1", "status": "completed", "result": "img_b64"},
            {"type": "code_interpreter_call", "id": "ci1", "status": "completed", "container_id": "cnt1", "code": "1+1", "outputs": []},
            {"type": "local_shell_call", "id": "lsc1", "call_id": "l1", "action": {"type": "exec", "command": ["echo", "ok"], "env": {}}, "status": "completed"},
            {"type": "local_shell_call_output", "id": "lsco1", "call_id": "l1", "output": "ok"},
            {"type": "shell_call", "id": "sh1", "call_id": "s1", "action": {"commands": ["echo"], "timeout_ms": null, "max_output_length": null}, "status": "completed", "environment": null},
            {"type": "shell_call_output", "id": "sho1", "call_id": "s1", "status": "completed", "output": [], "max_output_length": null},
            {"type": "apply_patch_call", "id": "ap1", "call_id": "apc1", "status": "completed", "operation": {"type": "create_file", "path": "main.rs", "diff": "+fn main() {}"}},
            {"type": "apply_patch_call_output", "id": "apo1", "call_id": "apc1", "status": "completed", "output": "ok"},
            {"type": "mcp_list_tools", "id": "mcp_lt1", "server_label": "srv", "tools": []},
            {"type": "mcp_call", "id": "mcp_c1", "call_id": "mcp1", "server_label": "srv", "name": "tool1", "arguments": "{}", "status": "completed"},
            {"type": "mcp_approval_request", "id": "mcp_ar1", "server_label": "srv", "name": "tool1", "arguments": "{}"},
            {"type": "mcp_approval_response", "id": "mcp_resp1", "request_id": "req1", "approval_request_id": "ar1", "approve": true},
            {"type": "custom_tool_call", "call_id": "cust1", "name": "custom", "input": "{}"},
            {"type": "custom_tool_call_output", "id": "custo1", "call_id": "cust1", "output": "done", "status": "completed"},
            {"type": "future_unknown_tool", "data": 123}
        ]);

        let outputs: Vec<ResponseOutputItem> =
            serde_json::from_value(output_json).expect("decode output items");
        assert_eq!(outputs.len(), 29);

        let response = Response {
            id: "resp_test".into(),
            created_at: 1000,
            error: Nullable::Null,
            incomplete_details: Nullable::Null,
            instructions: Nullable::Null,
            metadata: Nullable::Null,
            model: "gpt-5.6-sol".into(),
            object: ResponseObjectTag::Response,
            output: outputs,
            parallel_tool_calls: false,
            temperature: Nullable::Null,
            tool_choice: ToolChoice::Auto,
            tools: vec![],
            top_p: Nullable::Null,
            status: Omittable::Value(ResponseStatus::Completed),
            background: Omittable::Omitted,
            completed_at: Omittable::Omitted,
            conversation: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            max_tool_calls: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            store: Omittable::Omitted,
            text: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            truncation: Omittable::Omitted,
            usage: Omittable::Omitted,
            user: Omittable::Omitted,
            moderation: Omittable::Omitted,
            extra: ExtraFields::new(),
        };

        let input_items = response.to_input_items();
        assert_eq!(
            input_items.len(),
            29,
            "Every single output item must convert to an input item without loss"
        );

        // Verify all converted input items serialize back to valid JSON
        let serialized_inputs = serde_json::to_value(&input_items).expect("serialize input items");
        assert_eq!(
            serialized_inputs
                .as_array()
                .expect("serialized input items must be an array")
                .len(),
            29
        );
        // The replayed assistant message keeps the pin-required
        // `annotations`/`logprobs` keys on its output_text part even when
        // the decoded arrays are empty.
        assert_eq!(
            serialized_inputs[0]["content"][0]["annotations"],
            json!([]),
            "replayed output_text must keep the required annotations key"
        );
        assert_eq!(
            serialized_inputs[0]["content"][0]["logprobs"],
            json!([]),
            "replayed output_text must keep the required logprobs key"
        );
    }

    #[test]
    fn official_status_and_execution_enums_retain_unknown_values() {
        // Pinned domains: ProgramOutputStatus completed|incomplete,
        // ApplyPatchCallStatus(Param) in_progress|completed,
        // ApplyPatchCallOutputStatus(Param) completed|failed,
        // ToolSearchExecutionType server|client.
        assert_eq!(ProgramOutputStatus::Completed.as_str(), "completed");
        assert_eq!(ProgramOutputStatus::Incomplete.as_str(), "incomplete");
        assert_eq!(ApplyPatchCallStatus::InProgress.as_str(), "in_progress");
        assert_eq!(ApplyPatchCallStatus::Completed.as_str(), "completed");
        assert_eq!(ApplyPatchCallOutputStatus::Completed.as_str(), "completed");
        assert_eq!(ApplyPatchCallOutputStatus::Failed.as_str(), "failed");
        assert_eq!(ToolSearchExecution::Server.as_str(), "server");
        assert_eq!(ToolSearchExecution::Client.as_str(), "client");

        let fixtures = [
            (
                json!({
                    "type": "program_output",
                    "id": "po1",
                    "call_id": "c_p1",
                    "result": "1\n",
                    "status": "queued"
                }),
                "queued",
            ),
            (
                json!({
                    "type": "apply_patch_call",
                    "id": "ap1",
                    "call_id": "apc1",
                    "status": "retrying",
                    "operation": {"type": "delete_file", "path": "main.rs"}
                }),
                "retrying",
            ),
            (
                json!({
                    "type": "apply_patch_call_output",
                    "id": "apo1",
                    "call_id": "apc1",
                    "status": "cancelled",
                    "output": "ok"
                }),
                "cancelled",
            ),
            (
                json!({
                    "type": "tool_search_call",
                    "id": "tsc1",
                    "call_id": null,
                    "execution": "hybrid",
                    "arguments": {},
                    "status": "completed"
                }),
                "hybrid",
            ),
            (
                json!({
                    "type": "tool_search_output",
                    "id": "tso1",
                    "call_id": null,
                    "execution": "offline",
                    "tools": [],
                    "status": "completed"
                }),
                "offline",
            ),
        ];
        for (fixture, unknown) in fixtures {
            let decoded: ResponseOutputItem =
                serde_json::from_value(fixture.clone()).expect("unknown enum values stay lossless");
            assert_eq!(
                serde_json::to_value(&decoded).expect("round-trip unknown enum"),
                fixture,
                "unknown value {unknown} must round-trip"
            );
        }

        let search = serde_json::from_value::<ResponseInputItem>(json!({
            "type": "tool_search_call",
            "arguments": {},
            "execution": "hybrid"
        }))
        .expect("input-side unknown execution stays lossless");
        assert_eq!(
            serde_json::to_value(&search).expect("round-trip input execution")["execution"],
            "hybrid"
        );
    }

    #[test]
    fn compact_service_tier_pins_the_five_official_values() {
        // Pinned CompactResponseMethodPublicBody.service_tier references
        // ServiceTierEnum: auto/default/fast/flex/priority. The create side
        // keeps the wider seven-value ServiceTier domain (3-01).
        const OFFICIAL_COMPACT_TIERS: [&str; 5] = ["auto", "default", "fast", "flex", "priority"];
        for value in OFFICIAL_COMPACT_TIERS {
            let decoded = CompactServiceTier::from_raw(value);
            assert!(
                decoded.is_known(),
                "official ServiceTierEnum value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let request =
                CompactResponseRequest::new("gpt-5.6-sol", "compact me").service_tier(decoded);
            assert_eq!(
                serde_json::to_value(&request).expect("serialize compact tier")["service_tier"],
                value
            );
        }
        for create_only in ["scale", "ultrafast"] {
            let decoded = CompactServiceTier::from_raw(create_only);
            assert!(
                !decoded.is_known(),
                "{create_only} belongs to the create domain, not ServiceTierEnum"
            );
            assert_eq!(decoded.as_str(), create_only);
            let round_tripped = serde_json::from_value::<CompactResponseRequest>(json!({
                "model": "gpt-5.6-sol",
                "service_tier": create_only
            }))
            .expect("unknown compact tiers stay lossless");
            assert_eq!(
                serde_json::to_value(&round_tripped).expect("re-encode")["service_tier"],
                create_only
            );
        }
        assert!(ServiceTier::from_raw("scale").is_known());
        assert!(ServiceTier::from_raw("ultrafast").is_known());
    }

    #[test]
    fn easy_input_message_role_pins_the_four_official_values() {
        // Pinned EasyInputMessage.role: user/assistant/system/developer
        // (python EasyInputMessageParam.role Literal). Multi-agent roles stay
        // decode-only through the open MessageRole (D0137口径, 3-04).
        const OFFICIAL_EASY_ROLES: [&str; 4] = ["user", "assistant", "system", "developer"];
        for value in OFFICIAL_EASY_ROLES {
            let decoded = EasyInputMessageRole::from_raw(value);
            assert!(
                decoded.is_known(),
                "official EasyInputMessage role {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let message = InputMessage::new(decoded, "hello");
            assert_eq!(message.role().as_str(), value);
            assert_eq!(
                serde_json::to_value(&message).expect("serialize easy role")["role"],
                value
            );
        }
        for decode_only in ["unknown", "critic", "discriminator", "tool"] {
            let decoded = EasyInputMessageRole::from_raw(decode_only);
            assert!(
                !decoded.is_known(),
                "{decode_only} is a decode-side MessageRole, not an easy-form role"
            );
        }
        assert_eq!(
            MessageRole::from(EasyInputMessageRole::Assistant),
            MessageRole::Assistant
        );
        assert_eq!(
            MessageRole::from(EasyInputMessageRole::from_raw("critic")),
            MessageRole::Unknown("critic".into())
        );
        assert_eq!(
            EasyInputMessageRole::from(StoredInputMessageRole::Developer),
            EasyInputMessageRole::Developer
        );

        // Decoding keeps the open MessageRole: multi-agent roles echoed in an
        // easy-form message remain lossless even though they cannot be built.
        let decoded: InputMessage = serde_json::from_value(json!({
            "role": "critic",
            "content": "harsh feedback"
        }))
        .expect("easy-form decode keeps the open role domain");
        assert_eq!(decoded.role(), &MessageRole::Critic);
        assert_eq!(
            serde_json::to_value(&decoded).expect("round-trip critic role")["role"],
            "critic"
        );
    }

    #[test]
    fn param_content_unions_exclude_computer_screenshot_outside_item_form() {
        // Pinned request unions: EasyInputMessage.content ->
        // InputMessageContentList -> InputContent {input_text, input_image,
        // input_file}; FunctionCallOutputItemParam.output likewise. Only the
        // item-form Message branch accepts ComputerScreenshotContent (3-05).
        let item_form: StoredInputMessage = serde_json::from_value(json!({
            "type": "message",
            "role": "user",
            "status": "completed",
            "content": [
                {"type": "computer_screenshot", "image_url": "https://example.test/s.png", "file_id": null, "detail": "auto"}
            ]
        }))
        .expect("item-form message keeps the computer_screenshot branch");
        assert!(matches!(
            item_form.content()[0],
            InputContent::ComputerScreenshot(_)
        ));

        let easy_form: InputMessage = serde_json::from_value(json!({
            "role": "user",
            "content": [
                {"type": "computer_screenshot", "image_url": "https://example.test/s.png", "file_id": null, "detail": "auto"}
            ]
        }))
        .expect("easy-form decode stays lossless through Unknown retention");
        let MessageContent::Parts(parts) = easy_form.content() else {
            panic!("array content must decode as parts");
        };
        assert!(
            !matches!(
                parts[0],
                EasyInputContent::Text(_) | EasyInputContent::Image(_) | EasyInputContent::File(_)
            ),
            "computer_screenshot is not a named easy-form branch"
        );
        assert_eq!(
            serde_json::to_value(&easy_form).expect("round-trip easy-form part"),
            json!({
                "role": "user",
                "content": [
                    {"type": "computer_screenshot", "image_url": "https://example.test/s.png", "file_id": null, "detail": "auto"}
                ]
            })
        );

        let function_output: FunctionCallOutput = serde_json::from_value(json!({
            "type": "function_call_output",
            "call_id": "call_1",
            "output": [
                {"type": "computer_screenshot", "image_url": "https://example.test/s.png", "file_id": null, "detail": "auto"}
            ]
        }))
        .expect("function output decode stays lossless through Unknown retention");
        let FunctionCallOutputParamValue::Content(parts) = function_output.output() else {
            panic!("array output must decode as content");
        };
        assert!(
            !matches!(
                parts[0],
                FunctionCallOutputContent::Text(_)
                    | FunctionCallOutputContent::Image(_)
                    | FunctionCallOutputContent::File(_)
            ),
            "computer_screenshot is not a named function-output branch"
        );
        assert_eq!(
            serde_json::to_value(&function_output).expect("round-trip function output part")["output"]
                [0]["type"],
            "computer_screenshot"
        );

        // The three-branch param unions stay isomorphic for legal parts.
        let easy_parts = vec![EasyInputContent::from(InputText::new("done"))];
        let converted: FunctionCallOutputParamValue = easy_parts.into();
        assert!(matches!(
            converted,
            FunctionCallOutputParamValue::Content(parts)
                if matches!(parts[0], FunctionCallOutputContent::Text(_))
        ));
        FunctionCallOutput::from_output(vec![EasyInputContent::from(InputImage::from_url(
            "https://example.test/a.png",
        ))])
        .validate()
        .expect("easy-form parts convert into valid function outputs");
    }

    #[test]
    fn per_host_item_status_enums_pin_official_domains() {
        // Pinned per-host domains (3-06): MessageStatus /
        // FunctionCallItemStatus 3 values; MCPToolCallStatus adds
        // calling/failed; WebSearchToolCall has searching+failed but no
        // incomplete; FileSearchToolCall pins all five; ImageGenToolCall
        // carries generating; CodeInterpreterToolCall carries interpreting.
        // Computer / local-shell / tool-search / reasoning hosts pin the same
        // three-value trio as function calls (4-01).
        let hosts: [(&str, Vec<&str>, Vec<&str>); 12] = [
            (
                "message",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "function_call",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "mcp_call",
                vec![
                    "in_progress",
                    "completed",
                    "incomplete",
                    "calling",
                    "failed",
                ],
                vec!["searching", "generating", "interpreting"],
            ),
            (
                "web_search_call",
                vec!["in_progress", "searching", "completed", "failed"],
                vec!["incomplete", "generating", "interpreting", "calling"],
            ),
            (
                "file_search_call",
                vec![
                    "in_progress",
                    "searching",
                    "completed",
                    "incomplete",
                    "failed",
                ],
                vec!["generating", "interpreting", "calling"],
            ),
            (
                "image_generation_call",
                vec!["in_progress", "completed", "generating", "failed"],
                vec!["searching", "incomplete", "interpreting", "calling"],
            ),
            (
                "code_interpreter_call",
                vec![
                    "in_progress",
                    "completed",
                    "incomplete",
                    "interpreting",
                    "failed",
                ],
                vec!["searching", "generating", "calling"],
            ),
            (
                "computer_call",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "local_shell_call",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "tool_search_call",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "reasoning",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
            (
                "local_shell_call_output",
                vec!["in_progress", "completed", "incomplete"],
                vec![
                    "searching",
                    "generating",
                    "interpreting",
                    "calling",
                    "failed",
                ],
            ),
        ];
        for (host, official, foreign) in hosts {
            let narrow = |value: &str| -> (bool, String) {
                match host {
                    "message" => {
                        let decoded = MessageStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    "mcp_call" => {
                        let decoded = McpToolCallStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    "web_search_call" => {
                        let decoded = WebSearchToolCallStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    "file_search_call" => {
                        let decoded = FileSearchToolCallStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    "image_generation_call" => {
                        let decoded = ImageGenToolCallStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    "code_interpreter_call" => {
                        let decoded = CodeInterpreterToolCallStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                    // The function-call trio also hosts the computer /
                    // local-shell / tool-search / reasoning item statuses.
                    _ => {
                        let decoded = FunctionCallItemStatus::from_raw(value);
                        (decoded.is_known(), decoded.as_str().to_owned())
                    }
                }
            };
            for value in &official {
                let (known, observed) = narrow(value);
                assert!(ResponseItemStatus::from_raw(*value).is_known());
                assert!(
                    known,
                    "{host} official status {value} must be a named variant"
                );
                assert_eq!(observed, *value, "{host} status {value} must round-trip");
            }
            for value in &foreign {
                let (known, observed) = narrow(value);
                assert!(!known, "{host} must not claim foreign status {value}");
                assert_eq!(observed, *value, "{host} retains {value} verbatim");
            }
        }

        // Narrow enums convert into the shared decode-side enum, including
        // verbatim Unknown passthrough.
        assert_eq!(
            ResponseItemStatus::from(WebSearchToolCallStatus::Searching),
            ResponseItemStatus::Searching
        );
        assert_eq!(
            ResponseItemStatus::from(McpToolCallStatus::Calling),
            ResponseItemStatus::Calling
        );
        assert_eq!(
            ResponseItemStatus::from(CodeInterpreterToolCallStatus::Interpreting),
            ResponseItemStatus::Interpreting
        );
        assert_eq!(
            ResponseItemStatus::from(ImageGenToolCallStatus::Generating),
            ResponseItemStatus::Generating
        );
        assert_eq!(
            ResponseItemStatus::from(FileSearchToolCallStatus::from_raw("paused")),
            ResponseItemStatus::Unknown("paused".into())
        );

        // Narrowed setters and constructors emit the pinned statuses.
        assert_eq!(
            serde_json::to_value(FileSearchCall::new(
                "fs_1",
                FileSearchToolCallStatus::Searching,
                ["rust"]
            ))
            .expect("serialize file search")["status"],
            "searching"
        );
        assert_eq!(
            serde_json::to_value(WebSearchCall::new(
                "ws_1",
                WebSearchToolCallStatus::Searching,
                WebSearchSearchAction::new().query("openai")
            ))
            .expect("serialize web search")["status"],
            "searching"
        );
        assert_eq!(
            serde_json::to_value(ImageGenerationCall::new(
                "ig_1",
                ImageGenToolCallStatus::Generating
            ))
            .expect("serialize image gen")["status"],
            "generating"
        );
        assert_eq!(
            serde_json::to_value(CodeInterpreterCall::new(
                "ci_1",
                CodeInterpreterToolCallStatus::Interpreting,
                "cntr_1"
            ))
            .expect("serialize interpreter")["status"],
            "interpreting"
        );
        assert_eq!(
            serde_json::to_value(
                McpCall::new("mcp_1", "docs", "search", JsonText::from("{}"))
                    .with_status(McpToolCallStatus::Calling)
            )
            .expect("serialize mcp call")["status"],
            "calling"
        );
        assert_eq!(
            serde_json::to_value(
                FunctionCallOutput::from_output("ok").status(FunctionCallItemStatus::Incomplete)
            )
            .expect("serialize function output")["status"],
            "incomplete"
        );
        assert_eq!(
            serde_json::to_value(
                FunctionCall::call("c1", "lookup", JsonText::from("{}"))
                    .with_status(FunctionCallItemStatus::Completed)
            )
            .expect("serialize function call")["status"],
            "completed"
        );
        assert_eq!(
            serde_json::to_value(ComputerCall::new(
                "cc_2",
                "call_cc",
                FunctionCallItemStatus::InProgress
            ))
            .expect("serialize computer call")["status"],
            "in_progress"
        );
        assert_eq!(
            serde_json::to_value(LocalShellCall::new(
                "ls_2",
                "call_ls",
                LocalShellExecAction::new(["echo"], [("PATH", "/bin")]),
                FunctionCallItemStatus::Completed
            ))
            .expect("serialize local shell call")["status"],
            "completed"
        );
        assert_eq!(
            serde_json::to_value(
                ToolSearchCallInput::new(json!({"query": "tools"}))
                    .status(FunctionCallItemStatus::Incomplete)
            )
            .expect("serialize tool search input")["status"],
            "incomplete"
        );
        assert_eq!(
            serde_json::to_value(
                ReasoningItem::new("rs_2", vec![SummaryTextContent::new("thought")])
                    .status(FunctionCallItemStatus::Completed)
            )
            .expect("serialize reasoning item")["status"],
            "completed"
        );
        assert_eq!(
            serde_json::to_value(
                LocalShellCallOutput::new("lsco_2", "call_ls", "ok")
                    .status(FunctionCallItemStatus::Incomplete)
            )
            .expect("serialize local shell output")["status"],
            "incomplete"
        );

        // Decode side keeps the shared eight-value union: a status foreign to
        // its host item stays lossless instead of failing the decode.
        for (fixture, host) in [
            (
                json!({
                    "type": "web_search_call",
                    "id": "ws_2",
                    "status": "incomplete",
                    "action": {"type": "search", "query": "openai"}
                }),
                "web_search_call",
            ),
            (
                json!({
                    "type": "image_generation_call",
                    "id": "ig_2",
                    "status": "interpreting",
                    "result": null
                }),
                "image_generation_call",
            ),
        ] {
            let decoded: ResponseOutputItem =
                serde_json::from_value(fixture.clone()).expect("decode keeps open statuses");
            assert_eq!(
                serde_json::to_value(&decoded).expect("round-trip open status"),
                fixture,
                "{host} must round-trip foreign statuses"
            );
        }
    }

    #[test]
    fn input_content_and_prompt_version_accept_official_nulls() {
        let text: InputText = serde_json::from_value(json!({
            "type": "input_text",
            "text": "hello",
            "prompt_cache_breakpoint": null
        }))
        .expect("official prompt_cache_breakpoint null");
        assert_eq!(text.prompt_cache_breakpoint_ref(), None);
        assert_eq!(
            serde_json::to_value(InputText::new("hello").prompt_cache_breakpoint_null())
                .expect("serialize")["prompt_cache_breakpoint"],
            Value::Null
        );

        let image: InputImage = serde_json::from_value(json!({
            "type": "input_image",
            "image_url": null,
            "file_id": null,
            "detail": "auto",
            "prompt_cache_breakpoint": null
        }))
        .expect("official InputImageContent locators");
        assert_eq!(image.image_url(), None);
        assert_eq!(image.file_id(), None);
        assert_eq!(image.detail_ref(), &ImageDetail::Auto);
        let sent_image = InputImage::from_url("https://example.test/a.png")
            .image_url_null()
            .file_id_null();
        let sent_image_value = serde_json::to_value(&sent_image).expect("serialize image locators");
        assert_eq!(sent_image_value["image_url"], Value::Null);
        assert_eq!(sent_image_value["file_id"], Value::Null);
        assert_eq!(sent_image_value["detail"], "auto");

        let file: InputFile = serde_json::from_value(json!({
            "type": "input_file",
            "file_id": null,
            "filename": null,
            "file_data": null,
            "file_url": null,
            "prompt_cache_breakpoint": null
        }))
        .expect("official InputFile nulls");
        assert_eq!(
            serde_json::to_value(&file).expect("round-trip")["file_id"],
            Value::Null
        );
        let sent_file = InputFile::from_file_id("file_1")
            .file_id_null()
            .file_url_null()
            .file_data_null()
            .filename_null();
        let sent_file_value = serde_json::to_value(&sent_file).expect("serialize file nulls");
        for key in ["file_id", "file_url", "file_data", "filename"] {
            assert_eq!(sent_file_value[key], Value::Null, "{key}");
        }

        let prompt = serde_json::from_value::<PromptReference>(json!({
            "id": "pmpt_weather",
            "version": null
        }))
        .expect("official Prompt.version null");
        assert_eq!(
            serde_json::to_value(PromptReference::new("pmpt_weather").version_null())
                .expect("serialize")["version"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(&prompt).expect("round-trip prompt")["version"],
            Value::Null
        );

        let screenshot = ComputerScreenshot::new()
            .image_url_null()
            .file_id_null()
            .detail(ImageDetail::Auto)
            .prompt_cache_breakpoint(PromptCacheBreakpoint::explicit());
        assert_eq!(
            serde_json::to_value(&screenshot).expect("serialize screenshot content"),
            json!({
                "type": "computer_screenshot",
                "image_url": null,
                "file_id": null,
                "detail": "auto",
                "prompt_cache_breakpoint": { "mode": "explicit" }
            })
        );
        let decoded_content: InputContent = serde_json::from_value(json!({
            "type": "computer_screenshot",
            "image_url": null,
            "file_id": null,
            "detail": "high",
            "prompt_cache_breakpoint": { "mode": "explicit" }
        }))
        .expect("official ComputerScreenshotContent is a known input part");
        assert!(matches!(
            decoded_content,
            InputContent::ComputerScreenshot(_)
        ));
        assert!(!matches!(decoded_content, InputContent::Unknown(_)));
    }

    #[test]
    fn prompt_cache_breakpoint_uses_mode_not_type() {
        let value = serde_json::to_value(PromptCacheBreakpoint::explicit()).expect("serialize");
        assert_eq!(value, json!({ "mode": "explicit" }));
        let decoded: PromptCacheBreakpoint =
            serde_json::from_value(json!({ "mode": "explicit" })).expect("decode");
        assert_eq!(decoded, PromptCacheBreakpoint::explicit());
    }

    #[test]
    fn prompt_cache_options_match_pinned_mode_and_ttl() {
        let options = PromptCacheOptionsParam::new()
            .mode(PromptCacheMode::Implicit)
            .thirty_minutes();
        let value = serde_json::to_value(&options).expect("serialize");
        assert_eq!(
            value,
            json!({
                "mode": "implicit",
                "ttl": "30m"
            })
        );
        let explicit = PromptCacheOptionsParam::with_mode(PromptCacheMode::Explicit);
        assert_eq!(
            serde_json::to_value(&explicit).expect("serialize"),
            json!({ "mode": "explicit" })
        );
        let decoded: PromptCacheOptionsParam = serde_json::from_value(json!({
            "mode": "implicit",
            "ttl": "30m"
        }))
        .expect("decode");
        assert_eq!(decoded.mode_ref(), Some(&PromptCacheMode::Implicit));
        assert_eq!(decoded.ttl_ref(), Some(&PromptCacheTtl::ThirtyMinutes));

        let applied: PromptCacheOptions = serde_json::from_value(json!({
            "mode": "implicit",
            "ttl": "30m"
        }))
        .expect("official response object requires both fields");
        assert_eq!(applied.mode(), &PromptCacheMode::Implicit);
        assert_eq!(applied.ttl(), &PromptCacheTtl::ThirtyMinutes);
    }

    #[test]
    fn reasoning_config_serializes_ga_mode_context_and_max_effort() {
        let reasoning = ReasoningConfig::new()
            .mode(ReasoningMode::Pro)
            .context(ReasoningContext::AllTurns)
            .effort(ReasoningEffort::Max)
            .summary(ReasoningSummary::Concise);
        assert_eq!(
            serde_json::to_value(&reasoning).expect("serialize"),
            json!({
                "mode": "pro",
                "context": "all_turns",
                "effort": "max",
                "summary": "concise"
            })
        );
    }

    #[test]
    fn create_request_sends_typed_context_moderation_and_explicit_nulls() {
        let request = CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .context_management(ContextManagement::compaction().compact_threshold(8_000))
            .moderation(ModerationConfig::new("omni-moderation-latest"))
            .safety_identifier_null()
            .top_logprobs_null();
        let value = serde_json::to_value(&request).expect("serialize");
        assert_eq!(
            value["context_management"],
            json!([{ "type": "compaction", "compact_threshold": 8000 }])
        );
        assert_eq!(
            value["moderation"],
            json!({ "model": "omni-moderation-latest" })
        );
        assert_eq!(value["safety_identifier"], Value::Null);
        assert_eq!(value["top_logprobs"], Value::Null);

        let reasoning = ReasoningConfig::new()
            .context_null()
            .effort_null()
            .generate_summary_null()
            .summary_null();
        assert_eq!(
            serde_json::to_value(&reasoning).expect("serialize reasoning nulls"),
            json!({
                "context": null,
                "effort": null,
                "generate_summary": null,
                "summary": null
            })
        );
        assert_eq!(
            serde_json::from_value::<ReasoningConfig>(json!({
                "context": null,
                "effort": null,
                "generate_summary": null,
                "summary": null
            }))
            .expect("decode official reasoning nulls"),
            reasoning
        );

        let policy = ModerationPolicy::default().input_null().output_null();
        assert_eq!(
            serde_json::to_value(&policy).expect("serialize policy nulls"),
            json!({ "input": null, "output": null })
        );
        let moderation = ModerationConfig::new("omni-moderation-latest").policy_null();
        assert_eq!(
            serde_json::to_value(&moderation).expect("serialize policy: null"),
            json!({ "model": "omni-moderation-latest", "policy": null })
        );

        let mcp = McpTool::remote("docs", "https://mcp.example")
            .headers_null()
            .allowed_tools_null()
            .allowed_callers_null()
            .require_approval_null();
        assert_eq!(
            serde_json::to_value(&mcp).expect("serialize mcp nulls"),
            json!({
                "type": "mcp",
                "server_label": "docs",
                "server_url": "https://mcp.example",
                "headers": null,
                "allowed_tools": null,
                "allowed_callers": null,
                "require_approval": null
            })
        );

        let filters = WebSearchFilters::default().allowed_domains_null();
        assert_eq!(
            serde_json::to_value(&filters).expect("serialize filters"),
            json!({ "allowed_domains": null })
        );
        assert_eq!(
            serde_json::to_value(WebSearchTool::new().user_location_null())
                .expect("serialize web search")["user_location"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(WebSearchPreviewTool::new().user_location_null())
                .expect("serialize preview")["user_location"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(ImageGenerationTool::new().input_fidelity_null())
                .expect("serialize image tool")["input_fidelity"],
            Value::Null
        );

        let cleared = CreateResponseRequest::empty()
            .background_null()
            .conversation_null()
            .context_management_null()
            .include_null()
            .max_output_tokens_null()
            .max_tool_calls_null()
            .metadata_null()
            .moderation_null()
            .parallel_tool_calls_null()
            .previous_response_id_null()
            .prompt_null()
            .prompt_cache_key_null()
            .prompt_cache_retention_null()
            .reasoning_null()
            .store_null()
            .temperature_null()
            .top_p_null()
            .truncation_null();
        let cleared_value = serde_json::to_value(&cleared).expect("serialize official nulls");
        for key in [
            "background",
            "conversation",
            "context_management",
            "include",
            "max_output_tokens",
            "max_tool_calls",
            "metadata",
            "moderation",
            "parallel_tool_calls",
            "previous_response_id",
            "prompt",
            "prompt_cache_key",
            "prompt_cache_retention",
            "reasoning",
            "store",
            "temperature",
            "top_p",
            "truncation",
        ] {
            assert_eq!(cleared_value[key], Value::Null, "{key}");
        }
        assert!(cleared_value.get("prompt_cache_options").is_none());
        assert!(
            serde_json::from_value::<CreateResponseRequest>(json!({
                "model": "gpt-5.6-sol",
                "input": "hello",
                "prompt_cache_options": null
            }))
            .is_err()
        );

        let text = ResponseTextConfig::default().verbosity_null();
        assert_eq!(
            serde_json::to_value(&text).expect("serialize")["verbosity"],
            Value::Null
        );
        assert_eq!(
            serde_json::from_value::<ResponseTextConfig>(json!({ "verbosity": null }))
                .expect("official Verbosity includes null")
                .verbosity_ref(),
            None
        );
    }

    #[test]
    fn follow_up_from_copies_stable_prefix_fields() {
        let previous = CreateResponseRequest::new("gpt-5.6-sol", "first")
            .instructions("Stay concise.")
            .tool(FunctionTool::new("lookup"));
        let response = Response {
            id: "resp_1".into(),
            created_at: 1,
            error: Nullable::Null,
            incomplete_details: Nullable::Null,
            instructions: Nullable::Null,
            metadata: Nullable::Null,
            model: "gpt-5.6-sol".into(),
            object: ResponseObjectTag::Response,
            output: vec![],
            parallel_tool_calls: false,
            temperature: Nullable::Null,
            tool_choice: ToolChoice::Auto,
            tools: vec![],
            top_p: Nullable::Null,
            status: Omittable::Omitted,
            background: Omittable::Omitted,
            completed_at: Omittable::Omitted,
            conversation: Omittable::Omitted,
            max_output_tokens: Omittable::Omitted,
            max_tool_calls: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
            prompt: Omittable::Omitted,
            prompt_cache_key: Omittable::Omitted,
            prompt_cache_options: Omittable::Omitted,
            prompt_cache_retention: Omittable::Omitted,
            reasoning: Omittable::Omitted,
            safety_identifier: Omittable::Omitted,
            service_tier: Omittable::Omitted,
            store: Omittable::Omitted,
            text: Omittable::Omitted,
            top_logprobs: Omittable::Omitted,
            truncation: Omittable::Omitted,
            usage: Omittable::Omitted,
            user: Omittable::Omitted,
            moderation: Omittable::Omitted,
            extra: ExtraFields::new(),
        };
        let follow = CreateResponseRequest::follow_up_from(&previous, &response, "next");
        let value = serde_json::to_value(&follow).expect("serialize");
        assert_eq!(value["previous_response_id"], "resp_1");
        assert_eq!(value["instructions"], "Stay concise.");
        assert_eq!(value["tools"][0]["name"], "lookup");

        assert!(
            serde_json::from_value::<CreateResponseRequest>(json!({
                "model": "gpt-5.6-sol",
                "input": "hello",
                "instructions": [{"type": "message", "role": "developer", "content": "no"}]
            }))
            .is_err(),
            "create instructions are string or null, not item arrays"
        );
        assert!(
            serde_json::from_value::<CompactResponseRequest>(json!({
                "model": "gpt-5.6-sol",
                "instructions": [{"type": "message", "role": "developer", "content": "no"}]
            }))
            .is_err(),
            "compact instructions are string or null, not item arrays"
        );
    }

    #[test]
    fn follow_up_from_does_not_carry_conversation() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_conv",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "object": "response",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0,
            "status": "completed"
        }))
        .expect("stored response");
        let previous = CreateResponseRequest::new("gpt-5.6-sol", "first")
            .conversation("conv_1")
            .instructions("Stay concise.");

        let follow = CreateResponseRequest::follow_up_from(&previous, &response, "next");
        let value = serde_json::to_value(&follow).expect("serialize follow-up");
        assert_eq!(value["previous_response_id"], "resp_conv");
        assert_eq!(value["instructions"], "Stay concise.");
        assert!(
            value.get("conversation").is_none(),
            "the pin forbids previous_response_id together with conversation"
        );

        let streaming_previous = CreateStreamingResponseRequest::new("gpt-5.6-sol", "first")
            .conversation("conv_1")
            .instructions("Stay concise.");
        let streaming =
            CreateStreamingResponseRequest::follow_up_from(&streaming_previous, &response, "next");
        let value = serde_json::to_value(&streaming).expect("serialize streaming follow-up");
        assert_eq!(value["previous_response_id"], "resp_conv");
        assert!(
            value.get("conversation").is_none(),
            "streaming follow-ups drop the conversation reference too"
        );
    }

    #[test]
    fn input_item_list_limit_rejects_zero_on_build_and_decode() {
        let error = ListResponseInputItemsParams::new()
            .limit(0)
            .expect_err("zero page size");
        assert_eq!(error.actual(), 0);

        let params = ListResponseInputItemsParams::new()
            .limit(1)
            .expect("floor is inclusive")
            .order(ResponseItemOrder::Ascending);
        assert_eq!(
            serde_json::to_value(&params).expect("serialize params"),
            json!({"limit": 1, "order": "asc"})
        );

        // The pinned 1..=100 range is descriptive prose; values above the
        // ceiling stay acceptable (D0154/D0174 stance).
        let above_prose = ListResponseInputItemsParams::new()
            .limit(250)
            .expect("prose ceiling is not enforced");
        assert_eq!(
            serde_json::to_value(&above_prose).expect("serialize above prose"),
            json!({"limit": 250})
        );

        assert!(
            serde_json::from_value::<ListResponseInputItemsParams>(json!({"limit": 0})).is_err(),
            "decode rejects a zero page size"
        );
        let decoded: ListResponseInputItemsParams =
            serde_json::from_value(json!({"limit": 20})).expect("decode pinned default");
        let value = serde_json::to_value(&decoded).expect("serialize decoded");
        assert_eq!(value["limit"], 20);
        assert!(
            serde_json::from_value::<ListResponseInputItemsParams>(json!({})).is_ok(),
            "limit stays omittable"
        );
    }

    #[test]
    fn text_format_json_schema_strict_accepts_official_null() {
        let decoded: TextFormatJsonSchema = serde_json::from_value(json!({
            "type": "json_schema",
            "name": "weather",
            "schema": {"type": "object"},
            "strict": null
        }))
        .expect("official TextResponseFormatJsonSchema strict null");
        assert_eq!(decoded.is_strict(), None);
        assert_eq!(
            serde_json::to_value(&decoded).expect("encode")["strict"],
            Value::Null
        );
        assert_eq!(
            TextFormatJsonSchema::new("weather", json!({"type": "object"}))
                .strict(true)
                .is_strict(),
            Some(true)
        );
    }

    #[test]
    fn annotation_union_roundtrips_known_tags() {
        let citation = json!({
            "type": "url_citation",
            "url": "https://example.com",
            "start_index": 0,
            "end_index": 4,
            "title": "Example"
        });
        let decoded: Annotation = serde_json::from_value(citation.clone()).expect("decode");
        assert!(matches!(decoded, Annotation::UrlCitation(_)));
        assert_eq!(serde_json::to_value(&decoded).expect("serialize"), citation);
    }

    #[test]
    fn response_decodes_python_sdk_echo_fields() {
        let value = json!({
            "id": "resp_1",
            "object": "response",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0,
            "service_tier": null,
            "prompt_cache_key": "cache-1",
            "prompt_cache_options": { "mode": "implicit", "ttl": "30m" },
            "prompt_cache_retention": "24h",
            "top_logprobs": 5,
            "background": null,
            "truncation": null,
            "moderation": {
                "input": {
                    "type": "moderation_result",
                    "model": "omni-moderation-latest",
                    "flagged": false,
                    "categories": { "hate": false },
                    "category_scores": { "hate": 0.01 },
                    "category_applied_input_types": { "hate": ["text"] }
                },
                "output": {
                    "type": "error",
                    "code": "moderation_unavailable",
                    "message": "output skipped"
                }
            }
        });
        let response: Response = serde_json::from_value(value).expect("decode");
        assert_eq!(response.service_tier(), None);
        assert_eq!(response.prompt_cache_key(), Some("cache-1"));
        assert_eq!(
            response.prompt_cache_retention(),
            Some(&PromptCacheRetention::TwentyFourHours)
        );
        assert_eq!(response.top_logprobs(), Some(5));
        assert_eq!(response.background(), None);
        assert_eq!(response.truncation(), None);
        assert_eq!(
            serde_json::to_value(&response).expect("re-encode official nulls")["background"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(&response).expect("re-encode official nulls")["truncation"],
            Value::Null
        );
        let moderation = response.moderation().expect("typed moderation");
        assert!(matches!(
            moderation.input(),
            ResponseModerationOutcome::Result { flagged: false, .. }
        ));
        assert!(matches!(
            moderation.output(),
            ResponseModerationOutcome::Error { code, .. } if code == "moderation_unavailable"
        ));
    }

    #[test]
    fn response_store_null_echo_decodes_and_round_trips() {
        let value = json!({
            "id": "resp_store",
            "object": "response",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0,
            "store": null
        });
        let response: Response = serde_json::from_value(value.clone())
            .expect("unofficial store null echo must not fail the decode");
        assert_eq!(
            serde_json::to_value(&response).expect("re-encode store null")["store"],
            Value::Null
        );

        let omitted: Response = serde_json::from_value(json!({
            "id": "resp_store",
            "object": "response",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0
        }))
        .expect("decode without store");
        assert!(
            serde_json::to_value(&omitted)
                .expect("re-encode omitted store")
                .get("store")
                .is_none()
        );
    }

    #[test]
    fn create_request_service_tier_null_matches_openapi() {
        let request = CreateResponseRequest::new("gpt-5.6-sol", "hello").service_tier_null();
        let value = serde_json::to_value(&request).expect("serialize");
        assert_eq!(value["service_tier"], Value::Null);
        request.validate().expect("null service_tier is in range");
    }

    #[test]
    fn create_request_validate_enforces_pinned_limits() {
        let ok = CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .temperature(0.0)
            .top_p(1.0)
            .top_logprobs(20)
            .max_output_tokens(16)
            .safety_identifier("user-1")
            .metadata("k", "v");
        ok.validate().expect("boundary values are accepted");

        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .temperature(2.1)
                .validate(),
            Err(CreateResponseConstraintError::Temperature { .. })
        ));
        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .top_p(-0.1)
                .validate(),
            Err(CreateResponseConstraintError::TopP { .. })
        ));
        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .top_logprobs(21)
                .validate(),
            Err(CreateResponseConstraintError::TopLogprobs { actual: 21, .. })
        ));
        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .max_output_tokens(15)
                .validate(),
            Err(CreateResponseConstraintError::MaxOutputTokens { actual: 15, .. })
        ));
        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .safety_identifier("a".repeat(65))
                .validate(),
            Err(CreateResponseConstraintError::SafetyIdentifier { actual: 65, .. })
        ));
        CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .context_management(
                ContextManagement::compaction().compact_threshold(MIN_COMPACT_THRESHOLD),
            )
            .validate()
            .expect("compact_threshold 1000 is accepted");
        assert!(matches!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .context_management(ContextManagement::compaction().compact_threshold(999))
                .validate(),
            Err(CreateResponseConstraintError::CompactThreshold { actual: 999, .. })
        ));
        let decoded = serde_json::from_value::<CreateResponseRequest>(json!({
            "model": "gpt-5.6-sol",
            "input": "hello",
            "context_management": [{ "type": "compaction", "compact_threshold": 1 }]
        }))
        .expect("serde remains lossless");
        assert!(matches!(
            decoded.validate(),
            Err(CreateResponseConstraintError::CompactThreshold { actual: 1, .. })
        ));
        CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .context_management(ContextManagement::compaction().compact_threshold_null())
            .validate()
            .expect("official compact_threshold null skips the numeric bound");
    }

    #[test]
    fn create_request_validate_enforces_official_file_and_skill_payload_limits() {
        assert_eq!(MAX_INPUT_TEXT_CHARS, 10_485_760);
        validate_input_text_chars(MAX_INPUT_TEXT_CHARS)
            .expect("input_text at the official maxLength is accepted");
        assert!(matches!(
            validate_input_text_chars(MAX_INPUT_TEXT_CHARS + 1),
            Err(CreateResponseConstraintError::InputText {
                actual: 10_485_761,
                maximum: 10_485_760
            })
        ));
        InputText::new("hello")
            .validate()
            .expect("short input_text is accepted");
        CreateResponseRequest::new(
            "gpt-5.6-sol",
            ResponseInput::items([InputMessage::user(MessageContent::Parts(vec![
                InputText::new("hello").into(),
            ]))]),
        )
        .validate()
        .expect("create-response input_text walks text");

        assert_eq!(MIN_APPLY_PATCH_PATH_CHARS, 1);
        validate_apply_patch_path_chars(MIN_APPLY_PATCH_PATH_CHARS)
            .expect("one-character apply_patch path is accepted");
        assert!(matches!(
            validate_apply_patch_path_chars(0),
            Err(CreateResponseConstraintError::ApplyPatchPath {
                actual: 0,
                minimum: 1
            })
        ));
        assert!(matches!(
            ApplyPatchCallInput::new(
                "ap_1",
                ApplyPatchCallStatus::Completed,
                ApplyPatchOperation::DeleteFile(ApplyPatchDeleteFile::new("")),
            )
            .validate(),
            Err(CreateResponseConstraintError::ApplyPatchPath { actual: 0, .. })
        ));

        assert_eq!(MAX_APPLY_PATCH_DIFF_CHARS, 10_485_760);
        validate_apply_patch_diff_chars(MAX_APPLY_PATCH_DIFF_CHARS)
            .expect("apply_patch diff at the official maxLength is accepted");
        assert!(matches!(
            validate_apply_patch_diff_chars(MAX_APPLY_PATCH_DIFF_CHARS + 1),
            Err(CreateResponseConstraintError::ApplyPatchDiff {
                actual: 10_485_761,
                maximum: 10_485_760
            })
        ));
        ApplyPatchCallInput::new(
            "ap_1",
            ApplyPatchCallStatus::Completed,
            ApplyPatchOperation::CreateFile(ApplyPatchCreateFile::new(
                "README.md",
                "--- /dev/null\n+++ README.md\n",
            )),
        )
        .validate()
        .expect("short apply_patch diffs are accepted");

        assert_eq!(MAX_INPUT_IMAGE_URL_CHARS, 20_971_520);
        validate_input_image_url_chars(MAX_INPUT_IMAGE_URL_CHARS)
            .expect("image_url at the official maxLength is accepted");
        assert!(matches!(
            validate_input_image_url_chars(MAX_INPUT_IMAGE_URL_CHARS + 1),
            Err(CreateResponseConstraintError::InputImageUrl {
                actual: 20_971_521,
                maximum: 20_971_520
            })
        ));
        InputImage::from_url("https://example.test/a.png")
            .validate()
            .expect("short image_url is accepted");
        InputImage::from_file_id("file_1")
            .image_url_null()
            .validate()
            .expect("official image_url null skips the length bound");
        CreateResponseRequest::new(
            "gpt-5.6-sol",
            ResponseInput::items([InputMessage::user(MessageContent::Parts(vec![
                InputImage::from_url("https://example.test/a.png").into(),
            ]))]),
        )
        .validate()
        .expect("create-response input_image walks image_url");

        assert_eq!(MAX_INPUT_FILE_DATA_CHARS, 73_400_320);
        assert_eq!(MIN_INLINE_SKILL_SOURCE_DATA_CHARS, 1);
        assert_eq!(MAX_INLINE_SKILL_SOURCE_DATA_CHARS, 70_254_592);

        validate_input_file_data_chars(MAX_INPUT_FILE_DATA_CHARS)
            .expect("file_data at the official maxLength is accepted");
        assert!(matches!(
            validate_input_file_data_chars(MAX_INPUT_FILE_DATA_CHARS + 1),
            Err(CreateResponseConstraintError::InputFileData {
                actual: 73_400_321,
                maximum: 73_400_320
            })
        ));
        InputFile::from_base64("Zg==", "note.txt")
            .validate()
            .expect("short file_data is accepted");
        InputFile::from_file_id("file_1")
            .file_data_null()
            .validate()
            .expect("official file_data null skips the length bound");

        let file_request = CreateResponseRequest::new(
            "gpt-5.6-sol",
            ResponseInput::items([InputMessage::user(MessageContent::Parts(vec![
                InputFile::from_base64("Zg==", "note.txt").into(),
            ]))]),
        );
        file_request
            .validate()
            .expect("create-response input_file walks file_data");

        validate_inline_skill_source_data_chars(MIN_INLINE_SKILL_SOURCE_DATA_CHARS)
            .expect("one-character inline skill data is accepted");
        validate_inline_skill_source_data_chars(MAX_INLINE_SKILL_SOURCE_DATA_CHARS)
            .expect("inline skill data at the official maxLength is accepted");
        assert!(matches!(
            validate_inline_skill_source_data_chars(0),
            Err(CreateResponseConstraintError::InlineSkillSourceData {
                actual: 0,
                minimum: 1,
                maximum: 70_254_592
            })
        ));
        assert!(matches!(
            validate_inline_skill_source_data_chars(MAX_INLINE_SKILL_SOURCE_DATA_CHARS + 1),
            Err(CreateResponseConstraintError::InlineSkillSourceData {
                actual: 70_254_593,
                ..
            })
        ));

        let empty_inline =
            CreateResponseRequest::new("gpt-5.6-sol", "hello").tools([FunctionShellTool::new()
                .environment(FunctionShellEnvironment::ContainerAuto(
                    FunctionShellContainerAuto::new().skills(vec![ContainerSkill::Inline(
                        InlineSkill::new("lint", "Run lints", InlineSkillSource::zip("")),
                    )]),
                ))]);
        assert!(matches!(
            empty_inline.validate(),
            Err(CreateResponseConstraintError::InlineSkillSourceData { actual: 0, .. })
        ));
        CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .tools([
                FunctionShellTool::new().environment(FunctionShellEnvironment::ContainerAuto(
                    FunctionShellContainerAuto::new().skills(vec![ContainerSkill::Inline(
                        InlineSkill::new("lint", "Run lints", InlineSkillSource::zip("UEs=")),
                    )]),
                )),
            ])
            .validate()
            .expect("documented inline skill zip data is accepted");

        let decoded = serde_json::from_value::<CreateResponseRequest>(json!({
            "model": "gpt-5.6-sol",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_file",
                    "file_data": "Zg==",
                    "filename": "note.txt"
                }]
            }]
        }))
        .expect("serde remains lossless for file_data");
        decoded
            .validate()
            .expect("decoded in-range file_data is accepted");
    }

    #[test]
    fn websocket_create_validate_enforces_official_stream_id() {
        assert_eq!(MIN_STREAM_ID_CHARS, 1);
        assert_eq!(MAX_STREAM_ID_CHARS, 256);
        validate_websocket_stream_id("agent_1").expect("official example stream_id is accepted");
        validate_websocket_stream_id("agent.1").expect("dot and hyphen charset is accepted");
        validate_websocket_stream_id(&"a".repeat(MAX_STREAM_ID_CHARS))
            .expect("stream_id at official maxLength is accepted");
        assert!(matches!(
            validate_websocket_stream_id(""),
            Err(CreateResponseConstraintError::StreamId { actual: 0, .. })
        ));
        assert!(matches!(
            validate_websocket_stream_id(&"a".repeat(MAX_STREAM_ID_CHARS + 1)),
            Err(CreateResponseConstraintError::StreamId { actual: 257, .. })
        ));
        assert!(matches!(
            validate_websocket_stream_id("agent 1"),
            Err(CreateResponseConstraintError::StreamId { actual: 7, .. })
        ));

        ResponsesCreateEvent::new("gpt-5.6-sol", "hello")
            .validate()
            .expect("omitted stream_id skips the bound");
        ResponsesCreateEvent::new("gpt-5.6-sol", "hello")
            .stream_id("agent_1")
            .validate()
            .expect("documented create-event stream_id is accepted");
        assert!(matches!(
            ResponsesCreateEvent::new("gpt-5.6-sol", "hello")
                .stream_id("agent/1")
                .validate(),
            Err(CreateResponseConstraintError::StreamId { .. })
        ));
        let decoded = serde_json::from_value::<ResponsesCreateEvent>(json!({
            "type": "response.create",
            "stream_id": "bad id",
            "model": "gpt-5.6-sol",
            "input": "hello"
        }))
        .expect("serde remains lossless for unofficial stream_id");
        assert!(matches!(
            decoded.validate(),
            Err(CreateResponseConstraintError::StreamId { .. })
        ));
    }

    #[test]
    fn create_response_fields_match_python_and_openapi_inventory() {
        let request = CreateResponseRequest::new("gpt-5.6-sol", "hello")
            .instructions("Stay concise.")
            .background(true)
            .conversation("conv_1")
            .context_management(ContextManagement::compaction())
            .include(ResponseIncludable::FileSearchResults)
            .max_output_tokens(32)
            .max_tool_calls(3)
            .metadata("trace", "1")
            .moderation(ModerationConfig::new("omni-moderation-latest"))
            .parallel_tool_calls(true)
            .prompt_cache_key("cache")
            .prompt_cache_options(PromptCacheOptionsParam::new().thirty_minutes())
            .prompt_cache_retention(PromptCacheRetention::InMemory)
            .safety_identifier("safety")
            .service_tier(ServiceTier::Ultrafast)
            .store(false)
            .temperature(0.5)
            .top_logprobs(2)
            .top_p(0.9)
            .truncation(TruncationStrategy::Auto)
            .user("legacy-user");
        let value = serde_json::to_value(&request).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "background",
                "context_management",
                "conversation",
                "include",
                "input",
                "instructions",
                "max_output_tokens",
                "max_tool_calls",
                "metadata",
                "model",
                "moderation",
                "parallel_tool_calls",
                "prompt_cache_key",
                "prompt_cache_options",
                "prompt_cache_retention",
                "safety_identifier",
                "service_tier",
                "store",
                "temperature",
                "top_logprobs",
                "top_p",
                "truncation",
                "user",
            ]
        );
    }

    #[test]
    fn compact_request_fields_match_python_and_openapi_inventory() {
        let request = CompactResponseRequest::new("gpt-5.6-sol", "compact me")
            .instructions("Keep the gist.")
            .previous_response_id("resp_1")
            .prompt_cache_key("cache")
            .prompt_cache_options(PromptCacheOptionsParam::new().thirty_minutes())
            .prompt_cache_retention(PromptCacheRetention::InMemory)
            .service_tier_null();
        let value = serde_json::to_value(&request).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "input",
                "instructions",
                "model",
                "previous_response_id",
                "prompt_cache_key",
                "prompt_cache_options",
                "prompt_cache_retention",
                "service_tier"
            ]
        );
        assert_eq!(value["service_tier"], Value::Null);
        request.validate().expect("documented fields stay in range");

        let cleared = CompactResponseRequest::empty()
            .instructions_null()
            .previous_response_id_null()
            .prompt_cache_options_null()
            .prompt_cache_retention_null();
        let cleared_value = serde_json::to_value(&cleared).expect("serialize official nulls");
        assert_eq!(cleared_value["instructions"], Value::Null);
        assert_eq!(cleared_value["previous_response_id"], Value::Null);
        assert_eq!(cleared_value["prompt_cache_options"], Value::Null);
        assert_eq!(cleared_value["prompt_cache_retention"], Value::Null);

        let null_model = CompactResponseRequest::empty().model_null();
        assert_eq!(
            serde_json::to_value(&null_model).expect("serialize")["model"],
            Value::Null
        );
        let decoded = serde_json::from_value::<CompactResponseRequest>(json!({
            "model": null,
            "input": "hello"
        }))
        .expect("official ModelIdsCompaction includes null");
        assert_eq!(decoded.model, Nullable::Null);
        assert_eq!(
            serde_json::to_value(CompactResponseRequest::empty()).expect("serialize empty")["model"],
            Value::Null
        );
    }

    #[test]
    fn count_input_tokens_request_sends_official_fields() {
        let request = CountInputTokensRequest::empty()
            .model_null()
            .input_null()
            .conversation_null()
            .previous_response_id("resp_1")
            .parallel_tool_calls(false)
            .reasoning(ReasoningConfig::new())
            .text(ResponseTextConfig::default())
            .truncation(TruncationStrategy::Auto);
        let value = serde_json::to_value(&request).expect("serialize");
        assert_eq!(value["model"], Value::Null);
        assert_eq!(value["input"], Value::Null);
        assert_eq!(value["conversation"], Value::Null);
        assert_eq!(value["previous_response_id"], "resp_1");
        assert_eq!(value["parallel_tool_calls"], false);
        assert_eq!(value["truncation"], "auto");
        assert!(value.get("reasoning").is_some());
        assert!(value.get("text").is_some());
        request.validate().expect("null input stays in range");
    }

    #[test]
    fn compact_request_validate_enforces_prompt_cache_key_limit() {
        CompactResponseRequest::new("gpt-5.6-sol", "hello")
            .prompt_cache_key("a".repeat(MAX_PROMPT_CACHE_KEY_CHARS))
            .validate()
            .expect("64-character key is accepted");
        assert!(matches!(
            CompactResponseRequest::new("gpt-5.6-sol", "hello")
                .prompt_cache_key("a".repeat(MAX_PROMPT_CACHE_KEY_CHARS + 1))
                .validate(),
            Err(CompactResponseConstraintError::PromptCacheKey { actual: 65, .. })
        ));

        let decoded = serde_json::from_value::<CompactResponseRequest>(json!({
            "model": "gpt-5.6-sol",
            "input": null,
            "prompt_cache_key": "a".repeat(65),
            "service_tier": null
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());
    }

    fn unofficial_empty_allowed_callers_tool() -> ResponseTool {
        ResponseTool::from(FunctionTool::new("lookup").allowed_callers(Vec::<String>::new()))
    }

    #[test]
    fn compact_and_count_validate_walk_official_input_items_and_tools() {
        let extra_tools = AdditionalToolsInput::new(vec![unofficial_empty_allowed_callers_tool()]);
        extra_tools
            .validate()
            .expect_err("empty allowed_callers is unofficial");
        assert!(matches!(
            extra_tools.validate(),
            Err(CreateResponseConstraintError::EmptyAllowedCallers)
        ));
        assert!(
            CreateResponseRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::AdditionalTools(extra_tools.clone())]
            )
            .validate()
            .is_err()
        );

        let search = ToolSearchOutputInput::new(vec![unofficial_empty_allowed_callers_tool()]);
        assert!(matches!(
            search.validate(),
            Err(CreateResponseConstraintError::EmptyAllowedCallers)
        ));
        assert!(
            CreateResponseRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::ToolSearchOutput(search.clone())]
            )
            .validate()
            .is_err()
        );

        assert!(matches!(
            CompactResponseRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::AdditionalTools(extra_tools)]
            )
            .validate(),
            Err(CompactResponseConstraintError::Input(
                CreateResponseConstraintError::EmptyAllowedCallers
            ))
        ));
        assert!(matches!(
            CountInputTokensRequest::empty()
                .tool(unofficial_empty_allowed_callers_tool())
                .validate(),
            Err(CountInputTokensConstraintError::Input(
                CreateResponseConstraintError::EmptyAllowedCallers
            ))
        ));
        assert!(matches!(
            CountInputTokensRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::ToolSearchOutput(search)]
            )
            .validate(),
            Err(CountInputTokensConstraintError::Input(
                CreateResponseConstraintError::EmptyAllowedCallers
            ))
        ));
        CompactResponseRequest::new("gpt-5.6-sol", "hello")
            .validate()
            .expect("text input stays in range");
        CountInputTokensRequest::new("gpt-5.6-sol", "hello")
            .tool(FunctionTool::new("lookup"))
            .validate()
            .expect("in-range tools stay accepted");
    }

    #[test]
    fn response_usage_decodes_compute_units() {
        let value = json!({
            "input_tokens": 10,
            "input_tokens_details": { "cached_tokens": 0, "cache_write_tokens": 0 },
            "output_tokens": 4,
            "output_tokens_details": { "reasoning_tokens": 0 },
            "total_tokens": 14,
            "compute_units": null
        });
        let usage: ResponseUsage = serde_json::from_value(value.clone()).expect("decode");
        assert_eq!(usage.compute_units(), None);
        assert!(!usage.extra_fields().contains_key("compute_units"));
        assert_eq!(serde_json::to_value(&usage).expect("re-encode"), value);

        let counted: ResponseUsage = serde_json::from_value(json!({
            "input_tokens": 10,
            "input_tokens_details": { "cached_tokens": 0, "cache_write_tokens": 0 },
            "output_tokens": 4,
            "output_tokens_details": { "reasoning_tokens": 0 },
            "total_tokens": 14,
            "compute_units": 3
        }))
        .expect("decode counted units");
        assert_eq!(counted.compute_units(), Some(3));
    }

    #[test]
    fn official_response_usage_requires_cache_write_tokens() {
        let official = json!({
            "input_tokens": 139,
            "input_tokens_details": {
                "cached_tokens": 0,
                "cache_write_tokens": 0
            },
            "output_tokens": 438,
            "output_tokens_details": { "reasoning_tokens": 64 },
            "total_tokens": 577
        });
        let usage: ResponseUsage =
            serde_json::from_value(official.clone()).expect("official compact usage");
        assert_eq!(usage.input_tokens_details().cached_tokens(), 0);
        assert_eq!(usage.input_tokens_details().cache_write_tokens(), 0);
        assert_eq!(
            serde_json::to_value(&usage).expect("re-encode official usage"),
            official
        );
        assert!(
            serde_json::from_value::<ResponseUsage>(json!({
                "input_tokens": 10,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens": 4,
                "output_tokens_details": { "reasoning_tokens": 0 },
                "total_tokens": 14
            }))
            .is_err(),
            "official required cache_write_tokens must not be omitted"
        );
    }

    #[test]
    fn official_response_prompt_cache_options_requires_ttl_and_mode() {
        let official = json!({
            "mode": "implicit",
            "ttl": "30m"
        });
        let options: PromptCacheOptions =
            serde_json::from_value(official.clone()).expect("official response echo");
        assert_eq!(options.mode(), &PromptCacheMode::Implicit);
        assert_eq!(options.ttl(), &PromptCacheTtl::ThirtyMinutes);
        assert_eq!(
            serde_json::to_value(&options).expect("re-encode official echo"),
            official
        );
        assert!(
            serde_json::from_value::<PromptCacheOptions>(json!({ "mode": "implicit" })).is_err(),
            "official PromptCacheOptions requires ttl"
        );
        assert!(
            serde_json::from_value::<PromptCacheOptions>(json!({ "ttl": "30m" })).is_err(),
            "official PromptCacheOptions requires mode"
        );
        let param: PromptCacheOptionsParam =
            serde_json::from_value(json!({ "ttl": "30m" })).expect("request param may omit mode");
        assert_eq!(param.ttl_ref(), Some(&PromptCacheTtl::ThirtyMinutes));
        assert_eq!(param.mode_ref(), None);
    }

    #[test]
    fn official_compact_resource_requires_usage() {
        let official = json!({
            "id": "resp_001",
            "object": "response.compaction",
            "created_at": 1_764_967_971_i64,
            "output": [],
            "usage": {
                "input_tokens": 139,
                "input_tokens_details": {
                    "cached_tokens": 0,
                    "cache_write_tokens": 0
                },
                "output_tokens": 438,
                "output_tokens_details": { "reasoning_tokens": 64 },
                "total_tokens": 577
            }
        });
        let compacted: CompactedResponse =
            serde_json::from_value(official).expect("official CompactResource");
        assert_eq!(compacted.id(), "resp_001");
        assert_eq!(compacted.usage().input_tokens(), 139);
        assert_eq!(
            compacted
                .usage()
                .input_tokens_details()
                .cache_write_tokens(),
            0
        );
        assert!(
            serde_json::from_value::<CompactedResponse>(json!({
                "id": "resp_001",
                "object": "response.compaction",
                "created_at": 1,
                "output": []
            }))
            .is_err(),
            "official CompactResource requires usage"
        );
        assert!(
            serde_json::from_value::<CompactedResponse>(json!({
                "id": "resp_001",
                "object": "response.compaction",
                "created_at": 1,
                "output": [],
                "usage": null
            }))
            .is_err(),
            "official CompactResource.usage is a required non-null ResponseUsage"
        );
    }

    #[test]
    fn official_compact_resource_output_decodes_user_messages_and_compaction_item() {
        // Pinned OpenAPI example for CompactResource, including the user-role
        // messages that the assistant-only ResponseOutputItem codec rejected
        // before the ItemField-equivalent input union was used. `usage` is
        // completed with the schema-required token-detail objects, which the
        // abbreviated spec example omits.
        let official = json!({
            "id": "resp_001",
            "object": "response.compaction",
            "output": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "Summarize our launch checklist from last week."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "You are performing a CONTEXT CHECKPOINT COMPACTION..."
                        }
                    ]
                },
                {
                    "type": "compaction",
                    "id": "cmp_001",
                    "encrypted_content": "encrypted-summary"
                }
            ],
            "created_at": 1731459200,
            "usage": {
                "input_tokens": 42897,
                "input_tokens_details": { "cached_tokens": 0, "cache_write_tokens": 0 },
                "output_tokens": 12000,
                "output_tokens_details": { "reasoning_tokens": 0 },
                "total_tokens": 54912
            }
        });
        let compacted: CompactedResponse =
            serde_json::from_value(official.clone()).expect("official CompactResource example");
        assert_eq!(compacted.id(), "resp_001");
        let output = compacted.output();
        assert_eq!(output.len(), 3);
        for item in &output[..2] {
            assert!(
                matches!(item, ResponseInputItem::StoredMessage(_)),
                "user message must decode as a stored input message: {item:?}"
            );
            assert_eq!(
                serde_json::to_value(item).expect("re-encode user message")["role"],
                "user"
            );
        }
        assert!(matches!(output[2], ResponseInputItem::Compaction(_)));
        assert_eq!(
            serde_json::to_value(&output[2]).expect("re-encode compaction item")["id"],
            "cmp_001"
        );
        assert_eq!(
            serde_json::to_value(&compacted).expect("round-trip official example"),
            official
        );
    }

    #[test]
    fn compact_output_stored_messages_decode_multi_agent_roles_losslessly() {
        // Compaction can echo multi-agent roles (critic/tool/discriminator)
        // in stored messages. The pinned ItemField Message branch accepts any
        // MessageRole, so decoding must keep unknown roles verbatim instead
        // of failing the 200 body; request construction still only exposes
        // the three StoredInputMessageRole values.
        let fixture = json!({
            "id": "resp_roles",
            "object": "response.compaction",
            "output": [
                {
                    "type": "message",
                    "role": "critic",
                    "content": [{"type": "input_text", "text": "critique"}]
                },
                {
                    "type": "message",
                    "role": "tool",
                    "content": [{"type": "input_text", "text": "tool output"}]
                },
                {
                    "type": "message",
                    "role": "future_role",
                    "content": [{"type": "input_text", "text": "forward compatible"}]
                },
                {"type": "compaction", "id": "cmp_2", "encrypted_content": "enc"}
            ],
            "created_at": 1731459200,
            "usage": {
                "input_tokens": 1,
                "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
                "output_tokens": 1,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": 2
            }
        });
        let compacted: CompactedResponse =
            serde_json::from_value(fixture.clone()).expect("multi-agent stored roles decode");
        let output = compacted.output();
        for (item, expected) in output.iter().zip(["critic", "tool", "future_role"]) {
            let ResponseInputItem::StoredMessage(message) = item else {
                panic!("role-bearing item must decode as a stored message: {item:?}");
            };
            assert_eq!(message.role().as_str(), expected);
        }
        assert_eq!(
            serde_json::to_value(&compacted).expect("round-trip keeps roles verbatim"),
            fixture
        );
        assert_eq!(
            serde_json::to_value(StoredInputMessage::new(
                StoredInputMessageRole::Developer,
                [InputText::new("instructions")],
            ))
            .expect("request construction keeps the pinned role domain")["role"],
            "developer"
        );
    }

    #[test]
    fn official_response_item_list_requires_cursor_ids() {
        assert!(
            serde_json::from_value::<ResponseInputItemList>(json!({
                "object": "list",
                "data": [],
                "has_more": false
            }))
            .is_err(),
            "official ResponseItemList requires first_id and last_id"
        );
        assert!(
            serde_json::from_value::<ResponseInputItemList>(json!({
                "object": "list",
                "data": [],
                "first_id": null,
                "last_id": null,
                "has_more": false
            }))
            .is_err(),
            "official ResponseItemList cursors are required non-null strings"
        );
        let empty = serde_json::from_value::<ResponseInputItemList>(json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "",
            "has_more": false
        }))
        .expect("empty official string cursors decode");
        assert_eq!(empty.first_id(), "");
        assert_eq!(empty.last_id(), "");
    }

    #[test]
    fn official_compact_request_requires_model() {
        assert!(
            serde_json::from_value::<CompactResponseRequest>(json!({})).is_err(),
            "official CompactResponseMethodPublicBody requires model"
        );
        assert!(
            serde_json::from_value::<CompactResponseRequest>(json!({ "input": "hello" })).is_err(),
            "omitting model is unofficial even when input is present"
        );
        let official_null = serde_json::from_value::<CompactResponseRequest>(json!({
            "model": null
        }))
        .expect("official ModelIdsCompaction includes null");
        assert_eq!(official_null.model, Nullable::Null);
        assert_eq!(
            serde_json::to_value(&official_null).expect("re-encode required null")["model"],
            Value::Null
        );
        let official = serde_json::from_value::<CompactResponseRequest>(json!({
            "model": "gpt-5.6-sol",
            "input": "compact me"
        }))
        .expect("official compact request");
        assert_eq!(official.model, Nullable::Value("gpt-5.6-sol".into()));
    }

    #[test]
    fn official_function_shell_action_param_omits_limits() {
        let official = json!({ "commands": ["echo"] });
        let param: FunctionShellActionParam =
            serde_json::from_value(official.clone()).expect("official FunctionShellActionParam");
        assert_eq!(param.commands(), ["echo"]);
        assert_eq!(param.timeout_ms_ref(), None);
        assert_eq!(param.max_output_length_ref(), None);
        assert_eq!(
            serde_json::to_value(&param).expect("re-encode omitted limits"),
            official
        );
        let item: FunctionShellCallInput = serde_json::from_value(json!({
            "type": "shell_call",
            "call_id": "s1",
            "action": { "commands": ["uname"] }
        }))
        .expect("official FunctionShellCallItemParam action");
        assert_eq!(item.action().commands(), ["uname"]);
        let action = serde_json::to_value(item.action()).expect("input action omits limits");
        assert!(
            !action
                .as_object()
                .expect("object")
                .contains_key("timeout_ms")
        );
        assert!(
            !action
                .as_object()
                .expect("object")
                .contains_key("max_output_length")
        );
        assert!(
            serde_json::from_value::<FunctionShellAction>(json!({ "commands": ["echo"] })).is_err(),
            "official FunctionShellAction requires timeout_ms and max_output_length"
        );
        let resource: FunctionShellAction = serde_json::from_value(json!({
            "commands": ["echo"],
            "timeout_ms": null,
            "max_output_length": null
        }))
        .expect("official FunctionShellAction required nulls");
        assert_eq!(resource.commands(), ["echo"]);
        assert_eq!(
            serde_json::to_value(&resource).expect("re-encode resource")["timeout_ms"],
            Value::Null
        );
    }

    #[test]
    fn official_output_content_names_reasoning_text() {
        let official = json!({
            "type": "reasoning_text",
            "text": "The user is asking..."
        });
        let decoded: OutputContent = serde_json::from_value(official.clone())
            .expect("official OutputContent reasoning_text");
        match &decoded {
            OutputContent::ReasoningText(part) => {
                assert_eq!(part.text(), "The user is asking...");
            }
            other => panic!("official reasoning_text must be named, got {other:?}"),
        }
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode reasoning_text"),
            official
        );

        let added: ContentPartAddedEvent = serde_json::from_value(json!({
            "type": "response.content_part.added",
            "item_id": "rs_123",
            "output_index": 0,
            "content_index": 0,
            "part": {
                "type": "reasoning_text",
                "text": "The"
            },
            "sequence_number": 1
        }))
        .expect("official content_part.added reasoning_text");
        assert!(matches!(
            added.part(),
            OutputContent::ReasoningText(part) if part.text() == "The"
        ));

        let done: ContentPartDoneEvent = serde_json::from_value(json!({
            "type": "response.content_part.done",
            "item_id": "rs_123",
            "output_index": 0,
            "content_index": 0,
            "part": {
                "type": "reasoning_text",
                "text": "The user is asking..."
            },
            "sequence_number": 4
        }))
        .expect("official content_part.done reasoning_text");
        assert!(matches!(done.part(), OutputContent::ReasoningText(_)));
    }

    #[test]
    fn official_input_image_content_requires_detail() {
        let official = json!({
            "type": "input_image",
            "image_url": "https://example.test/a.png",
            "detail": "high"
        });
        let decoded: InputImage =
            serde_json::from_value(official.clone()).expect("official InputImageContent");
        assert_eq!(decoded.detail_ref(), &ImageDetail::High);
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode")["detail"],
            "high"
        );
        assert!(
            serde_json::from_value::<InputImage>(json!({
                "type": "input_image",
                "image_url": "https://example.test/a.png"
            }))
            .is_err(),
            "official InputImageContent requires detail"
        );
        assert!(
            serde_json::from_value::<InputImage>(json!({
                "type": "input_image",
                "image_url": "https://example.test/a.png",
                "detail": null
            }))
            .is_err(),
            "official InputImageContent detail is not nullable"
        );
        assert_eq!(
            serde_json::to_value(InputImage::from_url("https://example.test/a.png"))
                .expect("constructor sends documented default")["detail"],
            "auto"
        );
    }

    #[test]
    fn official_function_call_output_param_omits_image_detail() {
        let official = json!({
            "type": "function_call_output",
            "call_id": "call_1",
            "output": [
                {
                    "type": "input_image",
                    "image_url": "https://example.test/a.png"
                }
            ]
        });
        let decoded: FunctionCallOutput = serde_json::from_value(official.clone())
            .expect("official FunctionCallOutputItemParam image omits detail");
        match decoded.output() {
            FunctionCallOutputParamValue::Content(parts) => match &parts[0] {
                FunctionCallOutputContent::Image(image) => {
                    assert_eq!(image.image_url(), Some("https://example.test/a.png"));
                    assert_eq!(
                        serde_json::to_value(image)
                            .expect("re-encode param image")
                            .get("detail"),
                        None
                    );
                }
                other => panic!("expected InputImageParam, got {other:?}"),
            },
            other => panic!("expected content output, got {other:?}"),
        }
        assert_eq!(
            serde_json::to_value(InputImageParam::from_url("https://example.test/a.png"))
                .expect("param constructor omits detail")
                .get("detail"),
            None
        );
        let with_null = serde_json::from_value::<FunctionCallOutput>(json!({
            "type": "function_call_output",
            "output": [{
                "type": "input_image",
                "image_url": "https://example.test/a.png",
                "detail": null
            }]
        }))
        .expect("official Param detail null");
        assert!(matches!(
            with_null.output(),
            FunctionCallOutputParamValue::Content(parts)
                if matches!(&parts[0], FunctionCallOutputContent::Image(_))
        ));
        assert!(
            serde_json::from_value::<InputImage>(json!({
                "type": "input_image",
                "image_url": "https://example.test/a.png"
            }))
            .is_err(),
            "resource InputImageContent still requires detail"
        );
    }

    #[test]
    fn official_file_search_ranker_matches_ranker_version_type() {
        const OFFICIAL_RANKER_VERSION: [&str; 2] = ["auto", "default-2024-11-15"];
        for value in OFFICIAL_RANKER_VERSION {
            let decoded = FileSearchRanker::from_raw(value);
            assert!(
                decoded.is_known(),
                "official RankerVersionType value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
        }
        assert!(
            !FileSearchRanker::from_raw("default_2024_08_21").is_known(),
            "Assistants FileSearchRanker is not a named Responses RankerVersionType"
        );
        assert_eq!(
            serde_json::to_value(FileSearchRankingOptions::new().ranker(FileSearchRanker::Auto))
                .expect("ranker")["ranker"],
            "auto"
        );
    }

    #[test]
    fn official_response_error_code_matches_response_error_code() {
        const OFFICIAL_CODES: [&str; 20] = [
            "server_error",
            "rate_limit_exceeded",
            "invalid_prompt",
            "data_residency_mismatch",
            "bio_policy",
            "vector_store_timeout",
            "invalid_image",
            "invalid_image_format",
            "invalid_base64_image",
            "invalid_image_url",
            "image_too_large",
            "image_too_small",
            "image_parse_error",
            "image_content_policy_violation",
            "invalid_image_mode",
            "image_file_too_large",
            "unsupported_image_media_type",
            "empty_image_file",
            "failed_to_download_image",
            "image_file_not_found",
        ];
        for value in OFFICIAL_CODES {
            let decoded = ResponseErrorCode::from_raw(value);
            assert!(
                decoded.is_known(),
                "official ResponseErrorCode value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let error: ResponseError = serde_json::from_value(json!({
                "code": value,
                "message": "failed"
            }))
            .unwrap_or_else(|error| panic!("official error code {value} must decode: {error}"));
            assert_eq!(error.code().as_str(), value);
            assert_eq!(
                serde_json::to_value(&error).expect("re-encode")["code"],
                value
            );
        }
        assert!(
            !ResponseErrorCode::from_raw("ERR_SOMETHING").is_known(),
            "stream ErrorPayload free-form codes are not ResponseErrorCode members"
        );
        let future: ResponseError = serde_json::from_value(json!({
            "code": "future_error",
            "message": "failed"
        }))
        .expect("unofficial codes remain lossless");
        assert_eq!(future.code().as_str(), "future_error");
        assert!(!future.code().is_known());
    }

    #[test]
    fn official_allowed_caller_matches_callable_tool_allowed_caller() {
        const OFFICIAL_CALLERS: [&str; 2] = ["direct", "programmatic"];
        for value in OFFICIAL_CALLERS {
            let decoded = AllowedCaller::from_raw(value);
            assert!(
                decoded.is_known(),
                "official CallableToolAllowedCaller value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
        }
        assert!(
            !AllowedCaller::from_raw("unknown_caller").is_known(),
            "unofficial callers remain Unknown"
        );
        let tool = FunctionTool::new("lookup").allowed_callers([AllowedCaller::Direct]);
        assert_eq!(
            serde_json::to_value(&tool).expect("serialize")["allowed_callers"],
            json!(["direct"])
        );
        let decoded: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "lookup",
            "allowed_callers": ["programmatic"]
        }))
        .expect("official programmatic caller");
        let value = serde_json::to_value(&decoded).expect("re-encode");
        assert_eq!(value["allowed_callers"], json!(["programmatic"]));
    }

    #[test]
    fn official_mcp_connector_id_matches_connector_enum() {
        const OFFICIAL_CONNECTORS: [&str; 8] = [
            "connector_dropbox",
            "connector_gmail",
            "connector_googlecalendar",
            "connector_googledrive",
            "connector_microsoftteams",
            "connector_outlookcalendar",
            "connector_outlookemail",
            "connector_sharepoint",
        ];
        for value in OFFICIAL_CONNECTORS {
            let decoded = McpConnectorId::from_raw(value);
            assert!(
                decoded.is_known(),
                "official MCP connector_id value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let tool = McpTool::connector("mail", value);
            assert_eq!(
                serde_json::to_value(&tool).expect("serialize")["connector_id"],
                value
            );
        }
        assert!(
            !McpConnectorId::from_raw("connector_future").is_known(),
            "unofficial connector ids remain Unknown"
        );
        let future = McpTool::connector("mail", "connector_future");
        assert_eq!(
            serde_json::to_value(&future).expect("unofficial connector")["connector_id"],
            "connector_future"
        );
    }

    #[test]
    fn official_input_file_content_uses_file_detail_domain() {
        const OFFICIAL_FILE_DETAIL: [&str; 3] = ["auto", "low", "high"];
        for value in OFFICIAL_FILE_DETAIL {
            let decoded = FileDetail::from_raw(value);
            assert!(
                decoded.is_known(),
                "official FileInputDetail value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let file: InputFile = serde_json::from_value(json!({
                "type": "input_file",
                "file_id": "file_1",
                "detail": value
            }))
            .unwrap_or_else(|error| panic!("official file detail {value} must decode: {error}"));
            assert_eq!(
                serde_json::to_value(&file).expect("re-encode")["detail"],
                value
            );
        }
        assert!(
            !FileDetail::from_raw("original").is_known(),
            "original is official ImageDetail, not FileInputDetail"
        );
        assert!(ImageDetail::from_raw("original").is_known());
        assert_eq!(
            serde_json::to_value(InputFile::from_file_id("file_1").detail(FileDetail::Low))
                .expect("setter")["detail"],
            "low"
        );
    }

    #[test]
    fn message_phase_roundtrips_on_input_and_output() {
        let input = InputMessage::user("hello")
            .with_type()
            .phase(MessagePhase::Commentary);
        let value = serde_json::to_value(&input).expect("serialize input");
        assert_eq!(value["phase"], "commentary");
        let decoded: InputMessage = serde_json::from_value(value).expect("decode input");
        assert_eq!(decoded.phase_ref(), Some(&MessagePhase::Commentary));

        let output = OutputMessage::new(
            "msg_1",
            MessageStatus::Completed,
            [OutputContent::from(OutputText::new("done"))],
        )
        .phase(MessagePhase::FinalAnswer);
        let value = serde_json::to_value(&output).expect("serialize output");
        assert_eq!(value["phase"], "final_answer");
        let decoded: OutputMessage = serde_json::from_value(value).expect("decode output");
        assert_eq!(decoded.phase_ref(), Some(&MessagePhase::FinalAnswer));
        assert!(!decoded.extra_fields().contains_key("phase"));
        assert_eq!(
            serde_json::to_value(
                OutputMessage::new(
                    "msg_2",
                    MessageStatus::Completed,
                    [OutputContent::from(OutputText::new("done"))],
                )
                .phase_null()
            )
            .expect("serialize output phase null")["phase"],
            Value::Null
        );
        let official_null: OutputMessage = serde_json::from_value(json!({
            "type": "message",
            "id": "msg_3",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "done", "annotations": [], "logprobs": []}],
            "phase": null
        }))
        .expect("official Message.phase null");
        assert_eq!(official_null.phase_ref(), None);
    }

    #[test]
    fn file_search_tool_fields_match_python_and_openapi_inventory() {
        let tool = FileSearchTool::new(["vs_1"])
            .max_num_results(8)
            .ranking_options(
                FileSearchRankingOptions::new()
                    .ranker(FileSearchRanker::Auto)
                    .score_threshold(0.4)
                    .hybrid_search(FileSearchHybridSearch::new(0.7, 0.3)),
            )
            .filters_null();
        let value = serde_json::to_value(&tool).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "filters",
                "max_num_results",
                "ranking_options",
                "type",
                "vector_store_ids"
            ]
        );
        assert_eq!(value["filters"], Value::Null);
        assert_eq!(
            value["ranking_options"]["hybrid_search"]["embedding_weight"],
            0.7
        );
    }

    #[test]
    fn file_search_tool_validate_enforces_pinned_limits() {
        FileSearchTool::new(["vs_1"])
            .max_num_results(MAX_FILE_SEARCH_RESULTS)
            .ranking_options(FileSearchRankingOptions::new().score_threshold(1.0))
            .validate()
            .expect("boundary values are accepted");

        assert!(matches!(
            FileSearchTool::new(["vs_1"]).max_num_results(0).validate(),
            Err(CreateResponseConstraintError::FileSearchMaxResults { actual: 0, .. })
        ));
        assert!(matches!(
            FileSearchTool::new(["vs_1"])
                .ranking_options(FileSearchRankingOptions::new().score_threshold(1.1))
                .validate(),
            Err(CreateResponseConstraintError::FileSearchScoreThreshold { .. })
        ));

        let decoded: FileSearchTool = serde_json::from_value(json!({
            "type": "file_search",
            "vector_store_ids": ["vs_1"],
            "max_num_results": 0,
            "ranking_options": { "score_threshold": 1.5 }
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());
        assert!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .tool(decoded)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn code_interpreter_tool_sends_allowed_callers() {
        let tool = CodeInterpreterTool::container_id("cntr_1").allowed_callers(["direct"]);
        let value = serde_json::to_value(&tool).expect("serialize");
        assert_eq!(value["allowed_callers"], json!(["direct"]));
        let null = CodeInterpreterTool::container_id("cntr_1").allowed_callers_null();
        assert_eq!(
            serde_json::to_value(&null).expect("serialize null")["allowed_callers"],
            Value::Null
        );
    }

    #[test]
    fn web_search_and_image_tools_match_python_and_openapi_inventory() {
        let search = WebSearchTool::new()
            .external_web_access(false)
            .filters(WebSearchFilters::allowed_domains([
                "pubmed.ncbi.nlm.nih.gov",
            ]))
            .user_location(WebSearchUserLocation::new().country("US").city("Boston"))
            .search_context_size(WebSearchContextSize::High);
        let location_nulls = WebSearchUserLocation::new()
            .country_null()
            .region_null()
            .city_null()
            .timezone_null();
        assert_eq!(
            serde_json::to_value(&location_nulls).expect("serialize location nulls"),
            json!({
                "type": "approximate",
                "country": null,
                "region": null,
                "city": null,
                "timezone": null
            })
        );
        assert_eq!(
            serde_json::from_value::<WebSearchUserLocation>(json!({
                "type": "approximate",
                "country": null,
                "region": null,
                "city": null,
                "timezone": null
            }))
            .expect("official ApproximateLocation nulls"),
            location_nulls
        );
        let value = serde_json::to_value(&search).expect("serialize search");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "external_web_access",
                "filters",
                "search_context_size",
                "type",
                "user_location"
            ]
        );
        assert_eq!(value["type"], "web_search");

        let dated = WebSearchTool::web_search_2025_08_26();
        assert_eq!(
            serde_json::to_value(&dated).expect("serialize dated web search")["type"],
            "web_search_2025_08_26"
        );
        let decoded_dated: ResponseTool = serde_json::from_value(json!({
            "type": "web_search_2025_08_26",
            "search_context_size": "high"
        }))
        .expect("official dated web search tool");
        match decoded_dated {
            ResponseTool::WebSearch(tool) => {
                assert_eq!(tool.kind(), &WebSearchToolTag::WebSearch20250826);
                assert_eq!(
                    tool.search_context_size,
                    Omittable::Value(WebSearchContextSize::High)
                );
            }
            other => panic!("dated web search must stay typed, got {other:?}"),
        }
        // `web_search_2025_08_26` is a valid request-tool type but is outside
        // the pinned ToolChoiceTypes.type domain, so it decodes through the
        // open-enum Unknown branch instead of a named HostedToolType variant.
        let choice: ToolChoice = serde_json::from_value(json!({
            "type": "web_search_2025_08_26"
        }))
        .expect("unofficial dated hosted tool_choice stays lossless");
        match choice {
            ToolChoice::Hosted(hosted) => {
                assert_eq!(hosted.kind().unknown_value(), Some("web_search_2025_08_26"));
                assert!(!hosted.kind().is_known());
                assert_eq!(hosted.kind().as_str(), "web_search_2025_08_26");
            }
            other => panic!("dated web search choice must stay hosted, got {other:?}"),
        }

        let preview = WebSearchPreviewTool::new()
            .search_context_size(WebSearchContextSize::Low)
            .search_content_types([WebSearchContentType::Text, WebSearchContentType::Image]);
        let value = serde_json::to_value(&preview).expect("serialize preview");
        assert_eq!(value["type"], "web_search_preview");
        assert_eq!(value["search_content_types"], json!(["text", "image"]));

        let dated_preview = WebSearchPreviewTool::web_search_preview_2025_03_11();
        assert_eq!(
            serde_json::to_value(&dated_preview).expect("serialize dated preview")["type"],
            "web_search_preview_2025_03_11"
        );
        let decoded_dated_preview: ResponseTool = serde_json::from_value(json!({
            "type": "web_search_preview_2025_03_11",
            "search_context_size": "low"
        }))
        .expect("official dated preview web search tool");
        match decoded_dated_preview {
            ResponseTool::WebSearchPreview(tool) => {
                assert_eq!(
                    tool.kind(),
                    &WebSearchPreviewToolTag::WebSearchPreview20250311
                );
                assert_eq!(
                    tool.search_context_size,
                    Omittable::Value(WebSearchContextSize::Low)
                );
            }
            other => panic!("dated preview web search must stay typed, got {other:?}"),
        }
        let preview_choice: ToolChoice = serde_json::from_value(json!({
            "type": "web_search_preview_2025_03_11"
        }))
        .expect("official dated preview hosted tool_choice");
        match preview_choice {
            ToolChoice::Hosted(hosted) => {
                assert_eq!(hosted.kind(), &HostedToolType::WebSearchPreview20250311);
            }
            other => panic!("dated preview choice must stay hosted, got {other:?}"),
        }

        let images = ImageGenerationTool::new()
            .model("gpt-image-1.5")
            .quality(ImageGenerationQuality::High)
            .size("1024x1024")
            .output_format(ImageGenerationOutputFormat::Png)
            .output_compression(80)
            .moderation(ImageGenerationModeration::Auto)
            .background(ImageGenerationBackground::Opaque)
            .input_fidelity(ImageGenerationInputFidelity::Low)
            .input_image_mask(ImageGenerationInputMask::new().file_id("file_mask"))
            .partial_images(2)
            .action(ImageGenerationAction::Edit);
        let value = serde_json::to_value(&images).expect("serialize images");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "action",
                "background",
                "input_fidelity",
                "input_image_mask",
                "model",
                "moderation",
                "output_compression",
                "output_format",
                "partial_images",
                "quality",
                "size",
                "type"
            ]
        );
        images.validate().expect("documented fields stay in range");
    }

    #[test]
    fn hosted_tool_choice_type_pins_the_eight_official_values() {
        // Pinned ToolChoiceTypes.type enum: exactly the eight values below.
        // `web_search` / `web_search_2025_08_26` are request-tool types, not
        // tool-choice values, so they fall back to the open Unknown branch.
        const OFFICIAL_HOSTED_TOOL_TYPES: [&str; 8] = [
            "file_search",
            "web_search_preview",
            "computer",
            "computer_use_preview",
            "computer_use",
            "web_search_preview_2025_03_11",
            "image_generation",
            "code_interpreter",
        ];
        for value in OFFICIAL_HOSTED_TOOL_TYPES {
            let decoded = HostedToolType::from_raw(value);
            assert!(
                decoded.is_known(),
                "official ToolChoiceTypes value {value} must be a named variant"
            );
            assert_eq!(decoded.as_str(), value);
            let choice: ToolChoice =
                serde_json::from_value(json!({ "type": value })).expect("decode hosted choice");
            assert!(
                matches!(choice, ToolChoice::Hosted(hosted) if hosted.kind().as_str() == value),
                "official hosted choice {value} must stay routed to the Hosted branch"
            );
        }
        for tool_only in ["web_search", "web_search_2025_08_26"] {
            let decoded = HostedToolType::from_raw(tool_only);
            assert!(
                !decoded.is_known(),
                "{tool_only} names a request tool, not a pinned tool_choice value"
            );
            assert_eq!(decoded.as_str(), tool_only);
        }
    }

    #[test]
    fn image_generation_tool_validate_enforces_pinned_limits() {
        let decoded: ImageGenerationTool = serde_json::from_value(json!({
            "type": "image_generation",
            "output_compression": 101,
            "partial_images": 4
        }))
        .expect("serde remains lossless");
        assert!(matches!(
            decoded.validate(),
            Err(CreateResponseConstraintError::ImageGenerationCompression { actual: 101, .. })
        ));
        assert!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .tool(ImageGenerationTool::new().partial_images(4))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn remaining_response_tools_send_official_fields() {
        let custom = CustomTool::new("extract")
            .description("Extract facts")
            .format(CustomToolFormat::Grammar(CustomGrammarFormat::new(
                CustomToolGrammarSyntax::Lark,
                "start: TEXT",
            )))
            .defer_loading(true)
            .allowed_callers(["programmatic"]);
        let value = serde_json::to_value(&custom).expect("serialize custom");
        assert_eq!(value["format"]["type"], "grammar");
        assert_eq!(value["format"]["syntax"], "lark");
        assert_eq!(value["allowed_callers"], json!(["programmatic"]));

        let shell = FunctionShellTool::new()
            .environment(FunctionShellEnvironment::ContainerReference(
                FunctionShellContainerReference::new("cntr_1"),
            ))
            .allowed_callers(["direct"]);
        let value = serde_json::to_value(&shell).expect("serialize shell");
        assert_eq!(value["environment"]["type"], "container_reference");
        assert_eq!(value["environment"]["container_id"], "cntr_1");

        let search = ToolSearchTool::new()
            .execution(ToolSearchExecution::Client)
            .description("Find deferred tools");
        let value = serde_json::to_value(&search).expect("serialize tool search");
        assert_eq!(value["execution"], "client");

        let patch = ApplyPatchTool::new().allowed_callers(["direct"]);
        assert_eq!(
            serde_json::to_value(&patch).expect("serialize patch")["allowed_callers"],
            json!(["direct"])
        );

        assert_eq!(
            serde_json::to_value(CustomTool::new("extract").allowed_callers_null())
                .expect("serialize custom null")["allowed_callers"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(FunctionShellTool::new().allowed_callers_null())
                .expect("serialize shell null")["allowed_callers"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(ApplyPatchTool::new().allowed_callers_null())
                .expect("serialize patch null")["allowed_callers"],
            Value::Null
        );
        let search_null = ToolSearchTool::new().description_null().parameters_null();
        let search_null_value = serde_json::to_value(&search_null).expect("serialize search nulls");
        assert_eq!(search_null_value["description"], Value::Null);
        assert_eq!(search_null_value["parameters"], Value::Null);

        let mcp_choice = serde_json::from_value::<ToolChoice>(json!({
            "type": "mcp",
            "server_label": "docs",
            "name": null
        }))
        .expect("official ToolChoiceMCP name null");
        assert_eq!(
            serde_json::to_value(&mcp_choice).expect("round-trip mcp name null")["name"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(McpToolChoice::server("docs").name_null())
                .expect("serialize mcp name null")["name"],
            Value::Null
        );
    }

    #[test]
    fn computer_call_fields_match_python_and_openapi_inventory() {
        let click = ComputerAction::Click(ComputerClickAction::new(
            ComputerClickButton::Left,
            100,
            200,
        ));
        let call: ComputerCall = serde_json::from_value(json!({
            "type": "computer_call",
            "id": "cu_1",
            "call_id": "call_1",
            "status": "completed",
            "pending_safety_checks": [{"id": "sc_1", "code": "malware", "message": "check"}],
            "action": {"type": "click", "button": "left", "x": 100, "y": 200},
            "actions": [{"type": "screenshot"}, {"type": "wait"}]
        }))
        .expect("decode computer call");
        assert!(matches!(call.action(), Some(ComputerAction::Click(_))));
        assert_eq!(call.actions().expect("actions").len(), 2);
        assert_eq!(call.pending_safety_checks()[0].id(), "sc_1");
        let value = serde_json::to_value(&click).expect("serialize click");
        assert_eq!(value["type"], "click");
        assert_eq!(value["button"], "left");

        let output = ComputerCallOutput::new(
            "call_1",
            ComputerScreenshot::new().image_url("https://example.com/a.png"),
        )
        .acknowledged_safety_checks([ComputerSafetyCheck::new("sc_1").code("malware")]);
        let value = serde_json::to_value(&output).expect("serialize output");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["acknowledged_safety_checks", "call_id", "output", "type"]
        );
        assert_eq!(value["output"]["type"], "computer_screenshot");
        assert_eq!(value["acknowledged_safety_checks"][0]["id"], "sc_1");

        let screenshot_nulls = serde_json::from_value::<ComputerScreenshot>(json!({
            "type": "computer_screenshot",
            "image_url": null,
            "file_id": null
        }))
        .expect("official ComputerScreenshotContent nulls");
        assert_eq!(screenshot_nulls.image_url_value(), None);
        assert_eq!(screenshot_nulls.file_id_value(), None);
        let sent = ComputerScreenshot::new().image_url_null().file_id_null();
        let sent_value = serde_json::to_value(&sent).expect("serialize screenshot nulls");
        assert_eq!(sent_value["image_url"], Value::Null);
        assert_eq!(sent_value["file_id"], Value::Null);

        let click_keys = ComputerClickAction::new(ComputerClickButton::Left, 1, 2)
            .keys(["SHIFT"])
            .keys_null();
        assert_eq!(
            serde_json::to_value(&click_keys).expect("serialize click keys")["keys"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(
                ComputerDragAction::new(vec![ComputerCoordinate::new(0, 0)]).keys_null()
            )
            .expect("serialize drag keys")["keys"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(ComputerMoveAction::new(1, 2).keys_null())
                .expect("serialize move keys")["keys"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(ComputerScrollAction::new(1, 2, 0, 10).keys_null())
                .expect("serialize scroll keys")["keys"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(ComputerDoubleClickAction::new(1, 2).keys(["CTRL"]))
                .expect("serialize double-click keys")["keys"],
            json!(["CTRL"])
        );
        let safety = ComputerSafetyCheck::new("sc_1").code_null().message_null();
        let safety_value = serde_json::to_value(&safety).expect("serialize safety nulls");
        assert_eq!(safety_value["code"], Value::Null);
        assert_eq!(safety_value["message"], Value::Null);
        assert_eq!(
            serde_json::to_value(
                serde_json::from_value::<ComputerClickAction>(json!({
                    "type": "click",
                    "button": "left",
                    "x": 1,
                    "y": 2,
                    "keys": null
                }))
                .expect("official ClickParam keys null")
            )
            .expect("serialize decoded click")["keys"],
            Value::Null
        );
        let _ = click_keys;
    }

    #[test]
    fn apply_patch_operation_fields_match_python_and_openapi_inventory() {
        let create = ApplyPatchOperation::CreateFile(ApplyPatchCreateFile::new(
            "src/main.rs",
            "@@\n+fn main() {}\n",
        ));
        let value = serde_json::to_value(&create).expect("serialize create");
        assert_eq!(value["type"], "create_file");
        assert_eq!(value["path"], "src/main.rs");
        assert_eq!(value["diff"], "@@\n+fn main() {}\n");

        let update = ApplyPatchOperation::UpdateFile(ApplyPatchUpdateFile::new(
            "src/lib.rs",
            "@@\n-old\n+new\n",
        ));
        let decoded: ApplyPatchCall = serde_json::from_value(json!({
            "type": "apply_patch_call",
            "id": "ap_1",
            "call_id": "call_ap",
            "status": "completed",
            "operation": {
                "type": "update_file",
                "path": "src/lib.rs",
                "diff": "@@\n-old\n+new\n"
            }
        }))
        .expect("decode apply patch");
        match decoded.operation() {
            ApplyPatchOperation::UpdateFile(op) => {
                assert_eq!(op.path(), "src/lib.rs");
                assert_eq!(op.diff(), "@@\n-old\n+new\n");
            }
            other => panic!("expected update_file, got {other:?}"),
        }
        let _ = update;

        let output =
            ApplyPatchCallOutputInput::new("call_ap", ApplyPatchCallOutputStatus::Completed)
                .output("patched");
        let value = serde_json::to_value(&output).expect("serialize output");
        assert_eq!(value["output"], "patched");
        assert_eq!(value["status"], "completed");
    }

    #[test]
    fn code_interpreter_allowlist_domain_secrets_serialize_and_validate() {
        // Pinned ContainerNetworkPolicyAllowlistParam.domain_secrets:
        // optional array (minItems 1) of {domain, name, value} with the
        // D0076 length limits (value 1..=10,485,760 chars).
        let policy = CodeInterpreterNetworkAllowlist::new(["api.example.test"])
            .with_secret(CodeInterpreterDomainSecret::new(
                "api.example.test",
                "API_TOKEN",
                "token-value",
            ))
            .with_secret(CodeInterpreterDomainSecret::new(
                "cdn.example.test",
                "CDN_KEY",
                "cdn-value",
            ));
        assert_eq!(policy.allowed_domains(), ["api.example.test"]);
        assert_eq!(policy.domain_secrets().len(), 2);
        assert_eq!(policy.domain_secrets()[0].domain(), "api.example.test");
        assert_eq!(policy.domain_secrets()[0].name(), "API_TOKEN");

        let tool = CodeInterpreterTool::auto(
            AutoCodeInterpreterContainer::new()
                .network_policy(CodeInterpreterNetworkPolicy::Allowlist(policy)),
        );
        let value = serde_json::to_value(&tool).expect("serialize domain secrets");
        assert_eq!(
            value["container"]["network_policy"]["domain_secrets"],
            json!([
                {"domain": "api.example.test", "name": "API_TOKEN", "value": "token-value"},
                {"domain": "cdn.example.test", "name": "CDN_KEY", "value": "cdn-value"}
            ])
        );
        assert!(!format!("{tool:?}").contains("token-value"));
        tool.validate()
            .expect("in-range domain secrets are accepted");

        let decoded: CodeInterpreterTool =
            serde_json::from_value(value.clone()).expect("decode domain secrets");
        assert_eq!(
            serde_json::to_value(&decoded).expect("round-trip domain secrets"),
            value
        );
        let auto = match decoded.container() {
            CodeInterpreterContainer::Auto(auto) => auto,
            other => panic!("expected auto container, got {other:?}"),
        };
        match &auto.network_policy {
            Omittable::Value(CodeInterpreterNetworkPolicy::Allowlist(policy)) => {
                assert_eq!(policy.domain_secrets().len(), 2);
            }
            other => panic!("expected allowlist policy, got {other:?}"),
        }

        let omitted =
            serde_json::to_value(CodeInterpreterNetworkAllowlist::new(["a.example.test"]))
                .expect("serialize without secrets");
        assert!(omitted.get("domain_secrets").is_none());

        // Pinned ContainerNetworkPolicyAllowlistParam.allowed_domains is
        // minItems 1: an empty allowlist is rejected by validate() without
        // sending the request (mirrors the containers-side
        // EmptyAllowedDomains semantics).
        assert!(matches!(
            CodeInterpreterTool::auto(AutoCodeInterpreterContainer::new().network_policy(
                CodeInterpreterNetworkPolicy::Allowlist(CodeInterpreterNetworkAllowlist::new(
                    Vec::<String>::new()
                ))
            ))
            .validate(),
            Err(CreateResponseConstraintError::EmptyAllowedDomains)
        ));
        let decoded_empty = serde_json::from_value::<CodeInterpreterTool>(json!({
            "type": "code_interpreter",
            "container": {
                "type": "auto",
                "network_policy": {
                    "type": "allowlist",
                    "allowed_domains": []
                }
            }
        }))
        .expect("serde remains lossless for an empty allowlist");
        assert!(matches!(
            decoded_empty.validate(),
            Err(CreateResponseConstraintError::EmptyAllowedDomains)
        ));

        let mut empty_secrets = CodeInterpreterNetworkAllowlist::new(["api.example.test"]);
        empty_secrets.domain_secrets = Omittable::Value(Vec::new());
        assert!(matches!(
            CodeInterpreterTool::auto(
                AutoCodeInterpreterContainer::new()
                    .network_policy(CodeInterpreterNetworkPolicy::Allowlist(empty_secrets))
            )
            .validate(),
            Err(CreateResponseConstraintError::EmptyDomainSecrets)
        ));
        assert!(matches!(
            CodeInterpreterTool::auto(
                AutoCodeInterpreterContainer::new().network_policy(
                    CodeInterpreterNetworkPolicy::Allowlist(
                        CodeInterpreterNetworkAllowlist::new(["api.example.test"]).with_secret(
                            CodeInterpreterDomainSecret::new("", "API_TOKEN", "token")
                        )
                    )
                )
            )
            .validate(),
            Err(CreateResponseConstraintError::DomainSecretDomain { actual: 0, .. })
        ));
        assert!(matches!(
            CodeInterpreterTool::auto(AutoCodeInterpreterContainer::new().network_policy(
                CodeInterpreterNetworkPolicy::Allowlist(
                    CodeInterpreterNetworkAllowlist::new(["api.example.test"]).with_secret(
                        CodeInterpreterDomainSecret::new("api.example.test", "", "token")
                    )
                )
            ))
            .validate(),
            Err(CreateResponseConstraintError::DomainSecretName { actual: 0, .. })
        ));
        assert!(matches!(
            CodeInterpreterTool::auto(AutoCodeInterpreterContainer::new().network_policy(
                CodeInterpreterNetworkPolicy::Allowlist(
                    CodeInterpreterNetworkAllowlist::new(["api.example.test"]).with_secret(
                        CodeInterpreterDomainSecret::new(
                            "api.example.test",
                            "API_TOKEN",
                            "x".repeat(MAX_DOMAIN_SECRET_VALUE_CHARS + 1)
                        )
                    )
                )
            ))
            .validate(),
            Err(CreateResponseConstraintError::DomainSecretValue {
                actual: 10_485_761,
                ..
            })
        ));
    }

    #[test]
    fn web_search_and_shell_actions_match_python_and_openapi_inventory() {
        let search = WebSearchAction::Search(
            WebSearchSearchAction::new()
                .queries(["openai rust sdk"])
                .sources([WebSearchSource::new("https://platform.openai.com")]),
        );
        let value = serde_json::to_value(&search).expect("serialize search");
        assert_eq!(value["type"], "search");
        assert_eq!(value["queries"], json!(["openai rust sdk"]));
        assert_eq!(value["sources"][0]["type"], "url");

        let find = WebSearchAction::Find(WebSearchFindAction::new("https://example.com", "sdk"));
        assert_eq!(
            serde_json::to_value(&find).expect("serialize find")["type"],
            "find_in_page"
        );

        let local =
            LocalShellAction::Exec(LocalShellExecAction::new(["ls", "-l"], [("PATH", "/bin")]));
        let value = serde_json::to_value(&local).expect("serialize local");
        assert_eq!(value["type"], "exec");
        assert_eq!(value["command"], json!(["ls", "-l"]));
        assert_eq!(value["env"]["PATH"], "/bin");

        let shell = FunctionShellAction::new(["echo", "hi"]).timeout_ms(1_000);
        let value = serde_json::to_value(&shell).expect("serialize shell");
        assert_eq!(value["commands"], json!(["echo", "hi"]));
        assert_eq!(value["timeout_ms"], 1000);
        assert_eq!(value["max_output_length"], Value::Null);
        let cleared = shell.timeout_ms_null().max_output_length_null();
        let cleared_value = serde_json::to_value(&cleared).expect("serialize shell nulls");
        assert_eq!(cleared_value["timeout_ms"], Value::Null);
        assert_eq!(cleared_value["max_output_length"], Value::Null);

        let decoded: FunctionShellCall = serde_json::from_value(json!({
            "type": "shell_call",
            "id": "sh1",
            "call_id": "s1",
            "status": "completed",
            "action": {
                "commands": ["echo"],
                "timeout_ms": null,
                "max_output_length": null
            },
            "environment": {"type": "container_reference", "container_id": "cntr_1"}
        }))
        .expect("decode shell call");
        assert_eq!(decoded.action().commands(), ["echo"]);
        assert!(matches!(
            decoded.environment(),
            Some(FunctionShellEnvironment::ContainerReference(_))
        ));
    }

    #[test]
    fn reasoning_and_code_interpreter_fields_match_python_and_openapi_inventory() {
        let reasoning = ReasoningItem::new("rs_1", vec![SummaryTextContent::new("thought")])
            .encrypted_content("enc_1")
            .content(vec![ReasoningTextContent::new("step")])
            .status(FunctionCallItemStatus::Completed);
        let value = serde_json::to_value(&reasoning).expect("serialize reasoning");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "content",
                "encrypted_content",
                "id",
                "status",
                "summary",
                "type"
            ]
        );
        assert_eq!(value["summary"][0]["type"], "summary_text");
        assert_eq!(value["content"][0]["type"], "reasoning_text");
        assert_eq!(reasoning.encrypted_content_ref(), Some("enc_1"));

        let decoded: ReasoningItem = serde_json::from_value(json!({
            "type": "reasoning",
            "id": "rs_2",
            "summary": [{"type": "summary_text", "text": "ok"}],
            "encrypted_content": null,
            "status": "incomplete"
        }))
        .expect("decode reasoning");
        assert_eq!(decoded.summary()[0].text(), "ok");
        assert!(decoded.encrypted_content_ref().is_none());

        let auto = AutoCodeInterpreterContainer::new()
            .file_ids(["file-1"])
            .memory_limit(CodeInterpreterMemoryLimit::FourGiB)
            .network_policy(CodeInterpreterNetworkPolicy::allowlist(["example.com"]));
        let tool = CodeInterpreterTool::auto(auto).allowed_callers(["direct"]);
        let value = serde_json::to_value(&tool).expect("serialize code interpreter");
        assert_eq!(value["container"]["type"], "auto");
        assert_eq!(value["container"]["file_ids"], json!(["file-1"]));
        assert_eq!(value["container"]["memory_limit"], "4g");
        assert_eq!(
            serde_json::to_value(AutoCodeInterpreterContainer::new().memory_limit_null())
                .expect("serialize CI memory_limit null")["memory_limit"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(FunctionShellContainerAuto::new().memory_limit_null())
                .expect("serialize shell memory_limit null")["memory_limit"],
            Value::Null
        );
        assert_eq!(value["container"]["network_policy"]["type"], "allowlist");
        assert!(matches!(
            tool.container(),
            CodeInterpreterContainer::Auto(_)
        ));
        tool.validate().expect("one file id is accepted");

        let too_many: Vec<String> = (0..=MAX_CODE_INTERPRETER_FILE_IDS)
            .map(|i| format!("file-{i}"))
            .collect();
        let invalid =
            CodeInterpreterTool::auto(AutoCodeInterpreterContainer::new().file_ids(too_many));
        assert!(matches!(
            invalid.validate(),
            Err(CreateResponseConstraintError::CodeInterpreterFileIds { actual: 51, .. })
        ));
        let decoded: CodeInterpreterTool = serde_json::from_value(json!({
            "type": "code_interpreter",
            "container": {
                "type": "auto",
                "file_ids": (0..=MAX_CODE_INTERPRETER_FILE_IDS)
                    .map(|i| format!("file-{i}"))
                    .collect::<Vec<_>>()
            }
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());

        let call: CodeInterpreterCall = serde_json::from_value(json!({
            "type": "code_interpreter_call",
            "id": "ci_1",
            "status": "interpreting",
            "container_id": "cntr_1",
            "code": "print(1)",
            "outputs": [
                {"type": "logs", "logs": "1\n"},
                {"type": "image", "url": "https://example.com/plot.png"}
            ]
        }))
        .expect("decode code interpreter call");
        assert_eq!(call.status, ResponseItemStatus::Interpreting);
        let outputs = call.outputs().expect("outputs");
        assert!(matches!(outputs[0], CodeInterpreterOutput::Logs(_)));
        assert!(matches!(outputs[1], CodeInterpreterOutput::Image(_)));
    }

    #[test]
    fn shell_output_and_tool_search_fields_match_python_and_openapi_inventory() {
        let chunk =
            FunctionShellCallOutputContent::new("hello\n", "", FunctionShellOutcome::exit(0));
        let output = FunctionShellCallOutputInput::new("s1", vec![chunk])
            .caller(ToolCallCaller::direct())
            .max_output_length(1024);
        output
            .validate()
            .expect("documented shell output is in range");
        let value = serde_json::to_value(&output).expect("serialize shell output");
        assert_eq!(value["type"], "shell_call_output");
        assert_eq!(value["output"][0]["stdout"], "hello\n");
        assert_eq!(value["output"][0]["outcome"]["type"], "exit");
        assert_eq!(value["output"][0]["outcome"]["exit_code"], 0);
        assert_eq!(value["caller"]["type"], "direct");
        assert_eq!(value["max_output_length"], 1024);
        assert_eq!(
            serde_json::to_value(output.max_output_length_null())
                .expect("serialize output length null")["max_output_length"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(
                FunctionShellCallOutputInput::new(
                    "s1",
                    vec![FunctionShellCallOutputContent::new(
                        "hello\n",
                        "",
                        FunctionShellOutcome::exit(0)
                    )]
                )
                .id_null()
                .caller_null()
                .status_null()
            )
            .expect("serialize official shell output nulls"),
            json!({
                "type": "shell_call_output",
                "call_id": "s1",
                "output": [{
                    "stdout": "hello\n",
                    "stderr": "",
                    "outcome": {"type": "exit", "exit_code": 0}
                }],
                "id": null,
                "caller": null,
                "status": null
            })
        );
        let empty_id = FunctionShellCallOutputInput::new(
            "",
            vec![FunctionShellCallOutputContent::new(
                "ok",
                "",
                FunctionShellOutcome::exit(0),
            )],
        );
        assert!(matches!(
            empty_id.validate(),
            Err(CreateResponseConstraintError::FunctionShellCallId { actual: 0, .. })
        ));
        let long_id = FunctionShellCallOutputInput::new(
            "a".repeat(MAX_FUNCTION_SHELL_CALL_ID_CHARS + 1),
            vec![FunctionShellCallOutputContent::new(
                "ok",
                "",
                FunctionShellOutcome::exit(0),
            )],
        );
        assert!(matches!(
            long_id.validate(),
            Err(CreateResponseConstraintError::FunctionShellCallId { actual: 65, .. })
        ));
        let decoded_illegal = serde_json::from_value::<FunctionShellCallOutputInput>(json!({
            "type": "shell_call_output",
            "call_id": "a".repeat(65),
            "output": [{
                "stdout": "ok",
                "stderr": "",
                "outcome": {"type": "exit", "exit_code": 0}
            }]
        }))
        .expect("serde remains lossless");
        assert!(decoded_illegal.validate().is_err());

        let call = FunctionShellCallInput::new("s1", FunctionShellAction::new(["echo"]))
            .id_null()
            .caller_null()
            .status_null()
            .environment_null();
        call.validate().expect("one-character call_id is accepted");
        assert_eq!(
            serde_json::to_value(&call).expect("serialize official shell call nulls"),
            json!({
                "type": "shell_call",
                "call_id": "s1",
                "action": {
                    "commands": ["echo"],
                    "timeout_ms": null,
                    "max_output_length": null
                },
                "id": null,
                "caller": null,
                "status": null,
                "environment": null
            })
        );
        assert!(matches!(
            FunctionShellCallInput::new("", FunctionShellAction::new(["echo"])).validate(),
            Err(CreateResponseConstraintError::FunctionShellCallId { actual: 0, .. })
        ));
        assert_eq!(MAX_FUNCTION_SHELL_OUTPUT_CHARS, 10_485_760);

        let decoded: FunctionShellCallOutput = serde_json::from_value(json!({
            "type": "shell_call_output",
            "id": "sho1",
            "call_id": "s1",
            "status": "completed",
            "max_output_length": null,
            "caller": {"type": "program", "caller_id": "prg_1"},
            "output": [{
                "stdout": "",
                "stderr": "boom",
                "outcome": {"type": "timeout"}
            }]
        }))
        .expect("decode shell output");
        assert!(matches!(
            decoded.output()[0].outcome(),
            FunctionShellOutcome::Timeout(_)
        ));
        assert!(matches!(
            decoded.caller,
            Omittable::Value(Nullable::Value(ToolCallCaller::Program(_)))
        ));

        let search =
            ToolSearchOutputInput::new(vec![ResponseTool::from(FunctionTool::new("lookup"))]);
        let value = serde_json::to_value(&search).expect("serialize tool search");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["name"], "lookup");

        let call = FunctionCall::new(
            "fc_1",
            "c1",
            "lookup",
            JsonText::from("{}"),
            FunctionCallItemStatus::Completed,
        )
        .namespace("ns")
        .caller(ToolCallCaller::program("prg_1"));
        let value = serde_json::to_value(&call).expect("serialize function call");
        assert_eq!(value["namespace"], "ns");
        assert_eq!(value["caller"]["type"], "program");
        assert_eq!(value["caller"]["caller_id"], "prg_1");

        let custom = CustomToolCallOutput::new(
            "cust1",
            vec![EasyInputContent::from(InputText::new("done"))],
        )
        .caller(ToolCallCaller::direct());
        let value = serde_json::to_value(&custom).expect("serialize custom output");
        assert_eq!(value["output"][0]["type"], "input_text");
        assert_eq!(value["caller"]["type"], "direct");
        let decoded: FunctionCallOutputResource = serde_json::from_value(json!({
            "type": "function_call_output",
            "id": "fco1",
            "output": "result",
            "status": "completed"
        }))
        .expect("decode function output resource");
        assert!(matches!(
            decoded.output(),
            FunctionCallOutputValue::Text(text) if text == "result"
        ));
    }

    #[test]
    fn file_search_result_attributes_support_omitted_null_and_present() {
        let omitted: FileSearchResult =
            serde_json::from_value(json!({ "file_id": "file-1", "text": "hit", "score": 0.5 }))
                .expect("decode without attributes");
        assert!(omitted.attributes_ref().is_none());
        assert!(
            serde_json::to_value(&omitted)
                .expect("re-encode omitted attributes")
                .get("attributes")
                .is_none()
        );

        let null_echo: FileSearchResult = serde_json::from_value(json!({
            "file_id": "file-1",
            "text": "hit",
            "attributes": null
        }))
        .expect("decode official attributes null");
        assert!(null_echo.attributes_ref().is_none());
        assert_eq!(
            serde_json::to_value(&null_echo).expect("re-encode null attributes")["attributes"],
            Value::Null
        );

        let call_json = json!({
            "type": "file_search_call",
            "id": "fs_1",
            "status": "completed",
            "queries": ["q"],
            "results": [{
                "file_id": "file-1",
                "text": "hit",
                "attributes": { "source": "docs", "rank": 1, "verified": true }
            }]
        });
        let call: FileSearchCall =
            serde_json::from_value(call_json.clone()).expect("decode present attributes");
        let attributes = call.results_ref().expect("results")[0]
            .attributes_ref()
            .expect("present attributes");
        assert_eq!(attributes.len(), 3);
        assert!(matches!(
            attributes.get("verified"),
            Some(FileSearchAttributeValue::Boolean(true))
        ));
        assert_eq!(
            serde_json::to_value(&call).expect("re-encode present attributes"),
            call_json
        );

        assert_eq!(
            serde_json::to_value(FileSearchResult::new().attributes_null())
                .expect("serialize builder attributes null")["attributes"],
            Value::Null
        );
    }

    #[test]
    fn file_search_results_and_typed_stream_parts_match_openapi_inventory() {
        let call = FileSearchCall::new("fs_1", FileSearchToolCallStatus::Searching, ["rust sdk"])
            .results(vec![
                FileSearchResult::new()
                    .file_id("file-1")
                    .filename("guide.md")
                    .text("openai rust")
                    .attributes([("source", FileSearchAttributeValue::String("docs".into()))])
                    .score(0.91),
            ]);
        let value = serde_json::to_value(&call).expect("serialize file search");
        assert_eq!(value["status"], "searching");
        assert_eq!(value["results"][0]["file_id"], "file-1");
        assert_eq!(value["results"][0]["filename"], "guide.md");
        assert_eq!(value["results"][0]["score"], 0.91);
        assert_eq!(value["results"][0]["attributes"]["source"], "docs");
        assert_eq!(
            call.results_ref().expect("results")[0].file_id_ref(),
            Some("file-1")
        );

        let decoded: FileSearchCall = serde_json::from_value(json!({
            "type": "file_search_call",
            "id": "fs_2",
            "status": "completed",
            "queries": ["q"],
            "results": null
        }))
        .expect("decode null results");
        assert!(decoded.results_ref().is_none());

        let part: ReasoningSummaryPartAddedEvent = serde_json::from_value(json!({
            "type": "response.reasoning_summary_part.added",
            "item_id": "rs_1",
            "output_index": 0,
            "summary_index": 0,
            "sequence_number": 1,
            "part": {"type": "summary_text", "text": "thinking"}
        }))
        .expect("decode summary part");
        assert_eq!(part.part.text(), "thinking");

        let annotation: OutputTextAnnotationAddedEvent = serde_json::from_value(json!({
            "type": "response.output_text.annotation.added",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "annotation_index": 0,
            "sequence_number": 2,
            "annotation": {
                "type": "url_citation",
                "url": "https://example.com",
                "start_index": 0,
                "end_index": 4,
                "title": "Example"
            }
        }))
        .expect("decode annotation");
        assert!(matches!(
            annotation.annotation,
            Nullable::Value(Annotation::UrlCitation(_))
        ));
        let annotation_null: OutputTextAnnotationAddedEvent = serde_json::from_value(json!({
            "type": "response.output_text.annotation.added",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "annotation_index": 0,
            "sequence_number": 3,
            "annotation": null
        }))
        .expect("official annotation null decodes");
        assert!(matches!(annotation_null.annotation, Nullable::Null));
        let routed = serde_json::from_value::<ResponseStreamEvent>(json!({
            "type": "response.output_text.annotation.added",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "annotation_index": 0,
            "sequence_number": 4,
            "annotation": null
        }))
        .expect("union routes official annotation null");
        assert!(matches!(
            routed,
            ResponseStreamEvent::OutputTextAnnotationAdded(event)
                if matches!(event.annotation, Nullable::Null)
        ));

        let local = LocalShellCallOutput::new("lsco1", "l1", "ok")
            .status(FunctionCallItemStatus::Completed);
        assert_eq!(
            serde_json::to_value(&local).expect("serialize local output")["status"],
            "completed"
        );
    }

    #[test]
    fn stream_event_optional_fields_match_openapi() {
        let shell: ShellCommandDeltaEvent = serde_json::from_value(json!({
            "type": "response.shell_call_command.delta",
            "output_index": 1,
            "command_index": 0,
            "delta": "ls",
            "sequence_number": 4,
            "obfuscation": "pad"
        }))
        .expect("decode official shell obfuscation");
        assert_eq!(shell.obfuscation().map(String::as_str), Some("pad"));
        assert_eq!(
            serde_json::to_value(&shell).expect("serialize shell")["obfuscation"],
            "pad"
        );
        assert!(
            serde_json::from_value::<ShellCommandDeltaEvent>(json!({
                "type": "response.shell_call_command.delta",
                "output_index": 1,
                "command_index": 0,
                "delta": "ls",
                "sequence_number": 4,
                "obfuscation": null
            }))
            .is_err(),
            "unofficial obfuscation null must fail"
        );

        let summary: ReasoningSummaryPartDoneEvent = serde_json::from_value(json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": "rs_1",
            "output_index": 0,
            "summary_index": 0,
            "sequence_number": 5,
            "part": {"type": "summary_text", "text": "done"},
            "status": "incomplete"
        }))
        .expect("decode official summary-part status");
        assert_eq!(
            summary.status(),
            Some(&ReasoningSummaryPartStatus::Incomplete)
        );
        let omitted: ReasoningSummaryPartDoneEvent = serde_json::from_value(json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": "rs_1",
            "output_index": 0,
            "summary_index": 0,
            "sequence_number": 5,
            "part": {"type": "summary_text", "text": "done"}
        }))
        .expect("decode omitted summary-part status");
        assert!(omitted.status().is_none());

        let image: ImageGenerationPartialImageEvent = serde_json::from_value(json!({
            "type": "response.image_generation_call.partial_image",
            "output_index": 2,
            "item_id": "ig_1",
            "partial_image_index": 1,
            "partial_image_b64": "aaaa",
            "sequence_number": 6,
            "size": "1024x1024",
            "quality": "high",
            "background": "opaque",
            "output_format": "png"
        }))
        .expect("decode official partial-image options");
        assert_eq!(image.size().map(String::as_str), Some("1024x1024"));
        assert_eq!(image.quality(), Some(&ImageGenerationQuality::High));
        assert_eq!(image.background(), Some(&ImageGenerationBackground::Opaque));
        assert_eq!(
            image.output_format(),
            Some(&ImageGenerationOutputFormat::Png)
        );
        let wire = serde_json::to_value(&image).expect("serialize partial image");
        assert_eq!(wire["size"], "1024x1024");
        assert_eq!(wire["quality"], "high");
        assert_eq!(wire["background"], "opaque");
        assert_eq!(wire["output_format"], "png");
        assert!(
            !image.extra_fields().contains_key("size"),
            "typed size must not fall through ExtraFields"
        );
    }

    #[test]
    fn shell_environment_fields_match_python_and_openapi_inventory() {
        let auto = FunctionShellContainerAuto::new()
            .file_ids(["file-1"])
            .memory_limit(CodeInterpreterMemoryLimit::FourGiB)
            .network_policy(CodeInterpreterNetworkPolicy::disabled())
            .skills(vec![ContainerSkill::Reference(
                ContainerSkillReference::new("skill_abc").version("latest"),
            )]);
        let tool =
            FunctionShellTool::new().environment(FunctionShellEnvironment::ContainerAuto(auto));
        let value = serde_json::to_value(&tool).expect("serialize shell auto");
        assert_eq!(value["environment"]["type"], "container_auto");
        assert_eq!(value["environment"]["file_ids"], json!(["file-1"]));
        assert_eq!(value["environment"]["memory_limit"], "4g");
        assert_eq!(value["environment"]["network_policy"]["type"], "disabled");
        assert_eq!(value["environment"]["skills"][0]["type"], "skill_reference");
        assert_eq!(value["environment"]["skills"][0]["skill_id"], "skill_abc");
        tool.validate().expect("one skill and file are accepted");

        let local = FunctionShellLocalEnvironment::new().skills(vec![LocalSkill::new(
            "lint",
            "Run lints",
            "/skills/lint",
        )]);
        let value = serde_json::to_value(
            FunctionShellTool::new().environment(FunctionShellEnvironment::Local(local)),
        )
        .expect("serialize local");
        assert_eq!(value["environment"]["skills"][0]["name"], "lint");
        assert_eq!(value["environment"]["skills"][0]["path"], "/skills/lint");

        let too_many: Vec<String> = (0..=MAX_SHELL_CONTAINER_FILE_IDS)
            .map(|i| format!("file-{i}"))
            .collect();
        let invalid =
            FunctionShellTool::new().environment(FunctionShellEnvironment::ContainerAuto(
                FunctionShellContainerAuto::new().file_ids(too_many),
            ));
        assert!(matches!(
            invalid.validate(),
            Err(CreateResponseConstraintError::ShellContainerFileIds { actual: 51, .. })
        ));
        let decoded: FunctionShellTool = serde_json::from_value(json!({
            "type": "shell",
            "environment": {
                "type": "container_auto",
                "file_ids": (0..=MAX_SHELL_CONTAINER_FILE_IDS)
                    .map(|i| format!("file-{i}"))
                    .collect::<Vec<_>>()
            }
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());
        assert!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .tool(decoded)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn mcp_call_error_union_matches_python_and_openapi_inventory() {
        let protocol = McpCallError::Protocol(McpProtocolError::new(-32601, "unknown tool"));
        let value = serde_json::to_value(&protocol).expect("serialize protocol");
        assert_eq!(value["type"], "mcp_protocol_error");
        assert_eq!(value["code"], -32601);
        assert_eq!(value["message"], "unknown tool");

        let http = McpCallError::Http(McpHttpError::new(502, "bad gateway"));
        assert_eq!(
            serde_json::to_value(&http).expect("serialize http")["type"],
            "http_error"
        );

        let decoded: McpCall = serde_json::from_value(json!({
            "type": "mcp_call",
            "id": "mcp_c1",
            "call_id": "mcp1",
            "server_label": "srv",
            "name": "tool1",
            "arguments": "{}",
            "status": "calling",
            "error": {
                "type": "mcp_tool_execution_error",
                "content": {"detail": "boom"}
            }
        }))
        .expect("decode mcp call with structured error");
        assert_eq!(
            decoded.status,
            Omittable::Value(ResponseItemStatus::Calling)
        );
        match decoded.error() {
            Some(McpCallError::Execution(error)) => {
                assert_eq!(error.content()["detail"], "boom");
            }
            other => panic!("expected execution error, got {other:?}"),
        }

        let future: McpCallError = serde_json::from_value(json!({
            "type": "mcp_future_error",
            "detail": 1
        }))
        .expect("future error tag is lossless");
        assert!(matches!(future, McpCallError::Unknown(_)));
    }

    #[test]
    fn remaining_item_fields_match_python_and_openapi_inventory() {
        let approval = McpApprovalResponse::reject("apr_1", "unsafe").with_id("aprsp_1");
        let value = serde_json::to_value(&approval).expect("serialize approval");
        assert_eq!(value["id"], "aprsp_1");
        assert_eq!(value["approval_request_id"], "apr_1");
        assert_eq!(value["approve"], false);
        assert_eq!(value["reason"], "unsafe");

        let null_reason: McpApprovalResponse = serde_json::from_value(json!({
            "type": "mcp_approval_response",
            "approval_request_id": "apr_2",
            "approve": false,
            "id": null,
            "reason": null
        }))
        .expect("official null id/reason decode");
        assert_eq!(null_reason.id(), None);
        assert_eq!(null_reason.reason(), None);

        let resource: McpApprovalResponseResource = serde_json::from_value(json!({
            "type": "mcp_approval_response",
            "id": "aprsp_2",
            "request_id": "req_1",
            "approval_request_id": "apr_3",
            "approve": false,
            "reason": "denied"
        }))
        .expect("decode approval resource");
        assert_eq!(resource.reason(), Some("denied"));

        let output =
            ApplyPatchCallOutputInput::new("call_ap", ApplyPatchCallOutputStatus::Completed)
                .output("patched")
                .caller(ToolCallCaller::direct());
        let value = serde_json::to_value(&output).expect("serialize apply-patch output");
        assert_eq!(value["caller"]["type"], "direct");
        assert_eq!(value["output"], "patched");

        let decoded: ApplyPatchCallOutput = serde_json::from_value(json!({
            "type": "apply_patch_call_output",
            "id": "apco_1",
            "call_id": "call_ap",
            "status": "completed",
            "caller": {"type": "program", "caller_id": "prog_1"},
            "created_by": "user_1",
            "output": "ok"
        }))
        .expect("decode apply-patch output");
        assert!(matches!(
            decoded.caller_ref(),
            Some(ToolCallCaller::Program(_))
        ));

        let search = ToolSearchCallInput::new(json!({"q": "shell"}))
            .with_id("tsc_1")
            .call_id("call_ts")
            .execution(ToolSearchExecution::Server)
            .status(FunctionCallItemStatus::Completed);
        let value = serde_json::to_value(&search).expect("serialize tool search");
        assert_eq!(value["id"], "tsc_1");
        assert_eq!(value["call_id"], "call_ts");
        assert_eq!(value["execution"], "server");
        assert_eq!(value["status"], "completed");

        let extra_tools = AdditionalToolsInput::new(Vec::new()).with_id("at_1");
        assert_eq!(
            serde_json::to_value(&extra_tools).expect("serialize additional tools")["id"],
            "at_1"
        );

        let compaction = CompactionSummaryInput::new("enc").with_id("cmp_1");
        assert_eq!(
            serde_json::to_value(&compaction).expect("serialize compaction")["id"],
            "cmp_1"
        );
    }

    #[test]
    fn resource_item_fields_match_python_and_openapi_inventory() {
        let resource: FunctionCallOutputResource = serde_json::from_value(json!({
            "type": "function_call_output",
            "id": "fco_1",
            "call_id": "call_1",
            "name": "lookup",
            "namespace": "tools",
            "status": "completed",
            "output": "ok",
            "caller": {"type": "direct"},
            "created_by": "user_9"
        }))
        .expect("decode function output resource");
        assert_eq!(resource.call_id(), Some("call_1"));
        assert_eq!(resource.created_by(), Some("user_9"));

        let response: Response = serde_json::from_value(json!({
            "id": "resp_1",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "object": "response",
            "output": [{
                "type": "function_call_output",
                "id": "fco_1",
                "call_id": "call_1",
                "name": "lookup",
                "namespace": "tools",
                "status": "completed",
                "output": "ok",
                "caller": {"type": "direct"},
                "created_by": "user_9"
            }],
            "parallel_tool_calls": false,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null
        }))
        .expect("decode response");
        let ResponseInputItem::FunctionCallOutput(input) = &response.to_input_items()[0] else {
            panic!("expected function call output");
        };
        assert_eq!(
            input.call_id,
            Omittable::Value(Nullable::Value(String::from("call_1")))
        );
        assert_eq!(
            input.name,
            Omittable::Value(Nullable::Value(String::from("lookup")))
        );
        assert_eq!(
            input.extra_fields().get("created_by"),
            Some(&json!("user_9")),
            "resource-only created_by must replay through extra"
        );

        let shell: FunctionShellCall = serde_json::from_value(json!({
            "type": "shell_call",
            "id": "sh1",
            "call_id": "s1",
            "status": "completed",
            "action": {"commands": ["echo"], "timeout_ms": null, "max_output_length": null},
            "environment": null,
            "created_by": "user_9"
        }))
        .expect("decode shell call");
        assert_eq!(shell.created_by, Omittable::Value(String::from("user_9")));

        let custom: CustomToolCall = serde_json::from_value(json!({
            "type": "custom_tool_call",
            "call_id": "cust1",
            "name": "custom",
            "input": "{}",
            "status": "completed",
            "created_by": "user_9"
        }))
        .expect("decode custom tool call");
        assert_eq!(
            custom.status,
            Omittable::Value(ResponseItemStatus::Completed)
        );
        assert_eq!(custom.created_by, Omittable::Value(String::from("user_9")));
    }

    #[test]
    fn to_input_items_replays_resource_only_created_by_into_extra() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_replay",
            "created_at": 2,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-5.6-sol",
            "object": "response",
            "output": [
                {
                    "type": "tool_search_call",
                    "id": "tsc_1",
                    "call_id": null,
                    "execution": "server",
                    "arguments": {"query": "shell"},
                    "status": "completed",
                    "created_by": "user_1"
                },
                {
                    "type": "compaction",
                    "id": "cmp_1",
                    "encrypted_content": "enc_payload",
                    "created_by": "user_2"
                },
                {
                    "type": "shell_call",
                    "id": "sh_1",
                    "call_id": "call_sh",
                    "action": {"commands": ["echo"], "timeout_ms": null, "max_output_length": null},
                    "status": "completed",
                    "environment": null,
                    "created_by": "user_3"
                },
                {
                    "type": "tool_search_call",
                    "id": "tsc_2",
                    "call_id": null,
                    "execution": "server",
                    "arguments": {},
                    "status": "completed"
                }
            ],
            "parallel_tool_calls": false,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null
        }))
        .expect("decode replay response");

        let inputs = response.to_input_items();

        let ResponseInputItem::ToolSearchCall(tool_search) = &inputs[0] else {
            panic!("expected replayed tool-search call input");
        };
        assert_eq!(
            tool_search.extra_fields().get("created_by"),
            Some(&json!("user_1"))
        );
        assert_eq!(
            serde_json::to_value(tool_search).expect("encode replayed tool search")["created_by"],
            "user_1"
        );

        let ResponseInputItem::Compaction(compaction) = &inputs[1] else {
            panic!("expected replayed compaction input");
        };
        assert_eq!(
            compaction.extra_fields().get("created_by"),
            Some(&json!("user_2"))
        );
        assert_eq!(
            serde_json::to_value(compaction).expect("encode replayed compaction")["created_by"],
            "user_2"
        );

        let ResponseInputItem::FunctionShellCall(shell) = &inputs[2] else {
            panic!("expected replayed shell-call input");
        };
        assert_eq!(
            shell.extra_fields().get("created_by"),
            Some(&json!("user_3"))
        );

        let ResponseInputItem::ToolSearchCall(plain) = &inputs[3] else {
            panic!("expected tool-search call without created_by");
        };
        assert!(
            plain.extra_fields().is_empty(),
            "omitted created_by must not add replay noise"
        );
    }

    #[test]
    fn item_param_official_nulls_and_call_id_limits_match_openapi() {
        let output = FunctionCallOutput::new("c1", "ok")
            .id_null()
            .call_id_null()
            .name_null()
            .namespace_null()
            .caller_null()
            .status_null();
        output.validate().expect("explicit nulls stay in range");
        assert_eq!(
            serde_json::to_value(&output).expect("serialize function output nulls"),
            json!({
                "type": "function_call_output",
                "output": "ok",
                "id": null,
                "call_id": null,
                "name": null,
                "namespace": null,
                "caller": null,
                "status": null
            })
        );
        assert!(matches!(
            FunctionCallOutput::new("c1", "ok")
                .namespace("bad namespace")
                .validate(),
            Err(CreateResponseConstraintError::FunctionCallOutputNamespace { .. })
        ));
        let decoded = serde_json::from_value::<FunctionCallOutput>(json!({
            "type": "function_call_output",
            "output": "ok",
            "call_id": "a".repeat(65),
            "name": ""
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());

        let search = ToolSearchCallInput::new(json!({}))
            .id_null()
            .call_id_null()
            .status_null();
        search
            .validate()
            .expect("omitted-length call_id is accepted");
        assert_eq!(
            serde_json::to_value(&search).expect("serialize tool search nulls")["call_id"],
            Value::Null
        );
        assert!(matches!(
            ToolSearchCallInput::new(json!({})).call_id("").validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));

        let tools =
            ToolSearchOutputInput::new(vec![ResponseTool::from(FunctionTool::new("lookup"))])
                .id_null()
                .call_id_null()
                .status_null();
        tools.validate().expect("null call_id stays in range");
        assert_eq!(
            serde_json::to_value(&tools).expect("serialize tool search output nulls")["id"],
            Value::Null
        );

        let computer = ComputerCallOutput::new(
            "call_1",
            ComputerScreenshot::new().image_url("https://example.com/a.png"),
        )
        .id_null()
        .status_null();
        computer.validate().expect("one-character-minimum call_id");
        assert_eq!(
            serde_json::to_value(&computer).expect("serialize computer output nulls")["status"],
            Value::Null
        );
        assert!(matches!(
            ComputerCallOutput::new(
                "",
                ComputerScreenshot::new().image_url("https://example.com/a.png"),
            )
            .validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));

        let patch = ApplyPatchCallInput::new(
            "call_ap",
            ApplyPatchCallStatus::Completed,
            ApplyPatchOperation::CreateFile(ApplyPatchCreateFile::new(
                "a.rs",
                "@@\n+fn main() {}\n",
            )),
        )
        .id_null()
        .caller_null();
        patch.validate().expect("apply-patch call_id is in range");
        assert_eq!(
            serde_json::to_value(&patch).expect("serialize apply-patch nulls")["caller"],
            Value::Null
        );
        let patch_out =
            ApplyPatchCallOutputInput::new("call_ap", ApplyPatchCallOutputStatus::Completed)
                .id_null()
                .caller_null()
                .output_null();
        patch_out.validate().expect("null log text stays in range");
        assert_eq!(
            serde_json::to_value(&patch_out).expect("serialize apply-patch output nulls")["output"],
            Value::Null
        );

        assert_eq!(
            serde_json::to_value(
                AdditionalToolsInput::new(vec![ResponseTool::from(FunctionTool::new("lookup"))])
                    .id_null()
            )
            .expect("serialize additional tools null")["id"],
            Value::Null
        );
        let compaction = CompactionSummaryInput::new("enc").id_null();
        compaction
            .validate()
            .expect("short encrypted content is accepted");
        assert_eq!(
            serde_json::to_value(&compaction).expect("serialize compaction null")["id"],
            Value::Null
        );
        assert_eq!(MAX_COMPACTION_ENCRYPTED_CHARS, 20_971_520);
        assert_eq!(MAX_FUNCTION_CALL_OUTPUT_CHARS, 10_485_760);

        assert_eq!(
            serde_json::to_value(CustomToolCallOutput::new("cust1", "done").caller_null())
                .expect("serialize custom output caller null")["caller"],
            Value::Null
        );
    }

    #[test]
    fn function_call_program_and_remaining_official_nulls_match_openapi() {
        let call = FunctionCall::new(
            "fc_1",
            "c1",
            "lookup",
            JsonText::from("{}"),
            FunctionCallItemStatus::Completed,
        )
        .caller_null();
        assert_eq!(
            serde_json::to_value(&call).expect("serialize function caller null")["caller"],
            Value::Null
        );
        let decoded: FunctionCall = serde_json::from_value(json!({
            "type": "function_call",
            "call_id": "c1",
            "name": "lookup",
            "arguments": "{}",
            "caller": null
        }))
        .expect("decode official function caller null");
        assert!(matches!(decoded.caller, Omittable::Value(Nullable::Null)));

        assert_eq!(
            serde_json::to_value(CustomToolCall::new("cust1", "lookup", "{}").caller_null())
                .expect("serialize custom caller null")["caller"],
            Value::Null
        );

        let trigger = CompactionTrigger::new().id_null();
        assert_eq!(
            serde_json::to_value(&trigger).expect("serialize trigger id null")["id"],
            Value::Null
        );
        let decoded_trigger: CompactionTrigger = serde_json::from_value(json!({
            "type": "compaction_trigger",
            "id": null
        }))
        .expect("decode official trigger id null");
        assert!(matches!(
            decoded_trigger.id,
            Omittable::Value(Nullable::Null)
        ));

        let program = ProgramItem::new("p1", "c_p1", "print(1)", "fp");
        program.validate().expect("documented program is in range");
        assert!(matches!(
            ProgramItem::new("p1", "", "print(1)", "fp").validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));
        assert!(matches!(
            ProgramItem::new("p1", "x".repeat(65), "print(1)", "fp").validate(),
            Err(CreateResponseConstraintError::CallId { actual: 65, .. })
        ));
        let illegal_program: ProgramItem = serde_json::from_value(json!({
            "type": "program",
            "id": "p1",
            "call_id": "",
            "code": "print(1)",
            "fingerprint": "fp"
        }))
        .expect("serde remains lossless for illegal program call_id");
        assert!(matches!(
            illegal_program.validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));
        assert!(
            CreateResponseRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::Program(illegal_program)]
            )
            .validate()
            .is_err()
        );

        let program_out =
            ProgramOutputItem::new("po1", "c_p1", "1\n", ProgramOutputStatus::Completed);
        program_out
            .validate()
            .expect("documented program output is in range");
        assert!(matches!(
            ProgramOutputItem::new("po1", "", "1\n", ProgramOutputStatus::Completed).validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));

        assert_eq!(
            serde_json::to_value(McpApprovalResponse::approve("apr_1").id_null())
                .expect("serialize approval id null")["id"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(LocalShellCallOutput::new("ls1", "c1", "ok").status_null())
                .expect("serialize local-shell status null")["status"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(WebSearchOpenPageAction::new().url_null())
                .expect("serialize open-page url null")["url"],
            Value::Null
        );
        let exec = LocalShellExecAction::new(["echo"], [("PATH", "/bin")])
            .timeout_ms_null()
            .working_directory_null()
            .user_null();
        let exec_value = serde_json::to_value(&exec).expect("serialize local-shell exec nulls");
        assert_eq!(exec_value["timeout_ms"], Value::Null);
        assert_eq!(exec_value["working_directory"], Value::Null);
        assert_eq!(exec_value["user"], Value::Null);

        ProgramToolCallCaller::new("prg_1")
            .validate()
            .expect("documented program caller_id is in range");
        assert!(matches!(
            ProgramToolCallCaller::new("").validate(),
            Err(CreateResponseConstraintError::CallId { actual: 0, .. })
        ));
        let illegal_caller = FunctionCall::new(
            "fc_1",
            "c1",
            "lookup",
            JsonText::from("{}"),
            FunctionCallItemStatus::Completed,
        )
        .caller(ToolCallCaller::program("x".repeat(65)));
        assert!(matches!(
            illegal_caller.validate(),
            Err(CreateResponseConstraintError::CallId { actual: 65, .. })
        ));
        assert!(
            CreateResponseRequest::new(
                "gpt-5.6-sol",
                vec![ResponseInputItem::FunctionCall(illegal_caller)]
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn function_custom_and_mcp_call_constructors_match_openapi() {
        let call = FunctionCall::call("c1", "lookup", JsonText::from("{}"));
        assert_eq!(
            serde_json::to_value(&call).expect("serialize required-only function call"),
            json!({
                "type": "function_call",
                "call_id": "c1",
                "name": "lookup",
                "arguments": "{}"
            })
        );
        let echoed = call
            .with_id("fc_1")
            .with_status(FunctionCallItemStatus::Completed);
        let echoed_value = serde_json::to_value(&echoed).expect("serialize echoed function call");
        assert_eq!(echoed_value["id"], "fc_1");
        assert_eq!(echoed_value["status"], "completed");

        assert_eq!(
            serde_json::to_value(FunctionCallOutput::from_output("ok"))
                .expect("serialize official required-only function output"),
            json!({
                "type": "function_call_output",
                "output": "ok"
            })
        );
        assert_eq!(
            serde_json::to_value(FunctionCallOutput::from_output("ok").with_call_id("c1"))
                .expect("serialize function output call_id")["call_id"],
            "c1"
        );
        assert!(
            serde_json::from_value::<FunctionCall>(json!({
                "type": "function_call",
                "call_id": "c1",
                "name": "lookup",
                "arguments": "{}",
                "namespace": null
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CustomToolCall>(json!({
                "type": "custom_tool_call",
                "call_id": "cust1",
                "name": "extract",
                "input": "{}",
                "namespace": null
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CustomToolCallOutput>(json!({
                "type": "custom_tool_call_output",
                "call_id": "cust1",
                "output": "done",
                "id": null
            }))
            .is_err()
        );

        let custom = CustomToolCall::new("cust1", "extract", "{}")
            .id("ctc_1")
            .namespace("billing");
        assert_eq!(
            serde_json::to_value(&custom).expect("serialize custom call optionals"),
            json!({
                "type": "custom_tool_call",
                "call_id": "cust1",
                "name": "extract",
                "input": "{}",
                "id": "ctc_1",
                "namespace": "billing"
            })
        );
        assert_eq!(
            serde_json::to_value(CustomToolCallOutput::new("cust1", "done").id("cto_1"))
                .expect("serialize custom output id")["id"],
            "cto_1"
        );

        let mcp = McpCall::new("mcp_1", "docs", "search", JsonText::from("{}"))
            .approval_request_id_null()
            .output_null()
            .error_null();
        let mcp_value = serde_json::to_value(&mcp).expect("serialize mcp official nulls");
        assert_eq!(mcp_value["type"], "mcp_call");
        assert_eq!(mcp_value["approval_request_id"], Value::Null);
        assert_eq!(mcp_value["output"], Value::Null);
        assert_eq!(mcp_value["error"], Value::Null);
        let decoded: McpCall = serde_json::from_value(json!({
            "type": "mcp_call",
            "id": "mcp_1",
            "server_label": "docs",
            "name": "search",
            "arguments": "{}",
            "output": null,
            "error": null,
            "approval_request_id": null
        }))
        .expect("decode official mcp nulls");
        assert!(matches!(decoded.output, Omittable::Value(Nullable::Null)));
        assert!(matches!(decoded.error, Omittable::Value(Nullable::Null)));

        let listed = McpListedTool::new("search", json!({"type": "object"}))
            .description_null()
            .annotations_null();
        let listed_value = serde_json::to_value(&listed).expect("serialize listed tool nulls");
        assert_eq!(listed_value["description"], Value::Null);
        assert_eq!(listed_value["annotations"], Value::Null);
        assert_eq!(
            serde_json::to_value(McpListTools::new("lt_1", "docs", [listed]).error_null())
                .expect("serialize list-tools error null")["error"],
            Value::Null
        );
    }

    #[test]
    fn hosted_call_constructors_match_openapi_required_fields() {
        assert_eq!(
            serde_json::to_value(McpApprovalRequest::new(
                "apr_1",
                "docs",
                "search",
                JsonText::from("{}"),
            ))
            .expect("serialize approval request"),
            json!({
                "type": "mcp_approval_request",
                "id": "apr_1",
                "server_label": "docs",
                "name": "search",
                "arguments": "{}"
            })
        );

        let computer =
            ComputerCall::new("cc_1", "call_cc", FunctionCallItemStatus::Completed).with_action(
                ComputerAction::Click(ComputerClickAction::new(ComputerClickButton::Left, 1, 2)),
            );
        let computer_value = serde_json::to_value(&computer).expect("serialize computer call");
        assert_eq!(computer_value["type"], "computer_call");
        assert_eq!(computer_value["action"]["type"], "click");
        assert_eq!(computer_value["pending_safety_checks"], json!([]));

        let search = WebSearchCall::new(
            "ws_1",
            WebSearchToolCallStatus::Completed,
            WebSearchSearchAction::new().query("openai"),
        );
        assert_eq!(
            serde_json::to_value(&search).expect("serialize web search")["action"]["query"],
            "openai"
        );

        let image = ImageGenerationCall::new("ig_1", ImageGenToolCallStatus::InProgress);
        assert_eq!(
            serde_json::to_value(&image).expect("serialize image-gen required null")["result"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(image.with_result("img_b64")).expect("serialize image-gen result")
                ["result"],
            "img_b64"
        );

        let interpreter =
            CodeInterpreterCall::new("ci_1", CodeInterpreterToolCallStatus::InProgress, "cntr_1");
        let interpreter_value =
            serde_json::to_value(&interpreter).expect("serialize interpreter required nulls");
        assert_eq!(interpreter_value["code"], Value::Null);
        assert_eq!(interpreter_value["outputs"], Value::Null);
        assert_eq!(
            serde_json::to_value(interpreter.with_code("print(1)"))
                .expect("serialize interpreter code")["code"],
            "print(1)"
        );

        let shell = LocalShellCall::new(
            "ls_1",
            "call_ls",
            LocalShellExecAction::new(["echo"], [("PATH", "/bin")]),
            FunctionCallItemStatus::Completed,
        );
        assert_eq!(
            serde_json::to_value(&shell).expect("serialize local shell")["action"]["type"],
            "exec"
        );
    }

    #[test]
    fn tool_call_output_items_expose_read_accessors() {
        // Every hosted tool-call output item exposes at least id() and
        // status(); failure-relevant payloads stay readable the same way
        // python/node expose them as attributes (4-02).
        let file_search: FileSearchCall = serde_json::from_value(json!({
            "type": "file_search_call",
            "id": "fs_9",
            "status": "failed",
            "queries": ["rust"],
            "results": null
        }))
        .expect("decode file search");
        assert_eq!(file_search.id(), "fs_9");
        assert_eq!(file_search.status(), &ResponseItemStatus::Failed);
        assert_eq!(file_search.results_ref(), None);

        let web_search: WebSearchCall = serde_json::from_value(json!({
            "type": "web_search_call",
            "id": "ws_9",
            "status": "in_progress",
            "action": {"type": "search", "query": "openai"}
        }))
        .expect("decode web search");
        assert_eq!(web_search.id(), "ws_9");
        assert_eq!(web_search.status(), &ResponseItemStatus::InProgress);

        let computer: ComputerCall = serde_json::from_value(json!({
            "type": "computer_call",
            "id": "cc_9",
            "call_id": "call_cc",
            "status": "failed",
            "pending_safety_checks": []
        }))
        .expect("decode computer call");
        assert_eq!(computer.id(), "cc_9");
        assert_eq!(computer.call_id(), "call_cc");
        assert_eq!(computer.status(), &ResponseItemStatus::Failed);

        let shell: LocalShellCall = serde_json::from_value(json!({
            "type": "local_shell_call",
            "id": "ls_9",
            "call_id": "call_ls",
            "status": "completed",
            "action": {"type": "exec", "command": ["ls"], "env": {}}
        }))
        .expect("decode local shell call");
        assert_eq!(shell.id(), "ls_9");
        assert_eq!(shell.call_id(), "call_ls");
        assert_eq!(shell.status(), &ResponseItemStatus::Completed);

        let image: ImageGenerationCall = serde_json::from_value(json!({
            "type": "image_generation_call",
            "id": "ig_9",
            "status": "completed",
            "result": "aGVsbG8="
        }))
        .expect("decode image generation call");
        assert_eq!(image.id(), "ig_9");
        assert_eq!(image.status(), &ResponseItemStatus::Completed);
        assert_eq!(image.result(), Some("aGVsbG8="));
        assert_eq!(
            ImageGenerationCall::new("ig_n", ImageGenToolCallStatus::InProgress).result(),
            None
        );

        let interpreter: CodeInterpreterCall = serde_json::from_value(json!({
            "type": "code_interpreter_call",
            "id": "ci_9",
            "status": "interpreting",
            "container_id": "cntr_9",
            "code": "print(1)",
            "outputs": null
        }))
        .expect("decode interpreter call");
        assert_eq!(interpreter.id(), "ci_9");
        assert_eq!(interpreter.status(), &ResponseItemStatus::Interpreting);
        assert_eq!(interpreter.container_id(), "cntr_9");
        assert_eq!(interpreter.code(), Some("print(1)"));
        assert_eq!(
            CodeInterpreterCall::new("ci_n", CodeInterpreterToolCallStatus::InProgress, "cntr_n")
                .code(),
            None
        );

        let listed = McpListTools::new("lt_9", "docs", []);
        assert_eq!(listed.id(), "lt_9");
        assert_eq!(listed.error(), None);
        assert_eq!(
            McpListTools::new("lt_9", "docs", [])
                .with_error("connection refused")
                .error(),
            Some("connection refused")
        );

        let mcp: McpCall = serde_json::from_value(json!({
            "type": "mcp_call",
            "id": "mcp_9",
            "server_label": "docs",
            "name": "search",
            "arguments": "{}",
            "status": "failed"
        }))
        .expect("decode mcp call");
        assert_eq!(mcp.id(), "mcp_9");
        assert_eq!(mcp.status(), Some(&ResponseItemStatus::Failed));
        assert_eq!(
            McpCall::new("mcp_n", "docs", "search", JsonText::from("{}")).status(),
            None
        );
    }

    #[test]
    fn function_tool_parameters_and_name_match_python_and_openapi_inventory() {
        let omitted: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "lookup"
        }))
        .expect("official FunctionToolParam may omit parameters");
        assert_eq!(omitted.parameters_ref(), None);

        let null_params: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "lookup",
            "parameters": null
        }))
        .expect("official parameters null");
        assert_eq!(null_params.parameters, Omittable::Value(Nullable::Null));
        assert_eq!(
            serde_json::to_value(FunctionTool::new("lookup").parameters_null())
                .expect("serialize null parameters")["parameters"],
            Value::Null
        );
        let cleared = FunctionTool::new("lookup")
            .description_null()
            .output_schema_null()
            .strict_null()
            .allowed_callers_null()
            .parameters_null();
        let cleared_value = serde_json::to_value(&cleared).expect("serialize official nulls");
        for key in [
            "description",
            "output_schema",
            "strict",
            "allowed_callers",
            "parameters",
        ] {
            assert_eq!(cleared_value[key], Value::Null, "{key}");
        }

        assert!(FunctionTool::new("lookup").validate().is_ok());
        let illegal: FunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "bad name"
        }))
        .expect("serde remains lossless");
        assert!(matches!(
            illegal.validate(),
            Err(CreateResponseConstraintError::FunctionToolName { actual: 8, .. })
        ));
        assert!(
            CreateResponseRequest::new("gpt-5.6-sol", "hello")
                .tool(illegal)
                .validate()
                .is_err()
        );

        let empty_callers = FunctionTool::new("lookup").allowed_callers(Vec::<String>::new());
        assert!(matches!(
            empty_callers.validate(),
            Err(CreateResponseConstraintError::EmptyAllowedCallers)
        ));

        let empty_ns = NamespaceTool::new("", "desc", Vec::<NamespaceToolEntry>::new());
        assert!(matches!(
            empty_ns.validate(),
            Err(CreateResponseConstraintError::EmptyNamespaceName)
        ));
        let no_tools = NamespaceTool::new("crm", "desc", Vec::<NamespaceToolEntry>::new());
        assert!(matches!(
            no_tools.validate(),
            Err(CreateResponseConstraintError::EmptyNamespaceTools)
        ));
        let nested = NamespaceTool::new(
            "crm",
            "desc",
            vec![NamespaceToolEntry::from(
                FunctionTool::new("lookup").allowed_callers(Vec::<String>::new()),
            )],
        );
        assert!(matches!(
            nested.validate(),
            Err(CreateResponseConstraintError::EmptyAllowedCallers)
        ));

        let valid_tunnel = McpTool::tunnel("crm", "tunnel_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        valid_tunnel
            .validate()
            .expect("pinned tunnel_id is accepted");
        let bad_tunnel = McpTool::tunnel("crm", "not-a-tunnel");
        assert!(matches!(
            bad_tunnel.validate(),
            Err(CreateResponseConstraintError::McpTunnelId)
        ));
        let decoded: McpTool = serde_json::from_value(json!({
            "type": "mcp",
            "server_label": "crm",
            "tunnel_id": "tunnel_short"
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());
    }

    #[test]
    fn namespace_tool_entries_are_function_or_custom_only() {
        // The pinned NamespaceToolParam.tools.items union is
        // oneOf [FunctionToolParam, CustomToolParam]; hosted tools such as
        // web_search cannot be constructed for this position, and a nested
        // hosted-tool tag decodes losslessly as Unknown rather than silently
        // becoming a valid namespace member.
        let namespace = NamespaceTool::new(
            "crm",
            "CRM tools",
            vec![
                NamespaceToolEntry::from(FunctionTool::new("lookup")),
                NamespaceToolEntry::from(CustomTool::new("render")),
            ],
        );
        let value = serde_json::to_value(&namespace).expect("serialize namespace");
        assert_eq!(value["type"], "namespace");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][1]["type"], "custom");
        namespace
            .validate()
            .expect("function/custom entries are valid");
        assert_eq!(namespace.tools().len(), 2);

        let decoded: NamespaceTool =
            serde_json::from_value(value.clone()).expect("decode namespace");
        assert_eq!(
            serde_json::to_value(&decoded).expect("round-trip namespace"),
            value
        );

        let nested_hosted: NamespaceTool = serde_json::from_value(json!({
            "type": "namespace",
            "name": "crm",
            "description": "CRM tools",
            "tools": [
                {"type": "web_search"},
                {"type": "future_nested_tool", "name": "new"}
            ]
        }))
        .expect("unofficial nested tags stay lossless");
        assert!(matches!(
            nested_hosted.tools()[0],
            NamespaceToolEntry::Unknown(_)
        ));
        assert!(matches!(
            nested_hosted.tools()[1],
            NamespaceToolEntry::Unknown(_)
        ));
        assert_eq!(
            serde_json::to_value(&nested_hosted).expect("round-trip unknown nested tools")["tools"]
                [0]["type"],
            "web_search"
        );
    }

    #[test]
    fn audio_events_decode_without_ghost_response_id() {
        let done = json!({"type": "response.audio.done", "sequence_number": 1});
        let decoded: AudioDoneEvent =
            serde_json::from_value(done.clone()).expect("decode audio done");
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), done);

        let with_ghost = json!({
            "type": "response.audio.done",
            "sequence_number": 2,
            "response_id": "resp_1"
        });
        let decoded: AudioDoneEvent =
            serde_json::from_value(with_ghost.clone()).expect("preserve ghost response_id");
        assert_eq!(
            decoded.extra_fields().get("response_id"),
            Some(&json!("resp_1"))
        );
        assert_eq!(
            serde_json::to_value(&decoded).expect("round trip ghost"),
            with_ghost
        );
    }

    #[test]
    fn shell_output_content_events_use_object_payloads() {
        let delta = json!({
            "type": "response.shell_call_output_content.delta",
            "item_id": "shell_1",
            "output_index": 0,
            "command_index": 0,
            "delta": {"stdout": "hello"},
            "sequence_number": 4
        });
        let decoded: ShellOutputContentDeltaEvent =
            serde_json::from_value(delta.clone()).expect("decode shell delta");
        assert_eq!(decoded.delta().stdout(), Some("hello"));
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), delta);

        let done = json!({
            "type": "response.shell_call_output_content.done",
            "item_id": "shell_1",
            "output_index": 0,
            "command_index": 0,
            "output": [{
                "stdout": "hello",
                "stderr": "",
                "outcome": {"type": "exit", "exit_code": 0}
            }],
            "sequence_number": 5
        });
        let decoded: ShellOutputContentDoneEvent =
            serde_json::from_value(done.clone()).expect("decode shell done");
        assert_eq!(decoded.output().len(), 1);
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), done);
    }

    #[test]
    fn local_shell_output_and_mcp_approval_decode_without_ghost_fields() {
        let shell = json!({
            "type": "local_shell_call_output",
            "id": "lso_1",
            "output": "ok"
        });
        let decoded: LocalShellCallOutput =
            serde_json::from_value(shell.clone()).expect("decode without ghost call_id");
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), shell);

        let approval = json!({
            "type": "mcp_approval_response",
            "id": "mcp_resp_1",
            "approval_request_id": "mcpr_1",
            "approve": true
        });
        let decoded: McpApprovalResponseResource =
            serde_json::from_value(approval.clone()).expect("decode without ghost request_id");
        assert_eq!(
            serde_json::to_value(&decoded).expect("round trip"),
            approval
        );
    }

    #[test]
    fn mcp_approval_output_fixture_preserves_the_ghost_request_id() {
        // OVR-0007 / D0008 / D0111: the pinned schema lists `request_id` in
        // `required` without ever defining it. The frozen synthetic output
        // carries the ghost field; the typed DTO must decode without
        // fabricating it while the flattened extra fields keep it verbatim
        // through a re-encode.
        let decoded: McpApprovalResponse = serde_json::from_str(MCP_APPROVAL_OUTPUT_FIXTURE)
            .expect("mcp-approval output fixture decodes");
        assert_eq!(decoded.approval_request_id(), "mcpr_synthetic_1");
        assert!(decoded.is_approved());
        assert_eq!(decoded.id(), None);
        assert_eq!(decoded.reason(), None);

        let re_encoded = serde_json::to_value(&decoded).expect("re-encode approval");
        assert_eq!(re_encoded["request_id"], json!("req_synthetic_1"));
        assert_eq!(
            re_encoded,
            serde_json::from_str::<Value>(MCP_APPROVAL_OUTPUT_FIXTURE).expect("fixture is JSON")
        );
    }

    #[test]
    fn mcp_approval_output_fixture_without_request_id_round_trips_both_shapes() {
        // The sibling fixture drops the ghost field entirely: both the input
        // DTO and the output resource must stay decodable without it, and
        // neither may re-encode a fabricated `request_id` (D0111).
        let dto: McpApprovalResponse =
            serde_json::from_str(MCP_APPROVAL_OUTPUT_WITHOUT_REQUEST_ID_FIXTURE)
                .expect("input DTO decodes without request_id");
        assert_eq!(dto.id(), Some("mcp_resp_synthetic_1"));
        assert_eq!(dto.approval_request_id(), "mcpr_synthetic_1");
        assert!(dto.is_approved());
        assert_eq!(
            serde_json::to_value(&dto).expect("re-encode input DTO"),
            serde_json::from_str::<Value>(MCP_APPROVAL_OUTPUT_WITHOUT_REQUEST_ID_FIXTURE)
                .expect("fixture is JSON")
        );

        let resource: McpApprovalResponseResource =
            serde_json::from_str(MCP_APPROVAL_OUTPUT_WITHOUT_REQUEST_ID_FIXTURE)
                .expect("output resource decodes without request_id");
        assert_eq!(resource.reason(), None);
        assert!(resource.extra_fields().get("request_id").is_none());
        assert_eq!(
            serde_json::to_value(&resource).expect("re-encode resource"),
            serde_json::from_str::<Value>(MCP_APPROVAL_OUTPUT_WITHOUT_REQUEST_ID_FIXTURE)
                .expect("fixture is JSON")
        );
    }

    #[test]
    fn event_logprobs_omit_bytes_and_optional_top_logprobs() {
        let fixture = json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hi",
            "sequence_number": 6,
            "logprobs": [{"token": "Hi", "logprob": -0.1}]
        });
        let decoded: OutputTextDeltaEvent =
            serde_json::from_value(fixture.clone()).expect("decode event logprobs");
        assert_eq!(decoded.logprobs()[0].token(), "Hi");
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), fixture);
    }

    #[test]
    fn incomplete_details_reason_is_optional() {
        let fixture = json!({});
        let decoded: IncompleteDetails =
            serde_json::from_value(fixture.clone()).expect("decode empty incomplete details");
        assert_eq!(decoded.reason(), None);
        assert_eq!(serde_json::to_value(&decoded).expect("round trip"), fixture);
    }

    fn exhaustive_event_response_value() -> Value {
        json!({
            "id": "resp_exhaustive",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-test",
            "object": "response",
            "output": [],
            "parallel_tool_calls": true,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null
        })
    }

    fn lifecycle_stream_event(tag: &str) -> Value {
        json!({
            "type": tag,
            "sequence_number": 1,
            "response": exhaustive_event_response_value()
        })
    }

    fn item_snapshot_stream_event(tag: &str) -> Value {
        json!({
            "type": tag,
            "output_index": 0,
            "item": {
                "type": "message",
                "id": "msg_exhaustive",
                "status": "completed",
                "role": "assistant",
                "content": []
            },
            "sequence_number": 1
        })
    }

    fn content_part_stream_event(tag: &str) -> Value {
        json!({
            "type": tag,
            "item_id": "msg_exhaustive",
            "output_index": 0,
            "content_index": 0,
            "part": {"type": "output_text", "text": "hi", "annotations": [], "logprobs": []},
            "sequence_number": 1
        })
    }

    fn tool_status_stream_event(tag: &str) -> Value {
        json!({
            "type": tag,
            "item_id": "tool_exhaustive",
            "output_index": 0,
            "sequence_number": 1
        })
    }

    /// Branch predicate for one pinned stream-event discriminator.
    type StreamEventBranch = fn(&ResponseStreamEvent) -> bool;

    #[test]
    fn every_stable_stream_event_tag_decodes_routes_and_reencodes() {
        // Exhaustive positive decode coverage for all 58 pinned SSE tags
        // (8-04): each entry carries the minimal legal payload for its tag,
        // asserts the tagged union routes it to the typed branch (a payload
        // silently downgraded to `Unknown` would keep its `type` on
        // re-encode, so branch identity needs its own predicate), and
        // re-encodes equal.
        let table: &[(&str, Value, StreamEventBranch)] = &[
            // Audio family: official events carry no output binding fields.
            (
                "response.audio.delta",
                json!({"type": "response.audio.delta", "delta": "==audio==", "sequence_number": 1}),
                |event| matches!(event, ResponseStreamEvent::AudioDelta(_)),
            ),
            (
                "response.audio.done",
                json!({"type": "response.audio.done", "sequence_number": 1}),
                |event| matches!(event, ResponseStreamEvent::AudioDone(_)),
            ),
            (
                "response.audio.transcript.delta",
                json!({
                    "type": "response.audio.transcript.delta",
                    "delta": "spoken",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::AudioTranscriptDelta(_)),
            ),
            (
                "response.audio.transcript.done",
                json!({"type": "response.audio.transcript.done", "sequence_number": 1}),
                |event| matches!(event, ResponseStreamEvent::AudioTranscriptDone(_)),
            ),
            // Code-interpreter hosted tool: code delta/done plus lifecycle trio.
            (
                "response.code_interpreter_call_code.delta",
                json!({
                    "type": "response.code_interpreter_call_code.delta",
                    "output_index": 0,
                    "item_id": "ci_exhaustive",
                    "delta": "print(1)",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::CodeInterpreterCodeDelta(_)),
            ),
            (
                "response.code_interpreter_call_code.done",
                json!({
                    "type": "response.code_interpreter_call_code.done",
                    "output_index": 0,
                    "item_id": "ci_exhaustive",
                    "code": "print(1)",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::CodeInterpreterCodeDone(_)),
            ),
            (
                "response.code_interpreter_call.completed",
                tool_status_stream_event("response.code_interpreter_call.completed"),
                |event| matches!(event, ResponseStreamEvent::CodeInterpreterCompleted(_)),
            ),
            (
                "response.code_interpreter_call.in_progress",
                tool_status_stream_event("response.code_interpreter_call.in_progress"),
                |event| matches!(event, ResponseStreamEvent::CodeInterpreterInProgress(_)),
            ),
            (
                "response.code_interpreter_call.interpreting",
                tool_status_stream_event("response.code_interpreter_call.interpreting"),
                |event| matches!(event, ResponseStreamEvent::CodeInterpreterInterpreting(_)),
            ),
            // Response lifecycle, including the queued/in_progress snapshots.
            (
                "response.queued",
                lifecycle_stream_event("response.queued"),
                |event| matches!(event, ResponseStreamEvent::Queued(_)),
            ),
            (
                "response.created",
                lifecycle_stream_event("response.created"),
                |event| matches!(event, ResponseStreamEvent::Created(_)),
            ),
            (
                "response.in_progress",
                lifecycle_stream_event("response.in_progress"),
                |event| matches!(event, ResponseStreamEvent::InProgress(_)),
            ),
            (
                "response.completed",
                lifecycle_stream_event("response.completed"),
                |event| matches!(event, ResponseStreamEvent::Completed(_)),
            ),
            (
                "response.failed",
                lifecycle_stream_event("response.failed"),
                |event| matches!(event, ResponseStreamEvent::Failed(_)),
            ),
            (
                "response.incomplete",
                lifecycle_stream_event("response.incomplete"),
                |event| matches!(event, ResponseStreamEvent::Incomplete(_)),
            ),
            // Item and content-part snapshots.
            (
                "response.output_item.added",
                item_snapshot_stream_event("response.output_item.added"),
                |event| matches!(event, ResponseStreamEvent::OutputItemAdded(_)),
            ),
            (
                "response.output_item.done",
                item_snapshot_stream_event("response.output_item.done"),
                |event| matches!(event, ResponseStreamEvent::OutputItemDone(_)),
            ),
            (
                "response.content_part.added",
                content_part_stream_event("response.content_part.added"),
                |event| matches!(event, ResponseStreamEvent::ContentPartAdded(_)),
            ),
            (
                "response.content_part.done",
                content_part_stream_event("response.content_part.done"),
                |event| matches!(event, ResponseStreamEvent::ContentPartDone(_)),
            ),
            (
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "item_id": "msg_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "hi",
                    "logprobs": [],
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::OutputTextDelta(_)),
            ),
            (
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": "msg_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "text": "hi",
                    "logprobs": [],
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::OutputTextDone(_)),
            ),
            // Refusal delta family.
            (
                "response.refusal.delta",
                json!({
                    "type": "response.refusal.delta",
                    "item_id": "msg_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "cannot",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::RefusalDelta(_)),
            ),
            (
                "response.refusal.done",
                json!({
                    "type": "response.refusal.done",
                    "item_id": "msg_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "refusal": "cannot help",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::RefusalDone(_)),
            ),
            // Function-call argument deltas.
            (
                "response.function_call_arguments.delta",
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "fc_exhaustive",
                    "output_index": 0,
                    "delta": "{\"city\":",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::FunctionCallArgumentsDelta(_)),
            ),
            (
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_exhaustive",
                    "output_index": 0,
                    "name": "weather",
                    "arguments": "{}",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::FunctionCallArgumentsDone(_)),
            ),
            // File-search hosted-tool lifecycle trio.
            (
                "response.file_search_call.completed",
                tool_status_stream_event("response.file_search_call.completed"),
                |event| matches!(event, ResponseStreamEvent::FileSearchCompleted(_)),
            ),
            (
                "response.file_search_call.in_progress",
                tool_status_stream_event("response.file_search_call.in_progress"),
                |event| matches!(event, ResponseStreamEvent::FileSearchInProgress(_)),
            ),
            (
                "response.file_search_call.searching",
                tool_status_stream_event("response.file_search_call.searching"),
                |event| matches!(event, ResponseStreamEvent::FileSearchSearching(_)),
            ),
            // Shell command and shell output content families.
            (
                "response.shell_call_command.added",
                json!({
                    "type": "response.shell_call_command.added",
                    "output_index": 0,
                    "command_index": 0,
                    "command": "ls",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ShellCommandAdded(_)),
            ),
            (
                "response.shell_call_command.delta",
                json!({
                    "type": "response.shell_call_command.delta",
                    "output_index": 0,
                    "command_index": 0,
                    "delta": "-la",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ShellCommandDelta(_)),
            ),
            (
                "response.shell_call_command.done",
                json!({
                    "type": "response.shell_call_command.done",
                    "output_index": 0,
                    "command_index": 0,
                    "command": "ls -la",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ShellCommandDone(_)),
            ),
            (
                "response.shell_call_output_content.delta",
                json!({
                    "type": "response.shell_call_output_content.delta",
                    "item_id": "shell_exhaustive",
                    "output_index": 0,
                    "command_index": 0,
                    "delta": {},
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ShellOutputContentDelta(_)),
            ),
            (
                "response.shell_call_output_content.done",
                json!({
                    "type": "response.shell_call_output_content.done",
                    "item_id": "shell_exhaustive",
                    "output_index": 0,
                    "command_index": 0,
                    "output": [],
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ShellOutputContentDone(_)),
            ),
            // Reasoning summary part/text and reasoning text delta families.
            (
                "response.reasoning_summary_part.added",
                json!({
                    "type": "response.reasoning_summary_part.added",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "summary_index": 0,
                    "part": {"type": "summary_text", "text": "thought"},
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningSummaryPartAdded(_)),
            ),
            (
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "summary_index": 0,
                    "part": {"type": "summary_text", "text": "thought"},
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningSummaryPartDone(_)),
            ),
            (
                "response.reasoning_summary_text.delta",
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "summary_index": 0,
                    "delta": "thought",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningSummaryTextDelta(_)),
            ),
            (
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "summary_index": 0,
                    "text": "thought",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningSummaryTextDone(_)),
            ),
            (
                "response.reasoning_text.delta",
                json!({
                    "type": "response.reasoning_text.delta",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "thinking",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningTextDelta(_)),
            ),
            (
                "response.reasoning_text.done",
                json!({
                    "type": "response.reasoning_text.done",
                    "item_id": "rs_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "text": "thinking",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ReasoningTextDone(_)),
            ),
            // Web-search hosted-tool lifecycle trio.
            (
                "response.web_search_call.completed",
                tool_status_stream_event("response.web_search_call.completed"),
                |event| matches!(event, ResponseStreamEvent::WebSearchCompleted(_)),
            ),
            (
                "response.web_search_call.in_progress",
                tool_status_stream_event("response.web_search_call.in_progress"),
                |event| matches!(event, ResponseStreamEvent::WebSearchInProgress(_)),
            ),
            (
                "response.web_search_call.searching",
                tool_status_stream_event("response.web_search_call.searching"),
                |event| matches!(event, ResponseStreamEvent::WebSearchSearching(_)),
            ),
            // Image-generation hosted tool: lifecycle trio plus partial image.
            (
                "response.image_generation_call.completed",
                tool_status_stream_event("response.image_generation_call.completed"),
                |event| matches!(event, ResponseStreamEvent::ImageGenerationCompleted(_)),
            ),
            (
                "response.image_generation_call.generating",
                tool_status_stream_event("response.image_generation_call.generating"),
                |event| matches!(event, ResponseStreamEvent::ImageGenerationGenerating(_)),
            ),
            (
                "response.image_generation_call.in_progress",
                tool_status_stream_event("response.image_generation_call.in_progress"),
                |event| matches!(event, ResponseStreamEvent::ImageGenerationInProgress(_)),
            ),
            (
                "response.image_generation_call.partial_image",
                json!({
                    "type": "response.image_generation_call.partial_image",
                    "output_index": 0,
                    "item_id": "img_exhaustive",
                    "partial_image_index": 0,
                    "partial_image_b64": "cGFydGlhbA==",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::ImageGenerationPartialImage(_)),
            ),
            // Native remote MCP call and list-tools families.
            (
                "response.mcp_call_arguments.delta",
                json!({
                    "type": "response.mcp_call_arguments.delta",
                    "item_id": "mcp_exhaustive",
                    "output_index": 0,
                    "delta": "{\"query\":",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::McpCallArgumentsDelta(_)),
            ),
            (
                "response.mcp_call_arguments.done",
                json!({
                    "type": "response.mcp_call_arguments.done",
                    "item_id": "mcp_exhaustive",
                    "output_index": 0,
                    "arguments": "{}",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::McpCallArgumentsDone(_)),
            ),
            (
                "response.mcp_call.in_progress",
                tool_status_stream_event("response.mcp_call.in_progress"),
                |event| matches!(event, ResponseStreamEvent::McpCallInProgress(_)),
            ),
            (
                "response.mcp_call.completed",
                tool_status_stream_event("response.mcp_call.completed"),
                |event| matches!(event, ResponseStreamEvent::McpCallCompleted(_)),
            ),
            (
                "response.mcp_call.failed",
                tool_status_stream_event("response.mcp_call.failed"),
                |event| matches!(event, ResponseStreamEvent::McpCallFailed(_)),
            ),
            (
                "response.mcp_list_tools.in_progress",
                tool_status_stream_event("response.mcp_list_tools.in_progress"),
                |event| matches!(event, ResponseStreamEvent::McpListToolsInProgress(_)),
            ),
            (
                "response.mcp_list_tools.completed",
                tool_status_stream_event("response.mcp_list_tools.completed"),
                |event| matches!(event, ResponseStreamEvent::McpListToolsCompleted(_)),
            ),
            (
                "response.mcp_list_tools.failed",
                tool_status_stream_event("response.mcp_list_tools.failed"),
                |event| matches!(event, ResponseStreamEvent::McpListToolsFailed(_)),
            ),
            // Annotation and custom-tool input deltas.
            (
                "response.output_text.annotation.added",
                json!({
                    "type": "response.output_text.annotation.added",
                    "item_id": "msg_exhaustive",
                    "output_index": 0,
                    "content_index": 0,
                    "annotation_index": 0,
                    "annotation": {
                        "type": "file_path",
                        "file_id": "file_exhaustive",
                        "index": 0
                    },
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::OutputTextAnnotationAdded(_)),
            ),
            (
                "response.custom_tool_call_input.delta",
                json!({
                    "type": "response.custom_tool_call_input.delta",
                    "output_index": 0,
                    "item_id": "ct_exhaustive",
                    "delta": "{\"rows\":",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::CustomToolCallInputDelta(_)),
            ),
            (
                "response.custom_tool_call_input.done",
                json!({
                    "type": "response.custom_tool_call_input.done",
                    "output_index": 0,
                    "item_id": "ct_exhaustive",
                    "input": "{\"rows\": []}",
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::CustomToolCallInputDone(_)),
            ),
            // Standalone SSE error event.
            (
                "error",
                json!({
                    "type": "error",
                    "code": null,
                    "message": "exhaustive",
                    "param": null,
                    "sequence_number": 1
                }),
                |event| matches!(event, ResponseStreamEvent::Error(_)),
            ),
        ];

        let mut table_tags: Vec<&str> = table.iter().map(|(tag, _, _)| *tag).collect();
        table_tags.sort_unstable();
        let mut pinned: Vec<&str> = STABLE_RESPONSE_STREAM_EVENT_DISCRIMINATORS.to_vec();
        pinned.sort_unstable();
        assert_eq!(
            table_tags, pinned,
            "the exhaustive fixture table must track the pinned discriminator manifest"
        );

        for (tag, payload, is_branch) in table {
            let decoded: ResponseStreamEvent = serde_json::from_value(payload.clone())
                .unwrap_or_else(|error| panic!("decode {tag}: {error}"));
            assert!(
                is_branch(&decoded),
                "tag {tag} must route to its typed branch instead of {decoded:?}"
            );
            assert_eq!(decoded.sequence_number(), Some(1), "tag {tag}");
            assert_eq!(
                serde_json::to_value(&decoded)
                    .unwrap_or_else(|error| panic!("encode {tag}: {error}")),
                *payload,
                "tag {tag} must re-encode to its minimal payload"
            );
        }
    }

    /// Branch predicate for one annotation discriminator.
    type AnnotationBranch = fn(&Annotation) -> bool;

    #[test]
    fn annotation_branches_round_trip_all_four_citation_shapes() {
        let fixtures: [(Value, AnnotationBranch); 4] = [
            (
                json!({
                    "type": "file_citation",
                    "file_id": "file_cited",
                    "index": 3,
                    "filename": "source.txt"
                }),
                |annotation: &Annotation| matches!(annotation, Annotation::FileCitation(_)),
            ),
            (
                json!({
                    "type": "url_citation",
                    "url": "https://example.test/doc",
                    "start_index": 0,
                    "end_index": 4,
                    "title": "Example"
                }),
                |annotation: &Annotation| matches!(annotation, Annotation::UrlCitation(_)),
            ),
            (
                json!({
                    "type": "container_file_citation",
                    "container_id": "cntr_cited",
                    "file_id": "file_cited",
                    "start_index": 0,
                    "end_index": 4,
                    "filename": "container.txt"
                }),
                |annotation: &Annotation| {
                    matches!(annotation, Annotation::ContainerFileCitation(_))
                },
            ),
            (
                json!({
                    "type": "file_path",
                    "file_id": "file_cited",
                    "index": 5
                }),
                |annotation: &Annotation| matches!(annotation, Annotation::FilePath(_)),
            ),
        ];

        let mut embedded = Vec::new();
        for (payload, is_branch) in fixtures {
            let decoded: Annotation = serde_json::from_value(payload.clone())
                .unwrap_or_else(|error| panic!("decode annotation {}: {error}", payload["type"]));
            assert!(
                is_branch(&decoded),
                "annotation {} must decode to its typed branch",
                payload["type"]
            );
            assert_eq!(
                serde_json::to_value(&decoded).expect("re-encode annotation"),
                payload
            );
            embedded.push(payload);
        }

        // The same four shapes stay lossless inside an annotated output part.
        let part = json!({
            "type": "output_text",
            "text": "cited four ways",
            "annotations": embedded,
            "logprobs": []
        });
        let decoded: OutputContent =
            serde_json::from_value(part.clone()).expect("decode annotated output part");
        let OutputContent::Text(text) = &decoded else {
            panic!("output_text part must decode to the text branch");
        };
        assert_eq!(text.annotations().len(), 4);
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode annotated output part"),
            part
        );
    }

    #[test]
    fn custom_tool_text_format_decodes_and_round_trips() {
        // 8-R1: the `text` format branch (unconstrained input) was previously
        // exercised only through the grammar variant.
        let text = json!({"type": "text"});
        let decoded: CustomToolFormat =
            serde_json::from_value(text.clone()).expect("decode text format");
        assert!(matches!(decoded, CustomToolFormat::Text(_)));
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode text format"),
            text
        );

        let future = json!({"type": "future_format", "detail": {"nested": true}});
        let decoded: CustomToolFormat =
            serde_json::from_value(future.clone()).expect("decode future format");
        assert!(matches!(decoded, CustomToolFormat::Unknown(_)));
        assert_eq!(
            serde_json::to_value(&decoded).expect("re-encode future format"),
            future
        );

        let tool = json!({
            "type": "custom",
            "name": "render",
            "format": {"type": "text"}
        });
        let decoded: CustomTool =
            serde_json::from_value(tool.clone()).expect("decode custom tool with text format");
        assert_eq!(
            serde_json::to_value(&decoded).expect("round-trip custom tool"),
            tool
        );
    }

    #[test]
    fn stream_input_and_tool_unions_reject_malformed_discriminators() {
        // 8-06: the three shapes that must never be downgraded to `Unknown` —
        // a non-object, an object without `type`, and a non-string `type` —
        // pinned at the DTO layer for the two kernel-tagged unions.
        let malformed = [
            json!("response.completed"),
            json!([1, 2]),
            json!({"sequence_number": 1}),
            json!({"type": 7}),
            json!({"type": null}),
        ];
        for payload in &malformed {
            assert!(
                serde_json::from_value::<ResponseStreamEvent>(payload.clone()).is_err(),
                "stream event must reject {payload}"
            );
            assert!(
                serde_json::from_value::<ResponseTool>(payload.clone()).is_err(),
                "response tool must reject {payload}"
            );
        }

        let non_object = serde_json::from_value::<ResponseStreamEvent>(json!("response.completed"))
            .expect_err("a string is not a tagged object");
        assert!(non_object.to_string().contains("must be a JSON object"));
        let missing_type = serde_json::from_value::<ResponseTool>(json!({"strict": true}))
            .expect_err("missing type");
        assert!(
            missing_type
                .to_string()
                .contains("missing string field `type`")
        );
        let non_string = serde_json::from_value::<ResponseStreamEvent>(json!({"type": 7}))
            .expect_err("numeric type");
        assert!(non_string.to_string().contains("`type` must be a string"));

        // The input-item union hand-rolls its discriminator and additionally
        // accepts untagged `role`/`id` forms, so its rejection shapes are
        // pinned separately: only an object with none of `type`/`role`/`id`
        // counts as missing the discriminator.
        for payload in [
            json!("function_call"),
            json!(7),
            json!({"output_index": 0}),
            json!({"type": 7}),
            json!({"type": null}),
        ] {
            assert!(
                serde_json::from_value::<ResponseInputItem>(payload.clone()).is_err(),
                "input item must reject {payload}"
            );
        }
        let missing_tag = serde_json::from_value::<ResponseInputItem>(json!({"output_index": 0}))
            .expect_err("an object without type, role, or id is not an input item");
        assert!(missing_tag.to_string().contains("missing `type`"));
        let non_string_tag = serde_json::from_value::<ResponseInputItem>(json!({"type": 7}))
            .expect_err("input item type must be a string");
        assert!(
            non_string_tag
                .to_string()
                .contains("`type` must be a string")
        );
    }
}
