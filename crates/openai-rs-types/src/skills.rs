//! Wire DTOs for Skills and immutable Skill Versions.
//!
//! Skill and version creation upload one zip or up to 500 directory files.
//! Those request containers reuse [`ReplayableMultipartSource`] and never
//! implement JSON Serde. Skill content is a raw HTTP body represented by
//! [`SkillContent`].

use std::{collections::HashSet, fmt};

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

/// A validated relative path retained as a multipart filename for directory
/// Skill uploads.
///
/// This deliberately differs from the generic [`crate::files::MultipartFileName`]:
/// directory uploads need `/` separators, while ordinary multipart filenames
/// must remain basenames.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeRelativeSkillPath(Box<str>);

impl SafeRelativeSkillPath {
    /// Validates a normalized, non-traversing relative path.
    pub fn new(value: impl Into<String>) -> Result<Self, SkillUploadPathError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 1024
            && !value.starts_with('/')
            && !value.ends_with('/')
            && !value.contains('\\')
            && !value.contains(':')
            && !value.contains('"')
            && !value.chars().any(char::is_control)
            && value
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        if !valid {
            return Err(SkillUploadPathError);
        }
        Ok(Self(value.into_boxed_str()))
    }

    /// Returns the normalized relative path used on the multipart wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SafeRelativeSkillPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SafeRelativeSkillPath([REDACTED])")
    }
}

/// A Skill directory path was absolute, traversing, malformed, or unsafe for a
/// multipart header.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("Skill upload path must be a normalized safe relative path")]
pub struct SkillUploadPathError;

/// A directory upload had an invalid file count or repeated relative path.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillDirectoryUploadError {
    /// The upload contained zero or more than 500 files.
    #[error(transparent)]
    FileCount(#[from] SkillUploadFileCountError),
    /// Two sources would be sent with the same relative multipart filename.
    #[error("Skill directory upload contains a duplicate relative path")]
    DuplicatePath,
}

fn validate_directory_entries(
    entries: impl IntoIterator<Item = (SafeRelativeSkillPath, ReplayableMultipartSource)>,
) -> Result<(Vec<ReplayableMultipartSource>, Vec<SafeRelativeSkillPath>), SkillDirectoryUploadError>
{
    let entries: Vec<_> = entries.into_iter().collect();
    if entries.is_empty() || entries.len() > 500 {
        return Err(SkillUploadFileCountError {
            count: entries.len(),
        }
        .into());
    }
    let mut unique = HashSet::with_capacity(entries.len());
    let mut files = Vec::with_capacity(entries.len());
    let mut paths = Vec::with_capacity(entries.len());
    for (path, file) in entries {
        if !unique.insert(path.clone()) {
            return Err(SkillDirectoryUploadError::DuplicatePath);
        }
        paths.push(path);
        files.push(file);
    }
    Ok((files, paths))
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
    relative_paths: Option<Vec<SafeRelativeSkillPath>>,
    /// Official SDK sends `files` for a scalar source and `files[]` for a list.
    files_array_field: bool,
}

impl CreateSkillRequest {
    /// Upload a single zip bundle or one directory file.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource) -> Self {
        Self {
            files: vec![file],
            relative_paths: None,
            files_array_field: false,
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
            relative_paths: None,
            files_array_field: true,
        })
    }

    /// Uploads a directory while preserving each validated relative path.
    pub fn from_directory_files(
        files: impl IntoIterator<Item = (SafeRelativeSkillPath, ReplayableMultipartSource)>,
    ) -> Result<Self, SkillDirectoryUploadError> {
        let (files, relative_paths) = validate_directory_entries(files)?;
        Ok(Self {
            files,
            relative_paths: Some(relative_paths),
            files_array_field: true,
        })
    }

    /// Ordered multipart file fields.
    #[must_use]
    pub fn files(&self) -> &[ReplayableMultipartSource] {
        &self.files
    }

    /// Relative multipart paths for a directory upload, in file order.
    #[must_use]
    pub fn relative_paths(&self) -> Option<&[SafeRelativeSkillPath]> {
        self.relative_paths.as_deref()
    }

    /// Official multipart field name: `files` for a scalar, `files[]` for a list.
    #[must_use]
    pub fn files_field_name(&self) -> &'static str {
        if self.files_array_field {
            "files[]"
        } else {
            "files"
        }
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
    relative_paths: Option<Vec<SafeRelativeSkillPath>>,
    /// Official SDK sends `files` for a scalar source and `files[]` for a list.
    files_array_field: bool,
    /// Whether this version becomes the default.
    pub default: Omittable<bool>,
}

impl CreateSkillVersionRequest {
    /// Upload a single zip bundle or one directory file.
    #[must_use]
    pub fn new(file: ReplayableMultipartSource) -> Self {
        Self {
            files: vec![file],
            relative_paths: None,
            files_array_field: false,
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
            relative_paths: None,
            files_array_field: true,
            default: Omittable::Omitted,
        })
    }

    /// Uploads a directory version while preserving validated relative paths.
    pub fn from_directory_files(
        files: impl IntoIterator<Item = (SafeRelativeSkillPath, ReplayableMultipartSource)>,
    ) -> Result<Self, SkillDirectoryUploadError> {
        let (files, relative_paths) = validate_directory_entries(files)?;
        Ok(Self {
            files,
            relative_paths: Some(relative_paths),
            files_array_field: true,
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

    /// Relative multipart paths for a directory upload, in file order.
    #[must_use]
    pub fn relative_paths(&self) -> Option<&[SafeRelativeSkillPath]> {
        self.relative_paths.as_deref()
    }

    /// Official multipart field name: `files` for a scalar, `files[]` for a list.
    #[must_use]
    pub fn files_field_name(&self) -> &'static str {
        if self.files_array_field {
            "files[]"
        } else {
            "files"
        }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::*;

    assert_impl_all!(SkillResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(SkillVersionResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(SkillListResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(SkillVersionListResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(SetDefaultSkillVersionBody: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(DeletedSkillResource: Serialize, DeserializeOwned, Send, Sync);
    assert_not_impl_any!(CreateSkillRequest: Serialize, DeserializeOwned);
    assert_not_impl_any!(CreateSkillVersionRequest: Serialize, DeserializeOwned);
    assert_not_impl_any!(SkillContent: Serialize, DeserializeOwned);

    fn ok<T, E: fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    fn source(secret: &[u8]) -> ReplayableMultipartSource {
        ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(secret))
    }

    #[test]
    fn list_params_validate_limit_and_preserve_open_order() {
        assert_eq!(ok(SkillListLimit::new(0)).get(), 0);
        assert_eq!(ok(SkillListLimit::new(100)).get(), 100);
        assert!(SkillListLimit::new(101).is_err());
        assert!(serde_json::from_str::<SkillListLimit>("101").is_err());

        let fixture = json!({"limit": 20, "order": "future", "after": "skill_1"});
        let params = ok(serde_json::from_value::<SkillListParams>(fixture.clone()));
        match &params.order {
            Omittable::Value(order) => assert_eq!(order.as_str(), "future"),
            Omittable::Omitted => panic!("fixture must contain order"),
        }
        assert_eq!(ok(serde_json::to_value(params)), fixture);
    }

    #[test]
    fn skill_and_list_responses_preserve_null_cursors_and_extras() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "id": "skill_1",
                "object": "skill",
                "name": "research",
                "description": "Search references",
                "created_at": 1,
                "default_version": "1",
                "latest_version": "2",
                "skill_future": true
            }],
            "first_id": "skill_1",
            "last_id": "skill_1",
            "has_more": true,
            "page_future": 1
        });
        let page = ok(serde_json::from_value::<SkillListResource>(fixture.clone()));
        assert_eq!(page.next_after(), Some("skill_1"));
        assert!(page.data[0].extra().contains_key("skill_future"));
        assert!(page.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(page)), fixture);

        let empty = json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": null,
            "has_more": false
        });
        let page = ok(serde_json::from_value::<SkillListResource>(empty.clone()));
        assert!(page.first_id.is_null());
        assert!(page.last_id.is_null());
        assert_eq!(ok(serde_json::to_value(page)), empty);
    }

    #[test]
    fn version_list_update_and_delete_round_trip() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "object": "skill.version",
                "id": "skillver_1",
                "skill_id": "skill_1",
                "version": "1",
                "created_at": 1,
                "name": "research",
                "description": "v1",
                "version_future": true
            }],
            "first_id": "skillver_1",
            "last_id": "skillver_1",
            "has_more": true
        });
        let page = ok(serde_json::from_value::<SkillVersionListResource>(
            fixture.clone(),
        ));
        assert_eq!(page.next_after(), Some("skillver_1"));
        assert!(page.data[0].extra().contains_key("version_future"));
        assert_eq!(ok(serde_json::to_value(page)), fixture);

        assert_eq!(
            ok(serde_json::to_value(SetDefaultSkillVersionBody::new("2"))),
            json!({"default_version": "2"})
        );

        let deleted = json!({
            "object": "skill.version.deleted",
            "deleted": true,
            "id": "skillver_1",
            "version": "1",
            "delete_future": true
        });
        let response = ok(serde_json::from_value::<DeletedSkillVersionResource>(
            deleted.clone(),
        ));
        assert!(response.extra().contains_key("delete_future"));
        assert_eq!(ok(serde_json::to_value(response)), deleted);
    }

    #[test]
    fn multipart_skill_uploads_are_bounded_replayable_and_redacted() {
        assert!(CreateSkillRequest::from_files(Vec::new()).is_err());
        assert!(CreateSkillRequest::from_files((0..501).map(|_| source(b"file"))).is_err());

        let request = ok(CreateSkillRequest::from_files([
            source(b"secret-skill-a"),
            source(b"secret-skill-b"),
        ]));
        assert_eq!(request.files().len(), 2);
        assert_eq!(request.files_field_name(), "files[]");
        assert_eq!(
            CreateSkillRequest::new(source(b"zip")).files_field_name(),
            "files"
        );
        assert_eq!(
            ok(CreateSkillRequest::from_files([source(b"one")])).files_field_name(),
            "files[]"
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("secret-skill"));

        let version = CreateSkillVersionRequest::new(source(b"secret-zip")).set_default(true);
        assert_eq!(version.files().len(), 1);
        assert!(matches!(version.default, Omittable::Value(true)));
        assert!(!format!("{version:?}").contains("secret-zip"));
    }

    #[test]
    fn skill_content_is_raw_not_json_and_has_safe_debug() {
        let content = SkillContent::new(Vec::from(b"PK\x03\x04secret").into_boxed_slice());
        assert_eq!(content.as_bytes(), b"PK\x03\x04secret");
        let debug = format!("{content:?}");
        assert!(debug.contains("len"));
        assert!(!debug.contains("secret"));
    }
}
