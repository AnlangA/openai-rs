//! Wire types for the OpenAI Responses API.
//!
//! The types in this module intentionally mirror the JSON protocol. Request
//! constructors and builders keep the common path free of hand-written JSON,
//! while response unions retain future tagged variants without hiding malformed
//! payloads for tags that this crate already knows.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{ExtraFields, JsonText, Nullable, Omittable};

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
            /// A value added by the service after this crate was released.
            Unknown(Box<str>),
        }

        impl $name {
            /// Returns the exact string carried on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Unknown(value) => value,
                }
            }

            /// Preserves an arbitrary wire value, recognizing current values.
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

macro_rules! tagged_union {
    ($(#[$meta:meta])* pub enum $name:ident {
        $($variant:ident($ty:ty) => $wire:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// A future variant retained as a complete semantic JSON object.
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
                let discriminator = object_discriminator(&value).map_err(D::Error::custom)?;
                match discriminator.as_str() {
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

fn object_discriminator(value: &Value) -> Result<String, &'static str> {
    let object = value
        .as_object()
        .ok_or("tagged value must be a JSON object")?;
    object
        .get("type")
        .ok_or("tagged object is missing string field `type`")?
        .as_str()
        .map(str::to_owned)
        .ok_or("tagged object field `type` must be a string")
}

/// A future tagged object, including its discriminator and every raw field.
///
/// The map is immutable through the public API, so `discriminator` can never
/// drift from its retained `type` property.
#[derive(Clone, PartialEq)]
pub struct UnknownTaggedObject {
    discriminator: Box<str>,
    raw: Map<String, Value>,
}

impl UnknownTaggedObject {
    /// Validates and retains an unknown tagged JSON object.
    pub fn from_value(value: Value) -> Result<Self, UnknownTaggedObjectError> {
        let discriminator = object_discriminator(&value)
            .map_err(UnknownTaggedObjectError::Invalid)?
            .into_boxed_str();
        let Value::Object(raw) = value else {
            return Err(UnknownTaggedObjectError::Invalid(
                "tagged value must be a JSON object",
            ));
        };
        Ok(Self { discriminator, raw })
    }

    /// Returns the exact unknown discriminator.
    #[must_use]
    pub fn discriminator(&self) -> &str {
        &self.discriminator
    }

    /// Borrows all retained object fields, including `type`.
    #[must_use]
    pub const fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }

    /// Converts this value back into its semantic JSON object.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

impl fmt::Debug for UnknownTaggedObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnknownTaggedObject")
            .field("discriminator", &self.discriminator)
            .field("field_count", &self.raw.len())
            .finish()
    }
}

impl Serialize for UnknownTaggedObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UnknownTaggedObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A supplied value was not a tagged JSON object.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnknownTaggedObjectError {
    /// The discriminator was absent or had the wrong JSON kind.
    #[error("{0}")]
    Invalid(&'static str),
}

open_string_enum! {
    /// Lifecycle state of a response.
    pub enum ResponseStatus {
        Queued => "queued",
        InProgress => "in_progress",
        Completed => "completed",
        Failed => "failed",
        Incomplete => "incomplete",
        Cancelled => "cancelled"
    }
}

open_string_enum! {
    /// Lifecycle state of one response item.
    pub enum ResponseItemStatus {
        InProgress => "in_progress",
        Completed => "completed",
        Incomplete => "incomplete",
        Failed => "failed"
    }
}

open_string_enum! {
    /// Role assigned to a Responses message.
    pub enum MessageRole {
        User => "user",
        Assistant => "assistant",
        System => "system",
        Developer => "developer"
    }
}

open_string_enum! {
    /// Requested image fidelity.
    pub enum ImageDetail {
        Auto => "auto",
        Low => "low",
        High => "high",
        Original => "original"
    }
}

open_string_enum! {
    /// Context truncation policy.
    pub enum TruncationStrategy {
        Auto => "auto",
        Disabled => "disabled"
    }
}

open_string_enum! {
    /// Amount of reasoning requested from a compatible model.
    pub enum ReasoningEffort {
        None => "none",
        Minimal => "minimal",
        Low => "low",
        Medium => "medium",
        High => "high",
        XHigh => "xhigh"
    }
}

open_string_enum! {
    /// Requested form of a reasoning summary.
    pub enum ReasoningSummary {
        Auto => "auto",
        Concise => "concise",
        Detailed => "detailed"
    }
}

open_string_enum! {
    /// Why a response stopped before completing.
    pub enum IncompleteReason {
        MaxOutputTokens => "max_output_tokens",
        ContentFilter => "content_filter"
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
            extra: ExtraFields::new(),
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
}

/// An image input addressed by URL or uploaded file id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputImage {
    #[serde(rename = "type")]
    kind: InputImageTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<ImageDetail>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputImage {
    /// Creates an image input from a URL or data URL.
    #[must_use]
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            image_url: Omittable::Value(url.into()),
            file_id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates an image input from an uploaded file id.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        Self {
            kind: InputImageTag::InputImage,
            detail: Omittable::Omitted,
            image_url: Omittable::Omitted,
            file_id: Omittable::Value(file_id.into()),
            extra: ExtraFields::new(),
        }
    }

    /// Sets the requested fidelity.
    #[must_use]
    pub fn detail(mut self, detail: ImageDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }

    /// Returns the image URL when present.
    #[must_use]
    pub fn image_url(&self) -> Option<&str> {
        match &self.image_url {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the uploaded file id when present.
    #[must_use]
    pub fn file_id(&self) -> Option<&str> {
        match &self.file_id {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A file input addressed by URL, uploaded id, or base64 file data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputFile {
    #[serde(rename = "type")]
    kind: InputFileTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_data: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filename: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<ImageDetail>,
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
            extra: ExtraFields::new(),
        }
    }

    /// Creates a file input from an uploaded file id.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_id = Omittable::Value(file_id.into());
        value
    }

    /// Creates a file input from a remote URL.
    #[must_use]
    pub fn from_url(file_url: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_url = Omittable::Value(file_url.into());
        value
    }

    /// Creates a file input from base64 data and a filename.
    #[must_use]
    pub fn from_base64(file_data: impl Into<String>, filename: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_data = Omittable::Value(file_data.into());
        value.filename = Omittable::Value(filename.into());
        value
    }

    /// Sets the file/image detail preference.
    #[must_use]
    pub fn detail(mut self, detail: ImageDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

tagged_union! {
    /// One rich content part in an input message.
    pub enum InputContent {
        Text(InputText) => "input_text",
        Image(InputImage) => "input_image",
        File(InputFile) => "input_file"
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

/// Text or an ordered list of rich message content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain text content.
    Text(String),
    /// Rich content parts.
    Parts(Vec<InputContent>),
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

impl From<Vec<InputContent>> for MessageContent {
    fn from(value: Vec<InputContent>) -> Self {
        Self::Parts(value)
    }
}

/// A Responses input message.
///
/// The service accepts request messages without an explicit `type`; the
/// constructor emits that compact shape. Decoding also accepts an explicit
/// `"type":"message"` and validates it when present.
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
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputMessage {
    /// Creates a message for the supplied role.
    #[must_use]
    pub fn new(role: MessageRole, content: impl Into<MessageContent>) -> Self {
        Self {
            kind: Omittable::Omitted,
            role,
            content: content.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl Into<MessageContent>) -> Self {
        Self::new(MessageRole::User, content)
    }

    /// Creates a developer message.
    #[must_use]
    pub fn developer(content: impl Into<MessageContent>) -> Self {
        Self::new(MessageRole::Developer, content)
    }

    /// Creates a system message.
    #[must_use]
    pub fn system(content: impl Into<MessageContent>) -> Self {
        Self::new(MessageRole::System, content)
    }

    /// Emits the optional `type: "message"` request property.
    #[must_use]
    pub fn with_type(mut self) -> Self {
        self.kind = Omittable::Value(InputMessageTag::Message);
        self
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
}

/// Role accepted by the stored `InputMessage` schema.
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

/// The item-form input message used inside the expanded `Item` union.
///
/// This differs from [`InputMessage`]'s ergonomic schema: content is always an
/// array, assistant is not an accepted role, and a returned item may carry a
/// status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredInputMessage {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<InputMessageTag>,
    role: StoredInputMessageRole,
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
            role,
            status: Omittable::Omitted,
            content: content.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
        }
    }

    /// Sets the returned item status.
    #[must_use]
    pub fn status(mut self, status: ResponseItemStatus) -> Self {
        self.status = Omittable::Value(status);
        self
    }

    /// Returns content parts.
    #[must_use]
    pub fn content(&self) -> &[InputContent] {
        &self.content
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
    parameters: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_schema: Omittable<Nullable<Value>>,
    strict: Nullable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    defer_loading: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<String>>>,
}

impl FunctionTool {
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
            parameters: Value::Object(parameters),
            output_schema: Omittable::Omitted,
            strict: Nullable::Null,
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
        self.parameters = parameters;
        self
    }

    /// Serializes a schema representation without requiring JSON text.
    pub fn parameters_from<T: Serialize>(
        mut self,
        parameters: &T,
    ) -> Result<Self, serde_json::Error> {
        self.parameters = serde_json::to_value(parameters)?;
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
        self.strict = Nullable::Value(strict);
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
    pub fn allowed_callers(mut self, callers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allowed_callers = Omittable::Value(Nullable::Value(
            callers.into_iter().map(Into::into).collect(),
        ));
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

    /// Returns the parameters JSON Schema.
    #[must_use]
    pub const fn parameters_ref(&self) -> &Value {
        &self.parameters
    }

    /// Returns the explicit strict flag when present.
    #[must_use]
    pub fn is_strict(&self) -> Option<bool> {
        match self.strict {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }
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
    connector_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tunnel_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    authorization: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    headers: Omittable<Nullable<BTreeMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_tools: Omittable<Nullable<McpAllowedTools>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    allowed_callers: Omittable<Nullable<Vec<String>>>,
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
    pub fn connector(server_label: impl Into<String>, connector_id: impl Into<String>) -> Self {
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
    pub fn allowed_callers(mut self, callers: impl IntoIterator<Item = impl Into<String>>) -> Self {
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

    /// Controls deferred loading for compatible models.
    #[must_use]
    pub fn defer_loading(mut self, defer_loading: bool) -> Self {
        self.defer_loading = Omittable::Value(defer_loading);
        self
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
        WebSearch(WebSearchTool) => "web_search",
        Mcp(McpTool) => "mcp",
        CodeInterpreter(CodeInterpreterTool) => "code_interpreter",
        Programmatic(ProgrammaticTool) => "programmatic_tool_calling",
        ImageGeneration(ImageGenerationTool) => "image_generation",
        LocalShell(LocalShellTool) => "local_shell",
        FunctionShell(FunctionShellTool) => "shell",
        Custom(CustomTool) => "custom",
        Namespace(NamespaceTool) => "namespace",
        ToolSearch(ToolSearchTool) => "tool_search",
        WebSearchPreview(WebSearchPreviewTool) => "web_search_preview",
        ApplyPatch(ApplyPatchTool) => "apply_patch"
    }
}

impl From<FunctionTool> for ResponseTool {
    fn from(value: FunctionTool) -> Self {
        Self::Function(value)
    }
}

impl From<McpTool> for ResponseTool {
    fn from(value: McpTool) -> Self {
        Self::Mcp(value)
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
    status: Omittable<ResponseItemStatus>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCall {
    /// Creates a complete function call item.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: JsonText,
        status: ResponseItemStatus,
    ) -> Self {
        Self {
            kind: FunctionCallTag::FunctionCall,
            id: Omittable::Value(id.into()),
            call_id: call_id.into(),
            name: name.into(),
            arguments,
            status: Omittable::Value(status),
            extra: ExtraFields::new(),
        }
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

    /// Returns the item status.
    #[must_use]
    pub fn status(&self) -> Option<&ResponseItemStatus> {
        match &self.status {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// String or rich content supplied as a function call output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FunctionCallOutputValue {
    /// An opaque text value, commonly a JSON string.
    Text(String),
    /// Typed text/image/file content parts.
    Content(Vec<InputContent>),
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

impl From<Vec<InputContent>> for FunctionCallOutputValue {
    fn from(value: Vec<InputContent>) -> Self {
        Self::Content(value)
    }
}

/// Output supplied for a preceding function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallOutput {
    #[serde(rename = "type")]
    kind: FunctionCallOutputTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    call_id: Omittable<Nullable<String>>,
    output: FunctionCallOutputValue,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    namespace: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    caller: Omittable<Nullable<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<Nullable<ResponseItemStatus>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl FunctionCallOutput {
    /// Creates a function output from an opaque string.
    #[must_use]
    pub fn new(call_id: impl Into<String>, output: impl Into<FunctionCallOutputValue>) -> Self {
        Self {
            kind: FunctionCallOutputTag::FunctionCallOutput,
            call_id: Omittable::Value(Nullable::Value(call_id.into())),
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

    /// Sets an item id for stored input items.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Omittable::Value(Nullable::Value(id.into()));
        self
    }

    /// Sets an item status for stored input items.
    #[must_use]
    pub fn status(mut self, status: ResponseItemStatus) -> Self {
        self.status = Omittable::Value(Nullable::Value(status));
        self
    }

    /// Records the tool name that produced this output.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(Nullable::Value(name.into()));
        self
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
    pub const fn output(&self) -> &FunctionCallOutputValue {
        &self.output
    }

    /// Parses a JSON output into a caller-selected type.
    pub fn deserialize_output<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<T, serde_json::Error> {
        match &self.output {
            FunctionCallOutputValue::Text(output) => serde_json::from_str(output),
            FunctionCallOutputValue::Content(output) => {
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
    #[serde(flatten)]
    extra: ExtraFields,
}

impl McpListedTool {
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
    error: Omittable<Nullable<String>>,
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
}

/// A user decision for a native remote MCP approval request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpApprovalResponse {
    #[serde(rename = "type")]
    kind: McpApprovalResponseTag,
    approval_request_id: String,
    approve: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reason: Omittable<String>,
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
            reason: Omittable::Value(reason.into()),
            extra: ExtraFields::new(),
        }
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
}

literal_tag!(OutputTextTag, OutputText, "output_text");
literal_tag!(RefusalTag, Refusal, "refusal");
literal_tag!(OutputMessageTag, Message, "message");
literal_tag!(AssistantRoleTag, Assistant, "assistant");

/// Text generated by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputText {
    #[serde(rename = "type")]
    kind: OutputTextTag,
    text: String,
    annotations: Vec<Value>,
    logprobs: Vec<Value>,
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
    pub fn annotations(&self) -> &[Value] {
        &self.annotations
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
        Refusal(Refusal) => "refusal"
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

/// A message produced by the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputMessage {
    #[serde(rename = "type")]
    kind: OutputMessageTag,
    id: String,
    status: ResponseItemStatus,
    role: AssistantRoleTag,
    content: Vec<OutputContent>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl OutputMessage {
    /// Creates an assistant message, primarily for adapters and tests.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: ResponseItemStatus,
        content: impl IntoIterator<Item = impl Into<OutputContent>>,
    ) -> Self {
        Self {
            kind: OutputMessageTag::Message,
            id: id.into(),
            status,
            role: AssistantRoleTag::Assistant,
            content: content.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
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
            OutputContent::Refusal(_) | OutputContent::Unknown(_) => None,
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
            Some("message") if object.contains_key("id") => serde_json::from_value(value)
                .map(Self::OutputMessage)
                .map_err(D::Error::custom),
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
    name: Omittable<String>,
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
        self.name = Omittable::Value(name.into());
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
    strict: Omittable<bool>,
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
        self.strict = Omittable::Value(strict);
        self
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

impl Default for TextFormat {
    fn default() -> Self {
        Self::Text(TextFormatText::default())
    }
}

/// Text-generation configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseTextConfig {
    format: TextFormat,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    verbosity: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseTextConfig {
    /// Creates text configuration for a format.
    #[must_use]
    pub fn new(format: TextFormat) -> Self {
        Self {
            format,
            verbosity: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Requests a verbosity value supported by the selected model.
    #[must_use]
    pub fn verbosity(mut self, verbosity: impl Into<String>) -> Self {
        self.verbosity = Omittable::Value(verbosity.into());
        self
    }

    /// Returns the requested format.
    #[must_use]
    pub const fn format(&self) -> &TextFormat {
        &self.format
    }
}

/// Reasoning configuration echoed by a response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    effort: Omittable<Nullable<ReasoningEffort>>,
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

    /// Sets the requested effort.
    #[must_use]
    pub fn effort(mut self, effort: ReasoningEffort) -> Self {
        self.effort = Omittable::Value(Nullable::Value(effort));
        self
    }

    /// Sets the requested summary style.
    #[must_use]
    pub fn summary(mut self, summary: ReasoningSummary) -> Self {
        self.summary = Omittable::Value(Nullable::Value(summary));
        self
    }

    /// Returns the non-null effort when supplied.
    #[must_use]
    pub fn effort_ref(&self) -> Option<&ReasoningEffort> {
        match &self.effort {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
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
    version: Omittable<String>,
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
        self.version = Omittable::Value(version.into());
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
    context_management: Omittable<Nullable<Vec<Value>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Nullable<Vec<String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_output_tokens: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_tool_calls: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    moderation: Omittable<Nullable<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    parallel_tool_calls: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt: Omittable<Nullable<PromptReference>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_key: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_options: Omittable<Value>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_cache_retention: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning: Omittable<Nullable<ReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    safety_identifier: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<String>,
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
    top_logprobs: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_p: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<Nullable<TruncationStrategy>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<String>,
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
    stream_options: Omittable<ResponseStreamOptions>,
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

            /// Associates the response with a conversation.
            #[must_use]
            pub fn conversation(mut self, conversation: impl Into<ConversationReference>) -> Self {
                self.body.conversation = Omittable::Value(Nullable::Value(conversation.into()));
                self
            }

            /// Serializes and adds one context-management rule.
            pub fn context_management<T: Serialize>(
                mut self,
                rule: &T,
            ) -> Result<Self, serde_json::Error> {
                let mut rules = match std::mem::take(&mut self.body.context_management) {
                    Omittable::Value(Nullable::Value(rules)) => rules,
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
                };
                rules.push(serde_json::to_value(rule)?);
                self.body.context_management = Omittable::Value(Nullable::Value(rules));
                Ok(self)
            }

            /// Adds one optional response field to include.
            #[must_use]
            pub fn include(mut self, include: impl Into<String>) -> Self {
                let mut includes = match std::mem::take(&mut self.body.include) {
                    Omittable::Value(Nullable::Value(includes)) => includes,
                    Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
                };
                includes.push(include.into());
                self.body.include = Omittable::Value(Nullable::Value(includes));
                self
            }

            /// Caps generated tokens.
            #[must_use]
            pub fn max_output_tokens(mut self, max_output_tokens: u32) -> Self {
                self.body.max_output_tokens = Omittable::Value(Nullable::Value(max_output_tokens));
                self
            }

            /// Caps total built-in tool calls.
            #[must_use]
            pub fn max_tool_calls(mut self, max_tool_calls: u32) -> Self {
                self.body.max_tool_calls = Omittable::Value(Nullable::Value(max_tool_calls));
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

            /// Serializes moderation configuration without requiring JSON text.
            pub fn moderation<T: Serialize>(
                mut self,
                moderation: &T,
            ) -> Result<Self, serde_json::Error> {
                self.body.moderation =
                    Omittable::Value(Nullable::Value(serde_json::to_value(moderation)?));
                Ok(self)
            }

            /// Controls parallel tool calls.
            #[must_use]
            pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
                self.body.parallel_tool_calls = Omittable::Value(Nullable::Value(enabled));
                self
            }

            /// Continues from a prior response id.
            #[must_use]
            pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
                self.body.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
                self
            }

            /// Uses a reusable prompt template.
            #[must_use]
            pub fn prompt(mut self, prompt: PromptReference) -> Self {
                self.body.prompt = Omittable::Value(Nullable::Value(prompt));
                self
            }

            /// Sets a prompt-cache key.
            #[must_use]
            pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
                self.body.prompt_cache_key = Omittable::Value(Nullable::Value(key.into()));
                self
            }

            /// Serializes prompt-cache options without requiring JSON text.
            pub fn prompt_cache_options<T: Serialize>(
                mut self,
                options: &T,
            ) -> Result<Self, serde_json::Error> {
                self.body.prompt_cache_options = Omittable::Value(serde_json::to_value(options)?);
                Ok(self)
            }

            /// Sets the deprecated prompt-cache retention policy.
            #[must_use]
            pub fn prompt_cache_retention(mut self, retention: impl Into<String>) -> Self {
                self.body.prompt_cache_retention =
                    Omittable::Value(Nullable::Value(retention.into()));
                self
            }

            /// Sets reasoning configuration.
            #[must_use]
            pub fn reasoning(mut self, reasoning: ReasoningConfig) -> Self {
                self.body.reasoning = Omittable::Value(Nullable::Value(reasoning));
                self
            }

            /// Sets an abuse-detection safety identifier.
            #[must_use]
            pub fn safety_identifier(mut self, identifier: impl Into<String>) -> Self {
                self.body.safety_identifier = Omittable::Value(identifier.into());
                self
            }

            /// Requests a service tier.
            #[must_use]
            pub fn service_tier(mut self, service_tier: impl Into<String>) -> Self {
                self.body.service_tier = Omittable::Value(service_tier.into());
                self
            }

            /// Controls response storage.
            #[must_use]
            pub fn store(mut self, store: bool) -> Self {
                self.body.store = Omittable::Value(Nullable::Value(store));
                self
            }

            /// Sets sampling temperature.
            #[must_use]
            pub fn temperature(mut self, temperature: f64) -> Self {
                self.body.temperature = Omittable::Value(Nullable::Value(temperature));
                self
            }

            /// Sets text output configuration.
            #[must_use]
            pub fn text(mut self, text: ResponseTextConfig) -> Self {
                self.body.text = Omittable::Value(text);
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

            /// Requests token log probabilities at each output position.
            #[must_use]
            pub fn top_logprobs(mut self, top_logprobs: u32) -> Self {
                self.body.top_logprobs = Omittable::Value(top_logprobs);
                self
            }

            /// Sets the truncation strategy.
            #[must_use]
            pub fn truncation(mut self, truncation: TruncationStrategy) -> Self {
                self.body.truncation = Omittable::Value(Nullable::Value(truncation));
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
        self.stream_options = Omittable::Value(stream_options);
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

literal_tag!(ResponseObjectTag, Response, "response");

/// An error returned when the model could not generate a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    code: String,
    message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseError {
    /// Returns the machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &str {
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
    reason: IncompleteReason,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl IncompleteDetails {
    /// Returns the incomplete reason.
    #[must_use]
    pub const fn reason(&self) -> &IncompleteReason {
        &self.reason
    }
}

/// Token accounting for model input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputTokensDetails {
    cached_tokens: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    cache_write_tokens: Omittable<u64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl InputTokensDetails {
    /// Returns the number of cached input tokens.
    #[must_use]
    pub const fn cached_tokens(&self) -> u64 {
        self.cached_tokens
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
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ResponseUsage {
    /// Returns total input tokens.
    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Returns total output tokens.
    #[must_use]
    pub const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Returns input plus output tokens.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
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
    background: Omittable<bool>,
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
    reasoning: Omittable<Nullable<ReasoningConfig>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    safety_identifier: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    service_tier: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    store: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<ResponseTextConfig>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    truncation: Omittable<TruncationStrategy>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<Nullable<ResponseUsage>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<Nullable<String>>,
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

    /// Converts replayable output items into the corresponding input items.
    #[must_use]
    pub fn to_input_items(&self) -> Vec<ResponseInputItem> {
        self.output
            .iter()
            .filter_map(|item| match item {
                ResponseOutputItem::Message(value) => {
                    Some(ResponseInputItem::OutputMessage(value.clone()))
                }
                ResponseOutputItem::FunctionCall(value) => {
                    Some(ResponseInputItem::FunctionCall(value.clone()))
                }
                ResponseOutputItem::FileSearchCall(value) => {
                    Some(ResponseInputItem::FileSearchCall(value.clone()))
                }
                ResponseOutputItem::WebSearchCall(value) => {
                    Some(ResponseInputItem::WebSearchCall(value.clone()))
                }
                ResponseOutputItem::ComputerCall(value) => {
                    Some(ResponseInputItem::ComputerCall(value.clone()))
                }
                ResponseOutputItem::Reasoning(value) => {
                    Some(ResponseInputItem::Reasoning(value.clone()))
                }
                ResponseOutputItem::Program(value) => {
                    Some(ResponseInputItem::Program(value.clone()))
                }
                ResponseOutputItem::ProgramOutput(value) => {
                    Some(ResponseInputItem::ProgramOutput(value.clone()))
                }
                ResponseOutputItem::ImageGenerationCall(value) => {
                    Some(ResponseInputItem::ImageGenerationCall(value.clone()))
                }
                ResponseOutputItem::CodeInterpreterCall(value) => {
                    Some(ResponseInputItem::CodeInterpreterCall(value.clone()))
                }
                ResponseOutputItem::LocalShellCall(value) => {
                    Some(ResponseInputItem::LocalShellCall(value.clone()))
                }
                ResponseOutputItem::LocalShellCallOutput(value) => {
                    Some(ResponseInputItem::LocalShellCallOutput(value.clone()))
                }
                ResponseOutputItem::McpListTools(value) => {
                    Some(ResponseInputItem::McpListTools(value.clone()))
                }
                ResponseOutputItem::McpCall(value) => {
                    Some(ResponseInputItem::McpCall(value.clone()))
                }
                ResponseOutputItem::McpApprovalRequest(value) => {
                    Some(ResponseInputItem::McpApprovalRequest(value.clone()))
                }
                ResponseOutputItem::CustomToolCall(value) => {
                    Some(ResponseInputItem::CustomToolCall(value.clone()))
                }
                ResponseOutputItem::FunctionCallOutput(_)
                | ResponseOutputItem::ComputerCallOutput(_)
                | ResponseOutputItem::ToolSearchCall(_)
                | ResponseOutputItem::ToolSearchOutput(_)
                | ResponseOutputItem::AdditionalTools(_)
                | ResponseOutputItem::Compaction(_)
                | ResponseOutputItem::FunctionShellCall(_)
                | ResponseOutputItem::FunctionShellCallOutput(_)
                | ResponseOutputItem::ApplyPatchCall(_)
                | ResponseOutputItem::ApplyPatchCallOutput(_)
                | ResponseOutputItem::McpApprovalResponse(_)
                | ResponseOutputItem::CustomToolCallOutput(_)
                | ResponseOutputItem::Unknown(_) => None,
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompactResponseRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    input: Omittable<ResponseInput>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    previous_response_id: Omittable<Nullable<String>>,
}

impl CompactResponseRequest {
    /// Creates an empty compact request.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a compact request with model and input.
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<ResponseInput>) -> Self {
        Self {
            model: Omittable::Value(model.into()),
            input: Omittable::Value(input.into()),
            instructions: Omittable::Omitted,
            previous_response_id: Omittable::Omitted,
        }
    }

    /// Sets the model id.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(model.into());
        self
    }

    /// Sets the input to compact.
    #[must_use]
    pub fn input(mut self, input: impl Into<ResponseInput>) -> Self {
        self.input = Omittable::Value(input.into());
        self
    }

    /// Sets compaction instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Omittable::Value(Nullable::Value(instructions.into()));
        self
    }

    /// Continues from a stored response.
    #[must_use]
    pub fn previous_response_id(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Omittable::Value(Nullable::Value(id.into()));
        self
    }
}

literal_tag!(CompactedResponseTag, Compaction, "response.compaction");

/// A compacted Responses resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactedResponse {
    id: String,
    created_at: i64,
    #[serde(rename = "object")]
    object: CompactedResponseTag,
    output: Vec<ResponseOutputItem>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<Nullable<ResponseUsage>>,
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
    pub fn output(&self) -> &[ResponseOutputItem] {
        &self.output
    }

    /// Returns future fields retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query parameters for a response's input-item page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListResponseInputItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<String>,
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
    pub fn include(mut self, include: impl Into<String>) -> Self {
        self.include.push(include.into());
        self
    }

    /// Sets the requested page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Sets ascending or descending ordering.
    #[must_use]
    pub fn order(mut self, order: impl Into<String>) -> Self {
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
    first_id: Nullable<String>,
    last_id: Nullable<String>,
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

    /// Returns the final item id for cursor pagination.
    #[must_use]
    pub fn last_id(&self) -> Option<&str> {
        match &self.last_id {
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
    #[must_use]
    pub fn conversation(mut self, conversation: impl Into<ConversationReference>) -> Self {
        self.conversation = Omittable::Value(Nullable::Value(conversation.into()));
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
    logprobs: Vec<Value>,
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
    logprobs: Vec<Value>,
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
    code: String,
    message: String,
    param: Nullable<String>,
    sequence_number: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl StreamErrorEvent {
    /// Returns the machine-readable code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
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

required_tagged_record!(
    CompactionTrigger,
    CompactionTriggerTag,
    CompactionTrigger,
    "compaction_trigger",
    {}
);

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
required_tagged_record!(ProgramOutputItem, ProgramOutputItemTag, ProgramOutput, "program_output", {
    id: String,
    call_id: String,
    result: String,
    status: String
});
required_tagged_record!(FileSearchCall, FileSearchCallTag, FileSearchCall, "file_search_call", {
    id: String,
    status: ResponseItemStatus,
    queries: Vec<String>
});
required_tagged_record!(ComputerCall, ComputerCallTag, ComputerCall, "computer_call", {
    id: String,
    call_id: String,
    pending_safety_checks: Vec<Value>,
    status: ResponseItemStatus
});
required_tagged_record!(
    ComputerCallOutput,
    ComputerCallOutputTag,
    ComputerCallOutput,
    "computer_call_output",
    {
        call_id: String,
        output: Value
    }
);
required_tagged_record!(
    ComputerCallOutputResource,
    ComputerCallOutputResourceTag,
    ComputerCallOutputResource,
    "computer_call_output",
    {
        id: String,
        call_id: String,
        output: Value,
        status: ResponseItemStatus
    }
);
required_tagged_record!(WebSearchCall, WebSearchCallTag, WebSearchCall, "web_search_call", {
    id: String,
    status: ResponseItemStatus,
    action: Value
});
required_tagged_record!(
    FunctionCallOutputResource,
    FunctionCallOutputResourceTag,
    FunctionCallOutputResource,
    "function_call_output",
    {
        id: String,
        output: Value,
        status: ResponseItemStatus
    }
);
required_tagged_record!(
    ToolSearchCallInput,
    ToolSearchCallInputTag,
    ToolSearchCall,
    "tool_search_call",
    { arguments: Value }
);
required_tagged_record!(ToolSearchCall, ToolSearchCallTag, ToolSearchCall, "tool_search_call", {
    id: String,
    call_id: Nullable<String>,
    execution: String,
    arguments: Value,
    status: ResponseItemStatus
});
required_tagged_record!(
    ToolSearchOutputInput,
    ToolSearchOutputInputTag,
    ToolSearchOutput,
    "tool_search_output",
    { tools: Vec<Value> }
);
required_tagged_record!(ToolSearchOutput, ToolSearchOutputTag, ToolSearchOutput, "tool_search_output", {
    id: String,
    call_id: Nullable<String>,
    execution: String,
    tools: Vec<Value>,
    status: ResponseItemStatus
});
required_tagged_record!(
    AdditionalToolsInput,
    AdditionalToolsInputTag,
    AdditionalTools,
    "additional_tools",
    {
        role: MessageRole,
        tools: Vec<ResponseTool>
    }
);
required_tagged_record!(AdditionalTools, AdditionalToolsTag, AdditionalTools, "additional_tools", {
    id: String,
    role: MessageRole,
    tools: Vec<ResponseTool>
});
required_tagged_record!(ReasoningItem, ReasoningItemTag, Reasoning, "reasoning", {
    id: String,
    summary: Vec<Value>
});
required_tagged_record!(
    CompactionSummaryInput,
    CompactionSummaryInputTag,
    Compaction,
    "compaction",
    { encrypted_content: String }
);
required_tagged_record!(CompactionItem, CompactionItemTag, Compaction, "compaction", {
    id: String,
    encrypted_content: String
});
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
required_tagged_record!(
    CodeInterpreterCall,
    CodeInterpreterCallTag,
    CodeInterpreterCall,
    "code_interpreter_call",
    {
        id: String,
        status: ResponseItemStatus,
        container_id: String,
        code: Nullable<String>,
        outputs: Nullable<Vec<Value>>
    }
);
required_tagged_record!(LocalShellCall, LocalShellCallTag, LocalShellCall, "local_shell_call", {
    id: String,
    call_id: String,
    action: Value,
    status: ResponseItemStatus
});
required_tagged_record!(
    LocalShellCallOutput,
    LocalShellCallOutputTag,
    LocalShellCallOutput,
    "local_shell_call_output",
    {
        id: String,
        call_id: String,
        output: String
    }
);
required_tagged_record!(
    FunctionShellCallInput,
    FunctionShellCallInputTag,
    FunctionShellCall,
    "shell_call",
    {
        call_id: String,
        action: Value
    }
);
required_tagged_record!(FunctionShellCall, FunctionShellCallTag, FunctionShellCall, "shell_call", {
    id: String,
    call_id: String,
    action: Value,
    status: ResponseItemStatus,
    environment: Nullable<Value>
});
required_tagged_record!(
    FunctionShellCallOutputInput,
    FunctionShellCallOutputInputTag,
    FunctionShellCallOutput,
    "shell_call_output",
    {
        call_id: String,
        output: Vec<Value>
    }
);
required_tagged_record!(
    FunctionShellCallOutput,
    FunctionShellCallOutputTag,
    FunctionShellCallOutput,
    "shell_call_output",
    {
        id: String,
        call_id: String,
        status: ResponseItemStatus,
        output: Vec<Value>,
        max_output_length: Nullable<u64>
    }
);
required_tagged_record!(
    ApplyPatchCallInput,
    ApplyPatchCallInputTag,
    ApplyPatchCall,
    "apply_patch_call",
    {
        call_id: String,
        status: String,
        operation: Value
    }
);
required_tagged_record!(ApplyPatchCall, ApplyPatchCallTag, ApplyPatchCall, "apply_patch_call", {
    id: String,
    call_id: String,
    status: String,
    operation: Value
});
required_tagged_record!(
    ApplyPatchCallOutputInput,
    ApplyPatchCallOutputInputTag,
    ApplyPatchCallOutput,
    "apply_patch_call_output",
    {
        call_id: String,
        status: String
    }
);
required_tagged_record!(
    ApplyPatchCallOutput,
    ApplyPatchCallOutputTag,
    ApplyPatchCallOutput,
    "apply_patch_call_output",
    {
        id: String,
        call_id: String,
        status: String
    }
);
required_tagged_record!(
    McpApprovalResponseResource,
    McpApprovalResponseResourceTag,
    McpApprovalResponse,
    "mcp_approval_response",
    {
        id: String,
        request_id: String,
        approval_request_id: String,
        approve: bool
    }
);
required_tagged_record!(CustomToolCall, CustomToolCallTag, CustomToolCall, "custom_tool_call", {
    call_id: String,
    name: String,
    input: String
});
required_tagged_record!(
    CustomToolCallOutput,
    CustomToolCallOutputTag,
    CustomToolCallOutput,
    "custom_tool_call_output",
    {
        call_id: String,
        output: Value
    }
);
required_tagged_record!(
    CustomToolCallOutputResource,
    CustomToolCallOutputResourceTag,
    CustomToolCallOutputResource,
    "custom_tool_call_output",
    {
        id: String,
        call_id: String,
        output: Value,
        status: ResponseItemStatus
    }
);

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

/// File-search tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchTool {
    #[serde(rename = "type")]
    kind: FileSearchToolTag,
    vector_store_ids: Vec<String>,
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
            extra: ExtraFields::new(),
        }
    }

    /// Returns selected vector-store ids.
    #[must_use]
    pub fn vector_store_ids(&self) -> &[String] {
        &self.vector_store_ids
    }
}

tag_only_tool!(ComputerTool, ComputerToolTag, Computer, "computer");
tag_only_tool!(WebSearchTool, WebSearchToolTag, WebSearch, "web_search");
tag_only_tool!(
    ProgrammaticTool,
    ProgrammaticToolTag,
    ProgrammaticToolCalling,
    "programmatic_tool_calling"
);
tag_only_tool!(
    ImageGenerationTool,
    ImageGenerationToolTag,
    ImageGeneration,
    "image_generation"
);
tag_only_tool!(LocalShellTool, LocalShellToolTag, LocalShell, "local_shell");
tag_only_tool!(FunctionShellTool, FunctionShellToolTag, Shell, "shell");
tag_only_tool!(ToolSearchTool, ToolSearchToolTag, ToolSearch, "tool_search");
tag_only_tool!(
    WebSearchPreviewTool,
    WebSearchPreviewToolTag,
    WebSearchPreview,
    "web_search_preview"
);
tag_only_tool!(ApplyPatchTool, ApplyPatchToolTag, ApplyPatch, "apply_patch");

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

/// Code-interpreter tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeInterpreterTool {
    #[serde(rename = "type")]
    kind: CodeInterpreterToolTag,
    container: Value,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CodeInterpreterTool {
    /// Selects an existing container by id.
    #[must_use]
    pub fn container_id(container_id: impl Into<String>) -> Self {
        Self {
            kind: CodeInterpreterToolTag::CodeInterpreter,
            container: Value::String(container_id.into()),
            extra: ExtraFields::new(),
        }
    }

    /// Serializes an automatic-container configuration.
    pub fn automatic<T: Serialize>(container: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            kind: CodeInterpreterToolTag::CodeInterpreter,
            container: serde_json::to_value(container)?,
            extra: ExtraFields::new(),
        })
    }
}

literal_tag!(CustomToolTag, Custom, "custom");

/// A named custom free-form tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTool {
    #[serde(rename = "type")]
    kind: CustomToolTag,
    name: String,
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
            extra: ExtraFields::new(),
        }
    }
}

literal_tag!(NamespaceToolTag, Namespace, "namespace");

/// A namespace that groups tools for deferred discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceTool {
    #[serde(rename = "type")]
    kind: NamespaceToolTag,
    name: String,
    description: String,
    tools: Vec<ResponseTool>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl NamespaceTool {
    /// Creates a namespace and its nested tools.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        tools: impl IntoIterator<Item = impl Into<ResponseTool>>,
    ) -> Self {
        Self {
            kind: NamespaceToolTag::Namespace,
            name: name.into(),
            description: description.into(),
            tools: tools.into_iter().map(Into::into).collect(),
            extra: ExtraFields::new(),
        }
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
        Auto => "auto",
        Required => "required"
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
    pub enum HostedToolType {
        FileSearch => "file_search",
        WebSearchPreview => "web_search_preview",
        Computer => "computer",
        ComputerUsePreview => "computer_use_preview",
        ComputerUse => "computer_use",
        WebSearchPreview20250311 => "web_search_preview_2025_03_11",
        ImageGeneration => "image_generation",
        CodeInterpreter => "code_interpreter"
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
        literal_tag!($tag_name, $tag_variant, $wire);

        #[doc = concat!("Streaming event `", $wire, "`.")]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag_name,
            $($field: $ty,)*
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
required_stream_event!(AudioDoneEvent, AudioDoneEventTag, AudioDone, "response.audio.done", {
    response_id: String
});
required_stream_event!(
    AudioTranscriptDeltaEvent,
    AudioTranscriptDeltaEventTag,
    AudioTranscriptDelta,
    "response.audio.transcript.delta",
    {
        response_id: String,
        delta: String
    }
);
required_stream_event!(
    AudioTranscriptDoneEvent,
    AudioTranscriptDoneEventTag,
    AudioTranscriptDone,
    "response.audio.transcript.done",
    { response_id: String }
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
required_stream_event!(
    ShellOutputContentDeltaEvent,
    ShellOutputContentDeltaEventTag,
    ShellOutputContentDelta,
    "response.shell_call_output_content.delta",
    {
        item_id: String,
        output_index: u64,
        command_index: u64,
        delta: String
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
        output: String
    }
);
required_stream_event!(
    ReasoningSummaryPartAddedEvent,
    ReasoningSummaryPartAddedEventTag,
    ReasoningSummaryPartAdded,
    "response.reasoning_summary_part.added",
    {
        item_id: String,
        output_index: u64,
        summary_index: u64,
        part: Value
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
        part: Value
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
        annotation: Value
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
        assert_json_dto::<MessageRole>();
        assert_json_dto::<ImageDetail>();
        assert_json_dto::<TruncationStrategy>();
        assert_json_dto::<ReasoningEffort>();
        assert_json_dto::<ReasoningSummary>();
        assert_json_dto::<IncompleteReason>();
        assert_json_dto::<InputText>();
        assert_json_dto::<InputImage>();
        assert_json_dto::<InputFile>();
        assert_json_dto::<InputContent>();
        assert_json_dto::<MessageContent>();
        assert_json_dto::<InputMessage>();
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
        assert_json_dto::<FunctionCall>();
        assert_json_dto::<FunctionCallOutput>();
        assert_json_dto::<McpListedTool>();
        assert_json_dto::<McpListTools>();
        assert_json_dto::<McpCall>();
        assert_json_dto::<McpApprovalRequest>();
        assert_json_dto::<McpApprovalResponse>();
        assert_json_dto::<FunctionCallOutputValue>();
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
        assert_json_dto::<ReasoningConfig>();
        assert_json_dto::<ConversationObjectReference>();
        assert_json_dto::<ConversationReference>();
        assert_json_dto::<PromptReference>();
        assert_json_dto::<ResponseStreamOptions>();
        assert_json_dto::<CreateResponseRequest>();
        assert_json_dto::<CreateStreamingResponseRequest>();
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
        assert_json_dto::<ComputerCall>();
        assert_json_dto::<ComputerCallOutput>();
        assert_json_dto::<ComputerCallOutputResource>();
        assert_json_dto::<WebSearchCall>();
        assert_json_dto::<FunctionCallOutputResource>();
        assert_json_dto::<ToolSearchCallInput>();
        assert_json_dto::<ToolSearchCall>();
        assert_json_dto::<ToolSearchOutputInput>();
        assert_json_dto::<ToolSearchOutput>();
        assert_json_dto::<AdditionalToolsInput>();
        assert_json_dto::<AdditionalTools>();
        assert_json_dto::<ReasoningItem>();
        assert_json_dto::<CompactionSummaryInput>();
        assert_json_dto::<CompactionItem>();
        assert_json_dto::<ImageGenerationCall>();
        assert_json_dto::<CodeInterpreterCall>();
        assert_json_dto::<LocalShellCall>();
        assert_json_dto::<LocalShellCallOutput>();
        assert_json_dto::<FunctionShellCallInput>();
        assert_json_dto::<FunctionShellCall>();
        assert_json_dto::<FunctionShellCallOutputInput>();
        assert_json_dto::<FunctionShellCallOutput>();
        assert_json_dto::<ApplyPatchCallInput>();
        assert_json_dto::<ApplyPatchCall>();
        assert_json_dto::<ApplyPatchCallOutputInput>();
        assert_json_dto::<ApplyPatchCallOutput>();
        assert_json_dto::<McpApprovalResponseResource>();
        assert_json_dto::<CustomToolCall>();
        assert_json_dto::<CustomToolCallOutput>();
        assert_json_dto::<CustomToolCallOutputResource>();
        assert_json_dto::<FileSearchTool>();
        assert_json_dto::<ComputerTool>();
        assert_json_dto::<ComputerUsePreviewTool>();
        assert_json_dto::<WebSearchTool>();
        assert_json_dto::<CodeInterpreterTool>();
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
            "programmatic_tool_calling",
            "image_generation",
            "local_shell",
            "shell",
            "tool_search",
            "web_search_preview",
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
                "input_tokens_details": {"cached_tokens": 2},
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
            ResponseItemStatus::Completed,
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
}
