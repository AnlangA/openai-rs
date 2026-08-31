//! Typed wire models for the Files and Uploads APIs.
//!
//! JSON request bodies use [`Omittable`] for optional properties. Multipart
//! requests are intentionally separate: [`ReplayableMultipartSource`]
//! describes bytes that can be replayed without placing an open file handle or
//! binary payload inside a JSON DTO.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{
    ExtraFields, FileId, Nullable, Omittable, UploadId, opaque_string_id, open_string_enum,
};

opaque_string_id! {
    /// Opaque identifier of one part of a multipart Upload.
    pub struct UploadPartId;
}

open_string_enum! {
    /// Purpose accepted when creating a file.
    ///
    /// Future purpose strings can be supplied explicitly through `Unknown`.
    pub enum FilePurpose {
        Assistants = "assistants",
        Batch = "batch",
        FineTune = "fine-tune",
        Vision = "vision",
        UserData = "user_data",
        Evals = "evals",
    }
}

open_string_enum! {
    /// Purpose returned on a stored file object.
    ///
    /// This is separate from [`FilePurpose`] because output files have values
    /// that are not valid inputs to file creation.
    pub enum FileObjectPurpose {
        Assistants = "assistants",
        AssistantsOutput = "assistants_output",
        Batch = "batch",
        BatchOutput = "batch_output",
        FineTune = "fine-tune",
        FineTuneResults = "fine-tune-results",
        Vision = "vision",
        UserData = "user_data",
    }
}

open_string_enum! {
    /// Deprecated processing status retained on file responses.
    pub enum FileStatus {
        Uploaded = "uploaded",
        Processed = "processed",
        Error = "error",
    }
}

open_string_enum! {
    /// Lifecycle status of a multipart Upload.
    pub enum UploadStatus {
        Pending = "pending",
        Completed = "completed",
        Cancelled = "cancelled",
        Expired = "expired",
    }
}

open_string_enum! {
    /// Sort order for file collection requests.
    pub enum FileSortOrder {
        Ascending = "asc",
        Descending = "desc",
    }
}

open_string_enum! {
    /// Object discriminator returned for one file.
    pub enum FileObjectType {
        File = "file",
    }
}

open_string_enum! {
    /// Object discriminator returned by the file collection endpoint.
    pub enum FileListObjectType {
        List = "list",
    }
}

open_string_enum! {
    /// Object discriminator returned for a multipart Upload.
    pub enum UploadObjectType {
        Upload = "upload",
    }
}

open_string_enum! {
    /// Object discriminator returned for one Upload part.
    pub enum UploadPartObjectType {
        UploadPart = "upload.part",
    }
}

open_string_enum! {
    /// Timestamp anchor for a file expiration policy.
    pub enum FileExpirationAnchor {
        CreatedAt = "created_at",
    }
}

/// Minimum expiration delay accepted by the current API contract.
pub const MIN_FILE_EXPIRATION_SECONDS: u64 = 3_600;

/// Maximum expiration delay accepted by the current API contract.
pub const MAX_FILE_EXPIRATION_SECONDS: u64 = 2_592_000;

/// A validated file expiration policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExpirationAfter {
    anchor: FileExpirationAnchor,
    seconds: u64,
}

impl FileExpirationAfter {
    /// Creates a policy anchored at file creation time.
    pub fn new(seconds: u64) -> Result<Self, FileExpirationError> {
        Self::from_raw_anchor(FileExpirationAnchor::CreatedAt, seconds)
    }

    /// Creates a policy with an explicitly supplied, forward-compatible
    /// anchor.
    pub fn from_raw_anchor(
        anchor: FileExpirationAnchor,
        seconds: u64,
    ) -> Result<Self, FileExpirationError> {
        if !(MIN_FILE_EXPIRATION_SECONDS..=MAX_FILE_EXPIRATION_SECONDS).contains(&seconds) {
            return Err(FileExpirationError { seconds });
        }
        Ok(Self { anchor, seconds })
    }

    /// Returns the timestamp anchor.
    #[must_use]
    pub const fn anchor(&self) -> &FileExpirationAnchor {
        &self.anchor
    }

    /// Returns the delay after the anchor in seconds.
    #[must_use]
    pub const fn seconds(&self) -> u64 {
        self.seconds
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileExpirationAfterWire {
    anchor: FileExpirationAnchor,
    seconds: u64,
}

impl Serialize for FileExpirationAfter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        FileExpirationAfterWire {
            anchor: self.anchor.clone(),
            seconds: self.seconds,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FileExpirationAfter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FileExpirationAfterWire::deserialize(deserializer)?;
        Self::from_raw_anchor(wire.anchor, wire.seconds).map_err(D::Error::custom)
    }
}

/// A file expiration delay falls outside the contract range.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error(
    "file expiration must be between {MIN_FILE_EXPIRATION_SECONDS} and {MAX_FILE_EXPIRATION_SECONDS} seconds, got {seconds}"
)]
pub struct FileExpirationError {
    seconds: u64,
}

impl FileExpirationError {
    /// Returns the rejected delay.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }
}

/// Metadata returned for a file stored by OpenAI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileObject {
    id: FileId,
    object: FileObjectType,
    bytes: i64,
    created_at: i64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_at: Omittable<i64>,
    filename: String,
    purpose: FileObjectPurpose,
    status: FileStatus,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status_details: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FileObject {
    /// Returns the opaque file identifier.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &FileObjectType {
        &self.object
    }

    /// Returns the file size in bytes.
    #[must_use]
    pub const fn bytes(&self) -> i64 {
        self.bytes
    }

    /// Returns the creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns the expiration timestamp when the property was present.
    #[must_use]
    pub const fn expires_at(&self) -> Option<i64> {
        match self.expires_at {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns the original filename.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Returns the purpose, including unknown future values.
    #[must_use]
    pub const fn purpose(&self) -> &FileObjectPurpose {
        &self.purpose
    }

    /// Returns the deprecated processing status.
    #[must_use]
    pub const fn status(&self) -> &FileStatus {
        &self.status
    }

    /// Returns deprecated status details when present.
    #[must_use]
    pub fn status_details(&self) -> Option<&str> {
        match &self.status_details {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Effective collection size when `limit` is omitted.
///
/// The pinned schema documents a default of 10,000 but imposes no `maximum`,
/// so no upper bound is invented here.
pub const DEFAULT_FILE_LIST_LIMIT: u32 = 10_000;

/// A validated `GET /files` page size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct FileListLimit(u32);

impl FileListLimit {
    /// Validates a page size of at least 1.
    pub const fn new(value: u32) -> Result<Self, FileListLimitError> {
        if value == 0 {
            Err(FileListLimitError { value })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated page size.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for FileListLimit {
    type Error = FileListLimitError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FileListLimit> for u32 {
    fn from(value: FileListLimit) -> Self {
        value.get()
    }
}

impl Serialize for FileListLimit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for FileListLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A file list page size below the documented minimum of 1.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("file list limit must be at least 1, got {value}")]
pub struct FileListLimitError {
    value: u32,
}

impl FileListLimitError {
    /// Returns the rejected page size.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.value
    }
}

/// Query parameters for `GET /files`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    purpose: Omittable<FilePurpose>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<FileListLimit>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<FileSortOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<FileId>,
}

impl FileListParams {
    /// Creates an unfiltered list request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters files by a known or explicitly raw purpose.
    #[must_use]
    pub fn with_purpose(mut self, purpose: FilePurpose) -> Self {
        self.purpose = Omittable::Value(purpose);
        self
    }

    /// Selects a validated page size.
    #[must_use]
    pub fn with_limit(mut self, limit: FileListLimit) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Validates and selects a page size.
    pub fn try_with_limit(self, limit: u32) -> Result<Self, FileListLimitError> {
        FileListLimit::new(limit).map(|limit| self.with_limit(limit))
    }

    /// Selects ascending or descending creation order.
    #[must_use]
    pub fn with_order(mut self, order: FileSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Continues listing after an opaque file cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<FileId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Clears the purpose filter.
    #[must_use]
    pub fn clear_purpose(mut self) -> Self {
        self.purpose = Omittable::Omitted;
        self
    }

    /// Clears the page-size override.
    #[must_use]
    pub fn clear_limit(mut self) -> Self {
        self.limit = Omittable::Omitted;
        self
    }

    /// Clears the order override.
    #[must_use]
    pub fn clear_order(mut self) -> Self {
        self.order = Omittable::Omitted;
        self
    }

    /// Clears the pagination cursor.
    #[must_use]
    pub fn clear_after(mut self) -> Self {
        self.after = Omittable::Omitted;
        self
    }

    /// Returns the exact purpose presence state.
    #[must_use]
    pub const fn purpose(&self) -> &Omittable<FilePurpose> {
        &self.purpose
    }

    /// Returns the exact page-limit presence state.
    #[must_use]
    pub const fn limit(&self) -> &Omittable<FileListLimit> {
        &self.limit
    }

    /// Returns the exact sort-order presence state.
    #[must_use]
    pub const fn order(&self) -> &Omittable<FileSortOrder> {
        &self.order
    }

    /// Returns the exact cursor presence state.
    #[must_use]
    pub const fn after_cursor(&self) -> &Omittable<FileId> {
        &self.after
    }

    /// Returns the requested page size or the documented server default.
    #[must_use]
    pub const fn effective_limit(&self) -> u32 {
        match self.limit {
            Omittable::Omitted => DEFAULT_FILE_LIST_LIMIT,
            Omittable::Value(limit) => limit.get(),
        }
    }

    /// Returns the requested order or the documented server default.
    #[must_use]
    pub fn effective_order(&self) -> FileSortOrder {
        match &self.order {
            Omittable::Omitted => FileSortOrder::Descending,
            Omittable::Value(order) => order.clone(),
        }
    }
}

/// One page returned by `GET /files`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileListPage {
    object: FileListObjectType,
    data: Vec<FileObject>,
    first_id: FileId,
    last_id: FileId,
    has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FileListPage {
    /// Returns the collection discriminator.
    #[must_use]
    pub const fn object(&self) -> &FileListObjectType {
        &self.object
    }

    /// Returns the files in server order.
    #[must_use]
    pub fn data(&self) -> &[FileObject] {
        &self.data
    }

    /// Consumes the page and returns its files.
    #[must_use]
    pub fn into_data(self) -> Vec<FileObject> {
        self.data
    }

    /// Returns the first cursor in this page.
    #[must_use]
    pub const fn first_id(&self) -> &FileId {
        &self.first_id
    }

    /// Returns the last cursor in this page.
    #[must_use]
    pub const fn last_id(&self) -> &FileId {
        &self.last_id
    }

    /// Returns whether another page is available.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Compatibility name matching the OpenAPI response schema.
pub type ListFilesResponse = FileListPage;

/// Confirmation returned by `DELETE /files/{file_id}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteFileResponse {
    id: FileId,
    object: FileObjectType,
    deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeleteFileResponse {
    /// Returns the deleted file identifier.
    #[must_use]
    pub const fn id(&self) -> &FileId {
        &self.id
    }

    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &FileObjectType {
        &self.object
    }

    /// Returns whether deletion completed.
    #[must_use]
    pub const fn deleted(&self) -> bool {
        self.deleted
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Raw content returned by `GET /files/{file_id}/content`.
///
/// File content is an HTTP body, not a JSON string; this wrapper therefore has
/// no Serde implementation.
#[derive(Clone, PartialEq, Eq)]
pub struct FileContent(Box<[u8]>);

impl FileContent {
    /// Owns the downloaded bytes.
    #[must_use]
    pub fn new(bytes: impl Into<Box<[u8]>>) -> Self {
        Self(bytes.into())
    }

    /// Borrows the downloaded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns UTF-8 text when the content is textual.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    /// Consumes the wrapper and returns its bytes.
    #[must_use]
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }
}

impl fmt::Debug for FileContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileContent")
            .field("len", &self.0.len())
            .finish()
    }
}

/// A multipart filename validated against header and path injection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct MultipartFileName(String);

impl MultipartFileName {
    /// Validates a non-empty basename suitable for multipart metadata.
    pub fn new(value: impl Into<String>) -> Result<Self, MultipartFileNameError> {
        let value = value.into();
        if value.is_empty() {
            return Err(MultipartFileNameError::Empty);
        }
        if value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '"'))
        {
            return Err(MultipartFileNameError::UnsafeCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the validated filename.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the filename.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for MultipartFileName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MultipartFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for MultipartFileName {
    type Err = MultipartFileNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for MultipartFileName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MultipartFileName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(D::Error::custom))
    }
}

/// Why a multipart filename was rejected.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MultipartFileNameError {
    /// Multipart filenames cannot be empty.
    #[error("multipart filename cannot be empty")]
    Empty,
    /// Path separators, quotes, and control characters are unsafe in a
    /// multipart filename.
    #[error("multipart filename contains an unsafe character")]
    UnsafeCharacter,
}

/// A media type validated for safe use in a multipart header.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct MultipartMediaType(String);

impl MultipartMediaType {
    /// Validates a MIME type and rejects control/header injection.
    pub fn new(value: impl Into<String>) -> Result<Self, MultipartMediaTypeError> {
        let value = value.into();
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(MultipartMediaTypeError);
        }

        let essence = value
            .split_once(';')
            .map_or(value.as_str(), |(head, _)| head);
        let Some((top_level, subtype)) = essence.split_once('/') else {
            return Err(MultipartMediaTypeError);
        };
        if top_level.is_empty()
            || subtype.is_empty()
            || !top_level.chars().all(is_mime_token_character)
            || !subtype.chars().all(is_mime_token_character)
        {
            return Err(MultipartMediaTypeError);
        }

        Ok(Self(value))
    }

    /// Returns the validated media type.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the media type.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

fn is_mime_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

impl AsRef<str> for MultipartMediaType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MultipartMediaType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for MultipartMediaType {
    type Err = MultipartMediaTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for MultipartMediaType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MultipartMediaType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(D::Error::custom))
    }
}

/// A string is not a safe multipart media type.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("invalid multipart media type")]
pub struct MultipartMediaTypeError;

/// Replayable data used for one multipart field.
///
/// `Bytes` is immutable shared memory. `Path` is a descriptor that the
/// transport can reopen for each attempt. Open files, readers, and streams are
/// deliberately excluded so retries never depend on an implicit cursor.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplayableMultipartSource {
    /// Immutable bytes that can be cloned cheaply for retries.
    Bytes {
        data: Arc<[u8]>,
        file_name: Omittable<MultipartFileName>,
        media_type: Omittable<MultipartMediaType>,
    },
    /// A filesystem path that can be reopened for each attempt.
    Path {
        path: PathBuf,
        file_name: Omittable<MultipartFileName>,
        media_type: Omittable<MultipartMediaType>,
    },
}

impl ReplayableMultipartSource {
    /// Creates a replayable in-memory source.
    #[must_use]
    pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> Self {
        Self::Bytes {
            data: data.into(),
            file_name: Omittable::Omitted,
            media_type: Omittable::Omitted,
        }
    }

    /// Creates a source that the transport reopens from this path.
    ///
    /// Construction performs no filesystem access.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::Path {
            path: path.into(),
            file_name: Omittable::Omitted,
            media_type: Omittable::Omitted,
        }
    }

    /// Sets the multipart filename.
    #[must_use]
    pub fn with_file_name(mut self, file_name: MultipartFileName) -> Self {
        *self.file_name_mut() = Omittable::Value(file_name);
        self
    }

    /// Validates and sets the multipart filename.
    pub fn try_with_file_name(
        self,
        file_name: impl Into<String>,
    ) -> Result<Self, MultipartFileNameError> {
        MultipartFileName::new(file_name).map(|file_name| self.with_file_name(file_name))
    }

    /// Sets the multipart media type.
    #[must_use]
    pub fn with_media_type(mut self, media_type: MultipartMediaType) -> Self {
        *self.media_type_mut() = Omittable::Value(media_type);
        self
    }

    /// Validates and sets the multipart media type.
    pub fn try_with_media_type(
        self,
        media_type: impl Into<String>,
    ) -> Result<Self, MultipartMediaTypeError> {
        MultipartMediaType::new(media_type).map(|media_type| self.with_media_type(media_type))
    }

    /// Clears an explicit multipart filename.
    #[must_use]
    pub fn clear_file_name(mut self) -> Self {
        *self.file_name_mut() = Omittable::Omitted;
        self
    }

    /// Clears an explicit multipart media type.
    #[must_use]
    pub fn clear_media_type(mut self) -> Self {
        *self.media_type_mut() = Omittable::Omitted;
        self
    }

    /// Returns in-memory bytes, or `None` for a path source.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes { data, .. } => Some(data),
            Self::Path { .. } => None,
        }
    }

    /// Returns the path, or `None` for an in-memory source.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path { path, .. } => Some(path),
            Self::Bytes { .. } => None,
        }
    }

    /// Returns the explicit multipart filename when present.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        match self.file_name_field() {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value.as_str()),
        }
    }

    /// Returns the explicit multipart media type when present.
    #[must_use]
    pub fn media_type(&self) -> Option<&str> {
        match self.media_type_field() {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value.as_str()),
        }
    }

    fn file_name_field(&self) -> &Omittable<MultipartFileName> {
        match self {
            Self::Bytes { file_name, .. } | Self::Path { file_name, .. } => file_name,
        }
    }

    fn file_name_mut(&mut self) -> &mut Omittable<MultipartFileName> {
        match self {
            Self::Bytes { file_name, .. } | Self::Path { file_name, .. } => file_name,
        }
    }

    fn media_type_field(&self) -> &Omittable<MultipartMediaType> {
        match self {
            Self::Bytes { media_type, .. } | Self::Path { media_type, .. } => media_type,
        }
    }

    fn media_type_mut(&mut self) -> &mut Omittable<MultipartMediaType> {
        match self {
            Self::Bytes { media_type, .. } | Self::Path { media_type, .. } => media_type,
        }
    }
}

impl fmt::Debug for ReplayableMultipartSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes {
                data,
                file_name,
                media_type,
            } => formatter
                .debug_struct("ReplayableMultipartSource::Bytes")
                .field("len", &data.len())
                .field("file_name", file_name)
                .field("media_type", media_type)
                .finish(),
            Self::Path {
                path: _,
                file_name,
                media_type,
            } => formatter
                .debug_struct("ReplayableMultipartSource::Path")
                .field("path", &"[REDACTED]")
                .field("file_name", file_name)
                .field("media_type", media_type)
                .finish(),
        }
    }
}

/// Multipart request for `POST /files`.
///
/// This request deliberately has no Serde implementation because `file` is a
/// multipart binary field rather than JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateFileRequest {
    file: ReplayableMultipartSource,
    purpose: FilePurpose,
    expires_after: Omittable<FileExpirationAfter>,
}

impl CreateFileRequest {
    /// Creates a minimal multipart file request.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource, purpose: FilePurpose) -> Self {
        Self {
            file,
            purpose,
            expires_after: Omittable::Omitted,
        }
    }

    /// Adds an expiration policy.
    #[must_use]
    pub fn with_expires_after(mut self, expires_after: FileExpirationAfter) -> Self {
        self.expires_after = Omittable::Value(expires_after);
        self
    }

    /// Clears the expiration policy.
    #[must_use]
    pub fn clear_expires_after(mut self) -> Self {
        self.expires_after = Omittable::Omitted;
        self
    }

    /// Returns the replayable `file` field source.
    #[must_use]
    pub const fn file(&self) -> &ReplayableMultipartSource {
        &self.file
    }

    /// Returns the `purpose` form field.
    #[must_use]
    pub const fn purpose(&self) -> &FilePurpose {
        &self.purpose
    }

    /// Returns the exact expiration-field presence state.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<FileExpirationAfter> {
        &self.expires_after
    }
}

/// JSON body for `POST /uploads`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadRequest {
    filename: String,
    purpose: FilePurpose,
    bytes: i64,
    mime_type: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    expires_after: Omittable<FileExpirationAfter>,
}

impl CreateUploadRequest {
    /// Creates a minimal Upload request.
    #[must_use]
    pub fn new(
        filename: impl Into<String>,
        purpose: FilePurpose,
        bytes: i64,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            filename: filename.into(),
            purpose,
            bytes,
            mime_type: mime_type.into(),
            expires_after: Omittable::Omitted,
        }
    }

    /// Adds an expiration policy.
    #[must_use]
    pub fn with_expires_after(mut self, expires_after: FileExpirationAfter) -> Self {
        self.expires_after = Omittable::Value(expires_after);
        self
    }

    /// Clears the expiration policy.
    #[must_use]
    pub fn clear_expires_after(mut self) -> Self {
        self.expires_after = Omittable::Omitted;
        self
    }

    /// Returns the target filename.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Returns the file purpose.
    #[must_use]
    pub const fn purpose(&self) -> &FilePurpose {
        &self.purpose
    }

    /// Returns the declared total byte count.
    #[must_use]
    pub const fn bytes(&self) -> i64 {
        self.bytes
    }

    /// Returns the declared MIME type.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Returns the exact expiration-field presence state.
    #[must_use]
    pub const fn expires_after(&self) -> &Omittable<FileExpirationAfter> {
        &self.expires_after
    }
}

/// Multipart request for `POST /uploads/{upload_id}/parts`.
///
/// This request has no Serde implementation because `data` is a multipart
/// binary field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddUploadPartRequest {
    data: ReplayableMultipartSource,
}

impl AddUploadPartRequest {
    /// Creates a request containing one replayable byte chunk.
    #[must_use]
    pub const fn new(data: ReplayableMultipartSource) -> Self {
        Self { data }
    }

    /// Returns the replayable `data` field source.
    #[must_use]
    pub const fn data(&self) -> &ReplayableMultipartSource {
        &self.data
    }
}

/// JSON body for `POST /uploads/{upload_id}/complete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteUploadRequest {
    part_ids: Vec<UploadPartId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    md5: Omittable<String>,
}

impl CompleteUploadRequest {
    /// Creates a completion request with parts in their intended file order.
    #[must_use]
    pub fn new(part_ids: impl IntoIterator<Item = UploadPartId>) -> Self {
        Self {
            part_ids: part_ids.into_iter().collect(),
            md5: Omittable::Omitted,
        }
    }

    /// Adds an MD5 checksum string for server-side verification.
    #[must_use]
    pub fn with_md5(mut self, md5: impl Into<String>) -> Self {
        self.md5 = Omittable::Value(md5.into());
        self
    }

    /// Clears the optional MD5 checksum.
    #[must_use]
    pub fn clear_md5(mut self) -> Self {
        self.md5 = Omittable::Omitted;
        self
    }

    /// Returns part identifiers in their requested concatenation order.
    #[must_use]
    pub fn part_ids(&self) -> &[UploadPartId] {
        &self.part_ids
    }

    /// Returns the checksum when present.
    #[must_use]
    pub fn md5(&self) -> Option<&str> {
        match &self.md5 {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some(value),
        }
    }
}

/// A multipart Upload returned by create, complete, and cancel operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Upload {
    id: UploadId,
    bytes: i64,
    created_at: i64,
    expires_at: i64,
    filename: String,
    purpose: FilePurpose,
    status: UploadStatus,
    object: UploadObjectType,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    file: Omittable<Nullable<FileObject>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Upload {
    /// Returns the opaque Upload identifier.
    #[must_use]
    pub const fn id(&self) -> &UploadId {
        &self.id
    }

    /// Returns the intended total byte count.
    #[must_use]
    pub const fn bytes(&self) -> i64 {
        self.bytes
    }

    /// Returns the creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns the expiration timestamp in Unix seconds.
    #[must_use]
    pub const fn expires_at(&self) -> i64 {
        self.expires_at
    }

    /// Returns the target filename.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Returns the open file purpose string.
    #[must_use]
    pub const fn purpose(&self) -> &FilePurpose {
        &self.purpose
    }

    /// Returns the open lifecycle status.
    #[must_use]
    pub const fn status(&self) -> &UploadStatus {
        &self.status
    }

    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &UploadObjectType {
        &self.object
    }

    /// Returns the exact omitted/null/value state of the completed file.
    #[must_use]
    pub const fn file(&self) -> &Omittable<Nullable<FileObject>> {
        &self.file
    }

    /// Returns the completed file only when the service supplied an object.
    #[must_use]
    pub fn completed_file(&self) -> Option<&FileObject> {
        match &self.file {
            Omittable::Value(Nullable::Value(file)) => Some(file),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One byte chunk accepted by a multipart Upload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UploadPart {
    id: UploadPartId,
    object: UploadPartObjectType,
    created_at: i64,
    upload_id: UploadId,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl UploadPart {
    /// Returns the opaque part identifier.
    #[must_use]
    pub const fn id(&self) -> &UploadPartId {
        &self.id
    }

    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &UploadPartObjectType {
        &self.object
    }

    /// Returns the creation timestamp in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns the parent Upload identifier.
    #[must_use]
    pub const fn upload_id(&self) -> &UploadId {
        &self.upload_id
    }

    /// Returns response properties unknown to this crate version.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use proptest::prelude::*;
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::{
        AddUploadPartRequest, CompleteUploadRequest, CreateFileRequest, CreateUploadRequest,
        DeleteFileResponse, FileContent, FileExpirationAfter, FileListLimit, FileListPage,
        FileListParams, FileObject, FileObjectPurpose, FilePurpose, FileSortOrder, FileStatus,
        MAX_FILE_EXPIRATION_SECONDS, MIN_FILE_EXPIRATION_SECONDS, MultipartFileName,
        MultipartMediaType, ReplayableMultipartSource, Upload, UploadPart, UploadPartId,
        UploadStatus,
    };
    use crate::{Nullable, Omittable};

    assert_impl_all!(FileObject: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FileListParams: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FileListPage: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(DeleteFileResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CreateUploadRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CompleteUploadRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Upload: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(UploadPart: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ReplayableMultipartSource: Clone, Send, Sync);
    assert_impl_all!(MultipartFileName: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(MultipartMediaType: Serialize, DeserializeOwned, Send, Sync);
    assert_not_impl_any!(ReplayableMultipartSource: Serialize);
    assert_not_impl_any!(CreateFileRequest: Serialize);
    assert_not_impl_any!(AddUploadPartRequest: Serialize);
    assert_not_impl_any!(FileContent: Serialize);

    fn base_file_json() -> Value {
        json!({
            "id": "file-abc123",
            "object": "file",
            "bytes": 120000,
            "created_at": 1677610602,
            "filename": "training.jsonl",
            "purpose": "fine-tune",
            "status": "processed"
        })
    }

    fn base_upload_json() -> Value {
        json!({
            "id": "upload_abc123",
            "object": "upload",
            "bytes": 2147483648_u64,
            "created_at": 1719184911,
            "expires_at": 1719188511,
            "filename": "training.jsonl",
            "purpose": "fine-tune",
            "status": "pending"
        })
    }

    #[test]
    fn file_object_preserves_unknown_enums_and_extra_fields() {
        let mut wire = base_file_json();
        let object = wire.as_object_mut().expect("fixture is an object");
        object.insert(String::from("purpose"), json!("future-purpose/v2"));
        object.insert(String::from("status"), json!("quarantined"));
        object.insert(String::from("future"), json!({"nested": [1, 2, 3]}));

        let file = serde_json::from_value::<FileObject>(wire.clone()).expect("decode file");

        assert_eq!(file.purpose().unknown_value(), Some("future-purpose/v2"));
        assert_eq!(file.status().unknown_value(), Some("quarantined"));
        assert_eq!(
            file.extra_fields().get("future"),
            Some(&json!({"nested": [1, 2, 3]}))
        );
        assert_eq!(serde_json::to_value(file).expect("encode file"), wire);
    }

    #[test]
    fn file_status_is_required_even_though_deprecated() {
        let mut wire = base_file_json();
        wire.as_object_mut()
            .expect("fixture is an object")
            .remove("status");

        let error = serde_json::from_value::<FileObject>(wire)
            .expect_err("missing required status must fail");
        assert!(error.to_string().contains("missing field `status`"));
    }

    #[test]
    fn optional_file_fields_remain_omitted() {
        let file = serde_json::from_value::<FileObject>(base_file_json()).expect("decode file");

        assert_eq!(file.expires_at(), None);
        assert_eq!(file.status_details(), None);
        let encoded = serde_json::to_value(file).expect("encode file");
        assert!(encoded.get("expires_at").is_none());
        assert!(encoded.get("status_details").is_none());
    }

    #[test]
    fn response_only_purposes_do_not_become_known_create_purposes() {
        let mut wire = base_file_json();
        wire.as_object_mut()
            .expect("fixture is an object")
            .insert(String::from("purpose"), json!("assistants_output"));
        let file = serde_json::from_value::<FileObject>(wire).expect("decode output file");

        assert_eq!(file.purpose(), &FileObjectPurpose::AssistantsOutput);
        assert_eq!(
            FilePurpose::from_raw("assistants_output").unknown_value(),
            Some("assistants_output")
        );
    }

    #[test]
    fn file_list_params_build_without_manual_json() {
        let params = FileListParams::new()
            .with_purpose(FilePurpose::Batch)
            .with_limit(FileListLimit::new(25).expect("valid limit"))
            .with_order(FileSortOrder::Ascending)
            .after("file-cursor");

        assert_eq!(
            serde_json::to_value(&params).expect("encode params"),
            json!({
                "purpose": "batch",
                "limit": 25,
                "order": "asc",
                "after": "file-cursor"
            })
        );
        assert_eq!(
            serde_json::to_value(FileListParams::new()).expect("encode defaults"),
            json!({})
        );
        assert_eq!(FileListParams::new().effective_limit(), 10_000);
        assert_eq!(
            FileListParams::new().effective_order(),
            FileSortOrder::Descending
        );
    }

    #[test]
    fn file_list_limit_requires_at_least_one() {
        // The pinned schema documents "between 1 and 10,000" in prose but has
        // no `maximum`, and the official Python SDK passes the value through
        // unbounded, so only the lower bound is enforced.
        assert!(FileListLimit::new(0).is_err());
        assert!(serde_json::from_str::<FileListLimit>("0").is_err());
        assert_eq!(
            FileListLimit::new(1).expect("minimum is valid").get(),
            1_u32
        );
        assert_eq!(
            FileListLimit::new(u32::MAX)
                .expect("no invented upper bound")
                .get(),
            u32::MAX
        );
        assert_eq!(
            serde_json::from_str::<FileListLimit>("10001")
                .expect("value above the documented prose ceiling stays valid")
                .get(),
            10_001_u32
        );
    }

    #[test]
    fn list_page_and_delete_response_keep_future_fields() {
        let page_wire = json!({
            "object": "future.list",
            "data": [base_file_json()],
            "first_id": "file-first",
            "last_id": "file-last",
            "has_more": true,
            "next_region": "us-west"
        });
        let page = serde_json::from_value::<FileListPage>(page_wire.clone()).expect("decode page");
        assert_eq!(page.object().unknown_value(), Some("future.list"));
        assert_eq!(
            page.extra_fields().get("next_region"),
            Some(&json!("us-west"))
        );
        assert_eq!(serde_json::to_value(page).expect("encode page"), page_wire);

        let delete_wire = json!({
            "id": "file-old",
            "object": "future.file",
            "deleted": true,
            "audit_id": "audit-1"
        });
        let deleted = serde_json::from_value::<DeleteFileResponse>(delete_wire.clone())
            .expect("decode delete response");
        assert_eq!(deleted.object().unknown_value(), Some("future.file"));
        assert_eq!(
            serde_json::to_value(deleted).expect("encode deletion"),
            delete_wire
        );
    }

    #[test]
    fn expiration_policy_validates_range_and_shape() {
        assert!(FileExpirationAfter::new(MIN_FILE_EXPIRATION_SECONDS - 1).is_err());
        assert!(FileExpirationAfter::new(MAX_FILE_EXPIRATION_SECONDS + 1).is_err());
        let policy =
            FileExpirationAfter::new(MIN_FILE_EXPIRATION_SECONDS).expect("lower boundary is valid");
        assert_eq!(
            serde_json::to_value(&policy).expect("encode policy"),
            json!({"anchor": "created_at", "seconds": 3600})
        );
        assert!(
            serde_json::from_value::<FileExpirationAfter>(
                json!({"anchor": "created_at", "seconds": 3600, "future": true})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<FileExpirationAfter>(json!({
                "anchor": "created_at",
                "seconds": MAX_FILE_EXPIRATION_SECONDS + 1
            }))
            .is_err()
        );
    }

    #[test]
    fn multipart_source_is_replayable_and_hides_byte_contents_in_debug() {
        let secret_bytes = b"do-not-print-this-payload".to_vec();
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(secret_bytes))
            .try_with_file_name("training.jsonl")
            .expect("safe filename")
            .try_with_media_type("application/jsonl")
            .expect("safe media type");
        let cloned = source.clone();
        let debug = format!("{source:?}");

        assert_eq!(
            source.as_bytes(),
            Some(b"do-not-print-this-payload".as_slice())
        );
        assert_eq!(source, cloned);
        assert_eq!(source.file_name(), Some("training.jsonl"));
        assert!(!debug.contains("do-not-print-this-payload"));
        assert!(debug.contains("len"));

        let path = ReplayableMultipartSource::from_path("fixtures/training.jsonl");
        assert_eq!(path.path(), Some(Path::new("fixtures/training.jsonl")));
        assert_eq!(path.as_bytes(), None);
        assert!(!format!("{path:?}").contains("fixtures/training.jsonl"));
    }

    #[test]
    fn multipart_metadata_rejects_header_and_path_injection() {
        for invalid in [
            "",
            "../secret",
            r#"subdir\secret"#,
            "quoted\"name",
            "line\r\nbreak",
        ] {
            assert!(MultipartFileName::new(invalid).is_err());
            assert!(serde_json::from_value::<MultipartFileName>(json!(invalid)).is_err());
        }

        for invalid in ["", "text", " text/plain", "text/plain\r\nX-Evil: yes"] {
            assert!(MultipartMediaType::new(invalid).is_err());
            assert!(serde_json::from_value::<MultipartMediaType>(json!(invalid)).is_err());
        }

        assert!(MultipartFileName::new("训练集.jsonl").is_ok());
        assert!(MultipartMediaType::new("application/vnd.openai+json").is_ok());
    }

    #[test]
    fn multipart_requests_keep_binary_out_of_json() {
        let expiration = FileExpirationAfter::new(3_600).expect("valid expiration");
        let file = ReplayableMultipartSource::from_path("training.jsonl")
            .try_with_media_type("application/jsonl")
            .expect("safe media type");
        let request = CreateFileRequest::new(file.clone(), FilePurpose::FineTune)
            .with_expires_after(expiration);
        let part = AddUploadPartRequest::new(file);

        assert_eq!(request.file().path(), Some(Path::new("training.jsonl")));
        assert_eq!(request.purpose(), &FilePurpose::FineTune);
        assert_eq!(part.data().path(), Some(Path::new("training.jsonl")));
    }

    #[test]
    fn raw_file_content_is_not_mistaken_for_json_or_logged() {
        let content = FileContent::new(b"{not-json}\0binary-secret".as_slice());
        let debug = format!("{content:?}");

        assert_eq!(content.as_bytes(), b"{not-json}\0binary-secret");
        assert!(!debug.contains("binary-secret"));
        assert!(debug.contains("len"));
    }

    #[test]
    fn upload_create_and_complete_requests_have_strict_json() {
        let create = CreateUploadRequest::new(
            "training.jsonl",
            FilePurpose::FineTune,
            2_147_483_648,
            "text/jsonl",
        )
        .with_expires_after(FileExpirationAfter::new(3_600).expect("valid expiration"));
        assert_eq!(
            serde_json::to_value(&create).expect("encode create upload"),
            json!({
                "filename": "training.jsonl",
                "purpose": "fine-tune",
                "bytes": 2147483648_u64,
                "mime_type": "text/jsonl",
                "expires_after": {"anchor": "created_at", "seconds": 3600}
            })
        );
        assert!(
            serde_json::from_value::<CreateUploadRequest>(json!({
                "filename": "training.jsonl",
                "purpose": "fine-tune",
                "bytes": 12,
                "mime_type": "text/jsonl",
                "unexpected": true
            }))
            .is_err()
        );

        let complete = CompleteUploadRequest::new([
            UploadPartId::new("part-first"),
            UploadPartId::new("part-second"),
        ])
        .with_md5("1B2M2Y8AsgTpgAmY7PhCfg==");
        assert_eq!(
            serde_json::to_value(complete).expect("encode completion"),
            json!({
                "part_ids": ["part-first", "part-second"],
                "md5": "1B2M2Y8AsgTpgAmY7PhCfg=="
            })
        );
    }

    #[test]
    fn upload_file_preserves_omitted_null_and_value_states() {
        let omitted = serde_json::from_value::<Upload>(base_upload_json()).expect("decode omitted");

        let mut null_wire = base_upload_json();
        null_wire
            .as_object_mut()
            .expect("fixture is an object")
            .insert(String::from("file"), Value::Null);
        let null = serde_json::from_value::<Upload>(null_wire).expect("decode null");

        let mut value_wire = base_upload_json();
        value_wire
            .as_object_mut()
            .expect("fixture is an object")
            .insert(String::from("file"), base_file_json());
        let value = serde_json::from_value::<Upload>(value_wire).expect("decode file value");

        assert!(matches!(omitted.file(), Omittable::Omitted));
        assert!(matches!(null.file(), Omittable::Value(Nullable::Null)));
        assert!(matches!(value.file(), Omittable::Value(Nullable::Value(_))));
        assert!(value.completed_file().is_some());
    }

    #[test]
    fn upload_object_is_required_by_the_sdk_contract_override() {
        let mut wire = base_upload_json();
        wire.as_object_mut()
            .expect("fixture is an object")
            .remove("object");

        let error = serde_json::from_value::<Upload>(wire)
            .expect_err("official SDK contract requires upload object");
        assert!(error.to_string().contains("missing field `object`"));
    }

    #[test]
    fn upload_byte_counts_above_four_gib_round_trip() {
        let bytes = 5_i64 * 1024 * 1024 * 1024;
        let request = CreateUploadRequest::new(
            "large.bin",
            FilePurpose::UserData,
            bytes,
            "application/octet-stream",
        );
        let wire = serde_json::to_vec(&request).expect("encode large upload request");
        let decoded = serde_json::from_slice::<CreateUploadRequest>(&wire)
            .expect("decode large upload request");

        assert_eq!(decoded.bytes(), bytes);
        assert_eq!(decoded, request);
    }

    #[test]
    fn upload_and_part_preserve_unknown_values_and_fields() {
        let mut upload_wire = base_upload_json();
        let upload_object = upload_wire.as_object_mut().expect("fixture is an object");
        upload_object.insert(String::from("status"), json!("paused"));
        upload_object.insert(String::from("region"), json!("eu"));
        let upload = serde_json::from_value::<Upload>(upload_wire.clone()).expect("decode upload");
        assert_eq!(upload.status().unknown_value(), Some("paused"));
        assert_eq!(upload.extra_fields().get("region"), Some(&json!("eu")));
        assert_eq!(
            serde_json::to_value(upload).expect("encode upload"),
            upload_wire
        );

        let part_wire = json!({
            "id": "part_def456",
            "object": "future.upload.part",
            "created_at": 1719186911,
            "upload_id": "upload_abc123",
            "checksum": "future"
        });
        let part = serde_json::from_value::<UploadPart>(part_wire.clone()).expect("decode part");
        assert_eq!(part.object().unknown_value(), Some("future.upload.part"));
        assert_eq!(part.extra_fields().get("checksum"), Some(&json!("future")));
        assert_eq!(serde_json::to_value(part).expect("encode part"), part_wire);
    }

    proptest! {
        #[test]
        fn open_file_enums_preserve_arbitrary_strings(value in ".{0,128}") {
            let purpose = FilePurpose::from_raw(value.clone());
            let status = FileStatus::from_raw(value.clone());
            let upload_status = UploadStatus::from_raw(value.clone());

            let purpose_wire = serde_json::to_vec(&purpose).expect("encode purpose");
            let status_wire = serde_json::to_vec(&status).expect("encode file status");
            let upload_wire = serde_json::to_vec(&upload_status).expect("encode upload status");
            let decoded_purpose = serde_json::from_slice::<FilePurpose>(&purpose_wire)
                .expect("decode purpose");
            let decoded_status = serde_json::from_slice::<FileStatus>(&status_wire)
                .expect("decode file status");
            let decoded_upload_status = serde_json::from_slice::<UploadStatus>(&upload_wire)
                .expect("decode upload status");

            prop_assert_eq!(decoded_purpose.as_str(), value.as_str());
            prop_assert_eq!(decoded_status.as_str(), value.as_str());
            prop_assert_eq!(decoded_upload_status.as_str(), value.as_str());
        }

        #[test]
        fn list_params_round_trip(
            purpose in proptest::option::of(".{0,48}"),
            limit in proptest::option::of(1_u32..=1_000_000),
            after in proptest::option::of(".{0,48}")
        ) {
            let mut params = FileListParams::new();
            if let Some(purpose) = purpose {
                params = params.with_purpose(FilePurpose::from_raw(purpose));
            }
            if let Some(limit) = limit {
                params = params.with_limit(FileListLimit::new(limit).expect("generated limit is valid"));
            }
            if let Some(after) = after {
                params = params.after(after.as_str());
            }

            let wire = serde_json::to_vec(&params).expect("encode params");
            let decoded = serde_json::from_slice::<FileListParams>(&wire).expect("decode params");
            prop_assert_eq!(decoded, params);
        }

        #[test]
        fn complete_upload_request_round_trips(
            ids in proptest::collection::vec(".{0,32}", 0..24),
            md5 in proptest::option::of(".{0,64}")
        ) {
            let mut request = CompleteUploadRequest::new(
                ids.into_iter().map(UploadPartId::new)
            );
            if let Some(md5) = md5 {
                request = request.with_md5(md5);
            }

            let wire = serde_json::to_vec(&request).expect("encode completion request");
            let decoded = serde_json::from_slice::<CompleteUploadRequest>(&wire)
                .expect("decode completion request");
            prop_assert_eq!(decoded, request);
        }

        #[test]
        fn extra_fields_semantically_round_trip(entries in proptest::collection::btree_map("[a-z]{1,12}", any::<i64>(), 0..12)) {
            let mut wire = base_upload_json();
            let object = wire.as_object_mut().expect("fixture is an object");
            for (key, value) in entries {
                if !matches!(
                    key.as_str(),
                    "id" | "object" | "bytes" | "created_at" | "expires_at" | "filename" | "purpose" | "status" | "file"
                ) {
                    object.insert(key, Value::from(value));
                }
            }

            let upload = serde_json::from_value::<Upload>(wire.clone()).expect("decode upload");
            let encoded = serde_json::to_value(upload).expect("encode upload");
            prop_assert_eq!(encoded, wire);
        }
    }
}
