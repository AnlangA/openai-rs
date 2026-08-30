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
        #[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerNetworkAllowlist {
    #[serde(rename = "type")]
    kind: NetworkAllowlistTag,
    /// Non-empty allowed-domain list.
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub allowed_domains: Vec<String>,
    /// Optional domain-scoped secrets.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
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
