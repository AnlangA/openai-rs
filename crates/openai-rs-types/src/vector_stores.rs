//! Typed wire models for Vector Stores, attached files, file batches, and search.
//!
//! The types in this module preserve missing, explicit `null`, and present
//! values independently. Attribute maps and numeric ranges are validated both
//! by builders and during deserialization; recursive search filters use a
//! discriminator-aware enum so malformed known variants cannot silently become
//! unknown variants.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};
use thiserror::Error;

use crate::{
    ExtraFields, FileId, Nullable, Omittable, VectorStoreId, opaque_string_id,
    open_string_enum, responses::UnknownTaggedObject,
};

opaque_string_id! {
    /// Opaque identifier of a vector-store file batch.
    pub struct VectorStoreFileBatchId;
}

open_string_enum! {
    /// Object discriminator returned for a vector store.
    pub enum VectorStoreObjectType {
        VectorStore = "vector_store",
    }
}

open_string_enum! {
    /// Object discriminator returned after deleting a vector store.
    pub enum DeletedVectorStoreObjectType {
        Deleted = "vector_store.deleted",
    }
}

open_string_enum! {
    /// Object discriminator returned for one attached file.
    pub enum VectorStoreFileObjectType {
        File = "vector_store.file",
    }
}

open_string_enum! {
    /// Object discriminator returned after detaching a file.
    pub enum DeletedVectorStoreFileObjectType {
        Deleted = "vector_store.file.deleted",
    }
}

open_string_enum! {
    /// Object discriminator returned for a vector-store file batch.
    pub enum VectorStoreFileBatchObjectType {
        FilesBatch = "vector_store.files_batch",
    }
}

open_string_enum! {
    /// Collection discriminator used by list endpoints.
    pub enum VectorStoreListObjectType {
        List = "list",
    }
}

open_string_enum! {
    /// Object discriminator for a search-result page.
    pub enum VectorStoreSearchPageObjectType {
        Page = "vector_store.search_results.page",
    }
}

open_string_enum! {
    /// Object discriminator for parsed file content.
    pub enum VectorStoreFileContentPageObjectType {
        Page = "vector_store.file_content.page",
    }
}

open_string_enum! {
    /// Lifecycle state of a vector store.
    pub enum VectorStoreStatus {
        Expired = "expired",
        InProgress = "in_progress",
        Completed = "completed",
    }
}

open_string_enum! {
    /// Lifecycle state shared by attached files and file batches.
    pub enum VectorStoreFileStatus {
        InProgress = "in_progress",
        Completed = "completed",
        Cancelled = "cancelled",
        Failed = "failed",
    }
}

open_string_enum! {
    /// Last-error code returned for an attached file.
    pub enum VectorStoreFileErrorCode {
        ServerError = "server_error",
        UnsupportedFile = "unsupported_file",
        InvalidFile = "invalid_file",
    }
}

open_string_enum! {
    /// Creation-time sort order for vector-store list endpoints.
    pub enum VectorStoreSortOrder {
        Ascending = "asc",
        Descending = "desc",
    }
}

open_string_enum! {
    /// Expiration anchor accepted for vector stores.
    pub enum VectorStoreExpirationAnchor {
        LastActiveAt = "last_active_at",
    }
}

open_string_enum! {
    /// Comparison operation for an attribute filter.
    pub enum VectorStoreComparisonOperator {
        Equal = "eq",
        NotEqual = "ne",
        GreaterThan = "gt",
        GreaterThanOrEqual = "gte",
        LessThan = "lt",
        LessThanOrEqual = "lte",
        In = "in",
        NotIn = "nin",
    }
}

open_string_enum! {
    /// Boolean operation for a recursive filter group.
    pub enum VectorStoreCompoundOperator {
        And = "and",
        Or = "or",
    }
}

open_string_enum! {
    /// Ranker accepted by vector-store search.
    pub enum VectorStoreRanker {
        None = "none",
        Auto = "auto",
        Default2024_11_15 = "default-2024-11-15",
    }
}

open_string_enum! {
    /// Content kind returned by current vector-store content endpoints.
    pub enum VectorStoreContentType {
        Text = "text",
    }
}

/// Maximum number of metadata or attribute pairs.
pub const MAX_VECTOR_STORE_PROPERTIES: usize = 16;
/// Maximum key length in Unicode scalar values.
pub const MAX_VECTOR_STORE_KEY_CHARS: usize = 64;
/// Maximum string value length in Unicode scalar values.
pub const MAX_VECTOR_STORE_VALUE_CHARS: usize = 512;
/// Maximum initial file count for vector-store creation.
pub const MAX_VECTOR_STORE_INITIAL_FILES: usize = 500;
/// Maximum number of files accepted by a file-batch request.
pub const MAX_VECTOR_STORE_BATCH_FILES: usize = 2_000;
/// Minimum static chunk size.
pub const MIN_STATIC_CHUNK_TOKENS: u32 = 100;
/// Maximum static chunk size.
pub const MAX_STATIC_CHUNK_TOKENS: u32 = 4_096;
/// Minimum expiration period in days.
pub const MIN_VECTOR_STORE_EXPIRATION_DAYS: u16 = 1;
/// Maximum expiration period in days.
pub const MAX_VECTOR_STORE_EXPIRATION_DAYS: u16 = 365;
/// Default list page size.
pub const DEFAULT_VECTOR_STORE_LIST_LIMIT: u32 = 20;
/// Maximum list page size.
pub const MAX_VECTOR_STORE_LIST_LIMIT: u32 = 100;
/// Default number of vector search results.
pub const DEFAULT_VECTOR_STORE_SEARCH_RESULTS: u8 = 10;
/// Maximum vector search result count.
pub const MAX_VECTOR_STORE_SEARCH_RESULTS: u8 = 50;

/// Validation error for Vector Store request values.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum VectorStoreValidationError {
    /// A map contains too many properties.
    #[error("vector-store map contains {actual} properties; maximum is {maximum}")]
    TooManyProperties {
        /// Observed count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// An attribute or metadata key is too long.
    #[error("vector-store key has {actual} characters; maximum is {maximum}")]
    KeyTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A string attribute or metadata value is too long.
    #[error("vector-store string value has {actual} characters; maximum is {maximum}")]
    ValueTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Initial file count exceeds the request limit.
    #[error("vector store contains {actual} initial files; maximum is {maximum}")]
    TooManyInitialFiles {
        /// Observed count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// File-batch size is outside `1..=2000`.
    #[error("vector-store file batch contains {actual} files; expected 1..={maximum}")]
    InvalidBatchFileCount {
        /// Observed count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Both or neither file-batch input alternatives were supplied.
    #[error("vector-store file batch must contain exactly one of file_ids or files")]
    InvalidBatchInputChoice,
    /// Expiration period is outside `1..=365`.
    #[error("vector-store expiration must be 1..=365 days, got {days}")]
    InvalidExpirationDays {
        /// Rejected value.
        days: u16,
    },
    /// Static chunk size is outside `100..=4096`.
    #[error("static max_chunk_size_tokens must be 100..=4096, got {tokens}")]
    InvalidChunkSize {
        /// Rejected size.
        tokens: u32,
    },
    /// Chunk overlap exceeds half the maximum chunk size.
    #[error("chunk_overlap_tokens {overlap} exceeds half of max_chunk_size_tokens {maximum}")]
    InvalidChunkOverlap {
        /// Rejected overlap.
        overlap: u32,
        /// Selected maximum chunk size.
        maximum: u32,
    },
    /// Page size is outside `1..=100`.
    #[error("vector-store list limit must be 1..=100, got {limit}")]
    InvalidListLimit {
        /// Rejected value.
        limit: u32,
    },
    /// A query array is empty.
    #[error("vector-store search query array must not be empty")]
    EmptySearchQueries,
    /// Search result count is outside `1..=50`.
    #[error("vector-store max_num_results must be 1..=50, got {maximum}")]
    InvalidMaxResults {
        /// Rejected value.
        maximum: u8,
    },
    /// A score is non-finite or outside `[0, 1]`.
    #[error("vector-store score must be finite and between 0 and 1, got {score}")]
    InvalidScore {
        /// Rejected value rendered without retaining a floating-point field in
        /// this equality-capable error type.
        score: String,
    },
    /// A floating-point value cannot be represented as a JSON number.
    #[error("attribute number is not finite")]
    NonFiniteNumber,
}

fn validate_map_key(key: &str) -> Result<(), VectorStoreValidationError> {
    let actual = key.chars().count();
    if actual > MAX_VECTOR_STORE_KEY_CHARS {
        return Err(VectorStoreValidationError::KeyTooLong {
            actual,
            maximum: MAX_VECTOR_STORE_KEY_CHARS,
        });
    }
    Ok(())
}

fn validate_string_value(value: &str) -> Result<(), VectorStoreValidationError> {
    let actual = value.chars().count();
    if actual > MAX_VECTOR_STORE_VALUE_CHARS {
        return Err(VectorStoreValidationError::ValueTooLong {
            actual,
            maximum: MAX_VECTOR_STORE_VALUE_CHARS,
        });
    }
    Ok(())
}

/// Validated string metadata attached to a vector store.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct VectorStoreMetadata(BTreeMap<String, String>);

impl VectorStoreMetadata {
    /// Creates an empty map.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Inserts one validated pair.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>, VectorStoreValidationError> {
        let key = key.into();
        let value = value.into();
        validate_map_key(&key)?;
        validate_string_value(&value)?;
        if !self.0.contains_key(&key) && self.0.len() == MAX_VECTOR_STORE_PROPERTIES {
            return Err(VectorStoreValidationError::TooManyProperties {
                actual: self.0.len().saturating_add(1),
                maximum: MAX_VECTOR_STORE_PROPERTIES,
            });
        }
        Ok(self.0.insert(key, value))
    }

    /// Iterates over pairs in stable key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Number of pairs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for VectorStoreMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VectorStoreMetadata")
            .field("property_count", &self.0.len())
            .finish()
    }
}

impl Serialize for VectorStoreMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VectorStoreMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, String>::deserialize(deserializer)?;
        if values.len() > MAX_VECTOR_STORE_PROPERTIES {
            return Err(serde::de::Error::custom(
                VectorStoreValidationError::TooManyProperties {
                    actual: values.len(),
                    maximum: MAX_VECTOR_STORE_PROPERTIES,
                },
            ));
        }
        for (key, value) in &values {
            validate_map_key(key).and_then(|()| validate_string_value(value))
                .map_err(serde::de::Error::custom)?;
        }
        Ok(Self(values))
    }
}

/// Scalar value accepted in vector-store file attributes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum VectorStoreAttributeValue {
    /// String attribute.
    String(String),
    /// Arbitrary finite JSON number.
    Number(Number),
    /// Boolean attribute.
    Boolean(bool),
}

impl VectorStoreAttributeValue {
    /// Creates a validated string value.
    pub fn string(value: impl Into<String>) -> Result<Self, VectorStoreValidationError> {
        let value = value.into();
        validate_string_value(&value)?;
        Ok(Self::String(value))
    }

    /// Creates a JSON number from a finite `f64`.
    pub fn number(value: f64) -> Result<Self, VectorStoreValidationError> {
        Number::from_f64(value)
            .map(Self::Number)
            .ok_or(VectorStoreValidationError::NonFiniteNumber)
    }
}

impl From<bool> for VectorStoreAttributeValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for VectorStoreAttributeValue {
    fn from(value: i64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl From<u64> for VectorStoreAttributeValue {
    fn from(value: u64) -> Self {
        Self::Number(Number::from(value))
    }
}

/// Validated attributes attached to one vector-store file.
#[derive(Clone, Default, PartialEq)]
pub struct VectorStoreFileAttributes(BTreeMap<String, VectorStoreAttributeValue>);

impl VectorStoreFileAttributes {
    /// Creates an empty attribute map.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Inserts one validated attribute.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: VectorStoreAttributeValue,
    ) -> Result<Option<VectorStoreAttributeValue>, VectorStoreValidationError> {
        let key = key.into();
        validate_map_key(&key)?;
        if let VectorStoreAttributeValue::String(value) = &value {
            validate_string_value(value)?;
        }
        if !self.0.contains_key(&key) && self.0.len() == MAX_VECTOR_STORE_PROPERTIES {
            return Err(VectorStoreValidationError::TooManyProperties {
                actual: self.0.len().saturating_add(1),
                maximum: MAX_VECTOR_STORE_PROPERTIES,
            });
        }
        Ok(self.0.insert(key, value))
    }

    /// Returns an attribute.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&VectorStoreAttributeValue> {
        self.0.get(key)
    }

    /// Iterates in stable key order.
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (&str, &VectorStoreAttributeValue)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Number of attributes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for VectorStoreFileAttributes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VectorStoreFileAttributes")
            .field("property_count", &self.0.len())
            .finish()
    }
}

impl Serialize for VectorStoreFileAttributes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VectorStoreFileAttributes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, VectorStoreAttributeValue>::deserialize(deserializer)?;
        if values.len() > MAX_VECTOR_STORE_PROPERTIES {
            return Err(serde::de::Error::custom(
                VectorStoreValidationError::TooManyProperties {
                    actual: values.len(),
                    maximum: MAX_VECTOR_STORE_PROPERTIES,
                },
            ));
        }
        for (key, value) in &values {
            validate_map_key(key).map_err(serde::de::Error::custom)?;
            if let VectorStoreAttributeValue::String(value) = value {
                validate_string_value(value).map_err(serde::de::Error::custom)?;
            }
        }
        Ok(Self(values))
    }
}

/// Validated vector-store expiration policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VectorStoreExpirationAfter {
    anchor: VectorStoreExpirationAnchor,
    days: u16,
}

impl VectorStoreExpirationAfter {
    /// Creates a policy anchored at the store's last activity.
    pub fn new(days: u16) -> Result<Self, VectorStoreValidationError> {
        Self::from_raw_anchor(VectorStoreExpirationAnchor::LastActiveAt, days)
    }

    /// Creates a policy with a forward-compatible anchor.
    pub fn from_raw_anchor(
        anchor: VectorStoreExpirationAnchor,
        days: u16,
    ) -> Result<Self, VectorStoreValidationError> {
        if !(MIN_VECTOR_STORE_EXPIRATION_DAYS..=MAX_VECTOR_STORE_EXPIRATION_DAYS).contains(&days) {
            return Err(VectorStoreValidationError::InvalidExpirationDays { days });
        }
        Ok(Self { anchor, days })
    }

    /// Returns the anchor.
    #[must_use]
    pub const fn anchor(&self) -> &VectorStoreExpirationAnchor {
        &self.anchor
    }

    /// Returns the expiration period.
    #[must_use]
    pub const fn days(&self) -> u16 {
        self.days
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStoreExpirationAfterWire {
    anchor: VectorStoreExpirationAnchor,
    days: u16,
}

impl<'de> Deserialize<'de> for VectorStoreExpirationAfter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = VectorStoreExpirationAfterWire::deserialize(deserializer)?;
        Self::from_raw_anchor(wire.anchor, wire.days).map_err(serde::de::Error::custom)
    }
}

/// Validated parameters for static file chunking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StaticChunkingStrategy {
    max_chunk_size_tokens: u32,
    chunk_overlap_tokens: u32,
}

impl StaticChunkingStrategy {
    /// Creates a static strategy and enforces overlap no greater than half the
    /// maximum chunk size.
    pub fn new(
        max_chunk_size_tokens: u32,
        chunk_overlap_tokens: u32,
    ) -> Result<Self, VectorStoreValidationError> {
        if !(MIN_STATIC_CHUNK_TOKENS..=MAX_STATIC_CHUNK_TOKENS)
            .contains(&max_chunk_size_tokens)
        {
            return Err(VectorStoreValidationError::InvalidChunkSize {
                tokens: max_chunk_size_tokens,
            });
        }
        if chunk_overlap_tokens > max_chunk_size_tokens / 2 {
            return Err(VectorStoreValidationError::InvalidChunkOverlap {
                overlap: chunk_overlap_tokens,
                maximum: max_chunk_size_tokens,
            });
        }
        Ok(Self {
            max_chunk_size_tokens,
            chunk_overlap_tokens,
        })
    }

    /// Maximum tokens in one chunk.
    #[must_use]
    pub const fn max_chunk_size_tokens(&self) -> u32 {
        self.max_chunk_size_tokens
    }

    /// Tokens shared by adjacent chunks.
    #[must_use]
    pub const fn chunk_overlap_tokens(&self) -> u32 {
        self.chunk_overlap_tokens
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticChunkingStrategyWire {
    max_chunk_size_tokens: u32,
    chunk_overlap_tokens: u32,
}

impl<'de> Deserialize<'de> for StaticChunkingStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = StaticChunkingStrategyWire::deserialize(deserializer)?;
        Self::new(wire.max_chunk_size_tokens, wire.chunk_overlap_tokens)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum AutoChunkingTag {
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutoChunkingRequestWire {
    #[serde(rename = "type")]
    kind: AutoChunkingTag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum StaticChunkingTag {
    #[serde(rename = "static")]
    Static,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticChunkingWire {
    #[serde(rename = "type")]
    kind: StaticChunkingTag,
    r#static: StaticChunkingStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum OtherChunkingTag {
    #[serde(rename = "other")]
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OtherChunkingWire {
    #[serde(rename = "type")]
    kind: OtherChunkingTag,
}

fn tagged_type(value: &Value, context: &'static str) -> Result<&str, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{context} must be a JSON object"))?
        .get("type")
        .ok_or_else(|| format!("{context} is missing string field `type`"))?
        .as_str()
        .ok_or_else(|| format!("{context} field `type` must be a string"))
}

/// Chunking strategy accepted in create requests.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum VectorStoreChunkingStrategyRequest {
    /// Let OpenAI select the current default strategy.
    Auto,
    /// Use caller-selected static sizes.
    Static(StaticChunkingStrategy),
    /// Future strategy retained as a complete semantic object.
    Unknown(UnknownTaggedObject),
}

impl VectorStoreChunkingStrategyRequest {
    /// Creates an automatic strategy.
    #[must_use]
    pub const fn auto() -> Self {
        Self::Auto
    }

    /// Creates a static strategy.
    #[must_use]
    pub const fn static_strategy(strategy: StaticChunkingStrategy) -> Self {
        Self::Static(strategy)
    }
}

impl Serialize for VectorStoreChunkingStrategyRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Auto => AutoChunkingRequestWire {
                kind: AutoChunkingTag::Auto,
            }
            .serialize(serializer),
            Self::Static(strategy) => StaticChunkingWire {
                kind: StaticChunkingTag::Static,
                r#static: strategy.clone(),
            }
            .serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for VectorStoreChunkingStrategyRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "chunking strategy").map_err(serde::de::Error::custom)? {
            "auto" => serde_json::from_value::<AutoChunkingRequestWire>(value)
                .map(|_| Self::Auto)
                .map_err(serde::de::Error::custom),
            "static" => serde_json::from_value::<StaticChunkingWire>(value)
                .map(|wire| Self::Static(wire.r#static))
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Chunking strategy returned for an attached file.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum VectorStoreChunkingStrategy {
    /// Explicit static strategy.
    Static(StaticChunkingStrategy),
    /// Legacy or otherwise unspecified strategy.
    Other,
    /// Future strategy retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for VectorStoreChunkingStrategy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Static(strategy) => StaticChunkingWire {
                kind: StaticChunkingTag::Static,
                r#static: strategy.clone(),
            }
            .serialize(serializer),
            Self::Other => OtherChunkingWire {
                kind: OtherChunkingTag::Other,
            }
            .serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for VectorStoreChunkingStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "response chunking strategy")
            .map_err(serde::de::Error::custom)?
        {
            "static" => serde_json::from_value::<StaticChunkingWire>(value)
                .map(|wire| Self::Static(wire.r#static))
                .map_err(serde::de::Error::custom),
            "other" => serde_json::from_value::<OtherChunkingWire>(value)
                .map(|_| Self::Other)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Scalar or list value accepted by a comparison filter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum VectorStoreFilterValue {
    /// String scalar.
    String(String),
    /// Numeric scalar.
    Number(Number),
    /// Boolean scalar.
    Boolean(bool),
    /// String/number list used by `in` and `nin`.
    List(Vec<VectorStoreFilterListValue>),
}

/// One member of a comparison-filter list value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum VectorStoreFilterListValue {
    /// String list member.
    String(String),
    /// Numeric list member.
    Number(Number),
}

/// One comparison against a file attribute.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreComparisonFilter {
    #[serde(rename = "type")]
    operator: VectorStoreComparisonOperator,
    key: String,
    value: VectorStoreFilterValue,
}

impl VectorStoreComparisonFilter {
    /// Creates a comparison filter.
    #[must_use]
    pub fn new(
        operator: VectorStoreComparisonOperator,
        key: impl Into<String>,
        value: VectorStoreFilterValue,
    ) -> Self {
        Self {
            operator,
            key: key.into(),
            value,
        }
    }

    /// Comparison operator.
    #[must_use]
    pub const fn operator(&self) -> &VectorStoreComparisonOperator {
        &self.operator
    }

    /// Attribute key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Comparison value.
    #[must_use]
    pub const fn value(&self) -> &VectorStoreFilterValue {
        &self.value
    }
}

/// Recursive boolean group of vector-store filters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreCompoundFilter {
    #[serde(rename = "type")]
    operator: VectorStoreCompoundOperator,
    filters: Vec<VectorStoreFilter>,
}

impl VectorStoreCompoundFilter {
    /// Creates a recursive filter group.
    #[must_use]
    pub fn new(operator: VectorStoreCompoundOperator, filters: Vec<VectorStoreFilter>) -> Self {
        Self { operator, filters }
    }

    /// Boolean operator.
    #[must_use]
    pub const fn operator(&self) -> &VectorStoreCompoundOperator {
        &self.operator
    }

    /// Child filters.
    #[must_use]
    pub fn filters(&self) -> &[VectorStoreFilter] {
        &self.filters
    }
}

/// Discriminator-aware recursive attribute filter.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum VectorStoreFilter {
    /// Scalar comparison.
    Comparison(VectorStoreComparisonFilter),
    /// Recursive boolean group.
    Compound(Box<VectorStoreCompoundFilter>),
    /// Future filter retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for VectorStoreFilter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Comparison(value) => value.serialize(serializer),
            Self::Compound(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for VectorStoreFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "vector-store filter").map_err(serde::de::Error::custom)? {
            "and" | "or" => serde_json::from_value::<VectorStoreCompoundFilter>(value)
                .map(|value| Self::Compound(Box::new(value)))
                .map_err(serde::de::Error::custom),
            "eq" | "ne" | "gt" | "gte" | "lt" | "lte" | "in" | "nin" => {
                serde_json::from_value::<VectorStoreComparisonFilter>(value)
                    .map(Self::Comparison)
                    .map_err(serde::de::Error::custom)
            }
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}
