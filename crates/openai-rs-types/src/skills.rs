//! Wire DTOs for Skills and immutable Skill Versions.
//!
//! Skill and version creation upload one zip or up to 500 directory files.
//! Those request containers reuse [`ReplayableMultipartSource`] and never
//! implement JSON Serde. Skill content is a raw HTTP body represented by
//! [`SkillContent`].

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{
    ExtraFields, Nullable, Omittable,
    files::{FileContent, ReplayableMultipartSource},
};

crate::opaque_string_id! {
    /// Opaque Skill identifier.
    pub struct SkillId;
}

crate::opaque_string_id! {
    /// Opaque immutable Skill Version resource identifier.
    pub struct SkillVersionId;
}

crate::opaque_string_id! {
    /// Version number/path segment retained as an opaque string.
    pub struct SkillVersionNumber;
}

crate::open_string_enum! {
    /// Pagination order for Skills resources.
    pub enum SkillListOrder {
        Ascending = "asc",
        Descending = "desc"
    }
}

crate::open_string_enum! {
    /// Skill/list object discriminator.
    pub enum SkillObject {
        Skill = "skill",
        List = "list"
    }
}

crate::open_string_enum! {
    /// Skill Version object discriminator.
    pub enum SkillVersionObject {
        Version = "skill.version"
    }
}

crate::open_string_enum! {
    /// Deleted Skill object discriminator.
    pub enum DeletedSkillObject {
        Deleted = "skill.deleted"
    }
}

crate::open_string_enum! {
    /// Deleted Skill Version object discriminator.
    pub enum DeletedSkillVersionObject {
        Deleted = "skill.version.deleted"
    }
}

/// Skill list limit constrained by the pinned schema to `0..=100`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SkillListLimit(u8);

impl SkillListLimit {
    /// Validate a list limit.
    pub fn new(value: u8) -> Result<Self, SkillListLimitError> {
        if value <= 100 {
            Ok(Self(value))
        } else {
            Err(SkillListLimitError { actual: value })
        }
    }

    /// Return the validated numeric value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SkillListLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Invalid Skill list limit.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("Skill list limit must be between 0 and 100, got {actual}")]
pub struct SkillListLimitError {
    actual: u8,
}

impl SkillListLimitError {
    /// Rejected value.
    #[must_use]
    pub const fn actual(self) -> u8 {
        self.actual
    }
}

/// Query parameters shared by Skill and Skill Version listings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillListParams {
    /// Requested page size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<SkillListLimit>,
    /// Sort order.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<SkillListOrder>,
    /// Cursor from the previous page.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
}

/// Stored Skill resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillResource {
    /// Skill identifier.
    pub id: SkillId,
    /// Object discriminator.
    pub object: SkillObject,
    /// Skill name parsed from its bundle.
    pub name: String,
    /// Skill description parsed from its bundle.
    pub description: String,
    /// Creation timestamp in Unix seconds.
    pub created_at: u64,
    /// Default version number.
    pub default_version: SkillVersionNumber,
    /// Latest version number.
    pub latest_version: SkillVersionNumber,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SkillResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Page of Skills.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillListResource {
    /// List discriminator.
    pub object: SkillObject,
    /// Skills on this page.
    pub data: Vec<SkillResource>,
    /// First item ID, explicitly nullable on empty pages.
    pub first_id: Nullable<String>,
    /// Last item ID, explicitly nullable on empty pages.
    pub last_id: Nullable<String>,
    /// Whether another page is available.
    pub has_more: bool,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SkillListResource {
    /// Cursor for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Nullable::Value(id) => Some(id),
            Nullable::Null => None,
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Immutable Skill Version resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillVersionResource {
    /// Version resource identifier.
    pub id: SkillVersionId,
    /// Parent Skill identifier.
    pub skill_id: SkillId,
    /// Version number/path value.
    pub version: SkillVersionNumber,
    /// Creation timestamp in Unix seconds.
    pub created_at: u64,
    /// Name parsed from this version's bundle.
    pub name: String,
    /// Description parsed from this version's bundle.
    pub description: String,
    /// Object discriminator.
    pub object: SkillVersionObject,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SkillVersionResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Page of immutable Skill Versions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillVersionListResource {
    /// List discriminator.
    pub object: SkillObject,
    /// Versions on this page.
    pub data: Vec<SkillVersionResource>,
    /// First item ID, explicitly nullable on empty pages.
    pub first_id: Nullable<String>,
    /// Last item ID, explicitly nullable on empty pages.
    pub last_id: Nullable<String>,
    /// Whether another page is available.
    pub has_more: bool,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SkillVersionListResource {
    /// Cursor for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Nullable::Value(id) => Some(id),
            Nullable::Null => None,
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Body for selecting a Skill's default version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetDefaultSkillVersionBody {
    /// Version promoted to default.
    pub default_version: SkillVersionNumber,
}

impl SetDefaultSkillVersionBody {
    /// Construct a default-version update.
    #[must_use]
    pub fn new(default_version: impl Into<SkillVersionNumber>) -> Self {
        Self {
            default_version: default_version.into(),
        }
    }
}

/// Confirmation returned after deleting a Skill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedSkillResource {
    pub object: DeletedSkillObject,
    pub deleted: bool,
    pub id: SkillId,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedSkillResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation returned after deleting a Skill Version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedSkillVersionResource {
    pub object: DeletedSkillVersionObject,
    pub deleted: bool,
    pub id: SkillVersionId,
    pub version: SkillVersionNumber,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedSkillVersionResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Invalid number of files in a Skill bundle upload.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("Skill upload requires between 1 and 500 files, got {count}")]
pub struct SkillUploadFileCountError {
    count: usize,
}

impl SkillUploadFileCountError {
    /// Rejected file count.
    #[must_use]
    pub const fn count(self) -> usize {
        self.count
    }
}

/// Multipart request for creating a Skill.
///
/// The endpoint advertises an application/json content entry, but its schema is
/// binary `FileTypes`; this request remains upload-only and has no Serde impl.
#[derive(Clone, PartialEq)]
pub struct CreateSkillRequest {
    files: Vec<ReplayableMultipartSource>,
}

impl CreateSkillRequest {
    /// Upload a single zip bundle or one directory file.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource) -> Self {
        Self { files: vec![file] }
    }

    /// Upload one to 500 directory files.
    pub fn from_files(
        files: impl IntoIterator<Item = ReplayableMultipartSource>,
    ) -> Result<Self, SkillUploadFileCountError> {
        let files: Vec<_> = files.into_iter().collect();
        if files.is_empty() || files.len() > 500 {
            return Err(SkillUploadFileCountError { count: files.len() });
        }
        Ok(Self { files })
    }

    /// Ordered multipart file fields.
    #[must_use]
    pub fn files(&self) -> &[ReplayableMultipartSource] {
        &self.files
    }
}

impl fmt::Debug for CreateSkillRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateSkillRequest")
            .field("files", &self.files)
            .finish()
    }
}

/// Multipart request for creating an immutable Skill Version.
#[derive(Clone, PartialEq)]
pub struct CreateSkillVersionRequest {
    files: Vec<ReplayableMultipartSource>,
    /// Whether this version becomes the default.
    pub default: Omittable<bool>,
}

impl CreateSkillVersionRequest {
    /// Upload a single zip bundle or one directory file.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource) -> Self {
        Self {
            files: vec![file],
            default: Omittable::Omitted,
        }
    }

    /// Upload one to 500 directory files.
    pub fn from_files(
        files: impl IntoIterator<Item = ReplayableMultipartSource>,
    ) -> Result<Self, SkillUploadFileCountError> {
        let files: Vec<_> = files.into_iter().collect();
        if files.is_empty() || files.len() > 500 {
            return Err(SkillUploadFileCountError { count: files.len() });
        }
        Ok(Self {
            files,
            default: Omittable::Omitted,
        })
    }

    /// Mark the new version as default.
    #[must_use]
    pub fn set_default(mut self, default: bool) -> Self {
        self.default = Omittable::Value(default);
        self
    }

    /// Ordered multipart file fields.
    #[must_use]
    pub fn files(&self) -> &[ReplayableMultipartSource] {
        &self.files
    }
}

impl fmt::Debug for CreateSkillVersionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateSkillVersionRequest")
            .field("files", &self.files)
            .field("default", &self.default)
            .finish()
    }
}

/// Raw zip or JSON-text content returned by Skill content endpoints.
pub type SkillContent = FileContent;

