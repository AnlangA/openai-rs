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

fn validate_item_count(
    items: &[responses::ResponseInputItem],
) -> Result<(), ConversationValidationError> {
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct UpdateConversationRequestWire {
    metadata: Nullable<ConversationMetadata>,
}

/// Body for `POST /conversations/{conversation_id}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateConversationRequest {
    metadata: Nullable<ConversationMetadata>,
}

impl<'de> Deserialize<'de> for UpdateConversationRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = UpdateConversationRequestWire::deserialize(deserializer)?;
        if let Nullable::Value(metadata) = &wire.metadata {
            validate_metadata(metadata).map_err(D::Error::custom)?;
        }
        Ok(Self {
            metadata: wire.metadata,
        })
    }
}

impl UpdateConversationRequest {
    /// Replaces conversation metadata.
    pub fn new(metadata: ConversationMetadata) -> Result<Self, ConversationValidationError> {
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
literal_tag!(
    ConversationReasoningTextTag,
    ReasoningText,
    "reasoning_text"
);

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
text_content!(
    ConversationSummaryText,
    ConversationSummaryTextTag,
    SummaryText
);
text_content!(
    ConversationReasoningText,
    ConversationReasoningTextTag,
    ReasoningText
);

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

    fn to_response_content(
        &self,
    ) -> Result<responses::InputContent, ConversationItemConversionError> {
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

literal_tag!(
    ConversationComputerScreenshotTag,
    ComputerScreenshot,
    "computer_screenshot"
);

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
    detail: Omittable<responses::FileDetail>,
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
    pub fn detail(mut self, detail: responses::FileDetail) -> Self {
        self.detail = Omittable::Value(detail);
        self
    }

    fn to_response_content(
        &self,
    ) -> Result<responses::InputContent, ConversationItemConversionError> {
        let value = match (
            &self.file_id,
            &self.file_url,
            &self.file_data,
            &self.filename,
        ) {
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
        UnknownRole = "unknown",
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
                for part in &self.content {
                    if !matches!(
                        part,
                        ConversationMessageContent::OutputText(_)
                            | ConversationMessageContent::Refusal(_)
                    ) {
                        return Err(ConversationItemConversionError::ContentRoleMismatch {
                            role: self.role.as_str().to_owned(),
                            content_type: content_discriminator(part).to_owned(),
                        });
                    }
                }
                let value = serde_json::to_value(self)?;
                let output = serde_json::from_value::<responses::OutputMessage>(value)?;
                Ok(output.into())
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
                    _ => {
                        return Err(ConversationItemConversionError::UnsupportedMessageRole {
                            role: self.role.as_str().to_owned(),
                        });
                    }
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
                        ConversationMessageContent::InputFile(value) => value.to_response_content(),
                        _ => Err(ConversationItemConversionError::ContentRoleMismatch {
                            role: self.role.as_str().to_owned(),
                            content_type: content_discriminator(part).to_owned(),
                        }),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(responses::StoredInputMessage::new(role, content)
                    .status(self.status.clone().into())
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
    /// Custom-tool call.
    CustomToolCall(responses::CustomToolCall),
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
        match value {
            responses::ResponseOutputItem::Message(message) => {
                let value = serde_json::to_value(message)?;
                serde_json::from_value(value).map_err(ConversationItemConversionError::from)
            }
            value => {
                let value = serde_json::to_value(value)?;
                serde_json::from_value(value).map_err(ConversationItemConversionError::from)
            }
        }
    }
}

/// Conversation-specific name for the shared custom-tool call wire shape.
pub type ConversationCustomToolCall = responses::CustomToolCall;

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
    "CustomToolCall",
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

open_string_enum! {
    /// Optional fields that Conversations item endpoints may include.
    pub enum ConversationItemInclude {
        FileSearchResults = "file_search_call.results",
        WebSearchResults = "web_search_call.results",
        WebSearchSources = "web_search_call.action.sources",
        InputImageUrl = "message.input_image.image_url",
        ComputerOutputImageUrl = "computer_call_output.output.image_url",
        CodeInterpreterOutputs = "code_interpreter_call.outputs",
        ReasoningEncryptedContent = "reasoning.encrypted_content",
        OutputTextLogprobs = "message.output_text.logprobs",
    }
}

open_string_enum! {
    /// Sort order for listing conversation items.
    pub enum ConversationItemOrder {
        Ascending = "asc",
        Descending = "desc",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CreateConversationItemsRequestWire {
    items: Vec<responses::ResponseInputItem>,
}

/// Body for `POST /conversations/{conversation_id}/items`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateConversationItemsRequest {
    items: Vec<responses::ResponseInputItem>,
}

impl<'de> Deserialize<'de> for CreateConversationItemsRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateConversationItemsRequestWire::deserialize(deserializer)?;
        validate_item_count(&wire.items).map_err(D::Error::custom)?;
        Ok(Self { items: wire.items })
    }
}

impl CreateConversationItemsRequest {
    /// Creates a validated body containing up to twenty items.
    pub fn new(
        items: impl IntoIterator<Item = responses::ResponseInputItem>,
    ) -> Result<Self, ConversationValidationError> {
        let items = items.into_iter().collect::<Vec<_>>();
        validate_item_count(&items)?;
        Ok(Self { items })
    }

    /// Creates a body containing one item.
    #[must_use]
    pub fn one(item: impl Into<responses::ResponseInputItem>) -> Self {
        Self {
            items: vec![item.into()],
        }
    }

    /// Appends an item while enforcing the per-request maximum.
    pub fn item(
        mut self,
        item: impl Into<responses::ResponseInputItem>,
    ) -> Result<Self, ConversationValidationError> {
        self.items.push(item.into());
        validate_item_count(&self.items)?;
        Ok(self)
    }

    /// Returns request items.
    #[must_use]
    pub fn items(&self) -> &[responses::ResponseInputItem] {
        &self.items
    }
}

/// Query parameters used while adding items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateConversationItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Vec<ConversationItemInclude>>,
}

impl CreateConversationItemsParams {
    /// Creates empty query parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one optional field family.
    #[must_use]
    pub fn include(mut self, include: ConversationItemInclude) -> Self {
        let mut values = match std::mem::take(&mut self.include) {
            Omittable::Value(values) => values,
            Omittable::Omitted => Vec::new(),
        };
        values.push(include);
        self.include = Omittable::Value(values);
        self
    }
}

/// Query parameters for retrieving one item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RetrieveConversationItemParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Vec<ConversationItemInclude>>,
}

impl RetrieveConversationItemParams {
    /// Creates empty query parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one optional field family.
    #[must_use]
    pub fn include(mut self, include: ConversationItemInclude) -> Self {
        let mut values = match std::mem::take(&mut self.include) {
            Omittable::Value(values) => values,
            Omittable::Omitted => Vec::new(),
        };
        values.push(include);
        self.include = Omittable::Value(values);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct ListConversationItemsParamsWire {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<ConversationItemOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<ConversationItemId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Vec<ConversationItemInclude>>,
}

/// Query parameters for listing persisted conversation items.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ListConversationItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<ConversationItemOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<ConversationItemId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include: Omittable<Vec<ConversationItemInclude>>,
}

impl<'de> Deserialize<'de> for ListConversationItemsParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ListConversationItemsParamsWire::deserialize(deserializer)?;
        if let Omittable::Value(limit) = wire.limit {
            if !(1..=100).contains(&limit) {
                return Err(D::Error::custom(
                    ConversationValidationError::InvalidListLimit { limit },
                ));
            }
        }
        Ok(Self {
            limit: wire.limit,
            order: wire.order,
            after: wire.after,
            include: wire.include,
        })
    }
}

impl ListConversationItemsParams {
    /// Creates empty list parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a validated page size.
    pub fn limit(mut self, limit: u32) -> Result<Self, ConversationValidationError> {
        if !(1..=100).contains(&limit) {
            return Err(ConversationValidationError::InvalidListLimit { limit });
        }
        self.limit = Omittable::Value(limit);
        Ok(self)
    }

    /// Sets item ordering.
    #[must_use]
    pub fn order(mut self, order: ConversationItemOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Starts after an opaque item id.
    #[must_use]
    pub fn after(mut self, after: impl Into<ConversationItemId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Adds one optional field family.
    #[must_use]
    pub fn include(mut self, include: ConversationItemInclude) -> Self {
        let mut values = match std::mem::take(&mut self.include) {
            Omittable::Value(values) => values,
            Omittable::Omitted => Vec::new(),
        };
        values.push(include);
        self.include = Omittable::Value(values);
        self
    }

    /// Returns the opaque pagination cursor.
    #[must_use]
    pub fn after_ref(&self) -> Option<&ConversationItemId> {
        match &self.after {
            Omittable::Value(after) => Some(after),
            Omittable::Omitted => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ConversationItemListObjectTag {
    #[serde(rename = "list")]
    List,
}

/// Cursor page returned by item create/list endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationItemList {
    #[serde(rename = "object")]
    object: ConversationItemListObjectTag,
    data: Vec<ConversationItem>,
    has_more: bool,
    first_id: Nullable<ConversationItemId>,
    last_id: Nullable<ConversationItemId>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ConversationItemList {
    /// Returns page items.
    #[must_use]
    pub fn data(&self) -> &[ConversationItem] {
        &self.data
    }

    /// Returns whether another page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Returns the first id when the page is non-empty.
    #[must_use]
    pub fn first_id(&self) -> Option<&ConversationItemId> {
        match &self.first_id {
            Nullable::Value(id) => Some(id),
            Nullable::Null => None,
        }
    }

    /// Returns the last id for cursor pagination.
    #[must_use]
    pub fn last_id(&self) -> Option<&ConversationItemId> {
        match &self.last_id {
            Nullable::Value(id) => Some(id),
            Nullable::Null => None,
        }
    }

    /// Returns future response properties retained while decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

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
    fn public_wire_types_are_owned_and_bidirectional() {
        assert_json_dto::<ConversationId>();
        assert_json_dto::<ConversationItemId>();
        assert_json_dto::<CreateConversationRequest>();
        assert_json_dto::<UpdateConversationRequest>();
        assert_json_dto::<Conversation>();
        assert_json_dto::<DeletedConversation>();
        assert_json_dto::<ConversationText>();
        assert_json_dto::<ConversationSummaryText>();
        assert_json_dto::<ConversationReasoningText>();
        assert_json_dto::<ConversationInputImage>();
        assert_json_dto::<ConversationComputerScreenshot>();
        assert_json_dto::<ConversationInputFile>();
        assert_json_dto::<ConversationMessageContent>();
        assert_json_dto::<ConversationMessageRole>();
        assert_json_dto::<ConversationMessage>();
        assert_json_dto::<ConversationFunctionCall>();
        assert_json_dto::<ConversationItem>();
        assert_json_dto::<ConversationItemInclude>();
        assert_json_dto::<ConversationItemOrder>();
        assert_json_dto::<CreateConversationItemsRequest>();
        assert_json_dto::<CreateConversationItemsParams>();
        assert_json_dto::<RetrieveConversationItemParams>();
        assert_json_dto::<ListConversationItemsParams>();
        assert_json_dto::<ConversationItemList>();
    }

    #[test]
    fn create_request_preserves_missing_null_empty_and_typed_items() {
        let omitted: CreateConversationRequest =
            serde_json::from_value(json!({})).expect("decode omitted request");
        assert_eq!(
            serde_json::to_value(omitted).expect("encode omitted request"),
            json!({})
        );

        let null: CreateConversationRequest = serde_json::from_value(json!({
            "metadata": null,
            "items": null
        }))
        .expect("decode explicit nulls");
        assert_eq!(
            serde_json::to_value(null).expect("encode explicit nulls"),
            json!({"metadata": null, "items": null})
        );

        let empty: CreateConversationRequest = serde_json::from_value(json!({
            "metadata": {},
            "items": []
        }))
        .expect("decode explicit empty values");
        assert_eq!(
            serde_json::to_value(empty).expect("encode explicit empty values"),
            json!({"metadata": {}, "items": []})
        );

        let request = CreateConversationRequest::new()
            .metadata_entry("topic", "demo")
            .expect("valid metadata")
            .item(responses::InputMessage::user("Hello!"))
            .expect("one item");
        assert_eq!(
            serde_json::to_value(request).expect("encode typed request"),
            json!({
                "metadata": {"topic": "demo"},
                "items": [{"role": "user", "content": "Hello!"}]
            })
        );
    }

    #[test]
    fn request_validation_enforces_item_metadata_and_page_limits() {
        let item: responses::ResponseInputItem = responses::InputMessage::user("x").into();
        let items = vec![item; MAX_CONVERSATION_ITEMS_PER_REQUEST + 1];
        let request = CreateConversationItemsRequest::new(items.clone());
        assert!(matches!(
            request,
            Err(ConversationValidationError::TooManyItems { actual: 21, .. })
        ));
        assert!(
            serde_json::from_value::<CreateConversationRequest>(json!({"items": items})).is_err()
        );

        let mut metadata = ConversationMetadata::new();
        for index in 0..=MAX_CONVERSATION_METADATA_PROPERTIES {
            metadata.insert(format!("key_{index}"), String::from("value"));
        }
        assert!(matches!(
            UpdateConversationRequest::new(metadata),
            Err(ConversationValidationError::TooManyMetadataProperties { .. })
        ));
        let oversized_metadata = (0..=MAX_CONVERSATION_METADATA_PROPERTIES)
            .map(|index| (format!("key_{index}"), String::from("value")))
            .collect::<ConversationMetadata>();
        assert!(
            serde_json::from_value::<UpdateConversationRequest>(json!({
                "metadata": oversized_metadata
            }))
            .is_err()
        );

        assert!(matches!(
            ListConversationItemsParams::new().limit(0),
            Err(ConversationValidationError::InvalidListLimit { limit: 0 })
        ));
        assert!(
            serde_json::from_value::<ListConversationItemsParams>(json!({"limit": 101})).is_err()
        );
        assert!(ListConversationItemsParams::new().limit(100).is_ok());
    }

    #[test]
    fn update_requires_metadata_but_allows_explicit_null() {
        assert!(serde_json::from_value::<UpdateConversationRequest>(json!({})).is_err());
        let clear: UpdateConversationRequest =
            serde_json::from_value(json!({"metadata": null})).expect("decode clear metadata");
        assert!(clear.metadata().is_none());
        assert_eq!(
            serde_json::to_value(clear).expect("encode clear metadata"),
            json!({"metadata": null})
        );
    }

    #[test]
    fn conversation_resources_preserve_nullable_metadata_and_extra_fields() {
        let fixture = json!({
            "id": "conv_123",
            "object": "conversation",
            "created_at": 1741900000,
            "metadata": null,
            "future_resource_field": {"kept": true}
        });
        let conversation: Conversation =
            serde_json::from_value(fixture.clone()).expect("decode conversation");
        assert_eq!(conversation.id().as_str(), "conv_123");
        assert!(conversation.metadata().is_none());
        assert_eq!(
            conversation.extra_fields().get("future_resource_field"),
            Some(&json!({"kept": true}))
        );
        assert_eq!(
            serde_json::to_value(conversation).expect("round-trip conversation"),
            fixture
        );

        let deleted: DeletedConversation = serde_json::from_value(json!({
            "id": "conv_123",
            "object": "conversation.deleted",
            "deleted": true,
            "future": 1
        }))
        .expect("decode deleted conversation");
        assert!(deleted.is_deleted());
        assert_eq!(deleted.extra_fields().get("future"), Some(&json!(1)));
    }

    fn user_message_fixture() -> Value {
        json!({
            "type": "message",
            "id": "msg_user",
            "status": "completed",
            "role": "user",
            "content": [
                {"type": "input_text", "text": "Hello!", "future_content": 1},
                {
                    "type": "input_image",
                    "detail": "auto",
                    "image_url": null,
                    "file_id": "file_123"
                }
            ],
            "future_message": true
        })
    }

    fn assistant_message_fixture() -> Value {
        json!({
            "type": "message",
            "id": "msg_assistant",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "Hi there",
                "annotations": [],
                "logprobs": []
            }],
            "phase": "final_answer"
        })
    }

    #[test]
    fn ambiguous_message_resource_decodes_once_then_converts_by_role() {
        let user: ConversationItem = serde_json::from_value(user_message_fixture())
            .expect("decode general user message resource");
        let ConversationItem::Message(user) = user else {
            panic!("expected conversation message");
        };
        assert_eq!(user.role(), &ConversationMessageRole::User);
        let input = user
            .to_response_input_item()
            .expect("convert user resource to Responses input");
        assert!(matches!(
            &input,
            responses::ResponseInputItem::StoredMessage(_)
        ));

        let assistant: ConversationItem = serde_json::from_value(assistant_message_fixture())
            .expect("decode general assistant message resource");
        let ConversationItem::Message(assistant) = assistant else {
            panic!("expected conversation message");
        };
        let input = assistant
            .to_response_input_item()
            .expect("convert assistant resource to Responses input");
        assert!(matches!(
            &input,
            responses::ResponseInputItem::OutputMessage(_)
        ));
        assert_eq!(
            serde_json::to_value(input).expect("serialize converted assistant item")["phase"],
            "final_answer"
        );
    }

    #[test]
    fn output_message_converts_to_conversation_and_back_without_json_authorship() {
        let output = responses::OutputMessage::new(
            "msg_1",
            responses::ItemProgressStatus::Completed,
            [responses::OutputText::new("answer")],
        );
        let item = ConversationItem::try_from(responses::ResponseOutputItem::Message(output))
            .expect("convert output resource");
        let input = item
            .to_response_input_item()
            .expect("convert persisted output back to input");
        let responses::ResponseInputItem::OutputMessage(message) = input else {
            panic!("assistant conversation item must remain output message");
        };
        assert_eq!(message.id(), "msg_1");
        assert_eq!(message.text_parts().collect::<String>(), "answer");
    }

    #[test]
    fn incompatible_content_role_is_a_typed_conversion_error() {
        let message = ConversationMessage::new(
            "msg_bad",
            responses::ResponseItemStatus::Completed,
            ConversationMessageRole::Assistant,
            [ConversationMessageContent::InputText(
                responses::InputText::new("not assistant output"),
            )],
        );
        assert!(matches!(
            message.to_response_input_item(),
            Err(ConversationItemConversionError::ContentRoleMismatch { .. })
        ));
    }

    #[test]
    fn known_item_tags_are_strict_and_future_items_round_trip() {
        assert_eq!(CONVERSATION_ITEM_SCHEMAS.len(), 28);
        assert_eq!(CONVERSATION_ITEM_DISCRIMINATORS.len(), 28);
        for discriminator in CONVERSATION_ITEM_DISCRIMINATORS {
            assert!(
                serde_json::from_value::<ConversationItem>(json!({"type": discriminator})).is_err(),
                "known item tag {discriminator} must validate required fields"
            );
        }

        let malformed_content = serde_json::from_value::<ConversationMessageContent>(json!({
            "type": "input_text"
        }));
        assert!(malformed_content.is_err());

        let fixture = json!({
            "type": "future_conversation_item",
            "id": "future_1",
            "payload": {"nested": true}
        });
        let item: ConversationItem =
            serde_json::from_value(fixture.clone()).expect("decode future item");
        let ConversationItem::Unknown(unknown) = &item else {
            panic!("future item must remain unknown");
        };
        assert_eq!(unknown.discriminator(), "future_conversation_item");
        assert_eq!(
            serde_json::to_value(item).expect("round-trip future item"),
            fixture
        );
    }

    #[test]
    fn item_pages_and_query_builders_are_typed_and_lossless() {
        let fixture = json!({
            "object": "list",
            "data": [user_message_fixture()],
            "has_more": false,
            "first_id": null,
            "last_id": null,
            "future_page": "kept"
        });
        let page: ConversationItemList =
            serde_json::from_value(fixture.clone()).expect("decode empty-cursor page");
        assert_eq!(page.data().len(), 1);
        assert!(page.first_id().is_none());
        assert!(page.last_id().is_none());
        assert_eq!(
            serde_json::to_value(page).expect("round-trip page"),
            fixture
        );

        let params = ListConversationItemsParams::new()
            .limit(25)
            .expect("valid limit")
            .order(ConversationItemOrder::Ascending)
            .after("msg_cursor")
            .include(ConversationItemInclude::ReasoningEncryptedContent);
        assert_eq!(
            serde_json::to_value(params).expect("encode list params"),
            json!({
                "limit": 25,
                "order": "asc",
                "after": "msg_cursor",
                "include": ["reasoning.encrypted_content"]
            })
        );
    }
}
