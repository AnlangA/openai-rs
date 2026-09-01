//! Typed wire models for Vector Stores, attached files, file batches, and search.
//!
//! The types in this module preserve missing, explicit `null`, and present
//! values independently. Attribute maps and numeric ranges are validated by
//! builders; metadata maps additionally decode losslessly and move their
//! documented 16/64/512 limits into the opt-in `validate` hooks of
//! [`CreateVectorStoreRequest`] and [`UpdateVectorStoreRequest`], matching the
//! pinned-schema-only constraints of D0015/D0017. Recursive search filters use
//! a discriminator-aware enum so malformed known variants cannot silently
//! become unknown variants.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};
use thiserror::Error;

use crate::kernel::UnknownTaggedObject;
use crate::{
    ExtraFields, FileId, Nullable, Omittable, VectorStoreId, opaque_string_id, open_string_enum,
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
/// Effective page size when `limit` is omitted.
///
/// The pinned list schemas document a default of 20 but impose no `maximum`,
/// so no upper bound is invented here.
pub const DEFAULT_VECTOR_STORE_LIST_LIMIT: u32 = 20;
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
    /// Global request fields were combined with the per-file `files` form.
    ///
    /// The pinned `anyOf` accepts this combination on the wire because the
    /// `files` branch is satisfied, but the global `attributes` and
    /// `chunking_strategy` fields are documented as ignored for the per-file
    /// form, so builders reject it instead of silently dropping intent.
    #[error(
        "vector-store file-batch global fields (attributes, chunking_strategy) apply only to the file_ids form and cannot be combined with the per-file files form; set them on each per-file request instead"
    )]
    GlobalFieldsWithPerFileBatch,
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
    /// Page size is below the schema-backed minimum of 1.
    #[error("vector-store list limit must be at least 1, got {limit}")]
    InvalidListLimit {
        /// Rejected value.
        limit: u32,
    },
    /// A query array is empty.
    #[error("vector-store search query array must not be empty")]
    EmptySearchQueries,
    /// Search result count is outside `1..=50`.
    #[error("vector-store max_num_results must be 1..=50, got {actual}")]
    InvalidMaxResults {
        /// Rejected value.
        actual: u8,
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
///
/// Deserialization is lossless so oversized maps returned by the service still
/// decode. The documented 16/64/512 limits are enforced by [`insert`] and by
/// the opt-in `validate` methods on create/update requests.
///
/// [`insert`]: VectorStoreMetadata::insert
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

    /// Checks the documented map limits without mutating this value.
    ///
    /// Decoding accepts oversized metadata so responses stay readable; senders
    /// that want the documented limits enforced before a request call this
    /// method (or one of the request-level `validate` hooks).
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        if self.0.len() > MAX_VECTOR_STORE_PROPERTIES {
            return Err(VectorStoreValidationError::TooManyProperties {
                actual: self.0.len(),
                maximum: MAX_VECTOR_STORE_PROPERTIES,
            });
        }
        for (key, value) in &self.0 {
            validate_map_key(key)?;
            validate_string_value(value)?;
        }
        Ok(())
    }

    /// Iterates over pairs in stable key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Number of pairs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
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
        BTreeMap::<String, String>::deserialize(deserializer).map(Self)
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
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &VectorStoreAttributeValue)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Number of attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
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
    /// Creates a static strategy, enforcing the pinned `100..=4096` schema
    /// range for `max_chunk_size_tokens`.
    ///
    /// The descriptive `chunk_overlap_tokens <= max_chunk_size_tokens / 2`
    /// rule has no schema constraint; check it through [`validate`] before
    /// sending.
    ///
    /// [`validate`]: StaticChunkingStrategy::validate
    pub fn new(
        max_chunk_size_tokens: u32,
        chunk_overlap_tokens: u32,
    ) -> Result<Self, VectorStoreValidationError> {
        if !(MIN_STATIC_CHUNK_TOKENS..=MAX_STATIC_CHUNK_TOKENS).contains(&max_chunk_size_tokens) {
            return Err(VectorStoreValidationError::InvalidChunkSize {
                tokens: max_chunk_size_tokens,
            });
        }
        Ok(Self {
            max_chunk_size_tokens,
            chunk_overlap_tokens,
        })
    }

    /// Checks the documented rule that overlap must not exceed half the
    /// maximum chunk size.
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        if self.chunk_overlap_tokens > self.max_chunk_size_tokens / 2 {
            return Err(VectorStoreValidationError::InvalidChunkOverlap {
                overlap: self.chunk_overlap_tokens,
                maximum: self.max_chunk_size_tokens,
            });
        }
        Ok(())
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

fn tagged_type<'a>(value: &'a Value, context: &'static str) -> Result<&'a str, String> {
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

    /// Checks the descriptive overlap rule on a static strategy.
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        match self {
            Self::Static(strategy) => strategy.validate(),
            Self::Auto | Self::Unknown(_) => Ok(()),
        }
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
        match tagged_type(&value, "response chunking strategy").map_err(serde::de::Error::custom)? {
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

/// Initial file IDs for `POST /vector_stores`, validated against the 500-item
/// contract maximum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct VectorStoreInitialFileIds(Vec<FileId>);

impl VectorStoreInitialFileIds {
    /// Validates initial file IDs. An empty list remains distinct from omission.
    pub fn new(values: Vec<FileId>) -> Result<Self, VectorStoreValidationError> {
        if values.len() > MAX_VECTOR_STORE_INITIAL_FILES {
            return Err(VectorStoreValidationError::TooManyInitialFiles {
                actual: values.len(),
                maximum: MAX_VECTOR_STORE_INITIAL_FILES,
            });
        }
        Ok(Self(values))
    }

    /// Borrow the file IDs.
    #[must_use]
    pub fn as_slice(&self) -> &[FileId] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for VectorStoreInitialFileIds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<FileId>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Body for `POST /vector_stores`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateVectorStoreRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_ids: Omittable<VectorStoreInitialFileIds>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    description: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_after: Omittable<VectorStoreExpirationAfter>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    chunking_strategy: Omittable<VectorStoreChunkingStrategyRequest>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<VectorStoreMetadata>>,
}

impl CreateVectorStoreRequest {
    /// Creates an empty request that uses service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets initial files.
    pub fn with_file_ids(
        mut self,
        file_ids: Vec<FileId>,
    ) -> Result<Self, VectorStoreValidationError> {
        self.file_ids = Omittable::Value(VectorStoreInitialFileIds::new(file_ids)?);
        Ok(self)
    }

    /// Sets a name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }

    /// Sets a description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Omittable::Value(description.into());
        self
    }

    /// Sets an expiration policy.
    #[must_use]
    pub fn with_expiration(mut self, expiration: VectorStoreExpirationAfter) -> Self {
        self.expires_after = Omittable::Value(expiration);
        self
    }

    /// Sets the initial-file chunking strategy.
    #[must_use]
    pub fn with_chunking_strategy(mut self, strategy: VectorStoreChunkingStrategyRequest) -> Self {
        self.chunking_strategy = Omittable::Value(strategy);
        self
    }

    /// Attaches metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: VectorStoreMetadata) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }

    /// Sends explicit `null` metadata.
    #[must_use]
    pub fn with_metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits metadata after it was configured.
    #[must_use]
    pub fn clear_metadata(mut self) -> Self {
        self.metadata = Omittable::Omitted;
        self
    }

    /// Checks the documented request limits without sending the request.
    ///
    /// Decoding stays lossless (see [`VectorStoreMetadata`]), so oversized
    /// values can still be constructed and echoed; this opt-in hook enforces
    /// the pinned metadata 16/64/512 limits and the descriptive static-chunk
    /// overlap rule before the body is transmitted.
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        if let Omittable::Value(strategy) = &self.chunking_strategy {
            strategy.validate()?;
        }
        if let Omittable::Value(Nullable::Value(metadata)) = &self.metadata {
            metadata.validate()?;
        }
        Ok(())
    }

    /// Exact initial-file presence state.
    #[must_use]
    pub const fn file_ids(&self) -> &Omittable<VectorStoreInitialFileIds> {
        &self.file_ids
    }

    /// Exact name presence state.
    #[must_use]
    pub const fn name(&self) -> &Omittable<String> {
        &self.name
    }

    /// Exact description presence state.
    #[must_use]
    pub const fn description(&self) -> &Omittable<String> {
        &self.description
    }

    /// Exact expiration-policy presence state.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<VectorStoreExpirationAfter> {
        &self.expires_after
    }

    /// Exact chunking-strategy presence state.
    #[must_use]
    pub const fn chunking_strategy(&self) -> &Omittable<VectorStoreChunkingStrategyRequest> {
        &self.chunking_strategy
    }

    /// Exact metadata presence/nullability state.
    #[must_use]
    pub const fn metadata(&self) -> &Omittable<Nullable<VectorStoreMetadata>> {
        &self.metadata
    }
}

/// Body for `POST /vector_stores/{vector_store_id}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateVectorStoreRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_after: Omittable<Nullable<VectorStoreExpirationAfter>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<VectorStoreMetadata>>,
}

impl UpdateVectorStoreRequest {
    /// Creates a no-op patch body.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(Nullable::Value(name.into()));
        self
    }

    /// Clears the name with explicit `null`.
    #[must_use]
    pub fn with_name_null(mut self) -> Self {
        self.name = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits the name patch.
    #[must_use]
    pub fn clear_name(mut self) -> Self {
        self.name = Omittable::Omitted;
        self
    }

    /// Sets the expiration policy.
    #[must_use]
    pub fn with_expiration(mut self, expiration: VectorStoreExpirationAfter) -> Self {
        self.expires_after = Omittable::Value(Nullable::Value(expiration));
        self
    }

    /// Clears expiration with explicit `null`.
    #[must_use]
    pub fn with_expiration_null(mut self) -> Self {
        self.expires_after = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits the expiration patch.
    #[must_use]
    pub fn clear_expiration(mut self) -> Self {
        self.expires_after = Omittable::Omitted;
        self
    }

    /// Replaces metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: VectorStoreMetadata) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }

    /// Clears metadata with explicit `null`.
    #[must_use]
    pub fn with_metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits the metadata patch.
    #[must_use]
    pub fn clear_metadata(mut self) -> Self {
        self.metadata = Omittable::Omitted;
        self
    }

    /// Checks the documented metadata limits without sending the request.
    ///
    /// Mirrors [`CreateVectorStoreRequest::validate`] for the fields this
    /// patch body can carry.
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        if let Omittable::Value(Nullable::Value(metadata)) = &self.metadata {
            metadata.validate()?;
        }
        Ok(())
    }

    /// Exact name patch state.
    #[must_use]
    pub const fn name(&self) -> &Omittable<Nullable<String>> {
        &self.name
    }

    /// Exact expiration patch state.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<Nullable<VectorStoreExpirationAfter>> {
        &self.expires_after
    }

    /// Exact metadata patch state.
    #[must_use]
    pub const fn metadata(&self) -> &Omittable<Nullable<VectorStoreMetadata>> {
        &self.metadata
    }
}

/// File counters embedded in vector-store and file-batch responses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFileCounts {
    in_progress: i64,
    completed: i64,
    failed: i64,
    cancelled: i64,
    total: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFileCounts {
    /// Files still being processed.
    #[must_use]
    pub const fn in_progress(&self) -> i64 {
        self.in_progress
    }

    /// Successfully processed files.
    #[must_use]
    pub const fn completed(&self) -> i64 {
        self.completed
    }

    /// Failed files.
    #[must_use]
    pub const fn failed(&self) -> i64 {
        self.failed
    }

    /// Cancelled files.
    #[must_use]
    pub const fn cancelled(&self) -> i64 {
        self.cancelled
    }

    /// Total files.
    #[must_use]
    pub const fn total(&self) -> i64 {
        self.total
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A vector-store response object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStore {
    id: VectorStoreId,
    object: VectorStoreObjectType,
    created_at: i64,
    name: String,
    usage_bytes: i64,
    file_counts: VectorStoreFileCounts,
    status: VectorStoreStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_after: Omittable<VectorStoreExpirationAfter>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_at: Omittable<Nullable<i64>>,
    last_active_at: Nullable<i64>,
    metadata: Nullable<VectorStoreMetadata>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStore {
    /// Store identifier.
    #[must_use]
    pub const fn id(&self) -> &VectorStoreId {
        &self.id
    }

    /// Object discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreObjectType {
        &self.object
    }

    /// Store name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Indexed usage in bytes.
    #[must_use]
    pub const fn usage_bytes(&self) -> i64 {
        self.usage_bytes
    }

    /// File counters.
    #[must_use]
    pub const fn file_counts(&self) -> &VectorStoreFileCounts {
        &self.file_counts
    }

    /// Store lifecycle status.
    #[must_use]
    pub const fn status(&self) -> &VectorStoreStatus {
        &self.status
    }

    /// Optional expiration policy.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<VectorStoreExpirationAfter> {
        &self.expires_after
    }

    /// Exact optional-nullable expiration timestamp.
    #[must_use]
    pub const fn expires_at(&self) -> &Omittable<Nullable<i64>> {
        &self.expires_at
    }

    /// Exact required-nullable last-active state.
    #[must_use]
    pub const fn last_active_at(&self) -> &Nullable<i64> {
        &self.last_active_at
    }

    /// Exact required-nullable metadata state.
    #[must_use]
    pub const fn metadata(&self) -> &Nullable<VectorStoreMetadata> {
        &self.metadata
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation returned after deleting a vector store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedVectorStore {
    id: VectorStoreId,
    deleted: bool,
    object: DeletedVectorStoreObjectType,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedVectorStore {
    /// Deleted store identifier.
    #[must_use]
    pub const fn id(&self) -> &VectorStoreId {
        &self.id
    }

    /// Whether deletion completed.
    #[must_use]
    pub const fn deleted(&self) -> bool {
        self.deleted
    }

    /// Deletion object discriminator.
    #[must_use]
    pub const fn object(&self) -> &DeletedVectorStoreObjectType {
        &self.object
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Validated list page size used across Vector Store resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct VectorStoreListLimit(u32);

impl VectorStoreListLimit {
    /// Validates a page size of at least 1.
    ///
    /// The pinned schemas document "between 1 and 100" in prose but carry no
    /// `maximum`, so no upper bound is enforced.
    pub const fn new(value: u32) -> Result<Self, VectorStoreValidationError> {
        if value == 0 {
            Err(VectorStoreValidationError::InvalidListLimit { limit: value })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for VectorStoreListLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Query parameters for `GET /vector_stores`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<VectorStoreListLimit>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<VectorStoreSortOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<VectorStoreId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    before: Omittable<VectorStoreId>,
}

impl VectorStoreListParams {
    /// Creates a request using server defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets page size.
    #[must_use]
    pub fn with_limit(mut self, limit: VectorStoreListLimit) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Sets sort order.
    #[must_use]
    pub fn with_order(mut self, order: VectorStoreSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Sets the forward cursor.
    #[must_use]
    pub fn after(mut self, cursor: impl Into<VectorStoreId>) -> Self {
        self.after = Omittable::Value(cursor.into());
        self
    }

    /// Sets the backward cursor.
    #[must_use]
    pub fn before(mut self, cursor: impl Into<VectorStoreId>) -> Self {
        self.before = Omittable::Value(cursor.into());
        self
    }

    /// Effective server page size.
    #[must_use]
    pub const fn effective_limit(&self) -> u32 {
        match self.limit {
            Omittable::Omitted => DEFAULT_VECTOR_STORE_LIST_LIMIT,
            Omittable::Value(value) => value.get(),
        }
    }

    /// Exact page-limit state.
    #[must_use]
    pub const fn limit(&self) -> &Omittable<VectorStoreListLimit> {
        &self.limit
    }

    /// Exact order state.
    #[must_use]
    pub const fn order(&self) -> &Omittable<VectorStoreSortOrder> {
        &self.order
    }

    /// Exact forward cursor state.
    #[must_use]
    pub const fn after_cursor(&self) -> &Omittable<VectorStoreId> {
        &self.after
    }

    /// Exact backward cursor state.
    #[must_use]
    pub const fn before_cursor(&self) -> &Omittable<VectorStoreId> {
        &self.before
    }
}

/// Response from `GET /vector_stores`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListVectorStoresResponse {
    object: VectorStoreListObjectType,
    data: Vec<VectorStore>,
    first_id: VectorStoreId,
    last_id: VectorStoreId,
    has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListVectorStoresResponse {
    /// Collection discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreListObjectType {
        &self.object
    }

    /// Stores in this page.
    #[must_use]
    pub fn data(&self) -> &[VectorStore] {
        &self.data
    }

    /// First cursor.
    #[must_use]
    pub const fn first_id(&self) -> &VectorStoreId {
        &self.first_id
    }

    /// Last cursor.
    #[must_use]
    pub const fn last_id(&self) -> &VectorStoreId {
        &self.last_id
    }

    /// Whether another page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Last processing error for an attached vector-store file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFileLastError {
    code: VectorStoreFileErrorCode,
    message: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFileLastError {
    /// Error code.
    #[must_use]
    pub const fn code(&self) -> &VectorStoreFileErrorCode {
        &self.code
    }

    /// Human-readable description.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Body for attaching one existing file to a vector store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateVectorStoreFileRequest {
    file_id: FileId,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    chunking_strategy: Omittable<VectorStoreChunkingStrategyRequest>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    attributes: Omittable<Nullable<VectorStoreFileAttributes>>,
}

impl CreateVectorStoreFileRequest {
    /// Creates a request using automatic chunking and no attributes.
    #[must_use]
    pub fn new(file_id: impl Into<FileId>) -> Self {
        Self {
            file_id: file_id.into(),
            chunking_strategy: Omittable::Omitted,
            attributes: Omittable::Omitted,
        }
    }

    /// Sets a chunking strategy.
    #[must_use]
    pub fn with_chunking_strategy(mut self, strategy: VectorStoreChunkingStrategyRequest) -> Self {
        self.chunking_strategy = Omittable::Value(strategy);
        self
    }

    /// Sets file attributes.
    #[must_use]
    pub fn with_attributes(mut self, attributes: VectorStoreFileAttributes) -> Self {
        self.attributes = Omittable::Value(Nullable::Value(attributes));
        self
    }

    /// Sends explicit `null` attributes.
    #[must_use]
    pub fn with_attributes_null(mut self) -> Self {
        self.attributes = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits attributes after they were configured.
    #[must_use]
    pub fn clear_attributes(mut self) -> Self {
        self.attributes = Omittable::Omitted;
        self
    }

    /// File being attached.
    #[must_use]
    pub const fn file_id(&self) -> &FileId {
        &self.file_id
    }

    /// Exact optional chunking-strategy state.
    #[must_use]
    pub const fn chunking_strategy(&self) -> &Omittable<VectorStoreChunkingStrategyRequest> {
        &self.chunking_strategy
    }

    /// Exact optional-nullable attributes state.
    #[must_use]
    pub const fn attributes(&self) -> &Omittable<Nullable<VectorStoreFileAttributes>> {
        &self.attributes
    }
}

/// Body for replacing attached-file attributes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateVectorStoreFileAttributesRequest {
    attributes: Nullable<VectorStoreFileAttributes>,
}

impl UpdateVectorStoreFileAttributesRequest {
    /// Replaces attributes with a validated map.
    #[must_use]
    pub const fn new(attributes: VectorStoreFileAttributes) -> Self {
        Self {
            attributes: Nullable::Value(attributes),
        }
    }

    /// Clears attributes with explicit `null`.
    #[must_use]
    pub const fn clear() -> Self {
        Self {
            attributes: Nullable::Null,
        }
    }

    /// Exact required-nullable state.
    #[must_use]
    pub const fn attributes(&self) -> &Nullable<VectorStoreFileAttributes> {
        &self.attributes
    }
}

/// An existing file attached to a vector store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFile {
    id: FileId,
    object: VectorStoreFileObjectType,
    usage_bytes: i64,
    created_at: i64,
    vector_store_id: VectorStoreId,
    status: VectorStoreFileStatus,
    last_error: Nullable<VectorStoreFileLastError>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    chunking_strategy: Omittable<VectorStoreChunkingStrategy>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    attributes: Omittable<Nullable<VectorStoreFileAttributes>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFile {
    /// File identifier.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Object discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreFileObjectType {
        &self.object
    }

    /// Attachment creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Parent store identifier.
    #[must_use]
    pub const fn vector_store_id(&self) -> &VectorStoreId {
        &self.vector_store_id
    }

    /// Processing status.
    #[must_use]
    pub const fn status(&self) -> &VectorStoreFileStatus {
        &self.status
    }

    /// Indexed usage in bytes.
    #[must_use]
    pub const fn usage_bytes(&self) -> i64 {
        self.usage_bytes
    }

    /// Required-nullable last processing error.
    #[must_use]
    pub const fn last_error(&self) -> &Nullable<VectorStoreFileLastError> {
        &self.last_error
    }

    /// Optional chunking strategy.
    #[must_use]
    pub fn chunking_strategy(&self) -> Option<&VectorStoreChunkingStrategy> {
        match &self.chunking_strategy {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Exact optional-nullable attributes state.
    #[must_use]
    pub const fn attributes(&self) -> &Omittable<Nullable<VectorStoreFileAttributes>> {
        &self.attributes
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation returned after detaching a file from a vector store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedVectorStoreFile {
    id: FileId,
    deleted: bool,
    object: DeletedVectorStoreFileObjectType,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedVectorStoreFile {
    /// Detached file identifier.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Whether detachment completed.
    #[must_use]
    pub const fn deleted(&self) -> bool {
        self.deleted
    }

    /// Deletion object discriminator.
    #[must_use]
    pub const fn object(&self) -> &DeletedVectorStoreFileObjectType {
        &self.object
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query parameters shared by attached-file list endpoints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreFileListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<VectorStoreListLimit>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<VectorStoreSortOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<FileId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    before: Omittable<FileId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filter: Omittable<VectorStoreFileStatus>,
}

impl VectorStoreFileListParams {
    /// Creates an unfiltered request using server defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets page size.
    #[must_use]
    pub fn with_limit(mut self, limit: VectorStoreListLimit) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Sets sort order.
    #[must_use]
    pub fn with_order(mut self, order: VectorStoreSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Continues after a file cursor.
    #[must_use]
    pub fn after(mut self, cursor: impl Into<FileId>) -> Self {
        self.after = Omittable::Value(cursor.into());
        self
    }

    /// Continues before a file cursor.
    #[must_use]
    pub fn before(mut self, cursor: impl Into<FileId>) -> Self {
        self.before = Omittable::Value(cursor.into());
        self
    }

    /// Filters by file processing status.
    #[must_use]
    pub fn with_status(mut self, status: VectorStoreFileStatus) -> Self {
        self.filter = Omittable::Value(status);
        self
    }

    /// Effective server page size.
    #[must_use]
    pub const fn effective_limit(&self) -> u32 {
        match self.limit {
            Omittable::Omitted => DEFAULT_VECTOR_STORE_LIST_LIMIT,
            Omittable::Value(value) => value.get(),
        }
    }

    /// Exact page-limit state.
    #[must_use]
    pub const fn limit(&self) -> &Omittable<VectorStoreListLimit> {
        &self.limit
    }

    /// Exact order state.
    #[must_use]
    pub const fn order(&self) -> &Omittable<VectorStoreSortOrder> {
        &self.order
    }

    /// Exact forward cursor state.
    #[must_use]
    pub const fn after_cursor(&self) -> &Omittable<FileId> {
        &self.after
    }

    /// Exact backward cursor state.
    #[must_use]
    pub const fn before_cursor(&self) -> &Omittable<FileId> {
        &self.before
    }

    /// Exact status-filter state.
    #[must_use]
    pub const fn status_filter(&self) -> &Omittable<VectorStoreFileStatus> {
        &self.filter
    }
}

/// Response from an attached-file list endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListVectorStoreFilesResponse {
    object: VectorStoreListObjectType,
    data: Vec<VectorStoreFile>,
    first_id: FileId,
    last_id: FileId,
    has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListVectorStoreFilesResponse {
    /// Collection discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreListObjectType {
        &self.object
    }

    /// Attached files in this page.
    #[must_use]
    pub fn data(&self) -> &[VectorStoreFile] {
        &self.data
    }

    /// First cursor.
    #[must_use]
    pub const fn first_id(&self) -> &FileId {
        &self.first_id
    }

    /// Last cursor.
    #[must_use]
    pub const fn last_id(&self) -> &FileId {
        &self.last_id
    }

    /// Whether another page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// File identifiers for a vector-store file batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct VectorStoreBatchFileIds(Vec<FileId>);

impl VectorStoreBatchFileIds {
    /// Validates `1..=2000` file IDs.
    pub fn new(values: Vec<FileId>) -> Result<Self, VectorStoreValidationError> {
        if values.is_empty() || values.len() > MAX_VECTOR_STORE_BATCH_FILES {
            return Err(VectorStoreValidationError::InvalidBatchFileCount {
                actual: values.len(),
                maximum: MAX_VECTOR_STORE_BATCH_FILES,
            });
        }
        Ok(Self(values))
    }

    /// Borrow the identifiers.
    #[must_use]
    pub fn as_slice(&self) -> &[FileId] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for VectorStoreBatchFileIds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<FileId>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Per-file request objects for a vector-store file batch.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VectorStoreBatchFiles(Vec<CreateVectorStoreFileRequest>);

impl VectorStoreBatchFiles {
    /// Validates `1..=2000` per-file request objects.
    pub fn new(
        values: Vec<CreateVectorStoreFileRequest>,
    ) -> Result<Self, VectorStoreValidationError> {
        if values.is_empty() || values.len() > MAX_VECTOR_STORE_BATCH_FILES {
            return Err(VectorStoreValidationError::InvalidBatchFileCount {
                actual: values.len(),
                maximum: MAX_VECTOR_STORE_BATCH_FILES,
            });
        }
        Ok(Self(values))
    }

    /// Borrow the requests.
    #[must_use]
    pub fn as_slice(&self) -> &[CreateVectorStoreFileRequest] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for VectorStoreBatchFiles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<CreateVectorStoreFileRequest>::deserialize(
            deserializer,
        )?)
        .map_err(serde::de::Error::custom)
    }
}

/// Body for creating a vector-store file batch.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CreateVectorStoreFileBatchRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file_ids: Omittable<VectorStoreBatchFileIds>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    files: Omittable<VectorStoreBatchFiles>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    chunking_strategy: Omittable<VectorStoreChunkingStrategyRequest>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    attributes: Omittable<Nullable<VectorStoreFileAttributes>>,
}

impl CreateVectorStoreFileBatchRequest {
    /// Creates a batch from bare file IDs. Global chunking and attributes may
    /// subsequently be attached.
    pub fn from_file_ids(values: Vec<FileId>) -> Result<Self, VectorStoreValidationError> {
        Ok(Self {
            file_ids: Omittable::Value(VectorStoreBatchFileIds::new(values)?),
            files: Omittable::Omitted,
            chunking_strategy: Omittable::Omitted,
            attributes: Omittable::Omitted,
        })
    }

    /// Creates a batch from per-file request objects.
    pub fn from_files(
        values: Vec<CreateVectorStoreFileRequest>,
    ) -> Result<Self, VectorStoreValidationError> {
        Ok(Self {
            file_ids: Omittable::Omitted,
            files: Omittable::Value(VectorStoreBatchFiles::new(values)?),
            chunking_strategy: Omittable::Omitted,
            attributes: Omittable::Omitted,
        })
    }

    /// Applies one global chunking strategy. The API ignores this field for the
    /// per-file `files` form, so this method rejects that ambiguous
    /// combination.
    pub fn with_chunking_strategy(
        mut self,
        strategy: VectorStoreChunkingStrategyRequest,
    ) -> Result<Self, VectorStoreValidationError> {
        if self.files.is_value() {
            return Err(VectorStoreValidationError::GlobalFieldsWithPerFileBatch);
        }
        self.chunking_strategy = Omittable::Value(strategy);
        Ok(self)
    }

    /// Applies global attributes to the bare-ID form.
    pub fn with_attributes(
        mut self,
        attributes: VectorStoreFileAttributes,
    ) -> Result<Self, VectorStoreValidationError> {
        if self.files.is_value() {
            return Err(VectorStoreValidationError::GlobalFieldsWithPerFileBatch);
        }
        self.attributes = Omittable::Value(Nullable::Value(attributes));
        Ok(self)
    }

    /// Applies explicit `null` global attributes to the bare-ID form.
    pub fn with_attributes_null(mut self) -> Result<Self, VectorStoreValidationError> {
        if self.files.is_value() {
            return Err(VectorStoreValidationError::GlobalFieldsWithPerFileBatch);
        }
        self.attributes = Omittable::Value(Nullable::Null);
        Ok(self)
    }

    /// Omits global chunking after it was configured.
    #[must_use]
    pub fn clear_chunking_strategy(mut self) -> Self {
        self.chunking_strategy = Omittable::Omitted;
        self
    }

    /// Omits global attributes after they were configured.
    #[must_use]
    pub fn clear_attributes(mut self) -> Self {
        self.attributes = Omittable::Omitted;
        self
    }

    /// Exact bare-ID input state.
    #[must_use]
    pub const fn file_ids(&self) -> &Omittable<VectorStoreBatchFileIds> {
        &self.file_ids
    }

    /// Exact per-file input state.
    #[must_use]
    pub const fn files(&self) -> &Omittable<VectorStoreBatchFiles> {
        &self.files
    }

    /// Exact global chunking state.
    #[must_use]
    pub const fn chunking_strategy(&self) -> &Omittable<VectorStoreChunkingStrategyRequest> {
        &self.chunking_strategy
    }

    /// Exact optional-nullable global attributes state.
    #[must_use]
    pub const fn attributes(&self) -> &Omittable<Nullable<VectorStoreFileAttributes>> {
        &self.attributes
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateVectorStoreFileBatchRequestWire {
    #[serde(default)]
    file_ids: Omittable<VectorStoreBatchFileIds>,
    #[serde(default)]
    files: Omittable<VectorStoreBatchFiles>,
    #[serde(default)]
    chunking_strategy: Omittable<VectorStoreChunkingStrategyRequest>,
    #[serde(default)]
    attributes: Omittable<Nullable<VectorStoreFileAttributes>>,
}

impl<'de> Deserialize<'de> for CreateVectorStoreFileBatchRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CreateVectorStoreFileBatchRequestWire::deserialize(deserializer)?;
        // The pinned anyOf requires exactly one of file_ids/files to be
        // present. A per-file `files` body combined with the global
        // `attributes`/`chunking_strategy` fields satisfies the schema (the
        // `files` branch alone validates), so decoding keeps it; only the
        // builders reject that ambiguous combination.
        if wire.file_ids.is_value() == wire.files.is_value() {
            return Err(serde::de::Error::custom(
                VectorStoreValidationError::InvalidBatchInputChoice,
            ));
        }
        Ok(Self {
            file_ids: wire.file_ids,
            files: wire.files,
            chunking_strategy: wire.chunking_strategy,
            attributes: wire.attributes,
        })
    }
}

/// A vector-store file-batch response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFileBatch {
    id: VectorStoreFileBatchId,
    object: VectorStoreFileBatchObjectType,
    created_at: i64,
    vector_store_id: VectorStoreId,
    status: VectorStoreFileStatus,
    file_counts: VectorStoreFileCounts,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFileBatch {
    /// File-batch identifier.
    #[must_use]
    pub const fn id(&self) -> &VectorStoreFileBatchId {
        &self.id
    }

    /// Object discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreFileBatchObjectType {
        &self.object
    }

    /// Creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Parent vector store.
    #[must_use]
    pub const fn vector_store_id(&self) -> &VectorStoreId {
        &self.vector_store_id
    }

    /// Processing status.
    #[must_use]
    pub const fn status(&self) -> &VectorStoreFileStatus {
        &self.status
    }

    /// Per-status counters.
    #[must_use]
    pub const fn file_counts(&self) -> &VectorStoreFileCounts {
        &self.file_counts
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One or more natural-language queries supplied to vector search.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum VectorStoreSearchQuery {
    /// One query string.
    Text(String),
    /// Multiple query strings.
    Texts(Vec<String>),
}

/// Query representation returned by vector-store search.
///
/// The pinned component schema declares an array while the same pinned
/// endpoint's official example returns one string. Both shapes are retained
/// without normalization until that upstream discrepancy is resolved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum VectorStoreSearchQueryOutput {
    /// One query string.
    Text(String),
    /// One or more query strings.
    Texts(Vec<String>),
}

impl VectorStoreSearchQuery {
    /// Creates a non-empty list query.
    pub fn multiple(values: Vec<String>) -> Result<Self, VectorStoreValidationError> {
        if values.is_empty() {
            return Err(VectorStoreValidationError::EmptySearchQueries);
        }
        Ok(Self::Texts(values))
    }
}

impl From<String> for VectorStoreSearchQuery {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for VectorStoreSearchQuery {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl<'de> Deserialize<'de> for VectorStoreSearchQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Text(String),
            Texts(Vec<String>),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Text(value) => Ok(Self::Text(value)),
            Wire::Texts(values) => Self::multiple(values).map_err(serde::de::Error::custom),
        }
    }
}

/// Validated maximum number of vector search results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct VectorStoreMaxResults(u8);

impl VectorStoreMaxResults {
    /// Creates a result count in `1..=50`.
    pub const fn new(value: u8) -> Result<Self, VectorStoreValidationError> {
        if value == 0 || value > MAX_VECTOR_STORE_SEARCH_RESULTS {
            Err(VectorStoreValidationError::InvalidMaxResults { actual: value })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the result count.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for VectorStoreMaxResults {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Finite similarity or ranking threshold in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct VectorStoreScore(f64);

impl VectorStoreScore {
    /// Validates a score.
    pub fn new(value: f64) -> Result<Self, VectorStoreValidationError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(VectorStoreValidationError::InvalidScore {
                score: value.to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the score.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Serialize for VectorStoreScore {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for VectorStoreScore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Optional ranking controls for vector-store search.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreRankingOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ranker: Omittable<VectorStoreRanker>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    score_threshold: Omittable<VectorStoreScore>,
}

impl VectorStoreRankingOptions {
    /// Creates options using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a ranker.
    #[must_use]
    pub fn with_ranker(mut self, ranker: VectorStoreRanker) -> Self {
        self.ranker = Omittable::Value(ranker);
        self
    }

    /// Selects a validated score threshold.
    #[must_use]
    pub fn with_score_threshold(mut self, threshold: VectorStoreScore) -> Self {
        self.score_threshold = Omittable::Value(threshold);
        self
    }

    /// Exact ranker state.
    #[must_use]
    pub const fn ranker(&self) -> &Omittable<VectorStoreRanker> {
        &self.ranker
    }

    /// Exact score-threshold state.
    #[must_use]
    pub const fn score_threshold(&self) -> &Omittable<VectorStoreScore> {
        &self.score_threshold
    }
}

/// Body for `POST /vector_stores/{vector_store_id}/search`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStoreSearchRequest {
    query: VectorStoreSearchQuery,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    rewrite_query: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_num_results: Omittable<VectorStoreMaxResults>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    filters: Omittable<VectorStoreFilter>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    ranking_options: Omittable<VectorStoreRankingOptions>,
}

impl VectorStoreSearchRequest {
    /// Creates a minimal search request.
    #[must_use]
    pub fn new(query: impl Into<VectorStoreSearchQuery>) -> Self {
        Self {
            query: query.into(),
            rewrite_query: Omittable::Omitted,
            max_num_results: Omittable::Omitted,
            filters: Omittable::Omitted,
            ranking_options: Omittable::Omitted,
        }
    }

    /// Enables or disables query rewriting explicitly.
    #[must_use]
    pub fn with_rewrite_query(mut self, rewrite: bool) -> Self {
        self.rewrite_query = Omittable::Value(rewrite);
        self
    }

    /// Sets a result limit.
    #[must_use]
    pub fn with_max_results(mut self, maximum: VectorStoreMaxResults) -> Self {
        self.max_num_results = Omittable::Value(maximum);
        self
    }

    /// Applies an attribute filter.
    #[must_use]
    pub fn with_filter(mut self, filter: VectorStoreFilter) -> Self {
        self.filters = Omittable::Value(filter);
        self
    }

    /// Applies ranking options.
    #[must_use]
    pub fn with_ranking_options(mut self, options: VectorStoreRankingOptions) -> Self {
        self.ranking_options = Omittable::Value(options);
        self
    }

    /// Checks the pinned search constraints without sending the request.
    ///
    /// [`VectorStoreSearchQuery`] is an open enum, so a `Texts(vec![])` value
    /// can be assembled directly and bypass the `minItems: 1` rule that
    /// [`VectorStoreSearchQuery::multiple`] enforces at construction and
    /// decode. This opt-in hook re-reports that state as
    /// [`VectorStoreValidationError::EmptySearchQueries`] before the body is
    /// transmitted, mirroring the other request-level `validate` hooks.
    pub fn validate(&self) -> Result<(), VectorStoreValidationError> {
        if let VectorStoreSearchQuery::Texts(queries) = &self.query
            && queries.is_empty()
        {
            return Err(VectorStoreValidationError::EmptySearchQueries);
        }
        Ok(())
    }

    /// Effective result limit.
    #[must_use]
    pub const fn effective_max_results(&self) -> u8 {
        match self.max_num_results {
            Omittable::Omitted => DEFAULT_VECTOR_STORE_SEARCH_RESULTS,
            Omittable::Value(value) => value.get(),
        }
    }

    /// Search query.
    #[must_use]
    pub const fn query(&self) -> &VectorStoreSearchQuery {
        &self.query
    }

    /// Exact query-rewrite state.
    #[must_use]
    pub const fn rewrite_query(&self) -> &Omittable<bool> {
        &self.rewrite_query
    }

    /// Exact maximum-result state.
    #[must_use]
    pub const fn max_num_results(&self) -> &Omittable<VectorStoreMaxResults> {
        &self.max_num_results
    }

    /// Exact filter state.
    #[must_use]
    pub const fn filter(&self) -> &Omittable<VectorStoreFilter> {
        &self.filters
    }

    /// Exact ranking-options state.
    #[must_use]
    pub const fn ranking_options(&self) -> &Omittable<VectorStoreRankingOptions> {
        &self.ranking_options
    }
}

/// One text chunk returned by vector-store search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreSearchContent {
    #[serde(rename = "type")]
    kind: VectorStoreContentType,
    text: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreSearchContent {
    /// Content kind.
    #[must_use]
    pub const fn kind(&self) -> &VectorStoreContentType {
        &self.kind
    }

    /// Returned text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One scored vector-store search result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreSearchResult {
    file_id: FileId,
    filename: String,
    score: VectorStoreScore,
    attributes: Nullable<VectorStoreFileAttributes>,
    content: Vec<VectorStoreSearchContent>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreSearchResult {
    /// Source file identifier.
    #[must_use]
    pub const fn file_id(&self) -> &FileId {
        &self.file_id
    }

    /// Source filename.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Similarity score.
    #[must_use]
    pub const fn score(&self) -> VectorStoreScore {
        self.score
    }

    /// Required-nullable attributes.
    #[must_use]
    pub const fn attributes(&self) -> &Nullable<VectorStoreFileAttributes> {
        &self.attributes
    }

    /// Content chunks.
    #[must_use]
    pub fn content(&self) -> &[VectorStoreSearchContent] {
        &self.content
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Page returned by vector-store search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreSearchResultsPage {
    object: VectorStoreSearchPageObjectType,
    search_query: VectorStoreSearchQueryOutput,
    data: Vec<VectorStoreSearchResult>,
    has_more: bool,
    next_page: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreSearchResultsPage {
    /// Page object discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreSearchPageObjectType {
        &self.object
    }

    /// Query or rewritten queries actually searched.
    #[must_use]
    pub const fn search_query(&self) -> &VectorStoreSearchQueryOutput {
        &self.search_query
    }

    /// Search results.
    #[must_use]
    pub fn data(&self) -> &[VectorStoreSearchResult] {
        &self.data
    }

    /// Whether another page is advertised.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Required-nullable next-page token.
    #[must_use]
    pub const fn next_page(&self) -> &Nullable<String> {
        &self.next_page
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One parsed content item from an attached file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFileContent {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<VectorStoreContentType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFileContent {
    /// Optional content kind.
    #[must_use]
    pub fn kind(&self) -> Option<&VectorStoreContentType> {
        match &self.kind {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Optional text content.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match &self.text {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Parsed-content page for one attached file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStoreFileContentResponse {
    object: VectorStoreFileContentPageObjectType,
    data: Vec<VectorStoreFileContent>,
    has_more: bool,
    next_page: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VectorStoreFileContentResponse {
    /// Page object discriminator.
    #[must_use]
    pub const fn object(&self) -> &VectorStoreFileContentPageObjectType {
        &self.object
    }

    /// Parsed content items.
    #[must_use]
    pub fn data(&self) -> &[VectorStoreFileContent] {
        &self.data
    }

    /// Whether another page is advertised.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Required-nullable next-page token.
    #[must_use]
    pub const fn next_page(&self) -> &Nullable<String> {
        &self.next_page
    }

    /// Unknown response properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(CreateVectorStoreRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(UpdateVectorStoreRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(VectorStore: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CreateVectorStoreFileRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CreateVectorStoreFileBatchRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(VectorStoreSearchRequest: Serialize, DeserializeOwned, Send, Sync);

    fn minimal_store() -> Value {
        json!({
            "id": "vs_abc",
            "object": "vector_store",
            "created_at": 100,
            "name": "docs",
            "usage_bytes": 42,
            "file_counts": {
                "in_progress": 0,
                "completed": 1,
                "failed": 0,
                "cancelled": 0,
                "total": 1
            },
            "status": "completed",
            "last_active_at": null,
            "metadata": null
        })
    }

    #[test]
    fn store_required_nullable_fields_remain_distinct() {
        let mut value = minimal_store();
        value["future_field"] = json!({"retained": true});
        let store: VectorStore = serde_json::from_value(value.clone()).expect("decode store");
        assert!(store.last_active_at().is_null());
        assert!(store.metadata().is_null());
        assert_eq!(serde_json::to_value(store).expect("encode store"), value);

        let mut missing = minimal_store();
        missing
            .as_object_mut()
            .expect("object")
            .remove("last_active_at");
        assert!(serde_json::from_value::<VectorStore>(missing).is_err());
    }

    #[test]
    fn update_request_encodes_missing_null_and_value() {
        let omitted = serde_json::to_value(UpdateVectorStoreRequest::new()).expect("omitted");
        assert_eq!(omitted, json!({}));

        let null = serde_json::to_value(
            UpdateVectorStoreRequest::new()
                .with_name_null()
                .with_expiration_null()
                .with_metadata_null(),
        )
        .expect("null fields");
        assert_eq!(
            null,
            json!({"name": null, "expires_after": null, "metadata": null})
        );

        let value = serde_json::to_value(
            UpdateVectorStoreRequest::new()
                .with_name("renamed")
                .with_expiration(VectorStoreExpirationAfter::new(7).expect("expiration")),
        )
        .expect("values");
        assert_eq!(value["name"], "renamed");
        assert_eq!(value["expires_after"]["anchor"], "last_active_at");
    }

    #[test]
    fn vector_store_list_limit_requires_at_least_one() {
        // The pinned list schemas document "between 1 and 100" in prose but
        // carry no `maximum`, and the official Python SDK passes the value
        // through unbounded, so only the schema-backed lower bound is enforced
        // (D0154).
        assert!(VectorStoreListLimit::new(0).is_err());
        assert!(serde_json::from_str::<VectorStoreListLimit>("0").is_err());
        assert_eq!(
            VectorStoreListLimit::new(1)
                .expect("minimum is valid")
                .get(),
            1_u32
        );
        assert_eq!(
            VectorStoreListLimit::new(u32::MAX)
                .expect("no invented upper bound")
                .get(),
            u32::MAX
        );
        assert_eq!(
            serde_json::from_str::<VectorStoreListLimit>("101")
                .expect("value above the documented prose ceiling stays valid")
                .get(),
            101_u32
        );

        // The default stays the pinned 20 and both params surfaces share the
        // validated type.
        assert_eq!(
            VectorStoreListParams::new().effective_limit(),
            DEFAULT_VECTOR_STORE_LIST_LIMIT
        );
        let above_ceiling = VectorStoreListLimit::new(500).expect("prose-only ceiling");
        let encoded = serde_json::to_value(VectorStoreListParams::new().with_limit(above_ceiling))
            .expect("encode list params");
        assert_eq!(encoded, json!({"limit": 500}));
        let decoded: VectorStoreListParams =
            serde_json::from_value(encoded).expect("decode list params");
        assert_eq!(decoded.effective_limit(), 500);
        let file_params =
            serde_json::to_value(VectorStoreFileListParams::new().with_limit(above_ceiling))
                .expect("encode file list params");
        assert_eq!(file_params, json!({"limit": 500}));
    }

    #[test]
    fn attributes_enforce_shape_and_limits_on_decode() {
        let mut attributes = VectorStoreFileAttributes::new();
        attributes
            .insert(
                "tenant",
                VectorStoreAttributeValue::string("blue").expect("value"),
            )
            .expect("attribute");
        attributes
            .insert("active", VectorStoreAttributeValue::Boolean(true))
            .expect("attribute");
        assert_eq!(
            serde_json::to_value(&attributes).expect("encode"),
            json!({"active": true, "tenant": "blue"})
        );

        assert!(serde_json::from_value::<VectorStoreFileAttributes>(json!({"bad": null})).is_err());
        assert!(serde_json::from_value::<VectorStoreFileAttributes>(json!({"bad": {}})).is_err());

        let too_many = (0..=MAX_VECTOR_STORE_PROPERTIES)
            .map(|index| (format!("k{index}"), json!(index)))
            .collect::<serde_json::Map<_, _>>();
        assert!(
            serde_json::from_value::<VectorStoreFileAttributes>(Value::Object(too_many)).is_err()
        );
    }

    #[test]
    fn chunking_schema_range_is_decode_enforced_and_overlap_is_opt_in() {
        assert!(StaticChunkingStrategy::new(99, 0).is_err());
        assert!(StaticChunkingStrategy::new(4097, 0).is_err());
        // The overlap ceiling is descriptive (no schema constraint), so the
        // constructor and decode accept it and only validate() rejects.
        let oversized = StaticChunkingStrategy::new(800, 401).expect("constructor accepts overlap");
        assert!(matches!(
            oversized.validate(),
            Err(VectorStoreValidationError::InvalidChunkOverlap {
                overlap: 401,
                maximum: 800
            })
        ));

        let boundary = StaticChunkingStrategy::new(800, 400).expect("boundary accepted");
        boundary.validate().expect("half-overlap is valid");
        let request = VectorStoreChunkingStrategyRequest::static_strategy(boundary);
        request.validate().expect("request strategy validates");
        assert_eq!(
            serde_json::to_value(request).expect("encode"),
            json!({"type": "static", "static": {"max_chunk_size_tokens": 800, "chunk_overlap_tokens": 400}})
        );
        assert!(
            serde_json::from_value::<VectorStoreChunkingStrategyRequest>(json!({
                "type": "static",
                "static": {"max_chunk_size_tokens": 800, "chunk_overlap_tokens": 400}
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<VectorStoreChunkingStrategyRequest>(json!({
                "type": "static",
                "static": {"max_chunk_size_tokens": 99, "chunk_overlap_tokens": 0}
            }))
            .is_err()
        );
    }

    #[test]
    fn oversized_metadata_decodes_and_request_validate_rejects() {
        let oversized = (0..=MAX_VECTOR_STORE_PROPERTIES)
            .map(|index| (format!("k{index}"), json!("v")))
            .collect::<serde_json::Map<_, _>>();
        let decoded: VectorStoreMetadata =
            serde_json::from_value(Value::Object(oversized.clone())).expect("lossless decode");
        assert_eq!(decoded.len(), MAX_VECTOR_STORE_PROPERTIES + 1);
        assert_eq!(
            serde_json::to_value(decoded.clone()).expect("re-encode"),
            Value::Object(oversized.clone())
        );

        let long_key = Value::Object(
            [("k".repeat(MAX_VECTOR_STORE_KEY_CHARS + 1), Value::from("v"))]
                .into_iter()
                .collect(),
        );
        let long_value = json!({"k": "v".repeat(MAX_VECTOR_STORE_VALUE_CHARS + 1)});
        assert!(
            serde_json::from_value::<VectorStoreMetadata>(long_key.clone()).is_ok(),
            "oversized keys stay decodable"
        );
        assert!(
            serde_json::from_value::<VectorStoreMetadata>(long_value.clone()).is_ok(),
            "oversized values stay decodable"
        );

        let mut store_value = minimal_store();
        store_value["metadata"] = Value::Object(oversized.clone());
        let store: VectorStore = serde_json::from_value(store_value.clone()).expect("decode store");
        let Nullable::Value(store_metadata) = store.metadata() else {
            panic!("oversized metadata must decode as a value");
        };
        assert_eq!(store_metadata.len(), MAX_VECTOR_STORE_PROPERTIES + 1);
        assert_eq!(serde_json::to_value(store).expect("re-encode"), store_value);

        let create = CreateVectorStoreRequest::new()
            .with_metadata(decoded.clone())
            .validate();
        assert!(matches!(
            create,
            Err(VectorStoreValidationError::TooManyProperties { actual, .. })
                if actual == MAX_VECTOR_STORE_PROPERTIES + 1
        ));

        let long_key_metadata: VectorStoreMetadata =
            serde_json::from_value(long_key).expect("decode oversized key");
        assert!(matches!(
            CreateVectorStoreRequest::new()
                .with_metadata(long_key_metadata.clone())
                .validate(),
            Err(VectorStoreValidationError::KeyTooLong { .. })
        ));
        assert!(matches!(
            UpdateVectorStoreRequest::new()
                .with_metadata(long_key_metadata)
                .validate(),
            Err(VectorStoreValidationError::KeyTooLong { .. })
        ));

        let long_value_metadata: VectorStoreMetadata =
            serde_json::from_value(long_value).expect("decode oversized value");
        assert!(matches!(
            CreateVectorStoreRequest::new()
                .with_metadata(long_value_metadata)
                .validate(),
            Err(VectorStoreValidationError::ValueTooLong { .. })
        ));

        // Omitted and explicit-null metadata never trip the opt-in check.
        CreateVectorStoreRequest::new()
            .validate()
            .expect("empty body validates");
        UpdateVectorStoreRequest::new()
            .with_metadata_null()
            .validate()
            .expect("explicit null validates");
    }

    #[test]
    fn create_request_validate_checks_chunking_overlap() {
        let strategy = StaticChunkingStrategy::new(800, 401).expect("constructor accepts overlap");
        let request = CreateVectorStoreRequest::new().with_chunking_strategy(
            VectorStoreChunkingStrategyRequest::static_strategy(strategy),
        );
        assert!(matches!(
            request.validate(),
            Err(VectorStoreValidationError::InvalidChunkOverlap { .. })
        ));

        let auto = CreateVectorStoreRequest::new()
            .with_chunking_strategy(VectorStoreChunkingStrategyRequest::auto());
        auto.validate().expect("auto strategy has no overlap rule");
    }

    #[test]
    fn tagged_chunking_rejects_malformed_known_and_retains_unknown() {
        assert!(
            serde_json::from_value::<VectorStoreChunkingStrategyRequest>(json!({
                "type": "static"
            }))
            .is_err()
        );

        let value = json!({"type": "semantic_v2", "window": 12});
        let decoded: VectorStoreChunkingStrategyRequest =
            serde_json::from_value(value.clone()).expect("unknown strategy");
        assert!(matches!(
            decoded,
            VectorStoreChunkingStrategyRequest::Unknown(_)
        ));
        assert_eq!(serde_json::to_value(decoded).expect("encode"), value);
    }

    #[test]
    fn recursive_filters_roundtrip_and_known_malformed_fails() {
        let filter = VectorStoreFilter::Compound(Box::new(VectorStoreCompoundFilter::new(
            VectorStoreCompoundOperator::And,
            vec![VectorStoreFilter::Comparison(
                VectorStoreComparisonFilter::new(
                    VectorStoreComparisonOperator::Equal,
                    "tenant",
                    VectorStoreFilterValue::String("blue".into()),
                ),
            )],
        )));
        let encoded = serde_json::to_value(&filter).expect("encode filter");
        let decoded: VectorStoreFilter = serde_json::from_value(encoded.clone()).expect("decode");
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), encoded);

        assert!(
            serde_json::from_value::<VectorStoreFilter>(json!({
                "type": "eq", "key": "tenant"
            }))
            .is_err()
        );

        let future = json!({"type": "starts_with", "key": "tenant", "value": "blue"});
        let decoded: VectorStoreFilter =
            serde_json::from_value(future.clone()).expect("future filter");
        assert!(matches!(decoded, VectorStoreFilter::Unknown(_)));
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), future);
    }

    #[test]
    fn attached_file_keeps_required_null_and_optional_attribute_states() {
        let base = json!({
            "id": "file-abc",
            "object": "vector_store.file",
            "usage_bytes": 12,
            "created_at": 100,
            "vector_store_id": "vs-abc",
            "status": "completed",
            "last_error": null
        });
        let omitted: VectorStoreFile = serde_json::from_value(base.clone()).expect("omitted attrs");
        assert!(omitted.attributes().is_omitted());

        let mut null = base.clone();
        null["attributes"] = Value::Null;
        let null: VectorStoreFile = serde_json::from_value(null).expect("null attrs");
        assert!(matches!(
            null.attributes(),
            Omittable::Value(Nullable::Null)
        ));

        let mut missing_error = base;
        missing_error
            .as_object_mut()
            .expect("object")
            .remove("last_error");
        assert!(serde_json::from_value::<VectorStoreFile>(missing_error).is_err());
    }

    #[test]
    fn file_batch_requires_one_input_variant() {
        assert!(serde_json::from_value::<CreateVectorStoreFileBatchRequest>(json!({})).is_err());
        assert!(
            serde_json::from_value::<CreateVectorStoreFileBatchRequest>(json!({
                "file_ids": ["file-a"],
                "files": [{"file_id": "file-b"}]
            }))
            .is_err()
        );

        let request = CreateVectorStoreFileBatchRequest::from_file_ids(vec![FileId::new("file-a")])
            .expect("valid batch");
        assert_eq!(
            serde_json::to_value(request).expect("encode"),
            json!({"file_ids": ["file-a"]})
        );

        let null_attributes =
            CreateVectorStoreFileBatchRequest::from_file_ids(vec![FileId::new("file-a")])
                .expect("valid batch")
                .with_attributes_null()
                .expect("null global attributes");
        assert!(matches!(
            null_attributes.attributes(),
            Omittable::Value(Nullable::Null)
        ));
        assert_eq!(
            serde_json::to_value(null_attributes).expect("encode null attributes"),
            json!({"file_ids": ["file-a"], "attributes": null})
        );

        let per_file =
            CreateVectorStoreFileBatchRequest::from_files(vec![CreateVectorStoreFileRequest::new(
                "file-a",
            )])
            .expect("valid per-file batch");
        assert!(matches!(
            per_file
                .clone()
                .with_chunking_strategy(VectorStoreChunkingStrategyRequest::auto())
                .expect_err("global chunking rejects per-file form"),
            VectorStoreValidationError::GlobalFieldsWithPerFileBatch
        ));
        let mut attributes = VectorStoreFileAttributes::new();
        attributes
            .insert("tenant", VectorStoreAttributeValue::Boolean(true))
            .expect("attribute");
        assert!(matches!(
            per_file
                .clone()
                .with_attributes(attributes)
                .expect_err("global attributes reject per-file form"),
            VectorStoreValidationError::GlobalFieldsWithPerFileBatch
        ));
        assert!(matches!(
            per_file
                .with_attributes_null()
                .expect_err("null global attributes reject per-file form"),
            VectorStoreValidationError::GlobalFieldsWithPerFileBatch
        ));
    }

    #[test]
    fn per_file_batch_with_global_fields_decodes_but_builders_reject() {
        // The pinned anyOf is satisfied by the `files` branch alone, so the
        // schema-legal combinations below must decode and round-trip.
        let files_with_chunking = json!({
            "files": [{"file_id": "file-a"}],
            "chunking_strategy": {"type": "auto"}
        });
        let decoded: CreateVectorStoreFileBatchRequest =
            serde_json::from_value(files_with_chunking.clone()).expect("schema-legal combination");
        assert!(decoded.files().is_value());
        assert!(decoded.chunking_strategy().is_value());
        assert_eq!(
            serde_json::to_value(decoded).expect("re-encode"),
            files_with_chunking
        );

        let files_with_attributes = json!({
            "files": [{"file_id": "file-a", "attributes": {"tenant": "blue"}}],
            "attributes": null
        });
        let decoded: CreateVectorStoreFileBatchRequest =
            serde_json::from_value(files_with_attributes.clone()).expect("null global attributes");
        assert!(matches!(
            decoded.attributes(),
            Omittable::Value(Nullable::Null)
        ));
        assert_eq!(
            serde_json::to_value(decoded).expect("re-encode"),
            files_with_attributes
        );

        let mut attributes = VectorStoreFileAttributes::new();
        attributes
            .insert(
                "tenant",
                VectorStoreAttributeValue::string("blue").expect("value"),
            )
            .expect("attribute");
        let file_ids_with_globals =
            CreateVectorStoreFileBatchRequest::from_file_ids(vec![FileId::new("file-a")])
                .expect("valid batch")
                .with_chunking_strategy(VectorStoreChunkingStrategyRequest::auto())
                .expect("bare-ID form accepts globals")
                .with_attributes(attributes)
                .expect("bare-ID form accepts attributes");
        let encoded = serde_json::to_value(file_ids_with_globals).expect("encode");
        assert_eq!(encoded["file_ids"], json!(["file-a"]));
        assert_eq!(encoded["chunking_strategy"], json!({"type": "auto"}));
        assert_eq!(encoded["attributes"]["tenant"], "blue");
    }

    #[test]
    fn search_request_is_fully_typed() {
        let filter = VectorStoreFilter::Comparison(VectorStoreComparisonFilter::new(
            VectorStoreComparisonOperator::GreaterThan,
            "year",
            VectorStoreFilterValue::Number(Number::from(2024)),
        ));
        let request = VectorStoreSearchRequest::new("quarterly revenue")
            .with_rewrite_query(true)
            .with_max_results(VectorStoreMaxResults::new(25).expect("maximum"))
            .with_filter(filter)
            .with_ranking_options(
                VectorStoreRankingOptions::new()
                    .with_ranker(VectorStoreRanker::Auto)
                    .with_score_threshold(VectorStoreScore::new(0.4).expect("threshold")),
            );
        let value = serde_json::to_value(request).expect("encode request");
        assert_eq!(value["query"], "quarterly revenue");
        assert_eq!(value["filters"]["type"], "gt");
        assert_eq!(value["ranking_options"]["score_threshold"], 0.4);
    }

    #[test]
    fn search_request_validate_closes_the_direct_empty_query_array_hole() {
        // `VectorStoreSearchQuery::Texts` is a public variant, so an empty
        // query array can be assembled directly and would otherwise reach the
        // wire (minItems is enforced only by the constructor and decode).
        let empty = VectorStoreSearchRequest {
            query: VectorStoreSearchQuery::Texts(Vec::new()),
            ..VectorStoreSearchRequest::new("placeholder")
        };
        assert_eq!(
            empty.validate(),
            Err(VectorStoreValidationError::EmptySearchQueries)
        );

        // Every constructible shape that respects minItems stays valid.
        assert_eq!(VectorStoreSearchRequest::new("single").validate(), Ok(()));
        let multiple = VectorStoreSearchQuery::multiple(vec!["one".to_owned(), "two".to_owned()])
            .expect("non-empty queries");
        assert_eq!(VectorStoreSearchRequest::new(multiple).validate(), Ok(()));
    }

    #[test]
    fn max_results_bounds_report_the_rejected_actual_value() {
        assert_eq!(
            VectorStoreMaxResults::new(0),
            Err(VectorStoreValidationError::InvalidMaxResults { actual: 0 })
        );
        assert_eq!(
            VectorStoreMaxResults::new(MAX_VECTOR_STORE_SEARCH_RESULTS + 1),
            Err(VectorStoreValidationError::InvalidMaxResults {
                actual: MAX_VECTOR_STORE_SEARCH_RESULTS + 1
            })
        );
        // The rendered message keeps its original wording.
        assert_eq!(
            VectorStoreMaxResults::new(51)
                .expect_err("above the ceiling")
                .to_string(),
            "vector-store max_num_results must be 1..=50, got 51"
        );
        assert_eq!(
            VectorStoreMaxResults::new(MAX_VECTOR_STORE_SEARCH_RESULTS)
                .expect("ceiling is valid")
                .get(),
            MAX_VECTOR_STORE_SEARCH_RESULTS
        );
    }

    #[test]
    fn search_page_requires_nullable_next_page() {
        let official_example_shape = json!({
            "object": "vector_store.search_results.page",
            "search_query": "query",
            "data": [],
            "has_more": false,
            "next_page": null
        });
        let page: VectorStoreSearchResultsPage =
            serde_json::from_value(official_example_shape.clone()).expect("decode page");
        assert!(page.next_page().is_null());
        assert!(matches!(
            page.search_query(),
            VectorStoreSearchQueryOutput::Text(value) if value == "query"
        ));
        assert_eq!(
            serde_json::to_value(page).expect("re-encode"),
            official_example_shape
        );

        let array_page: VectorStoreSearchResultsPage = serde_json::from_value(json!({
            "object": "vector_store.search_results.page",
            "search_query": ["query", "rewritten query"],
            "data": [],
            "has_more": false,
            "next_page": null
        }))
        .expect("decode component-schema array");
        assert!(matches!(
            array_page.search_query(),
            VectorStoreSearchQueryOutput::Texts(values) if values.len() == 2
        ));

        assert!(
            serde_json::from_value::<VectorStoreSearchResultsPage>(json!({
                "object": "vector_store.search_results.page",
                "search_query": ["query"],
                "data": [],
                "has_more": false
            }))
            .is_err()
        );
    }

    proptest! {
        #[test]
        fn integer_attributes_roundtrip(key in "[a-z]{1,24}", value in any::<i64>()) {
            let mut attributes = VectorStoreFileAttributes::new();
            attributes.insert(key, value.into()).expect("valid generated attribute");
            let encoded = serde_json::to_vec(&attributes).expect("encode");
            let decoded: VectorStoreFileAttributes = serde_json::from_slice(&encoded).expect("decode");
            prop_assert_eq!(decoded, attributes);
        }

        #[test]
        fn valid_scores_roundtrip(value in 0.0f64..=1.0) {
            let score = VectorStoreScore::new(value).expect("generated score is valid");
            let encoded = serde_json::to_vec(&score).expect("encode");
            let decoded: VectorStoreScore = serde_json::from_slice(&encoded).expect("decode");
            prop_assert!((decoded.get() - value).abs() <= f64::EPSILON);
        }

        #[test]
        fn metadata_roundtrips(key in "[a-z]{1,20}", value in ".{0,40}") {
            let mut metadata = VectorStoreMetadata::new();
            metadata.insert(key, value).expect("generated metadata is valid");
            let encoded = serde_json::to_vec(&metadata).expect("encode");
            let decoded: VectorStoreMetadata = serde_json::from_slice(&encoded).expect("decode");
            prop_assert_eq!(decoded, metadata);
        }
    }

    #[test]
    fn metadata_deserializes_in_stable_map_order() {
        let decoded: VectorStoreMetadata =
            serde_json::from_value(json!({"z": "last", "a": "first"})).expect("decode");
        let actual = decoded.iter().collect::<BTreeMap<_, _>>();
        assert_eq!(actual.get("a"), Some(&"first"));
    }
}
