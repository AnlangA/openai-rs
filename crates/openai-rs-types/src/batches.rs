//! Typed wire models and JSONL helpers for the Batch API.
//!
//! Batch input and output files are newline-delimited JSON. [`BatchLine`]
//! carries a typed endpoint body, while [`BatchResultLine`] carries either a
//! typed response body or a structured per-line error. [`BatchJsonlWriter`]
//! and [`BatchJsonlReader`] process one bounded line at a time, so callers do
//! not need to format JSONL or buffer an entire batch file.

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    io::{self, BufRead, Write},
    marker::PhantomData,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

use crate::{
    BatchId, ExtraFields, FileId, ModelId, Nullable, Omittable, opaque_string_id, open_string_enum,
};

opaque_string_id! {
    /// Opaque identifier assigned to one executed request in a batch output file.
    pub struct BatchRequestId;
}

open_string_enum! {
    /// Object discriminator returned for a batch.
    pub enum BatchObjectType {
        Batch = "batch",
    }
}

open_string_enum! {
    /// Object discriminator returned by the batch collection endpoint.
    pub enum BatchListObjectType {
        List = "list",
    }
}

open_string_enum! {
    /// Lifecycle state of an asynchronous batch.
    pub enum BatchStatus {
        Validating = "validating",
        Failed = "failed",
        InProgress = "in_progress",
        Finalizing = "finalizing",
        Completed = "completed",
        Expired = "expired",
        Cancelling = "cancelling",
        Cancelled = "cancelled",
    }
}

open_string_enum! {
    /// Platform endpoint accepted by the pinned Batch API contract.
    pub enum BatchEndpoint {
        Responses = "/v1/responses",
        ChatCompletions = "/v1/chat/completions",
        Embeddings = "/v1/embeddings",
        Completions = "/v1/completions",
        Moderations = "/v1/moderations",
        ImageGenerations = "/v1/images/generations",
        ImageEdits = "/v1/images/edits",
        Videos = "/v1/videos",
    }
}

open_string_enum! {
    /// Completion window accepted when creating a batch.
    pub enum BatchCompletionWindow {
        TwentyFourHours = "24h",
    }
}

open_string_enum! {
    /// HTTP method carried by an input JSONL line.
    pub enum BatchHttpMethod {
        Post = "POST",
    }
}

open_string_enum! {
    /// Anchor used for output and error file expiration.
    pub enum BatchFileExpirationAnchor {
        CreatedAt = "created_at",
    }
}

/// Maximum number of metadata pairs accepted by the documented Batch API.
pub const MAX_BATCH_METADATA_PROPERTIES: usize = 16;
/// Maximum metadata key length in Unicode scalar values.
pub const MAX_BATCH_METADATA_KEY_CHARS: usize = 64;
/// Maximum metadata value length in Unicode scalar values.
pub const MAX_BATCH_METADATA_VALUE_CHARS: usize = 512;
/// Minimum generated-file lifetime accepted by the contract.
pub const MIN_BATCH_FILE_EXPIRATION_SECONDS: u64 = 3_600;
/// Maximum generated-file lifetime accepted by the contract.
pub const MAX_BATCH_FILE_EXPIRATION_SECONDS: u64 = 2_592_000;
/// Maximum number of request lines documented for one batch input file.
pub const MAX_BATCH_INPUT_LINES: usize = 50_000;
/// Maximum documented batch input file size (200 MiB).
pub const MAX_BATCH_INPUT_BYTES: usize = 200 * 1024 * 1024;
/// Default maximum accepted size of one JSONL line. A single line may legally
/// occupy the complete input-file budget.
pub const DEFAULT_BATCH_JSONL_LINE_LIMIT: usize = MAX_BATCH_INPUT_BYTES;

/// Validation error for a typed Batch request or helper value.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchValidationError {
    /// Metadata contains more than sixteen entries.
    #[error("batch metadata contains {actual} properties; maximum is {maximum}")]
    TooManyMetadataProperties {
        /// Observed property count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata key exceeds the documented limit.
    #[error("batch metadata key has {actual} characters; maximum is {maximum}")]
    MetadataKeyTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A metadata value exceeds the documented limit.
    #[error("batch metadata value has {actual} characters; maximum is {maximum}")]
    MetadataValueTooLong {
        /// Observed character count.
        actual: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// A generated-file expiration delay is outside the contract range.
    #[error(
        "batch output expiration must be between {MIN_BATCH_FILE_EXPIRATION_SECONDS} and {MAX_BATCH_FILE_EXPIRATION_SECONDS} seconds, got {seconds}"
    )]
    InvalidExpirationSeconds {
        /// Rejected delay.
        seconds: u64,
    },
    /// A custom identifier is empty.
    #[error("batch custom_id must not be empty")]
    EmptyCustomId,
    /// A success-body constructor received a non-success HTTP status.
    #[error("batch success body requires a 2xx status, got {status_code}")]
    InvalidSuccessStatus {
        /// Rejected status.
        status_code: u16,
    },
    /// An error-body constructor received a success HTTP status.
    #[error("batch error body requires a non-2xx status, got {status_code}")]
    InvalidErrorStatus {
        /// Rejected status.
        status_code: u16,
    },
}

/// Validated string-to-string metadata used by batch objects and requests.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct BatchMetadata(BTreeMap<String, String>);

impl BatchMetadata {
    /// Creates empty metadata.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Validates and inserts one metadata pair.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>, BatchValidationError> {
        let key = key.into();
        let value = value.into();
        validate_metadata_entry(&key, &value)?;
        if !self.0.contains_key(&key) && self.0.len() == MAX_BATCH_METADATA_PROPERTIES {
            return Err(BatchValidationError::TooManyMetadataProperties {
                actual: self.0.len().saturating_add(1),
                maximum: MAX_BATCH_METADATA_PROPERTIES,
            });
        }
        Ok(self.0.insert(key, value))
    }

    /// Returns a metadata value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Iterates over metadata in stable key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns the number of metadata properties.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the wrapper and returns the validated map.
    #[must_use]
    pub fn into_inner(self) -> BTreeMap<String, String> {
        self.0
    }
}

impl fmt::Debug for BatchMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BatchMetadata")
            .field("property_count", &self.0.len())
            .finish()
    }
}

impl TryFrom<BTreeMap<String, String>> for BatchMetadata {
    type Error = BatchValidationError;

    fn try_from(value: BTreeMap<String, String>) -> Result<Self, Self::Error> {
        validate_metadata(&value)?;
        Ok(Self(value))
    }
}

impl Serialize for BatchMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BatchMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

fn validate_metadata(value: &BTreeMap<String, String>) -> Result<(), BatchValidationError> {
    if value.len() > MAX_BATCH_METADATA_PROPERTIES {
        return Err(BatchValidationError::TooManyMetadataProperties {
            actual: value.len(),
            maximum: MAX_BATCH_METADATA_PROPERTIES,
        });
    }
    for (key, value) in value {
        validate_metadata_entry(key, value)?;
    }
    Ok(())
}

fn validate_metadata_entry(key: &str, value: &str) -> Result<(), BatchValidationError> {
    let key_chars = key.chars().count();
    if key_chars > MAX_BATCH_METADATA_KEY_CHARS {
        return Err(BatchValidationError::MetadataKeyTooLong {
            actual: key_chars,
            maximum: MAX_BATCH_METADATA_KEY_CHARS,
        });
    }
    let value_chars = value.chars().count();
    if value_chars > MAX_BATCH_METADATA_VALUE_CHARS {
        return Err(BatchValidationError::MetadataValueTooLong {
            actual: value_chars,
            maximum: MAX_BATCH_METADATA_VALUE_CHARS,
        });
    }
    Ok(())
}

/// Validated expiration policy for batch output and error files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BatchFileExpirationAfter {
    anchor: BatchFileExpirationAnchor,
    seconds: u64,
}

impl BatchFileExpirationAfter {
    /// Creates a policy anchored at the generated file's creation time.
    pub fn new(seconds: u64) -> Result<Self, BatchValidationError> {
        Self::from_raw_anchor(BatchFileExpirationAnchor::CreatedAt, seconds)
    }

    /// Creates a policy with a forward-compatible anchor value.
    pub fn from_raw_anchor(
        anchor: BatchFileExpirationAnchor,
        seconds: u64,
    ) -> Result<Self, BatchValidationError> {
        if !(MIN_BATCH_FILE_EXPIRATION_SECONDS..=MAX_BATCH_FILE_EXPIRATION_SECONDS)
            .contains(&seconds)
        {
            return Err(BatchValidationError::InvalidExpirationSeconds { seconds });
        }
        Ok(Self { anchor, seconds })
    }

    /// Returns the anchor.
    #[must_use]
    pub const fn anchor(&self) -> &BatchFileExpirationAnchor {
        &self.anchor
    }

    /// Returns the expiration delay in seconds.
    #[must_use]
    pub const fn seconds(&self) -> u64 {
        self.seconds
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchFileExpirationAfterWire {
    anchor: BatchFileExpirationAnchor,
    seconds: u64,
}

impl<'de> Deserialize<'de> for BatchFileExpirationAfter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = BatchFileExpirationAfterWire::deserialize(deserializer)?;
        Self::from_raw_anchor(wire.anchor, wire.seconds).map_err(serde::de::Error::custom)
    }
}

/// Body for `POST /batches`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBatchRequest {
    input_file_id: FileId,
    endpoint: BatchEndpoint,
    completion_window: BatchCompletionWindow,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<BatchMetadata>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_expires_after: Omittable<BatchFileExpirationAfter>,
}

impl CreateBatchRequest {
    /// Creates a request using the currently supported 24-hour window.
    #[must_use]
    pub fn new(input_file_id: impl Into<FileId>, endpoint: BatchEndpoint) -> Self {
        Self {
            input_file_id: input_file_id.into(),
            endpoint,
            completion_window: BatchCompletionWindow::TwentyFourHours,
            metadata: Omittable::Omitted,
            output_expires_after: Omittable::Omitted,
        }
    }

    /// Attaches metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: BatchMetadata) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }

    /// Sends an explicit `null` metadata property.
    #[must_use]
    pub fn with_metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }

    /// Omits metadata from the wire request.
    #[must_use]
    pub fn clear_metadata(mut self) -> Self {
        self.metadata = Omittable::Omitted;
        self
    }

    /// Sets expiration for generated output and error files.
    #[must_use]
    pub fn with_output_expiration(mut self, expiration: BatchFileExpirationAfter) -> Self {
        self.output_expires_after = Omittable::Value(expiration);
        self
    }

    /// Returns the input file identifier.
    #[must_use]
    pub const fn input_file_id(&self) -> &FileId {
        &self.input_file_id
    }

    /// Returns the selected endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &BatchEndpoint {
        &self.endpoint
    }

    /// Returns the completion window.
    #[must_use]
    pub const fn completion_window(&self) -> &BatchCompletionWindow {
        &self.completion_window
    }

    /// Returns the exact optional-nullable metadata state.
    #[must_use]
    pub const fn metadata(&self) -> &Omittable<Nullable<BatchMetadata>> {
        &self.metadata
    }

    /// Returns the optional generated-file expiration policy.
    #[must_use]
    pub const fn output_expires_after(&self) -> &Omittable<BatchFileExpirationAfter> {
        &self.output_expires_after
    }
}

/// Validation errors embedded in a batch object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchErrors {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    data: Omittable<Vec<BatchError>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchErrors {
    /// Returns the collection discriminator when present.
    #[must_use]
    pub fn object(&self) -> Option<&str> {
        match &self.object {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns validation errors when present.
    #[must_use]
    pub fn data(&self) -> Option<&[BatchError]> {
        match &self.data {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One validation error reported for an input batch file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchError {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    code: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    message: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    param: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    line: Omittable<Nullable<i64>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchError {
    /// Returns the optional error code.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match &self.code {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns the optional human-readable message.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match &self.message {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns the exact optional/nullable parameter state.
    #[must_use]
    pub const fn param(&self) -> &Omittable<Nullable<String>> {
        &self.param
    }

    /// Returns the exact optional/nullable line-number state.
    #[must_use]
    pub const fn line(&self) -> &Omittable<Nullable<i64>> {
        &self.line
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request counters grouped by terminal status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchRequestCounts {
    total: i64,
    completed: i64,
    failed: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchRequestCounts {
    /// Total request count.
    #[must_use]
    pub const fn total(&self) -> i64 {
        self.total
    }

    /// Successfully completed request count.
    #[must_use]
    pub const fn completed(&self) -> i64 {
        self.completed
    }

    /// Failed request count.
    #[must_use]
    pub const fn failed(&self) -> i64 {
        self.failed
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Cached-token breakdown in batch usage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchInputTokenDetails {
    cached_tokens: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchInputTokenDetails {
    /// Tokens served from cache.
    #[must_use]
    pub const fn cached_tokens(&self) -> i64 {
        self.cached_tokens
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Reasoning-token breakdown in batch usage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchOutputTokenDetails {
    reasoning_tokens: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchOutputTokenDetails {
    /// Tokens spent on reasoning.
    #[must_use]
    pub const fn reasoning_tokens(&self) -> i64 {
        self.reasoning_tokens
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Token accounting returned for newer batches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchUsage {
    input_tokens: i64,
    input_tokens_details: BatchInputTokenDetails,
    output_tokens: i64,
    output_tokens_details: BatchOutputTokenDetails,
    total_tokens: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchUsage {
    /// Input token count.
    #[must_use]
    pub const fn input_tokens(&self) -> i64 {
        self.input_tokens
    }

    /// Input token breakdown.
    #[must_use]
    pub const fn input_token_details(&self) -> &BatchInputTokenDetails {
        &self.input_tokens_details
    }

    /// Output token count.
    #[must_use]
    pub const fn output_tokens(&self) -> i64 {
        self.output_tokens
    }

    /// Output token breakdown.
    #[must_use]
    pub const fn output_token_details(&self) -> &BatchOutputTokenDetails {
        &self.output_tokens_details
    }

    /// Total token count.
    #[must_use]
    pub const fn total_tokens(&self) -> i64 {
        self.total_tokens
    }

    /// Returns fields added after this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One asynchronous Batch API job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    id: BatchId,
    object: BatchObjectType,
    endpoint: BatchEndpoint,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<ModelId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    errors: Omittable<Nullable<BatchErrors>>,
    input_file_id: FileId,
    completion_window: BatchCompletionWindow,
    status: BatchStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    output_file_id: Omittable<Nullable<FileId>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    error_file_id: Omittable<Nullable<FileId>>,
    created_at: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    in_progress_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    finalizing_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    completed_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    failed_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expired_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    cancelling_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    cancelled_at: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    request_counts: Omittable<BatchRequestCounts>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<BatchUsage>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<BatchMetadata>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Batch {
    /// Batch identifier.
    #[must_use]
    pub const fn id(&self) -> &BatchId {
        &self.id
    }

    /// Object discriminator.
    #[must_use]
    pub const fn object(&self) -> &BatchObjectType {
        &self.object
    }

    /// Batch status.
    #[must_use]
    pub const fn status(&self) -> &BatchStatus {
        &self.status
    }

    /// Endpoint used for each input line.
    #[must_use]
    pub const fn endpoint(&self) -> &BatchEndpoint {
        &self.endpoint
    }

    /// Input JSONL file identifier.
    #[must_use]
    pub const fn input_file_id(&self) -> &FileId {
        &self.input_file_id
    }

    /// Completion window selected for this batch.
    #[must_use]
    pub const fn completion_window(&self) -> &BatchCompletionWindow {
        &self.completion_window
    }

    /// Model reported by newer batch responses.
    #[must_use]
    pub fn model(&self) -> Option<&ModelId> {
        match &self.model {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Input-file validation errors when present.
    #[must_use]
    pub fn errors(&self) -> Option<&BatchErrors> {
        match &self.errors {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Exact optional-nullable validation-error state.
    #[must_use]
    pub const fn errors_state(&self) -> &Omittable<Nullable<BatchErrors>> {
        &self.errors
    }

    /// Output file identifier when available.
    #[must_use]
    pub fn output_file_id(&self) -> Option<&FileId> {
        match &self.output_file_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Exact optional-nullable output-file state.
    #[must_use]
    pub const fn output_file_id_state(&self) -> &Omittable<Nullable<FileId>> {
        &self.output_file_id
    }

    /// Error file identifier when available.
    #[must_use]
    pub fn error_file_id(&self) -> Option<&FileId> {
        match &self.error_file_id {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Exact optional-nullable error-file state.
    #[must_use]
    pub const fn error_file_id_state(&self) -> &Omittable<Nullable<FileId>> {
        &self.error_file_id
    }

    /// Creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    fn optional_timestamp(value: &Omittable<Nullable<i64>>) -> Option<i64> {
        match value {
            Omittable::Value(Nullable::Value(value)) => Some(*value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Processing-start timestamp.
    #[must_use]
    pub fn in_progress_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.in_progress_at)
    }

    /// Exact optional-nullable processing-start state.
    #[must_use]
    pub const fn in_progress_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.in_progress_at
    }

    /// Scheduled expiration timestamp.
    #[must_use]
    pub fn expires_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.expires_at)
    }

    /// Exact optional-nullable expiration state.
    #[must_use]
    pub const fn expires_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.expires_at
    }

    /// Finalization-start timestamp.
    #[must_use]
    pub fn finalizing_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.finalizing_at)
    }

    /// Exact optional-nullable finalization-start state.
    #[must_use]
    pub const fn finalizing_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.finalizing_at
    }

    /// Completion timestamp.
    #[must_use]
    pub fn completed_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.completed_at)
    }

    /// Exact optional-nullable completion state.
    #[must_use]
    pub const fn completed_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.completed_at
    }

    /// Failure timestamp.
    #[must_use]
    pub fn failed_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.failed_at)
    }

    /// Exact optional-nullable failure state.
    #[must_use]
    pub const fn failed_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.failed_at
    }

    /// Expired-state timestamp.
    #[must_use]
    pub fn expired_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.expired_at)
    }

    /// Exact optional-nullable expired state.
    #[must_use]
    pub const fn expired_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.expired_at
    }

    /// Cancellation-start timestamp.
    #[must_use]
    pub fn cancelling_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.cancelling_at)
    }

    /// Exact optional-nullable cancellation-start state.
    #[must_use]
    pub const fn cancelling_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.cancelling_at
    }

    /// Cancellation-complete timestamp.
    #[must_use]
    pub fn cancelled_at(&self) -> Option<i64> {
        Self::optional_timestamp(&self.cancelled_at)
    }

    /// Exact optional-nullable cancelled state.
    #[must_use]
    pub const fn cancelled_at_state(&self) -> &Omittable<Nullable<i64>> {
        &self.cancelled_at
    }

    /// Request counters when populated.
    #[must_use]
    pub fn request_counts(&self) -> Option<&BatchRequestCounts> {
        match &self.request_counts {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Usage accounting when populated.
    #[must_use]
    pub fn usage(&self) -> Option<&BatchUsage> {
        match &self.usage {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Exact optional-nullable metadata state.
    #[must_use]
    pub const fn metadata(&self) -> &Omittable<Nullable<BatchMetadata>> {
        &self.metadata
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Default and maximum page size for batch listing.
pub const DEFAULT_BATCH_LIST_LIMIT: u32 = 20;
/// Maximum page size for batch listing.
pub const MAX_BATCH_LIST_LIMIT: u32 = 100;

/// Validated batch list page size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BatchListLimit(u32);

impl BatchListLimit {
    /// Creates a page size in `1..=100`.
    pub const fn new(value: u32) -> Result<Self, BatchListLimitError> {
        if value == 0 || value > MAX_BATCH_LIST_LIMIT {
            Err(BatchListLimitError { value })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated size.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for BatchListLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Invalid batch list page size.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("batch list limit must be between 1 and {MAX_BATCH_LIST_LIMIT}, got {value}")]
pub struct BatchListLimitError {
    value: u32,
}

impl BatchListLimitError {
    /// Returns the rejected value.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.value
    }
}

/// Query parameters for `GET /batches`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<BatchId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<BatchListLimit>,
}

impl BatchListParams {
    /// Creates an unpaginated request using server defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Continues after an opaque batch cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<BatchId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Selects a validated page size.
    #[must_use]
    pub fn with_limit(mut self, limit: BatchListLimit) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Returns the effective page size.
    #[must_use]
    pub const fn effective_limit(&self) -> u32 {
        match self.limit {
            Omittable::Omitted => DEFAULT_BATCH_LIST_LIMIT,
            Omittable::Value(value) => value.get(),
        }
    }

    /// Exact cursor presence state.
    #[must_use]
    pub const fn after_cursor(&self) -> &Omittable<BatchId> {
        &self.after
    }

    /// Exact page-limit presence state.
    #[must_use]
    pub const fn limit(&self) -> &Omittable<BatchListLimit> {
        &self.limit
    }
}

/// Response from `GET /batches`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListBatchesResponse {
    object: BatchListObjectType,
    data: Vec<Batch>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    first_id: Omittable<BatchId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    last_id: Omittable<BatchId>,
    has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListBatchesResponse {
    /// Collection discriminator.
    #[must_use]
    pub const fn object(&self) -> &BatchListObjectType {
        &self.object
    }

    /// Batch items in this page.
    #[must_use]
    pub fn data(&self) -> &[Batch] {
        &self.data
    }

    /// First page item identifier when supplied by the service.
    #[must_use]
    pub fn first_id(&self) -> Option<&BatchId> {
        match &self.first_id {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Last page item identifier when supplied by the service.
    #[must_use]
    pub fn last_id(&self) -> Option<&BatchId> {
        match &self.last_id {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Whether another page may be available.
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

/// Validated caller-chosen identifier used to correlate batch results.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BatchCustomId(Box<str>);

impl BatchCustomId {
    /// Creates a non-empty custom identifier.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, BatchValidationError> {
        let value = value.into();
        if value.is_empty() {
            return Err(BatchValidationError::EmptyCustomId);
        }
        Ok(Self(value))
    }

    /// Borrows the wire value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BatchCustomId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Box::<str>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// One typed request line in a Batch API input JSONL file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchLine<O> {
    custom_id: BatchCustomId,
    method: BatchHttpMethod,
    url: BatchEndpoint,
    body: O,
}

impl<O> BatchLine<O> {
    /// Creates a `POST` line for a typed endpoint body.
    pub fn new(
        custom_id: impl Into<Box<str>>,
        endpoint: BatchEndpoint,
        body: O,
    ) -> Result<Self, BatchValidationError> {
        Ok(Self {
            custom_id: BatchCustomId::new(custom_id)?,
            method: BatchHttpMethod::Post,
            url: endpoint,
            body,
        })
    }

    /// Returns the correlation identifier.
    #[must_use]
    pub const fn custom_id(&self) -> &BatchCustomId {
        &self.custom_id
    }

    /// Returns the endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &BatchEndpoint {
        &self.url
    }

    /// Borrows the typed endpoint body.
    #[must_use]
    pub const fn body(&self) -> &O {
        &self.body
    }

    /// Consumes the line and returns its typed body.
    #[must_use]
    pub fn into_body(self) -> O {
        self.body
    }
}

/// Status-aware body embedded in a Batch HTTP response envelope.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum BatchLineResponseBody<O> {
    /// A `2xx` response decoded into the endpoint's success DTO.
    Success(O),
    /// A non-`2xx` response retained as semantic JSON for later typed error
    /// decoding without making the complete result file unreadable.
    Error(Value),
}

/// HTTP result embedded in a batch output line.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchLineResponse<O> {
    status_code: u16,
    request_id: String,
    body: BatchLineResponseBody<O>,
    extra: ExtraFields,
}

impl<O> BatchLineResponse<O> {
    /// Creates a typed successful response value.
    pub fn new(
        status_code: u16,
        request_id: impl Into<String>,
        body: O,
    ) -> Result<Self, BatchValidationError> {
        if !(200..300).contains(&status_code) {
            return Err(BatchValidationError::InvalidSuccessStatus { status_code });
        }
        Ok(Self {
            status_code,
            request_id: request_id.into(),
            body: BatchLineResponseBody::Success(body),
            extra: ExtraFields::default(),
        })
    }

    /// Creates a non-success HTTP result while retaining the error body.
    pub fn error(
        status_code: u16,
        request_id: impl Into<String>,
        body: Value,
    ) -> Result<Self, BatchValidationError> {
        if (200..300).contains(&status_code) {
            return Err(BatchValidationError::InvalidErrorStatus { status_code });
        }
        Ok(Self {
            status_code,
            request_id: request_id.into(),
            body: BatchLineResponseBody::Error(body),
            extra: ExtraFields::default(),
        })
    }

    /// HTTP status code returned for the request.
    #[must_use]
    pub const fn status_code(&self) -> u16 {
        self.status_code
    }

    /// OpenAI request identifier.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Typed endpoint response body.
    #[must_use]
    pub const fn body(&self) -> &BatchLineResponseBody<O> {
        &self.body
    }

    /// Returns the typed endpoint body for a successful status.
    #[must_use]
    pub const fn success_body(&self) -> Option<&O> {
        match &self.body {
            BatchLineResponseBody::Success(value) => Some(value),
            BatchLineResponseBody::Error(_) => None,
        }
    }

    /// Returns the retained non-success body.
    #[must_use]
    pub const fn error_body(&self) -> Option<&Value> {
        match &self.body {
            BatchLineResponseBody::Error(value) => Some(value),
            BatchLineResponseBody::Success(_) => None,
        }
    }

    /// Unknown response-envelope properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Serialize)]
struct BatchLineResponseRef<'a, B> {
    status_code: u16,
    request_id: &'a str,
    body: &'a B,
    #[serde(flatten)]
    extra: &'a ExtraFields,
}

impl<O> Serialize for BatchLineResponse<O>
where
    O: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.body {
            BatchLineResponseBody::Success(body) => BatchLineResponseRef {
                status_code: self.status_code,
                request_id: &self.request_id,
                body,
                extra: &self.extra,
            }
            .serialize(serializer),
            BatchLineResponseBody::Error(body) => BatchLineResponseRef {
                status_code: self.status_code,
                request_id: &self.request_id,
                body,
                extra: &self.extra,
            }
            .serialize(serializer),
        }
    }
}

#[derive(Deserialize)]
struct BatchLineResponseWire {
    status_code: u16,
    request_id: String,
    body: Value,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<'de, O> Deserialize<'de> for BatchLineResponse<O>
where
    O: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = BatchLineResponseWire::deserialize(deserializer)?;
        let body = if (200..300).contains(&wire.status_code) {
            serde_json::from_value(wire.body)
                .map(BatchLineResponseBody::Success)
                .map_err(serde::de::Error::custom)?
        } else {
            BatchLineResponseBody::Error(wire.body)
        };
        Ok(Self {
            status_code: wire.status_code,
            request_id: wire.request_id,
            body,
            extra: wire.extra,
        })
    }
}

/// Per-request failure embedded in a batch error output line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchLineError {
    code: String,
    message: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl BatchLineError {
    /// Creates a structured output-line error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            extra: ExtraFields::default(),
        }
    }

    /// Error code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Unknown error properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Exactly one outcome represented by a batch result line.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum BatchLineOutcome<O> {
    /// The endpoint returned an HTTP response with a typed body.
    Response(BatchLineResponse<O>),
    /// The request failed before a response body was produced.
    Error(BatchLineError),
}

/// One typed line read from a batch output or error JSONL file.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchResultLine<O> {
    id: BatchRequestId,
    custom_id: BatchCustomId,
    outcome: BatchLineOutcome<O>,
    extra: ExtraFields,
}

/// Alias emphasizing that [`BatchResultLine`] is the wire output-line type.
pub type BatchOutputLine<O> = BatchResultLine<O>;

impl<O> BatchResultLine<O> {
    /// Creates a response line.
    #[must_use]
    pub fn response(
        id: impl Into<BatchRequestId>,
        custom_id: BatchCustomId,
        response: BatchLineResponse<O>,
    ) -> Self {
        Self {
            id: id.into(),
            custom_id,
            outcome: BatchLineOutcome::Response(response),
            extra: ExtraFields::default(),
        }
    }

    /// Creates an error line.
    #[must_use]
    pub fn error(
        id: impl Into<BatchRequestId>,
        custom_id: BatchCustomId,
        error: BatchLineError,
    ) -> Self {
        Self {
            id: id.into(),
            custom_id,
            outcome: BatchLineOutcome::Error(error),
            extra: ExtraFields::default(),
        }
    }

    /// Executed request identifier.
    #[must_use]
    pub const fn id(&self) -> &BatchRequestId {
        &self.id
    }

    /// Caller correlation identifier.
    #[must_use]
    pub const fn custom_id(&self) -> &BatchCustomId {
        &self.custom_id
    }

    /// Typed response-or-error outcome.
    #[must_use]
    pub const fn outcome(&self) -> &BatchLineOutcome<O> {
        &self.outcome
    }

    /// Unknown output-line properties.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Serialize)]
struct BatchResultLineRef<'a, O> {
    id: &'a BatchRequestId,
    custom_id: &'a BatchCustomId,
    response: Option<&'a BatchLineResponse<O>>,
    error: Option<&'a BatchLineError>,
    #[serde(flatten)]
    extra: &'a ExtraFields,
}

impl<O> Serialize for BatchResultLine<O>
where
    O: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (response, error) = match &self.outcome {
            BatchLineOutcome::Response(value) => (Some(value), None),
            BatchLineOutcome::Error(value) => (None, Some(value)),
        };
        BatchResultLineRef {
            id: &self.id,
            custom_id: &self.custom_id,
            response,
            error,
            extra: &self.extra,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "O: DeserializeOwned"))]
struct BatchResultLineWire<O> {
    id: BatchRequestId,
    custom_id: BatchCustomId,
    response: Nullable<BatchLineResponse<O>>,
    error: Nullable<BatchLineError>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<'de, O> Deserialize<'de> for BatchResultLine<O>
where
    O: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = BatchResultLineWire::<O>::deserialize(deserializer)?;
        let outcome = match (wire.response, wire.error) {
            (Nullable::Value(response), Nullable::Null) => BatchLineOutcome::Response(response),
            (Nullable::Null, Nullable::Value(error)) => BatchLineOutcome::Error(error),
            (Nullable::Null, Nullable::Null) => {
                return Err(serde::de::Error::custom(
                    "batch result line must contain either response or error",
                ));
            }
            (Nullable::Value(_), Nullable::Value(_)) => {
                return Err(serde::de::Error::custom(
                    "batch result line cannot contain both response and error",
                ));
            }
        };
        Ok(Self {
            id: wire.id,
            custom_id: wire.custom_id,
            outcome,
            extra: wire.extra,
        })
    }
}

/// Error produced while streaming a typed JSONL file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BatchJsonlError {
    /// Underlying reader or writer error.
    #[error("batch JSONL I/O failed at line {line}: {source}")]
    Io {
        /// One-based line number.
        line: usize,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A typed line could not be encoded.
    #[error("batch JSONL encode failed at line {line}: {source}")]
    Encode {
        /// One-based line number.
        line: usize,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// A line was not valid for the requested type.
    #[error("batch JSONL decode failed at line {line}: {source}")]
    Decode {
        /// One-based line number.
        line: usize,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Empty JSONL lines are not accepted.
    #[error("batch JSONL line {line} is empty")]
    EmptyLine {
        /// One-based line number.
        line: usize,
    },
    /// One line exceeded its configured bound.
    #[error("batch JSONL line {line} exceeded {limit} bytes")]
    LineTooLong {
        /// One-based line number.
        line: usize,
        /// Configured byte limit.
        limit: usize,
    },
    /// The complete input exceeded the Batch API size limit.
    #[error("batch JSONL input exceeded {limit} bytes")]
    InputTooLarge {
        /// Configured byte limit.
        limit: usize,
    },
    /// The input contains too many request lines.
    #[error("batch JSONL input exceeded {limit} lines")]
    TooManyLines {
        /// Configured line-count limit.
        limit: usize,
    },
    /// Request custom identifiers must be unique within one input file.
    #[error("duplicate batch custom_id {custom_id:?} at line {line}")]
    DuplicateCustomId {
        /// One-based line number.
        line: usize,
        /// Repeated identifier.
        custom_id: String,
    },
    /// One input file cannot mix endpoint URLs.
    #[error("batch JSONL line {line} uses endpoint {actual:?}; expected {expected:?}")]
    MixedEndpoints {
        /// One-based line number.
        line: usize,
        /// Endpoint selected by the first line.
        expected: String,
        /// Endpoint found on this line.
        actual: String,
    },
}

/// Incremental writer for typed Batch API input JSONL.
pub struct BatchJsonlWriter<W> {
    writer: W,
    line_count: usize,
    byte_count: usize,
    max_lines: usize,
    max_bytes: usize,
    max_line_bytes: usize,
    custom_ids: HashSet<String>,
    endpoint: Option<BatchEndpoint>,
}

impl<W> fmt::Debug for BatchJsonlWriter<W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BatchJsonlWriter")
            .field("line_count", &self.line_count)
            .field("byte_count", &self.byte_count)
            .field("max_lines", &self.max_lines)
            .field("max_bytes", &self.max_bytes)
            .field("max_line_bytes", &self.max_line_bytes)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl<W: Write> BatchJsonlWriter<W> {
    /// Creates a writer using the documented batch limits.
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            line_count: 0,
            byte_count: 0,
            max_lines: MAX_BATCH_INPUT_LINES,
            max_bytes: MAX_BATCH_INPUT_BYTES,
            max_line_bytes: DEFAULT_BATCH_JSONL_LINE_LIMIT,
            custom_ids: HashSet::new(),
            endpoint: None,
        }
    }

    /// Overrides limits, primarily for bounded environments and tests.
    #[must_use]
    pub fn with_limits(
        mut self,
        max_lines: usize,
        max_bytes: usize,
        max_line_bytes: usize,
    ) -> Self {
        self.max_lines = max_lines;
        self.max_bytes = max_bytes;
        self.max_line_bytes = max_line_bytes;
        self
    }

    /// Encodes and writes one typed request line followed by `\n`.
    pub fn write_line<O>(&mut self, line: &BatchLine<O>) -> Result<(), BatchJsonlError>
    where
        O: Serialize,
    {
        let next_line = self.line_count.saturating_add(1);
        if next_line > self.max_lines {
            return Err(BatchJsonlError::TooManyLines {
                limit: self.max_lines,
            });
        }
        if self.custom_ids.contains(line.custom_id().as_str()) {
            return Err(BatchJsonlError::DuplicateCustomId {
                line: next_line,
                custom_id: line.custom_id().as_str().to_owned(),
            });
        }
        if let Some(expected) = &self.endpoint
            && expected != line.endpoint()
        {
            return Err(BatchJsonlError::MixedEndpoints {
                line: next_line,
                expected: expected.as_str().to_owned(),
                actual: line.endpoint().as_str().to_owned(),
            });
        }

        let encoded = serde_json::to_vec(line).map_err(|source| BatchJsonlError::Encode {
            line: next_line,
            source,
        })?;
        if encoded.len() > self.max_line_bytes {
            return Err(BatchJsonlError::LineTooLong {
                line: next_line,
                limit: self.max_line_bytes,
            });
        }
        let added = encoded.len().saturating_add(1);
        if self.byte_count.saturating_add(added) > self.max_bytes {
            return Err(BatchJsonlError::InputTooLarge {
                limit: self.max_bytes,
            });
        }

        self.writer
            .write_all(&encoded)
            .and_then(|()| self.writer.write_all(b"\n"))
            .map_err(|source| BatchJsonlError::Io {
                line: next_line,
                source,
            })?;
        self.custom_ids.insert(line.custom_id().as_str().to_owned());
        if self.endpoint.is_none() {
            self.endpoint = Some(line.endpoint().clone());
        }
        self.line_count = next_line;
        self.byte_count = self.byte_count.saturating_add(added);
        Ok(())
    }

    /// Flushes the underlying writer.
    pub fn flush(&mut self) -> Result<(), BatchJsonlError> {
        self.writer.flush().map_err(|source| BatchJsonlError::Io {
            line: self.line_count.saturating_add(1),
            source,
        })
    }

    /// Returns the number of successfully written lines.
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_count
    }

    /// Returns the number of encoded bytes, including newline delimiters.
    #[must_use]
    pub const fn byte_count(&self) -> usize {
        self.byte_count
    }

    /// Consumes this wrapper without implicitly flushing the writer.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }
}

/// Incremental, bounded decoder for a typed JSONL file.
pub struct BatchJsonlReader<R, T> {
    reader: R,
    buffer: Vec<u8>,
    line_count: usize,
    max_line_bytes: usize,
    finished: bool,
    marker: PhantomData<fn() -> T>,
}

impl<R, T> fmt::Debug for BatchJsonlReader<R, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BatchJsonlReader")
            .field("line_count", &self.line_count)
            .field("max_line_bytes", &self.max_line_bytes)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl<R: BufRead, T> BatchJsonlReader<R, T> {
    /// Creates a reader with the default per-line bound.
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            line_count: 0,
            max_line_bytes: DEFAULT_BATCH_JSONL_LINE_LIMIT,
            finished: false,
            marker: PhantomData,
        }
    }

    /// Overrides the maximum encoded bytes accepted for one line.
    #[must_use]
    pub fn with_line_limit(mut self, max_line_bytes: usize) -> Self {
        self.max_line_bytes = max_line_bytes;
        self
    }

    /// Returns the number of lines consumed so far.
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_count
    }

    /// Consumes this wrapper and returns the reader.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn read_next_line(&mut self) -> Result<Option<&[u8]>, BatchJsonlError> {
        self.buffer.clear();
        let line = self.line_count.saturating_add(1);
        loop {
            let available = self
                .reader
                .fill_buf()
                .map_err(|source| BatchJsonlError::Io { line, source })?;
            if available.is_empty() {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                break;
            }

            if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
                if self.buffer.len().saturating_add(position) > self.max_line_bytes {
                    self.finished = true;
                    return Err(BatchJsonlError::LineTooLong {
                        line,
                        limit: self.max_line_bytes,
                    });
                }
                self.buffer.extend_from_slice(&available[..position]);
                self.reader.consume(position.saturating_add(1));
                break;
            }

            let available_len = available.len();
            if self.buffer.len().saturating_add(available_len) > self.max_line_bytes {
                self.finished = true;
                return Err(BatchJsonlError::LineTooLong {
                    line,
                    limit: self.max_line_bytes,
                });
            }
            self.buffer.extend_from_slice(available);
            self.reader.consume(available_len);
        }

        if self.buffer.last() == Some(&b'\r') {
            self.buffer.pop();
        }
        self.line_count = line;
        if self.buffer.is_empty() {
            return Err(BatchJsonlError::EmptyLine { line });
        }
        Ok(Some(&self.buffer))
    }
}

impl<R, T> Iterator for BatchJsonlReader<R, T>
where
    R: BufRead,
    T: DeserializeOwned,
{
    type Item = Result<T, BatchJsonlError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let result = match self.read_next_line() {
            Ok(Some(bytes)) => {
                serde_json::from_slice(bytes).map_err(|source| BatchJsonlError::Decode {
                    line: self.line_count,
                    source,
                })
            }
            Ok(None) => {
                self.finished = true;
                return None;
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.finished = true;
        }
        Some(result)
    }
}

/// Creates a bounded typed JSONL reader.
#[must_use]
pub fn read_batch_jsonl<R, T>(reader: R) -> BatchJsonlReader<R, T>
where
    R: BufRead,
{
    BatchJsonlReader::new(reader)
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    fn minimal_batch() -> serde_json::Value {
        json!({
            "id": "batch_abc",
            "object": "batch",
            "endpoint": "/v1/responses",
            "input_file_id": "file-input",
            "completion_window": "24h",
            "status": "in_progress",
            "created_at": 123
        })
    }

    #[test]
    fn create_request_preserves_missing_null_and_value_metadata() {
        let base = CreateBatchRequest::new("file-input", BatchEndpoint::Responses);
        let omitted = serde_json::to_value(&base).expect("serialize omitted metadata");
        assert!(omitted.get("metadata").is_none());

        let null = serde_json::to_value(base.clone().with_metadata_null())
            .expect("serialize null metadata");
        assert_eq!(null["metadata"], serde_json::Value::Null);

        let mut metadata = BatchMetadata::new();
        metadata.insert("job", "nightly").expect("valid metadata");
        let value = serde_json::to_value(base.with_metadata(metadata)).expect("serialize metadata");
        assert_eq!(value["metadata"]["job"], "nightly");
    }

    #[test]
    fn batch_required_fields_and_unknown_status_are_lossless() {
        let mut value = minimal_batch();
        value["status"] = json!("paused_by_future_service");
        value["future"] = json!({"retained": true});
        let decoded: Batch = serde_json::from_value(value.clone()).expect("decode batch");
        assert_eq!(
            decoded.status().unknown_value(),
            Some("paused_by_future_service")
        );
        assert_eq!(serde_json::to_value(decoded).expect("encode batch"), value);

        let mut missing = minimal_batch();
        missing
            .as_object_mut()
            .expect("object")
            .remove("created_at");
        assert!(serde_json::from_value::<Batch>(missing).is_err());
    }

    #[test]
    fn lifecycle_fields_preserve_missing_null_and_value() {
        let mut value = minimal_batch();
        value["completed_at"] = serde_json::Value::Null;
        value["failed_at"] = json!(456);
        value["errors"] = serde_json::Value::Null;
        value["output_file_id"] = serde_json::Value::Null;
        let decoded: Batch = serde_json::from_value(value.clone()).expect("decode null states");
        assert!(matches!(
            decoded.completed_at_state(),
            Omittable::Value(Nullable::Null)
        ));
        assert_eq!(decoded.failed_at(), Some(456));
        assert!(matches!(
            decoded.errors_state(),
            Omittable::Value(Nullable::Null)
        ));
        assert!(decoded.output_file_id().is_none());
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), value);
    }

    #[test]
    fn result_line_enforces_exactly_one_outcome() {
        let success: BatchResultLine<serde_json::Value> = serde_json::from_value(json!({
            "id": "batch_req_1",
            "custom_id": "line-1",
            "response": {"status_code": 200, "request_id": "req_1", "body": {"ok": true}},
            "error": null,
            "future": {"retained": true}
        }))
        .expect("decode result");
        assert!(matches!(success.outcome(), BatchLineOutcome::Response(_)));
        assert_eq!(
            serde_json::to_value(&success).expect("re-encode result")["future"],
            json!({"retained": true})
        );

        assert!(
            serde_json::from_value::<BatchResultLine<serde_json::Value>>(json!({
                "id": "batch_req_1", "custom_id": "line-1", "response": null, "error": null
            }))
            .is_err()
        );
    }

    #[test]
    fn non_success_http_body_does_not_require_the_success_dto() {
        assert!(BatchLineResponse::new(400, "req", true).is_err());
        assert!(BatchLineResponse::<bool>::error(200, "req", json!({})).is_err());
        let result: BatchResultLine<bool> = serde_json::from_value(json!({
            "id": "batch_req_bad",
            "custom_id": "line-bad",
            "response": {
                "status_code": 400,
                "request_id": "req_bad",
                "body": {"error": {"message": "invalid request"}}
            },
            "error": null
        }))
        .expect("non-success body remains readable");
        let BatchLineOutcome::Response(response) = result.outcome() else {
            panic!("expected HTTP response outcome");
        };
        assert!(response.success_body().is_none());
        assert_eq!(
            response.error_body(),
            Some(&json!({"error": {"message": "invalid request"}}))
        );
    }

    #[test]
    fn writer_and_reader_roundtrip_without_manual_jsonl() {
        let first = BatchLine::new(
            "one",
            BatchEndpoint::Responses,
            json!({"model": "gpt-test"}),
        )
        .expect("line");
        let second = BatchLine::new(
            "two",
            BatchEndpoint::Responses,
            json!({"model": "gpt-test"}),
        )
        .expect("line");
        let mut writer = BatchJsonlWriter::new(Vec::new());
        writer.write_line(&first).expect("write first");
        writer.write_line(&second).expect("write second");
        let bytes = writer.into_inner();

        let decoded =
            read_batch_jsonl::<_, BatchLine<serde_json::Value>>(BufReader::new(Cursor::new(bytes)))
                .collect::<Result<Vec<_>, _>>()
                .expect("read JSONL");
        assert_eq!(decoded, vec![first, second]);
    }

    #[test]
    fn writer_rejects_duplicate_custom_ids() {
        let line = BatchLine::new("same", BatchEndpoint::Responses, json!({})).expect("valid line");
        let mut writer = BatchJsonlWriter::new(Vec::new());
        writer.write_line(&line).expect("first line");
        assert!(matches!(
            writer.write_line(&line),
            Err(BatchJsonlError::DuplicateCustomId { .. })
        ));
    }

    #[test]
    fn writer_rejects_mixed_endpoints() {
        let first = BatchLine::new("one", BatchEndpoint::Responses, json!({})).expect("first line");
        let second =
            BatchLine::new("two", BatchEndpoint::Embeddings, json!({})).expect("second line");
        let mut writer = BatchJsonlWriter::new(Vec::new());
        writer.write_line(&first).expect("write first");
        assert!(matches!(
            writer.write_line(&second),
            Err(BatchJsonlError::MixedEndpoints { .. })
        ));
    }

    #[test]
    fn reader_accepts_crlf_and_unterminated_final_line() {
        let bytes = b"{\"custom_id\":\"a\",\"method\":\"POST\",\"url\":\"/v1/responses\",\"body\":{}}\r\n{\"custom_id\":\"b\",\"method\":\"POST\",\"url\":\"/v1/responses\",\"body\":{}}";
        let values =
            read_batch_jsonl::<_, BatchLine<serde_json::Value>>(BufReader::new(bytes.as_slice()))
                .collect::<Result<Vec<_>, _>>()
                .expect("decode both lines");
        assert_eq!(values.len(), 2);
    }

    proptest! {
        #[test]
        fn typed_line_roundtrips(custom_id in "[A-Za-z0-9_-]{1,40}", value in any::<i64>()) {
            let line = BatchLine::new(custom_id, BatchEndpoint::Embeddings, json!({"value": value}))
                .expect("generated custom id is non-empty");
            let encoded = serde_json::to_vec(&line).expect("encode");
            let decoded: BatchLine<serde_json::Value> = serde_json::from_slice(&encoded).expect("decode");
            prop_assert_eq!(decoded, line);
        }

        #[test]
        fn unknown_endpoint_roundtrips(raw in "/v1/[a-z_]{1,24}") {
            let endpoint = BatchEndpoint::from_raw(raw.clone());
            let encoded = serde_json::to_string(&endpoint).expect("encode");
            let decoded: BatchEndpoint = serde_json::from_str(&encoded).expect("decode");
            prop_assert_eq!(decoded.as_str(), raw);
        }
    }
}
