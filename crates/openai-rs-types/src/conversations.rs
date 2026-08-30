//! Typed wire models for Conversations and their persisted items.
//!
//! Conversation creation accepts the same typed input items as Responses.
//! Persisted conversation items use a resource union with stricter identifiers
//! and statuses; conversions are therefore explicit and fallible where the two
//! schemas do not carry the same information.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ExtraFields, JsonText, Nullable, Omittable, open_string_enum,
    responses::{self, UnknownTaggedObject},
};

/// Maximum initial/additional items accepted in one Conversations request.
pub const MAX_CONVERSATION_ITEMS_PER_REQUEST: usize = 20;
/// Maximum number of metadata properties accepted by the API.
pub const MAX_CONVERSATION_METADATA_PROPERTIES: usize = 16;
/// Maximum metadata key length in Unicode scalar values.
pub const MAX_CONVERSATION_METADATA_KEY_CHARS: usize = 64;
/// Maximum metadata value length in Unicode scalar values.
pub const MAX_CONVERSATION_METADATA_VALUE_CHARS: usize = 512;

/// Opaque conversation identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationId(Box<str>);

impl ConversationId {
    /// Creates an identifier without assuming a prefix or length.
    #[must_use]
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// Borrows the wire identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ConversationId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ConversationId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Opaque identifier of one persisted conversation item.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationItemId(Box<str>);

impl ConversationItemId {
    /// Creates an identifier without assuming a resource prefix.
    #[must_use]
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// Borrows the wire identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ConversationItemId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ConversationItemId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ConversationItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Metadata attached to a conversation.
pub type ConversationMetadata = BTreeMap<String, String>;

/// Invalid Conversations request data.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConversationValidationError {
    /// More than twenty items were supplied in one request.
    #[error("conversation request contains {actual} items; maximum is {maximum}")]
    TooManyItems {
        /// Observed item count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Metadata contains too many pairs.
    #[error("conversation metadata contains {actual} properties; maximum is {maximum}")]
    TooManyMetadataProperties {
        /// Observed property count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata key is too long.
    #[error("conversation metadata key has {actual} characters; maximum is {maximum}")]
    MetadataKeyTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata value is too long.
    #[error("conversation metadata value has {actual} characters; maximum is {maximum}")]
    MetadataValueTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A list limit is outside the documented range.
    #[error("conversation item list limit must be between 1 and 100, got {limit}")]
    InvalidListLimit {
        /// Rejected list limit.
        limit: u32,
    },
}

fn validate_item_count(items: &[responses::ResponseInputItem]) -> Result<(), ConversationValidationError> {
    if items.len() > MAX_CONVERSATION_ITEMS_PER_REQUEST {
        return Err(ConversationValidationError::TooManyItems {
            actual: items.len(),
            maximum: MAX_CONVERSATION_ITEMS_PER_REQUEST,
        });
    }
    Ok(())
}

fn validate_metadata(metadata: &ConversationMetadata) -> Result<(), ConversationValidationError> {
    if metadata.len() > MAX_CONVERSATION_METADATA_PROPERTIES {
        return Err(ConversationValidationError::TooManyMetadataProperties {
            actual: metadata.len(),
            maximum: MAX_CONVERSATION_METADATA_PROPERTIES,
        });
    }
    for (key, value) in metadata {
        let key_length = key.chars().count();
        if key_length > MAX_CONVERSATION_METADATA_KEY_CHARS {
            return Err(ConversationValidationError::MetadataKeyTooLong {
                actual: key_length,
                maximum: MAX_CONVERSATION_METADATA_KEY_CHARS,
            });
        }
        let value_length = value.chars().count();
        if value_length > MAX_CONVERSATION_METADATA_VALUE_CHARS {
            return Err(ConversationValidationError::MetadataValueTooLong {
                actual: value_length,
                maximum: MAX_CONVERSATION_METADATA_VALUE_CHARS,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CreateConversationRequestWire {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<ConversationMetadata>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    items: Omittable<Nullable<Vec<responses::ResponseInputItem>>>,
}

/// Body for `POST /conversations`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CreateConversationRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<ConversationMetadata>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    items: Omittable<Nullable<Vec<responses::ResponseInputItem>>>,
}

impl<'de> Deserialize<'de> for CreateConversationRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateConversationRequestWire::deserialize(deserializer)?;
        if let Omittable::Value(Nullable::Value(metadata)) = &wire.metadata {
            validate_metadata(metadata).map_err(D::Error::custom)?;
        }
        if let Omittable::Value(Nullable::Value(items)) = &wire.items {
            validate_item_count(items).map_err(D::Error::custom)?;
        }
        Ok(Self {
            metadata: wire.metadata,
            items: wire.items,
        })
    }
}

impl CreateConversationRequest {
    /// Creates an empty body; both properties are optional.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets validated metadata.
    pub fn metadata(
        mut self,
        metadata: ConversationMetadata,
    ) -> Result<Self, ConversationValidationError> {
        validate_metadata(&metadata)?;
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        Ok(self)
    }

    /// Adds one validated metadata pair.
    pub fn metadata_entry(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ConversationValidationError> {
        let mut metadata = match std::mem::take(&mut self.metadata) {
            Omittable::Value(Nullable::Value(metadata)) => metadata,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => BTreeMap::new(),
        };
        metadata.insert(key.into(), value.into());
        validate_metadata(&metadata)?;
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        Ok(self)
    }

    /// Explicitly clears metadata with JSON `null`.
    #[must_use]
    pub fn metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }

    /// Sets up to twenty typed initial items.
    pub fn items(
        mut self,
        items: impl IntoIterator<Item = responses::ResponseInputItem>,
    ) -> Result<Self, ConversationValidationError> {
        let items = items.into_iter().collect::<Vec<_>>();
        validate_item_count(&items)?;
        self.items = Omittable::Value(Nullable::Value(items));
        Ok(self)
    }

    /// Appends one typed initial item.
    pub fn item(
        mut self,
        item: impl Into<responses::ResponseInputItem>,
    ) -> Result<Self, ConversationValidationError> {
        let mut items = match std::mem::take(&mut self.items) {
            Omittable::Value(Nullable::Value(items)) => items,
            Omittable::Omitted | Omittable::Value(Nullable::Null) => Vec::new(),
        };
        items.push(item.into());
        validate_item_count(&items)?;
        self.items = Omittable::Value(Nullable::Value(items));
        Ok(self)
    }

    /// Explicitly sends `items: null`.
    #[must_use]
    pub fn items_null(mut self) -> Self {
        self.items = Omittable::Value(Nullable::Null);
        self
    }
}

/// Body for `POST /conversations/{conversation_id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateConversationRequest {
    metadata: Nullable<ConversationMetadata>,
}

impl UpdateConversationRequest {
    /// Replaces conversation metadata.
    pub fn new(
        metadata: ConversationMetadata,
    ) -> Result<Self, ConversationValidationError> {
        validate_metadata(&metadata)?;
        Ok(Self {
            metadata: Nullable::Value(metadata),
        })
    }

    /// Explicitly clears conversation metadata.
    #[must_use]
    pub const fn clear_metadata() -> Self {
        Self {
            metadata: Nullable::Null,
        }
    }

    /// Returns the non-null metadata map.
    #[must_use]
    pub fn metadata(&self) -> Option<&ConversationMetadata> {
        match &self.metadata {
            Nullable::Value(metadata) => Some(metadata),
            Nullable::Null => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ConversationObjectTag {
    #[serde(rename = "conversation")]
    Conversation,
}

/// A persisted conversation resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    id: ConversationId,
    #[serde(rename = "object")]
    object: ConversationObjectTag,
    metadata: Nullable<ConversationMetadata>,
    created_at: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Conversation {
    /// Returns the opaque conversation id.
    #[must_use]
    pub const fn id(&self) -> &ConversationId {
        &self.id
    }

    /// Returns the Unix creation timestamp in seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns metadata when non-null.
    #[must_use]
    pub fn metadata(&self) -> Option<&ConversationMetadata> {
        match &self.metadata {
            Nullable::Value(metadata) => Some(metadata),
            Nullable::Null => None,
        }
    }

    /// Returns future response properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum DeletedConversationObjectTag {
    #[serde(rename = "conversation.deleted")]
    Deleted,
}

/// Confirmation returned by `DELETE /conversations/{conversation_id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedConversation {
    id: ConversationId,
    #[serde(rename = "object")]
    object: DeletedConversationObjectTag,
    deleted: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl DeletedConversation {
    /// Returns the deleted conversation id.
    #[must_use]
    pub const fn id(&self) -> &ConversationId {
        &self.id
    }

    /// Returns whether deletion completed.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// Returns future response properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// OpenAPI resource-name alias.
pub type ConversationResource = Conversation;
/// OpenAPI resource-name alias.
pub type DeletedConversationResource = DeletedConversation;

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

fn discriminator(value: &Value) -> Result<String, &'static str> {
    value
        .as_object()
        .ok_or("conversation union value must be a JSON object")?
        .get("type")
        .ok_or("conversation union object is missing string field `type`")?
        .as_str()
        .map(str::to_owned)
        .ok_or("conversation union field `type` must be a string")
}

literal_tag!(ConversationTextTag, Text, "text");
literal_tag!(ConversationSummaryTextTag, SummaryText, "summary_text");
literal_tag!(ConversationReasoningTextTag, ReasoningText, "reasoning_text");

macro_rules! text_content {
    ($name:ident, $tag:ident, $variant:ident) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            text: String,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Creates a text content part.
            #[must_use]
            pub fn new(text: impl Into<String>) -> Self {
                Self {
                    kind: $tag::$variant,
                    text: text.into(),
                    extra: ExtraFields::new(),
                }
            }

            /// Returns the text.
            #[must_use]
            pub fn text(&self) -> &str {
                &self.text
            }

            /// Returns future properties retained while decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

text_content!(ConversationText, ConversationTextTag, Text);
text_content!(ConversationSummaryText, ConversationSummaryTextTag, SummaryText);
text_content!(ConversationReasoningText, ConversationReasoningTextTag, ReasoningText);

literal_tag!(ConversationInputImageTag, InputImage, "input_image");

/// Persisted input-image content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationInputImage {
    #[serde(rename = "type")]
    kind: ConversationInputImageTag,
    detail: responses::ImageDetail,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_url: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationInputImage {
    /// Creates an image resource from a URL.
    #[must_use]
    pub fn from_url(url: impl Into<String>, detail: responses::ImageDetail) -> Self {
        Self {
            kind: ConversationInputImageTag::InputImage,
            detail,
            image_url: Omittable::Value(Nullable::Value(url.into())),
            file_id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates an image resource from an uploaded file.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>, detail: responses::ImageDetail) -> Self {
        Self {
            kind: ConversationInputImageTag::InputImage,
            detail,
            image_url: Omittable::Omitted,
            file_id: Omittable::Value(Nullable::Value(file_id.into())),
            extra: ExtraFields::new(),
        }
    }

    fn to_response_content(&self) -> Result<responses::InputContent, ConversationItemConversionError> {
        let value = match (&self.image_url, &self.file_id) {
            (Omittable::Value(Nullable::Value(url)), _) => {
                responses::InputImage::from_url(url.clone()).detail(self.detail.clone())
            }
            (_, Omittable::Value(Nullable::Value(file_id))) => {
                responses::InputImage::from_file_id(file_id.clone()).detail(self.detail.clone())
            }
            _ => return Err(ConversationItemConversionError::ImageHasNoSource),
        };
        Ok(value.into())
    }

    /// Returns future properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(ConversationComputerScreenshotTag, ComputerScreenshot, "computer_screenshot");

/// Persisted computer screenshot content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationComputerScreenshot {
    #[serde(rename = "type")]
    kind: ConversationComputerScreenshotTag,
    image_url: Nullable<String>,
    file_id: Nullable<String>,
    detail: responses::ImageDetail,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationComputerScreenshot {
    /// Creates a URL-backed screenshot.
    #[must_use]
    pub fn from_url(url: impl Into<String>, detail: responses::ImageDetail) -> Self {
        Self {
            kind: ConversationComputerScreenshotTag::ComputerScreenshot,
            image_url: Nullable::Value(url.into()),
            file_id: Nullable::Null,
            detail,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a file-backed screenshot.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>, detail: responses::ImageDetail) -> Self {
        Self {
            kind: ConversationComputerScreenshotTag::ComputerScreenshot,
            image_url: Nullable::Null,
            file_id: Nullable::Value(file_id.into()),
            detail,
            extra: ExtraFields::new(),
        }
    }
}

literal_tag!(ConversationInputFileTag, InputFile, "input_file");

/// Persisted input-file content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationInputFile {
    #[serde(rename = "type")]
    kind: ConversationInputFileTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filename: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_data: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_url: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<responses::ImageDetail>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationInputFile {
    fn empty() -> Self {
        Self {
            kind: ConversationInputFileTag::InputFile,
            file_id: Omittable::Omitted,
            filename: Omittable::Omitted,
            file_data: Omittable::Omitted,
            file_url: Omittable::Omitted,
            detail: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Creates a file resource from an uploaded id.
    #[must_use]
    pub fn from_file_id(file_id: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_id = Omittable::Value(Nullable::Value(file_id.into()));
        value
    }

    /// Creates a file resource from a remote URL.
    #[must_use]
    pub fn from_url(file_url: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_url = Omittable::Value(file_url.into());
        value
    }

    /// Creates a file resource from base64 data and a filename.
    #[must_use]
    pub fn from_base64(file_data: impl Into<String>, filename: impl Into<String>) -> Self {
        let mut value = Self::empty();
        value.file_data = Omittable::Value(file_data.into());
        value.filename = Omittable::Value(filename.into());
        value
    }

    /// Sets the file rendering detail.
    #[must_use]
    pub fn detail(mut self, detail: responses::ImageDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }

    fn to_response_content(&self) -> Result<responses::InputContent, ConversationItemConversionError> {
        let value = match (&self.file_id, &self.file_url, &self.file_data, &self.filename) {
            (Omittable::Value(Nullable::Value(file_id)), _, _, _) => {
                responses::InputFile::from_file_id(file_id.clone())
            }
            (_, Omittable::Value(file_url), _, _) => {
                responses::InputFile::from_url(file_url.clone())
            }
            (_, _, Omittable::Value(file_data), Omittable::Value(filename)) => {
                responses::InputFile::from_base64(file_data.clone(), filename.clone())
            }
            _ => return Err(ConversationItemConversionError::FileHasNoSource),
        };
        let value = match &self.detail {
            Omittable::Value(detail) => value.detail(detail.clone()),
            Omittable::Omitted => value,
        };
        Ok(value.into())
    }

    /// Returns future properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One content part in a persisted conversation message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConversationMessageContent {
    /// User/developer/system text.
    InputText(responses::InputText),
    /// Assistant output text.
    OutputText(responses::OutputText),
    /// Generic text content.
    Text(ConversationText),
    /// Reasoning summary text.
    SummaryText(ConversationSummaryText),
    /// Reasoning text.
    ReasoningText(ConversationReasoningText),
    /// Assistant refusal.
    Refusal(responses::Refusal),
    /// Input image.
    InputImage(ConversationInputImage),
    /// Computer screenshot.
    ComputerScreenshot(ConversationComputerScreenshot),
    /// Input file.
    InputFile(ConversationInputFile),
    /// Future content retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ConversationMessageContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::InputText(value) => value.serialize(serializer),
            Self::OutputText(value) => value.serialize(serializer),
            Self::Text(value) => value.serialize(serializer),
            Self::SummaryText(value) => value.serialize(serializer),
            Self::ReasoningText(value) => value.serialize(serializer),
            Self::Refusal(value) => value.serialize(serializer),
            Self::InputImage(value) => value.serialize(serializer),
            Self::ComputerScreenshot(value) => value.serialize(serializer),
            Self::InputFile(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ConversationMessageContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match discriminator(&value).map_err(D::Error::custom)?.as_str() {
            "input_text" => serde_json::from_value(value)
                .map(Self::InputText)
                .map_err(D::Error::custom),
            "output_text" => serde_json::from_value(value)
                .map(Self::OutputText)
                .map_err(D::Error::custom),
            "text" => serde_json::from_value(value)
                .map(Self::Text)
                .map_err(D::Error::custom),
            "summary_text" => serde_json::from_value(value)
                .map(Self::SummaryText)
                .map_err(D::Error::custom),
            "reasoning_text" => serde_json::from_value(value)
                .map(Self::ReasoningText)
                .map_err(D::Error::custom),
            "refusal" => serde_json::from_value(value)
                .map(Self::Refusal)
                .map_err(D::Error::custom),
            "input_image" => serde_json::from_value(value)
                .map(Self::InputImage)
                .map_err(D::Error::custom),
            "computer_screenshot" => serde_json::from_value(value)
                .map(Self::ComputerScreenshot)
                .map_err(D::Error::custom),
            "input_file" => serde_json::from_value(value)
                .map(Self::InputFile)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl From<responses::InputText> for ConversationMessageContent {
    fn from(value: responses::InputText) -> Self {
        Self::InputText(value)
    }
}

impl From<responses::OutputText> for ConversationMessageContent {
    fn from(value: responses::OutputText) -> Self {
        Self::OutputText(value)
    }
}

impl From<responses::Refusal> for ConversationMessageContent {
    fn from(value: responses::Refusal) -> Self {
        Self::Refusal(value)
    }
}

impl From<ConversationInputImage> for ConversationMessageContent {
    fn from(value: ConversationInputImage) -> Self {
        Self::InputImage(value)
    }
}

impl From<ConversationInputFile> for ConversationMessageContent {
    fn from(value: ConversationInputFile) -> Self {
        Self::InputFile(value)
    }
}

open_string_enum! {
    /// Role carried by a persisted conversation message.
    pub enum ConversationMessageRole {
        Unknown = "unknown",
        User = "user",
        Assistant = "assistant",
        System = "system",
        Critic = "critic",
        Discriminator = "discriminator",
        Developer = "developer",
        Tool = "tool",
    }
}

literal_tag!(ConversationMessageTag, Message, "message");

/// A persisted input or output message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationMessage {
    #[serde(rename = "type")]
    kind: ConversationMessageTag,
    id: ConversationItemId,
    status: responses::ResponseItemStatus,
    role: ConversationMessageRole,
    content: Vec<ConversationMessageContent>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    phase: Omittable<Nullable<String>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationMessage {
    /// Creates a persisted message representation.
    #[must_use]
    pub fn new(
        id: impl Into<ConversationItemId>,
        status: responses::ResponseItemStatus,
        role: ConversationMessageRole,
        content: impl IntoIterator<Item = impl Into<ConversationMessageContent>>,
    ) -> Self {
        Self {
            kind: ConversationMessageTag::Message,
            id: id.into(),
            status,
            role,
            content: content.into_iter().map(Into::into).collect(),
            phase: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets an assistant message phase.
    #[must_use]
    pub fn phase(mut self, phase: impl Into<String>) -> Self {
        self.phase = Omittable::Value(Nullable::Value(phase.into()));
        self
    }

    /// Returns the item id.
    #[must_use]
    pub const fn id(&self) -> &ConversationItemId {
        &self.id
    }

    /// Returns the item status.
    #[must_use]
    pub const fn status(&self) -> &responses::ResponseItemStatus {
        &self.status
    }

    /// Returns the message role.
    #[must_use]
    pub const fn role(&self) -> &ConversationMessageRole {
        &self.role
    }

    /// Returns message content in wire order.
    #[must_use]
    pub fn content(&self) -> &[ConversationMessageContent] {
        &self.content
    }

    /// Converts this resource to a legal Responses input item.
    pub fn to_response_input_item(
        &self,
    ) -> Result<responses::ResponseInputItem, ConversationItemConversionError> {
        match &self.role {
            ConversationMessageRole::Assistant => {
                let content = self
                    .content
                    .iter()
                    .map(|part| match part {
                        ConversationMessageContent::OutputText(value) => {
                            Ok(responses::OutputContent::Text(value.clone()))
                        }
                        ConversationMessageContent::Refusal(value) => {
                            Ok(responses::OutputContent::Refusal(value.clone()))
                        }
                        _ => Err(ConversationItemConversionError::ContentRoleMismatch {
                            role: self.role.as_str().to_owned(),
                            content_type: content_discriminator(part).to_owned(),
                        }),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(responses::OutputMessage::new(
                    self.id.as_str(),
                    self.status.clone(),
                    content,
                )
                .into())
            }
            ConversationMessageRole::User
            | ConversationMessageRole::System
            | ConversationMessageRole::Developer => {
                let role = match &self.role {
                    ConversationMessageRole::User => responses::StoredInputMessageRole::User,
                    ConversationMessageRole::System => responses::StoredInputMessageRole::System,
                    ConversationMessageRole::Developer => {
                        responses::StoredInputMessageRole::Developer
                    }
                    _ => return Err(ConversationItemConversionError::UnsupportedMessageRole {
                        role: self.role.as_str().to_owned(),
                    }),
                };
                let content = self
                    .content
                    .iter()
                    .map(|part| match part {
                        ConversationMessageContent::InputText(value) => {
                            Ok(responses::InputContent::Text(value.clone()))
                        }
                        ConversationMessageContent::InputImage(value) => {
                            value.to_response_content()
                        }
                        ConversationMessageContent::InputFile(value) => {
                            value.to_response_content()
                        }
                        _ => Err(ConversationItemConversionError::ContentRoleMismatch {
                            role: self.role.as_str().to_owned(),
                            content_type: content_discriminator(part).to_owned(),
                        }),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(responses::StoredInputMessage::new(role, content)
                    .status(self.status.clone())
                    .into())
            }
            _ => Err(ConversationItemConversionError::UnsupportedMessageRole {
                role: self.role.as_str().to_owned(),
            }),
        }
    }

    /// Returns future properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

fn content_discriminator(content: &ConversationMessageContent) -> &str {
    match content {
        ConversationMessageContent::InputText(_) => "input_text",
        ConversationMessageContent::OutputText(_) => "output_text",
        ConversationMessageContent::Text(_) => "text",
        ConversationMessageContent::SummaryText(_) => "summary_text",
        ConversationMessageContent::ReasoningText(_) => "reasoning_text",
        ConversationMessageContent::Refusal(_) => "refusal",
        ConversationMessageContent::InputImage(_) => "input_image",
        ConversationMessageContent::ComputerScreenshot(_) => "computer_screenshot",
        ConversationMessageContent::InputFile(_) => "input_file",
        ConversationMessageContent::Unknown(value) => value.discriminator(),
    }
}

literal_tag!(ConversationFunctionCallTag, FunctionCall, "function_call");

/// Persisted function call with required resource id and status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationFunctionCall {
    #[serde(rename = "type")]
    kind: ConversationFunctionCallTag,
    id: ConversationItemId,
    call_id: String,
    name: String,
    arguments: JsonText,
    status: responses::ResponseItemStatus,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationFunctionCall {
    /// Creates a persisted function call resource.
    #[must_use]
    pub fn new(
        id: impl Into<ConversationItemId>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: JsonText,
        status: responses::ResponseItemStatus,
    ) -> Self {
        Self {
            kind: ConversationFunctionCallTag::FunctionCall,
            id: id.into(),
            call_id: call_id.into(),
            name: name.into(),
            arguments,
            status,
            extra: ExtraFields::new(),
        }
    }

    /// Returns the resource id.
    #[must_use]
    pub const fn id(&self) -> &ConversationItemId {
        &self.id
    }

    /// Returns the function call id.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns lazy JSON arguments.
    #[must_use]
    pub const fn arguments(&self) -> &JsonText {
        &self.arguments
    }
}

literal_tag!(ConversationCustomToolCallTag, CustomToolCall, "custom_tool_call");

/// Persisted custom-tool call with required resource id and status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationCustomToolCall {
    #[serde(rename = "type")]
    kind: ConversationCustomToolCallTag,
    id: ConversationItemId,
    call_id: String,
    name: String,
    input: String,
    status: responses::ResponseItemStatus,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationCustomToolCall {
    /// Creates a persisted custom-tool call.
    #[must_use]
    pub fn new(
        id: impl Into<ConversationItemId>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
        status: responses::ResponseItemStatus,
    ) -> Self {
        Self {
            kind: ConversationCustomToolCallTag::CustomToolCall,
            id: id.into(),
            call_id: call_id.into(),
            name: name.into(),
            input: input.into(),
            status,
            extra: ExtraFields::new(),
        }
    }
}

/// One persisted item in a Conversation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConversationItem {
    /// General input/output message resource.
    Message(ConversationMessage),
    /// Function call resource.
    FunctionCall(ConversationFunctionCall),
    /// Function call output resource.
    FunctionCallOutput(responses::FunctionCallOutputResource),
    /// File-search call.
    FileSearchCall(responses::FileSearchCall),
    /// Web-search call.
    WebSearchCall(responses::WebSearchCall),
    /// Image-generation call.
    ImageGenerationCall(responses::ImageGenerationCall),
    /// Computer-use call.
    ComputerCall(responses::ComputerCall),
    /// Computer-use output resource.
    ComputerCallOutput(responses::ComputerCallOutputResource),
    /// Tool-search call.
    ToolSearchCall(responses::ToolSearchCall),
    /// Tool-search output.
    ToolSearchOutput(responses::ToolSearchOutput),
    /// Dynamically supplied additional tools.
    AdditionalTools(responses::AdditionalTools),
    /// Reasoning item.
    Reasoning(responses::ReasoningItem),
    /// Programmatic tool-calling program.
    Program(responses::ProgramItem),
    /// Program output.
    ProgramOutput(responses::ProgramOutputItem),
    /// Encrypted compaction summary.
    Compaction(responses::CompactionItem),
    /// Code-interpreter call.
    CodeInterpreterCall(responses::CodeInterpreterCall),
    /// Local-shell call.
    LocalShellCall(responses::LocalShellCall),
    /// Local-shell output.
    LocalShellCallOutput(responses::LocalShellCallOutput),
    /// Function-shell call.
    FunctionShellCall(responses::FunctionShellCall),
    /// Function-shell output.
    FunctionShellCallOutput(responses::FunctionShellCallOutput),
    /// Apply-patch call.
    ApplyPatchCall(responses::ApplyPatchCall),
    /// Apply-patch output.
    ApplyPatchCallOutput(responses::ApplyPatchCallOutput),
    /// MCP tool listing.
    McpListTools(responses::McpListTools),
    /// MCP approval request.
    McpApprovalRequest(responses::McpApprovalRequest),
    /// MCP approval response resource.
    McpApprovalResponse(responses::McpApprovalResponseResource),
    /// MCP tool call.
    McpCall(responses::McpCall),
    /// Custom-tool call resource.
    CustomToolCall(ConversationCustomToolCall),
    /// Custom-tool output.
    CustomToolCallOutput(responses::CustomToolCallOutput),
    /// Future item retained verbatim.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ConversationItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Message(value) => value.serialize(serializer),
            Self::FunctionCall(value) => value.serialize(serializer),
            Self::FunctionCallOutput(value) => value.serialize(serializer),
            Self::FileSearchCall(value) => value.serialize(serializer),
            Self::WebSearchCall(value) => value.serialize(serializer),
            Self::ImageGenerationCall(value) => value.serialize(serializer),
            Self::ComputerCall(value) => value.serialize(serializer),
            Self::ComputerCallOutput(value) => value.serialize(serializer),
            Self::ToolSearchCall(value) => value.serialize(serializer),
            Self::ToolSearchOutput(value) => value.serialize(serializer),
            Self::AdditionalTools(value) => value.serialize(serializer),
            Self::Reasoning(value) => value.serialize(serializer),
            Self::Program(value) => value.serialize(serializer),
            Self::ProgramOutput(value) => value.serialize(serializer),
            Self::Compaction(value) => value.serialize(serializer),
            Self::CodeInterpreterCall(value) => value.serialize(serializer),
            Self::LocalShellCall(value) => value.serialize(serializer),
            Self::LocalShellCallOutput(value) => value.serialize(serializer),
            Self::FunctionShellCall(value) => value.serialize(serializer),
            Self::FunctionShellCallOutput(value) => value.serialize(serializer),
            Self::ApplyPatchCall(value) => value.serialize(serializer),
            Self::ApplyPatchCallOutput(value) => value.serialize(serializer),
            Self::McpListTools(value) => value.serialize(serializer),
            Self::McpApprovalRequest(value) => value.serialize(serializer),
            Self::McpApprovalResponse(value) => value.serialize(serializer),
            Self::McpCall(value) => value.serialize(serializer),
            Self::CustomToolCall(value) => value.serialize(serializer),
            Self::CustomToolCallOutput(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ConversationItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match discriminator(&value).map_err(D::Error::custom)?.as_str() {
            "message" => serde_json::from_value(value)
                .map(Self::Message)
                .map_err(D::Error::custom),
            "function_call" => serde_json::from_value(value)
                .map(Self::FunctionCall)
                .map_err(D::Error::custom),
            "function_call_output" => serde_json::from_value(value)
                .map(Self::FunctionCallOutput)
                .map_err(D::Error::custom),
            "file_search_call" => serde_json::from_value(value)
                .map(Self::FileSearchCall)
                .map_err(D::Error::custom),
            "web_search_call" => serde_json::from_value(value)
                .map(Self::WebSearchCall)
                .map_err(D::Error::custom),
            "image_generation_call" => serde_json::from_value(value)
                .map(Self::ImageGenerationCall)
                .map_err(D::Error::custom),
            "computer_call" => serde_json::from_value(value)
                .map(Self::ComputerCall)
                .map_err(D::Error::custom),
            "computer_call_output" => serde_json::from_value(value)
                .map(Self::ComputerCallOutput)
                .map_err(D::Error::custom),
            "tool_search_call" => serde_json::from_value(value)
                .map(Self::ToolSearchCall)
                .map_err(D::Error::custom),
            "tool_search_output" => serde_json::from_value(value)
                .map(Self::ToolSearchOutput)
                .map_err(D::Error::custom),
            "additional_tools" => serde_json::from_value(value)
                .map(Self::AdditionalTools)
                .map_err(D::Error::custom),
            "reasoning" => serde_json::from_value(value)
                .map(Self::Reasoning)
                .map_err(D::Error::custom),
            "program" => serde_json::from_value(value)
                .map(Self::Program)
                .map_err(D::Error::custom),
            "program_output" => serde_json::from_value(value)
                .map(Self::ProgramOutput)
                .map_err(D::Error::custom),
            "compaction" => serde_json::from_value(value)
                .map(Self::Compaction)
                .map_err(D::Error::custom),
            "code_interpreter_call" => serde_json::from_value(value)
                .map(Self::CodeInterpreterCall)
                .map_err(D::Error::custom),
            "local_shell_call" => serde_json::from_value(value)
                .map(Self::LocalShellCall)
                .map_err(D::Error::custom),
            "local_shell_call_output" => serde_json::from_value(value)
                .map(Self::LocalShellCallOutput)
                .map_err(D::Error::custom),
            "shell_call" => serde_json::from_value(value)
                .map(Self::FunctionShellCall)
                .map_err(D::Error::custom),
            "shell_call_output" => serde_json::from_value(value)
                .map(Self::FunctionShellCallOutput)
                .map_err(D::Error::custom),
            "apply_patch_call" => serde_json::from_value(value)
                .map(Self::ApplyPatchCall)
                .map_err(D::Error::custom),
            "apply_patch_call_output" => serde_json::from_value(value)
                .map(Self::ApplyPatchCallOutput)
                .map_err(D::Error::custom),
            "mcp_list_tools" => serde_json::from_value(value)
                .map(Self::McpListTools)
                .map_err(D::Error::custom),
            "mcp_approval_request" => serde_json::from_value(value)
                .map(Self::McpApprovalRequest)
                .map_err(D::Error::custom),
            "mcp_approval_response" => serde_json::from_value(value)
                .map(Self::McpApprovalResponse)
                .map_err(D::Error::custom),
            "mcp_call" => serde_json::from_value(value)
                .map(Self::McpCall)
                .map_err(D::Error::custom),
            "custom_tool_call" => serde_json::from_value(value)
                .map(Self::CustomToolCall)
                .map_err(D::Error::custom),
            "custom_tool_call_output" => serde_json::from_value(value)
                .map(Self::CustomToolCallOutput)
                .map_err(D::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

impl ConversationItem {
    /// Converts a persisted item back into a Responses input item.
    pub fn to_response_input_item(
        &self,
    ) -> Result<responses::ResponseInputItem, ConversationItemConversionError> {
        if let Self::Message(message) = self {
            return message.to_response_input_item();
        }
        let value = serde_json::to_value(self)?;
        serde_json::from_value(value).map_err(ConversationItemConversionError::from)
    }
}

impl TryFrom<responses::ResponseOutputItem> for ConversationItem {
    type Error = ConversationItemConversionError;

    fn try_from(value: responses::ResponseOutputItem) -> Result<Self, Self::Error> {
        if let responses::ResponseOutputItem::Message(message) = value {
            let content = message
                .content()
                .iter()
                .map(|part| match part {
                    responses::OutputContent::Text(value) => {
                        Ok(ConversationMessageContent::OutputText(value.clone()))
                    }
                    responses::OutputContent::Refusal(value) => {
                        Ok(ConversationMessageContent::Refusal(value.clone()))
                    }
                    responses::OutputContent::Unknown(value) => {
                        Ok(ConversationMessageContent::Unknown(value.clone()))
                    }
                })
                .collect::<Result<Vec<_>, ConversationItemConversionError>>()?;
            return Ok(Self::Message(ConversationMessage::new(
                message.id(),
                message.status().clone(),
                ConversationMessageRole::Assistant,
                content,
            )));
        }
        let value = serde_json::to_value(value)?;
        serde_json::from_value(value).map_err(ConversationItemConversionError::from)
    }
}

/// Failure converting between persisted Conversation and Responses item schemas.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConversationItemConversionError {
    /// JSON codec failed while translating equivalent resource shapes.
    #[error("conversation/Responses item conversion failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A persisted message role has no equivalent Responses input role.
    #[error("conversation message role `{role}` cannot be converted to a Responses input item")]
    UnsupportedMessageRole {
        /// Unsupported role.
        role: String,
    },
    /// A content type is incompatible with its message role.
    #[error("conversation content `{content_type}` is incompatible with role `{role}`")]
    ContentRoleMismatch {
        /// Message role.
        role: String,
        /// Content discriminator.
        content_type: String,
    },
    /// An image has neither a URL nor a file id.
    #[error("persisted input image has no non-null URL or file id")]
    ImageHasNoSource,
    /// A file has no usable id, URL, or data/filename pair.
    #[error("persisted input file has no usable source")]
    FileHasNoSource,
}

/// Frozen schema-name inventory for the 28 Conversation item branches.
pub const CONVERSATION_ITEM_SCHEMAS: [&str; 28] = [
    "Message",
    "FunctionToolCallResource",
    "FunctionToolCallOutputResource",
    "FileSearchToolCall",
    "WebSearchToolCall",
    "ImageGenToolCall",
    "ComputerToolCall",
    "ComputerToolCallOutputResource",
    "ToolSearchCall",
    "ToolSearchOutput",
    "AdditionalTools",
    "ReasoningItem",
    "Program",
    "ProgramOutput",
    "CompactionBody",
    "CodeInterpreterToolCall",
    "LocalShellToolCall",
    "LocalShellToolCallOutput",
    "FunctionShellCall",
    "FunctionShellCallOutput",
    "ApplyPatchToolCall",
    "ApplyPatchToolCallOutput",
    "MCPListTools",
    "MCPApprovalRequest",
    "MCPApprovalResponseResource",
    "MCPToolCall",
    "CustomToolCallResource",
    "CustomToolCallOutput",
];

/// Discriminators aligned with [`CONVERSATION_ITEM_SCHEMAS`].
pub const CONVERSATION_ITEM_DISCRIMINATORS: [&str; 28] = [
    "message",
    "function_call",
    "function_call_output",
    "file_search_call",
    "web_search_call",
    "image_generation_call",
    "computer_call",
    "computer_call_output",
    "tool_search_call",
    "tool_search_output",
    "additional_tools",
    "reasoning",
    "program",
    "program_output",
    "compaction",
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
    "custom_tool_call",
    "custom_tool_call_output",
];
