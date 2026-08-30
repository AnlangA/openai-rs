//! Wire DTOs for Containers and Container Files.
//!
//! Existing File API objects are attached with a JSON request. New binary files
//! use [`ReplayableMultipartSource`] and a non-Serde request container. Downloaded
//! content is a raw HTTP body represented by [`ContainerFileContent`].

use std::fmt;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use crate::{
    ExtraFields, FileId, Omittable, WireSecret,
    files::{FileContent, ReplayableMultipartSource},
    responses::UnknownTaggedObject,
    skills::{SkillId, SkillVersionNumber},
};

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

macro_rules! strict_tagged_union {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident($ty:ty) = $wire:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// Future tagged object retained with all fields.
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
                let tag = discriminator(&value).map_err(D::Error::custom)?;
                match tag {
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

fn discriminator(value: &Value) -> Result<&str, &'static str> {
    let Value::Object(object) = value else {
        return Err("tagged Container value must be a JSON object");
    };
    object
        .get("type")
        .ok_or("tagged Container object is missing string field `type`")?
        .as_str()
        .ok_or("tagged Container object field `type` must be a string")
}

fn deserialize_non_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let values = Vec::<T>::deserialize(deserializer)?;
    if values.is_empty() {
        Err(D::Error::custom("array must contain at least one item"))
    } else {
        Ok(values)
    }
}

fn deserialize_present_non_empty_vec<'de, D, T>(
    deserializer: D,
) -> Result<Omittable<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserialize_non_empty_vec(deserializer).map(Omittable::Value)
}

crate::opaque_string_id! {
    /// Opaque Container identifier.
    pub struct ContainerId;
}

crate::opaque_string_id! {
    /// Opaque Container File identifier.
    pub struct ContainerFileId;
}

crate::open_string_enum! {
    /// Container memory limit.
    pub enum ContainerMemoryLimit {
        OneGiB = "1g",
        FourGiB = "4g",
        SixteenGiB = "16g",
        SixtyFourGiB = "64g"
    }
}

crate::open_string_enum! {
    /// Container list order.
    pub enum ContainerListOrder {
        Ascending = "asc",
        Descending = "desc"
    }
}

crate::open_string_enum! {
    /// Container lifecycle status.
    pub enum ContainerStatus {
        Running = "running",
        Active = "active",
        Deleted = "deleted"
    }
}

crate::open_string_enum! {
    /// Container object discriminator.
    pub enum ContainerObject {
        Container = "container"
    }
}

crate::open_string_enum! {
    /// Container File object discriminator.
    pub enum ContainerFileObject {
        File = "container.file"
    }
}

crate::open_string_enum! {
    /// Source that produced a Container File.
    pub enum ContainerFileSource {
        User = "user",
        Assistant = "assistant"
    }
}

crate::open_string_enum! {
    /// List response discriminator.
    pub enum ContainerListObject {
        List = "list"
    }
}

crate::open_string_enum! {
    /// Container expiration anchor.
    pub enum ContainerExpirationAnchor {
        LastActiveAt = "last_active_at"
    }
}

/// Required expiration policy on a create request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateContainerExpiration {
    /// Currently `last_active_at`; future strings are retained.
    pub anchor: ContainerExpirationAnchor,
    /// Minutes after the anchor.
    pub minutes: u64,
}

impl CreateContainerExpiration {
    /// Construct a last-active expiration policy.
    #[must_use]
    pub fn after_last_active(minutes: u64) -> Self {
        Self {
            anchor: ContainerExpirationAnchor::LastActiveAt,
            minutes,
        }
    }
}

/// Expiration policy returned on a Container.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerExpiration {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub anchor: Omittable<ContainerExpirationAnchor>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub minutes: Omittable<u64>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerExpiration {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(NetworkDisabledTag, Disabled, "disabled");

/// Request policy disabling outbound networking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNetworkDisabled {
    #[serde(rename = "type")]
    kind: NetworkDisabledTag,
}

impl ContainerNetworkDisabled {
    /// Construct a disabled network policy.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kind: NetworkDisabledTag::Disabled,
        }
    }
}

impl Default for ContainerNetworkDisabled {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain-scoped secret injected into a Container.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContainerDomainSecret {
    /// Associated domain.
    pub domain: String,
    /// Injected secret name.
    pub name: String,
    /// Explicit wire secret; Debug and Display are redacted.
    pub value: WireSecret,
}

impl ContainerDomainSecret {
    /// Construct a domain-scoped secret.
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
}

literal_tag!(NetworkAllowlistTag, Allowlist, "allowlist");

/// Request policy allowing only declared outbound domains.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContainerNetworkAllowlist {
    #[serde(rename = "type")]
    kind: NetworkAllowlistTag,
    /// Non-empty allowed-domain list.
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub allowed_domains: Vec<String>,
    /// Optional domain-scoped secrets.
    #[serde(
        default,
        deserialize_with = "deserialize_present_non_empty_vec",
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub domain_secrets: Omittable<Vec<ContainerDomainSecret>>,
}

impl ContainerNetworkAllowlist {
    /// Construct an allowlist. Returns `None` for an empty list.
    #[must_use]
    pub fn new(allowed_domains: impl IntoIterator<Item = String>) -> Option<Self> {
        let allowed_domains: Vec<_> = allowed_domains.into_iter().collect();
        (!allowed_domains.is_empty()).then_some(Self {
            kind: NetworkAllowlistTag::Allowlist,
            allowed_domains,
            domain_secrets: Omittable::Omitted,
        })
    }

    /// Add one domain-scoped secret.
    #[must_use]
    pub fn with_secret(mut self, secret: ContainerDomainSecret) -> Self {
        match &mut self.domain_secrets {
            Omittable::Value(secrets) => secrets.push(secret),
            Omittable::Omitted => self.domain_secrets = Omittable::Value(vec![secret]),
        }
        self
    }
}

strict_tagged_union! {
    /// Network access policy accepted when creating a Container.
    pub enum CreateContainerNetworkPolicy {
        Disabled(ContainerNetworkDisabled) = "disabled",
        Allowlist(ContainerNetworkAllowlist) = "allowlist"
    }
}

impl From<ContainerNetworkDisabled> for CreateContainerNetworkPolicy {
    fn from(value: ContainerNetworkDisabled) -> Self {
        Self::Disabled(value)
    }
}

impl From<ContainerNetworkAllowlist> for CreateContainerNetworkPolicy {
    fn from(value: ContainerNetworkAllowlist) -> Self {
        Self::Allowlist(value)
    }
}

crate::open_string_enum! {
    /// Network mode returned by the service.
    pub enum ContainerNetworkPolicyKind {
        Disabled = "disabled",
        Allowlist = "allowlist"
    }
}

/// Network policy returned on a Container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerNetworkPolicy {
    #[serde(rename = "type")]
    pub kind: ContainerNetworkPolicyKind,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub allowed_domains: Omittable<Vec<String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerNetworkPolicy {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(SkillReferenceTag, Reference, "skill_reference");

/// Reference to a Skill created through `/skills`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerSkillReference {
    #[serde(rename = "type")]
    kind: SkillReferenceTag,
    /// Referenced Skill.
    pub skill_id: SkillId,
    /// Optional version; omission selects the Skill default.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub version: Omittable<SkillVersionNumber>,
}

impl ContainerSkillReference {
    /// Reference a Skill's default version.
    #[must_use]
    pub fn new(skill_id: impl Into<SkillId>) -> Self {
        Self {
            kind: SkillReferenceTag::Reference,
            skill_id: skill_id.into(),
            version: Omittable::Omitted,
        }
    }

    /// Select an explicit version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<SkillVersionNumber>) -> Self {
        self.version = Omittable::Value(version.into());
        self
    }
}

literal_tag!(InlineSkillSourceTag, Base64, "base64");
literal_tag!(InlineSkillMediaTypeTag, Zip, "application/zip");

/// Base64 zip source for an inline Container Skill.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineSkillSource {
    #[serde(rename = "type")]
    kind: InlineSkillSourceTag,
    media_type: InlineSkillMediaTypeTag,
    data: String,
}

impl InlineSkillSource {
    /// Encode raw zip bytes.
    #[must_use]
    pub fn from_zip_bytes(bytes: impl AsRef<[u8]>) -> Self {
        Self {
            kind: InlineSkillSourceTag::Base64,
            media_type: InlineSkillMediaTypeTag::Zip,
            data: base64::engine::general_purpose::STANDARD.encode(bytes.as_ref()),
        }
    }

    /// Decode the retained zip bytes.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.data)
    }
}

impl fmt::Debug for InlineSkillSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InlineSkillSource")
            .field("media_type", &"application/zip")
            .field("encoded_len", &self.data.len())
            .finish()
    }
}

literal_tag!(InlineSkillTag, Inline, "inline");

/// Inline zip Skill attached directly to a Container request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InlineContainerSkill {
    #[serde(rename = "type")]
    kind: InlineSkillTag,
    /// Skill name.
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Base64 zip payload.
    pub source: InlineSkillSource,
}

impl InlineContainerSkill {
    /// Construct from raw zip bytes.
    #[must_use]
    pub fn from_zip_bytes(
        name: impl Into<String>,
        description: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Self {
        Self {
            kind: InlineSkillTag::Inline,
            name: name.into(),
            description: description.into(),
            source: InlineSkillSource::from_zip_bytes(bytes),
        }
    }
}

strict_tagged_union! {
    /// Skill attached to a new Container.
    pub enum CreateContainerSkill {
        Reference(ContainerSkillReference) = "skill_reference",
        Inline(InlineContainerSkill) = "inline"
    }
}

impl From<ContainerSkillReference> for CreateContainerSkill {
    fn from(value: ContainerSkillReference) -> Self {
        Self::Reference(value)
    }
}

impl From<InlineContainerSkill> for CreateContainerSkill {
    fn from(value: InlineContainerSkill) -> Self {
        Self::Inline(value)
    }
}

/// JSON body for creating a Container.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateContainerBody {
    /// Container name.
    pub name: String,
    /// Existing File API objects copied into the Container.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_ids: Omittable<Vec<FileId>>,
    /// Expiration policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_after: Omittable<CreateContainerExpiration>,
    /// Referenced or inline Skills.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub skills: Omittable<Vec<CreateContainerSkill>>,
    /// Memory limit.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub memory_limit: Omittable<ContainerMemoryLimit>,
    /// Network policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub network_policy: Omittable<CreateContainerNetworkPolicy>,
}

impl CreateContainerBody {
    /// Construct a minimal Container request.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            file_ids: Omittable::Omitted,
            expires_after: Omittable::Omitted,
            skills: Omittable::Omitted,
            memory_limit: Omittable::Omitted,
            network_policy: Omittable::Omitted,
        }
    }

    /// Add an existing File API object.
    #[must_use]
    pub fn with_file(mut self, file_id: impl Into<FileId>) -> Self {
        match &mut self.file_ids {
            Omittable::Value(files) => files.push(file_id.into()),
            Omittable::Omitted => self.file_ids = Omittable::Value(vec![file_id.into()]),
        }
        self
    }

    /// Add a referenced or inline Skill.
    #[must_use]
    pub fn with_skill(mut self, skill: impl Into<CreateContainerSkill>) -> Self {
        match &mut self.skills {
            Omittable::Value(skills) => skills.push(skill.into()),
            Omittable::Omitted => self.skills = Omittable::Value(vec![skill.into()]),
        }
        self
    }

    /// Set Container expiration.
    #[must_use]
    pub fn with_expiration(mut self, expiration: CreateContainerExpiration) -> Self {
        self.expires_after = Omittable::Value(expiration);
        self
    }

    /// Set a network policy.
    #[must_use]
    pub fn with_network_policy(mut self, policy: impl Into<CreateContainerNetworkPolicy>) -> Self {
        self.network_policy = Omittable::Value(policy.into());
        self
    }
}

/// Container resource returned by create/retrieve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerResource {
    /// Container identifier.
    pub id: ContainerId,
    /// Object discriminator, open because the frozen schema does not constrain it.
    pub object: ContainerObject,
    /// Container name.
    pub name: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Lifecycle status.
    pub status: ContainerStatus,
    /// Last activity timestamp.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_active_at: Omittable<u64>,
    /// Expiration policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_after: Omittable<ContainerExpiration>,
    /// Effective memory limit.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub memory_limit: Omittable<ContainerMemoryLimit>,
    /// Effective network policy.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub network_policy: Omittable<ContainerNetworkPolicy>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query parameters for listing Containers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ContainerListOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<ContainerId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

/// Page of Containers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerListResource {
    pub object: ContainerListObject,
    pub data: Vec<ContainerResource>,
    pub first_id: String,
    pub last_id: String,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerListResource {
    /// Cursor for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        self.has_more.then_some(self.last_id.as_str())
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query parameters for listing Container Files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerFileListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ContainerListOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<ContainerFileId>,
}

/// JSON body attaching an existing File API object to a Container.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerFileFromIdRequest {
    /// Existing file identifier.
    pub file_id: FileId,
}

impl CreateContainerFileFromIdRequest {
    /// Construct an attach request.
    #[must_use]
    pub fn new(file_id: impl Into<FileId>) -> Self {
        Self {
            file_id: file_id.into(),
        }
    }
}

/// Multipart request uploading a new binary file to a Container.
#[derive(Clone, PartialEq)]
pub struct CreateContainerFileUploadRequest {
    file: ReplayableMultipartSource,
}

impl CreateContainerFileUploadRequest {
    /// Construct a binary upload request.
    #[must_use]
    pub const fn new(file: ReplayableMultipartSource) -> Self {
        Self { file }
    }

    /// Replayable binary source.
    #[must_use]
    pub const fn file(&self) -> &ReplayableMultipartSource {
        &self.file
    }
}

impl fmt::Debug for CreateContainerFileUploadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateContainerFileUploadRequest")
            .field("file", &self.file)
            .finish()
    }
}

/// Container File metadata returned by create/retrieve/list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerFileResource {
    pub id: ContainerFileId,
    pub object: ContainerFileObject,
    pub container_id: ContainerId,
    pub created_at: u64,
    pub bytes: u64,
    pub path: String,
    pub source: ContainerFileSource,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerFileResource {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Page of Container Files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerFileListResource {
    pub object: ContainerListObject,
    pub data: Vec<ContainerFileResource>,
    pub first_id: String,
    pub last_id: String,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ContainerFileListResource {
    /// Cursor for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        self.has_more.then_some(self.last_id.as_str())
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Raw bytes returned by the Container File content endpoint.
pub type ContainerFileContent = FileContent;

/// Empty success body returned after deleting a Container.
pub type DeleteContainerResponse = ();

/// Empty success body returned after deleting a Container File.
pub type DeleteContainerFileResponse = ();

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::json;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::*;

    assert_impl_all!(CreateContainerBody: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ContainerResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ContainerListResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ContainerFileResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ContainerFileListResource: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CreateContainerFileFromIdRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_not_impl_any!(CreateContainerFileUploadRequest: Serialize, DeserializeOwned);
    assert_not_impl_any!(ContainerFileContent: Serialize, DeserializeOwned);

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
    fn create_builder_encodes_files_skills_network_and_inline_zip() {
        let allowlist = match ContainerNetworkAllowlist::new([
            "api.example.test".to_owned(),
            "cdn.example.test".to_owned(),
        ]) {
            Some(policy) => policy.with_secret(ContainerDomainSecret::new(
                "api.example.test",
                "API_TOKEN",
                "wire-secret-value",
            )),
            None => panic!("non-empty allowlist must construct"),
        };
        let inline = InlineContainerSkill::from_zip_bytes(
            "analysis",
            "Analyze data",
            b"PK\x03\x04secret-skill",
        );
        let request = CreateContainerBody::new("workspace")
            .with_file("file_1")
            .with_skill(ContainerSkillReference::new("skill_1").with_version("2"))
            .with_skill(inline)
            .with_expiration(CreateContainerExpiration::after_last_active(30))
            .with_network_policy(allowlist);

        let value = ok(serde_json::to_value(&request));
        assert_eq!(value["file_ids"][0], "file_1");
        assert_eq!(value["skills"][0]["type"], "skill_reference");
        assert_eq!(
            value["skills"][1]["source"]["media_type"],
            "application/zip"
        );
        assert_eq!(value["network_policy"]["type"], "allowlist");
        assert_eq!(
            value["network_policy"]["domain_secrets"][0]["value"],
            "wire-secret-value"
        );
        assert_eq!(
            ok(serde_json::to_value(ok(serde_json::from_value::<
                CreateContainerBody,
            >(value.clone())))),
            value
        );

        let debug = format!("{request:?}");
        assert!(!debug.contains("wire-secret-value"));
        assert!(!debug.contains("secret-skill"));
    }

    #[test]
    fn network_and_skill_known_tags_are_strict_unknown_tags_lossless() {
        assert!(
            serde_json::from_value::<CreateContainerNetworkPolicy>(json!({
                "type": "allowlist",
                "allowed_domains": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateContainerNetworkPolicy>(json!({
                "type": "allowlist",
                "allowed_domains": ["example.test"],
                "domain_secrets": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateContainerSkill>(json!({
                "type": "skill_reference"
            }))
            .is_err()
        );

        let future_policy = json!({"type": "proxy", "proxy_url": "https://proxy.test"});
        let policy = ok(serde_json::from_value::<CreateContainerNetworkPolicy>(
            future_policy.clone(),
        ));
        assert!(matches!(policy, CreateContainerNetworkPolicy::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(policy)), future_policy);

        let future_skill = json!({"type": "registry", "name": "future"});
        let skill = ok(serde_json::from_value::<CreateContainerSkill>(
            future_skill.clone(),
        ));
        assert!(matches!(skill, CreateContainerSkill::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(skill)), future_skill);
    }

    #[test]
    fn container_response_and_page_preserve_open_values_and_extras() {
        let fixture = json!({
            "object": "list",
            "data": [{
                "id": "cntr_1",
                "object": "container",
                "name": "workspace",
                "created_at": 1,
                "status": "hibernating",
                "last_active_at": 2,
                "expires_after": {
                    "anchor": "future_anchor",
                    "minutes": 30,
                    "expiration_future": true
                },
                "network_policy": {
                    "type": "future_policy",
                    "allowed_domains": ["example.test"],
                    "network_future": true
                },
                "container_future": 7
            }],
            "first_id": "cntr_1",
            "last_id": "cntr_1",
            "has_more": true,
            "page_future": true
        });
        let page = ok(serde_json::from_value::<ContainerListResource>(
            fixture.clone(),
        ));
        assert_eq!(page.next_after(), Some("cntr_1"));
        assert_eq!(page.data[0].status.as_str(), "hibernating");
        assert!(page.data[0].extra().contains_key("container_future"));
        match &page.data[0].expires_after {
            Omittable::Value(expiration) => {
                assert!(expiration.extra().contains_key("expiration_future"));
            }
            Omittable::Omitted => panic!("fixture must contain expiration"),
        }
        match &page.data[0].network_policy {
            Omittable::Value(policy) => {
                assert_eq!(policy.kind.as_str(), "future_policy");
                assert!(policy.extra().contains_key("network_future"));
            }
            Omittable::Omitted => panic!("fixture must contain network policy"),
        }
        assert!(page.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(page)), fixture);
    }

    #[test]
    fn container_file_attach_upload_and_metadata_keep_binary_separate() {
        assert_eq!(
            ok(serde_json::to_value(CreateContainerFileFromIdRequest::new(
                "file_1"
            ))),
            json!({"file_id": "file_1"})
        );

        let upload = CreateContainerFileUploadRequest::new(source(b"secret-binary-file"));
        assert_eq!(
            upload.file().as_bytes(),
            Some(b"secret-binary-file".as_slice())
        );
        assert!(!format!("{upload:?}").contains("secret-binary-file"));

        let fixture = json!({
            "id": "cfile_1",
            "object": "container.file",
            "container_id": "cntr_1",
            "created_at": 1,
            "bytes": 123,
            "path": "/mnt/data/input.txt",
            "source": "future_source",
            "file_future": true
        });
        let file = ok(serde_json::from_value::<ContainerFileResource>(
            fixture.clone(),
        ));
        assert_eq!(file.source.as_str(), "future_source");
        assert!(file.extra().contains_key("file_future"));
        assert_eq!(ok(serde_json::to_value(file)), fixture);
    }

    #[test]
    fn container_file_page_and_raw_content_are_lossless_and_non_json() {
        let file = json!({
            "id": "cfile_1",
            "object": "container.file",
            "container_id": "cntr_1",
            "created_at": 1,
            "bytes": 3,
            "path": "/a",
            "source": "user"
        });
        let fixture = json!({
            "object": "list",
            "data": [file],
            "first_id": "cfile_1",
            "last_id": "cfile_1",
            "has_more": true,
            "list_future": true
        });
        let page = ok(serde_json::from_value::<ContainerFileListResource>(
            fixture.clone(),
        ));
        assert_eq!(page.next_after(), Some("cfile_1"));
        assert!(page.extra().contains_key("list_future"));
        assert_eq!(ok(serde_json::to_value(page)), fixture);

        let content = ContainerFileContent::new(Vec::from(b"\0\xffraw-secret").into_boxed_slice());
        assert_eq!(content.as_bytes(), b"\0\xffraw-secret");
        let debug = format!("{content:?}");
        assert!(debug.contains("len"));
        assert!(!debug.contains("raw-secret"));
    }
}
