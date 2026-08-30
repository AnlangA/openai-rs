//! Stable Administration API wire DTOs.
//!
//! This module is exposed only by the crate's `admin` feature. It covers the
//! organization/project administration surface and provides a frozen operation
//! manifest so every supported route has explicit request and response schema
//! identities.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{ExtraFields, ModelId, Nullable, Omittable, WireSecret, responses::UnknownTaggedObject};

fn discriminator<'a>(value: &'a Value, field: &str) -> Result<&'a str, &'static str> {
    let Value::Object(object) = value else {
        return Err("tagged admin value must be a JSON object");
    };
    object
        .get(field)
        .ok_or("tagged admin object is missing its discriminator")?
        .as_str()
        .ok_or("tagged admin discriminator must be a string")
}

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
        field = $field:literal;
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
            /// Future variant retained as a semantic JSON object.
            Unknown(AdminUnknownObject),
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
                let tag = discriminator(&value, $field).map_err(D::Error::custom)?;
                match tag {
                    $($wire => serde_json::from_value::<$ty>(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => AdminUnknownObject::from_value(value, $field)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

/// Future tagged administration object.
#[derive(Clone, PartialEq)]
pub struct AdminUnknownObject {
    discriminator: Box<str>,
    raw: Map<String, Value>,
}

impl AdminUnknownObject {
    fn from_value(value: Value, field: &str) -> Result<Self, &'static str> {
        let discriminator = discriminator(&value, field)?.into();
        let Value::Object(raw) = value else {
            return Err("tagged admin value must be a JSON object");
        };
        Ok(Self { discriminator, raw })
    }

    /// Exact unknown discriminator.
    #[must_use]
    pub fn discriminator(&self) -> &str {
        &self.discriminator
    }

    /// Immutable complete JSON object.
    #[must_use]
    pub const fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }
}

impl fmt::Debug for AdminUnknownObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminUnknownObject")
            .field("discriminator", &self.discriminator)
            .field("field_count", &self.raw.len())
            .finish()
    }
}

impl Serialize for AdminUnknownObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

/// Feature-neutral semantic object used only where the stable schema explicitly
/// permits arbitrary properties.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdminJsonObject(Map<String, Value>);

impl AdminJsonObject {
    /// Borrow the immutable semantic object.
    #[must_use]
    pub const fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }
}

impl fmt::Debug for AdminJsonObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminJsonObject")
            .field("field_count", &self.0.len())
            .finish()
    }
}

crate::open_string_enum! {
    /// Common ascending/descending pagination order.
    pub enum AdminListOrder {
        Ascending = "asc",
        Descending = "desc"
    }
}

crate::open_string_enum! {
    /// Common administration list discriminator.
    pub enum AdminListObject {
        List = "list",
        Page = "page"
    }
}

/// Query parameters used by cursor-based administration listings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<AdminListOrder>,
}

/// Common `first_id`/`last_id` cursor page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminCursorPage<T> {
    pub object: AdminListObject,
    pub data: Vec<T>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub first_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_id: Omittable<Nullable<String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<T> AdminCursorPage<T> {
    /// Server-provided cursor for another page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Omittable::Value(Nullable::Value(id)) => Some(id),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Common `next` cursor page used by groups and roles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminNextPage<T> {
    pub object: AdminListObject,
    pub data: Vec<T>,
    pub has_more: bool,
    pub next: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<T> AdminNextPage<T> {
    /// Cursor for another page.
    #[must_use]
    pub fn next_cursor(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.next {
            Nullable::Value(next) => Some(next),
            Nullable::Null => None,
        }
    }

    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Administration API key object discriminator.
    pub enum AdminApiKeyObject {
        Key = "organization.admin_api_key"
    }
}

/// Owner summary on an Admin API key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminApiKeyOwner {
    #[serde(default, rename = "type", skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub created_at: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Redacted Admin API key metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminApiKey {
    pub object: AdminApiKeyObject,
    pub id: String,
    pub redacted_value: String,
    pub created_at: u64,
    pub expires_at: Nullable<u64>,
    pub owner: AdminApiKeyOwner,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_used_at: Omittable<Nullable<u64>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AdminApiKey {
    /// Future response fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Request to create an Admin API key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminApiKeyCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_in_seconds: Omittable<u64>,
}

impl AdminApiKeyCreateRequest {
    /// Construct a non-expiring key request.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            expires_in_seconds: Omittable::Omitted,
        }
    }
}

/// Admin API key returned once with its unredacted value.
#[derive(Clone, Serialize, Deserialize)]
pub struct AdminApiKeyCreateResponse {
    pub object: AdminApiKeyObject,
    pub id: String,
    pub redacted_value: String,
    pub created_at: u64,
    pub expires_at: Nullable<u64>,
    pub owner: AdminApiKeyOwner,
    pub value: WireSecret,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_used_at: Omittable<Nullable<u64>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl fmt::Debug for AdminApiKeyCreateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminApiKeyCreateResponse")
            .field("id", &self.id)
            .field("value", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

pub type ApiKeyList = AdminCursorPage<AdminApiKey>;

crate::open_string_enum! {
    /// Audit event type. Unknown current/future tenant events remain lossless.
    pub enum AuditEventType {
        ApiKeyCreated = "api_key.created",
        ApiKeyUpdated = "api_key.updated",
        ApiKeyDeleted = "api_key.deleted",
        CertificateCreated = "certificate.created",
        CertificateUpdated = "certificate.updated",
        CertificateDeleted = "certificate.deleted",
        GroupCreated = "group.created",
        GroupUpdated = "group.updated",
        GroupDeleted = "group.deleted",
        InviteSent = "invite.sent",
        InviteAccepted = "invite.accepted",
        InviteDeleted = "invite.deleted",
        LoginSucceeded = "login.succeeded",
        LoginFailed = "login.failed",
        ProjectCreated = "project.created",
        ProjectUpdated = "project.updated",
        ProjectArchived = "project.archived",
        ProjectDeleted = "project.deleted",
        RoleCreated = "role.created",
        RoleUpdated = "role.updated",
        RoleDeleted = "role.deleted",
        UserAdded = "user.added",
        UserUpdated = "user.updated",
        UserDeleted = "user.deleted"
    }
}

crate::open_string_enum! {
    /// Audit actor type.
    pub enum AuditActorType {
        Session = "session",
        ApiKey = "api_key"
    }
}

/// User summary within an audit actor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditActorUser {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub email: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Browser/session audit actor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditActorSession {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub ip_address: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<AuditActorUser>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// API key audit actor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditActorApiKey {
    #[serde(default, rename = "type", skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<AuditActorUser>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_account: Omittable<AdminJsonObject>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Actor that caused an audit event.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditLogActor {
    #[serde(default, rename = "type", skip_serializing_if = "Omittable::is_omitted")]
    pub kind: Omittable<AuditActorType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub session: Omittable<AuditActorSession>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key: Omittable<AuditActorApiKey>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Project summary on an audit event.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditProject {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Organization audit log entry.
///
/// Event-specific keys such as `project.created` are preserved in `extra` as
/// immutable semantic objects; the stable common envelope remains typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: AuditEventType,
    pub effective_at: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project: Omittable<AuditProject>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub actor: Omittable<Nullable<AuditLogActor>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AuditLog {
    /// Event-specific and future fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

pub type ListAuditLogsResponse = AdminCursorPage<AuditLog>;

/// Audit log filters and pagination.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditLogListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub effective_at: Omittable<BTreeMap<String, u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub event_types: Omittable<Vec<AuditEventType>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub actor_ids: Omittable<Vec<String>>,
    #[serde(flatten)]
    pub page: AdminListParams,
}

crate::open_string_enum! {
    /// Certificate resource scope discriminator.
    pub enum CertificateObject {
        Certificate = "certificate",
        Organization = "organization.certificate",
        Project = "organization.project.certificate"
    }
}

/// Certificate details; PEM content uses explicit wire-secret redaction.
#[derive(Clone, Serialize, Deserialize)]
pub struct CertificateDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub valid_at: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_at: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub content: Omittable<WireSecret>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl fmt::Debug for CertificateDetails {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateDetails")
            .field("valid_at", &self.valid_at)
            .field("expires_at", &self.expires_at)
            .field("content", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Uploaded certificate resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Certificate {
    pub object: CertificateObject,
    pub id: String,
    pub name: Nullable<String>,
    pub created_at: u64,
    pub certificate_details: CertificateDetails,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub active: Omittable<bool>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Certificate {
    /// Future fields.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Certificate upload request. Debug output never prints PEM content.
#[derive(Clone, Serialize, Deserialize)]
pub struct UploadCertificateRequest {
    pub certificate: WireSecret,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

impl fmt::Debug for UploadCertificateRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadCertificateRequest")
            .field("certificate", &"[REDACTED]")
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModifyCertificateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToggleCertificatesRequest {
    pub certificate_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CertificateScopeResponse {
    pub object: AdminListObject,
    pub data: Vec<Certificate>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ListCertificatesResponse = AdminCursorPage<Certificate>;
pub type ListProjectCertificatesResponse = AdminCursorPage<Certificate>;
pub type OrganizationCertificateActivationResponse = CertificateScopeResponse;
pub type OrganizationCertificateDeactivationResponse = CertificateScopeResponse;
pub type OrganizationProjectCertificateActivationResponse = CertificateScopeResponse;
pub type OrganizationProjectCertificateDeactivationResponse = CertificateScopeResponse;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteCertificateResponse {
    pub object: String,
    pub id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Organization/project data-retention mode.
    pub enum DataRetentionType {
        OrganizationDefault = "organization_default",
        None = "none",
        ZeroDataRetention = "zero_data_retention",
        ModifiedAbuseMonitoring = "modified_abuse_monitoring",
        EnhancedZeroDataRetention = "enhanced_zero_data_retention",
        EnhancedModifiedAbuseMonitoring = "enhanced_modified_abuse_monitoring"
    }
}

crate::open_string_enum! {
    /// Data-retention object discriminator.
    pub enum DataRetentionObject {
        Organization = "organization.data_retention",
        Project = "project.data_retention"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataRetentionResource {
    pub object: DataRetentionObject,
    #[serde(rename = "type")]
    pub retention_type: DataRetentionType,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type OrganizationDataRetention = DataRetentionResource;
pub type ProjectDataRetention = DataRetentionResource;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateOrganizationDataRetentionBody {
    pub retention_type: DataRetentionType,
}

pub type UpdateProjectDataRetentionBody = UpdateOrganizationDataRetentionBody;

crate::open_string_enum! {
    /// Group type.
    pub enum GroupType {
        Group = "group",
        TenantGroup = "tenant_group"
    }
}

crate::open_string_enum! {
    /// Group member user type.
    pub enum GroupUserType {
        User = "user",
        TenantUser = "tenant_user"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupResponse {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub is_scim_managed: bool,
    pub group_type: GroupType,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl GroupResponse {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Group response returned by update omits `group_type` in the frozen schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupResourceWithSuccess {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub is_scim_managed: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMemberUser {
    pub id: String,
    pub name: String,
    pub email: Nullable<String>,
    pub picture: Nullable<String>,
    pub is_service_account: Nullable<bool>,
    pub user_type: GroupUserType,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupBody {
    pub name: String,
}

pub type UpdateGroupBody = CreateGroupBody;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupUserBody {
    pub user_id: String,
}

crate::open_string_enum! {
    /// Assignment/deletion object discriminator.
    pub enum AssignmentObject {
        GroupUserAssignment = "organization.group.user.assignment",
        GroupRoleAssignment = "organization.group.role.assignment",
        UserRoleAssignment = "organization.user.role.assignment",
        Deleted = "organization.role.assignment.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupUserAssignment {
    pub object: AssignmentObject,
    pub user_id: String,
    pub group_id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupRoleAssignment {
    pub object: AssignmentObject,
    pub group: AdminJsonObject,
    pub role: Role,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRoleAssignment {
    pub object: AssignmentObject,
    pub user: AdminJsonObject,
    pub role: Role,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupDeletedResource {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupUserDeletedResource {
    pub object: AssignmentObject,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type GroupListResource = AdminNextPage<GroupResponse>;
pub type UserListResource = AdminNextPage<GroupMemberUser>;

crate::open_string_enum! {
    /// Organization user object discriminator.
    pub enum UserObject {
        User = "organization.user",
        ProjectUser = "organization.project.user"
    }
}

/// Organization user. The new dashboard fields are optional and lossless.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub object: UserObject,
    pub id: String,
    pub added_at: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub email: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub is_default: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub created: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<AdminJsonObject>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub is_service_account: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub is_scale_tier_authorized_purchaser: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub is_scim_managed: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key_last_used_at: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub technical_level: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub developer_persona: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub projects: Omittable<Nullable<AdminJsonObject>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl User {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserRoleUpdateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub technical_level: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub developer_persona: Omittable<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserDeleteResponse {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type UserListResponse = AdminCursorPage<User>;

crate::open_string_enum! {
    /// Role resource discriminator.
    pub enum RoleObject {
        Role = "role"
    }
}

/// Organization or project custom/predefined role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub object: RoleObject,
    pub id: String,
    pub name: String,
    pub description: Nullable<String>,
    pub permissions: Vec<String>,
    pub resource_type: String,
    pub predefined_role: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Role {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicCreateOrganizationRoleBody {
    pub role_name: String,
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublicUpdateOrganizationRoleBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role_name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub permissions: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<Nullable<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAssignOrganizationGroupRoleBody {
    pub role_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignedRoleDetails {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub resource_type: String,
    pub predefined_role: bool,
    pub description: Nullable<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub created_by: String,
    pub created_by_user_obj: AdminJsonObject,
    pub metadata: AdminJsonObject,
    pub assignment_sources: Vec<Value>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type PublicRoleListResource = AdminNextPage<Role>;
pub type RoleListResource = AdminNextPage<Role>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleDeletedResource {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedRoleAssignmentResource {
    pub object: AssignmentObject,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Organization invite role.
    pub enum InviteRole {
        Owner = "owner",
        Reader = "reader",
        Member = "member"
    }
}

crate::open_string_enum! {
    /// Organization invite state.
    pub enum InviteStatus {
        Accepted = "accepted",
        Expired = "expired",
        Pending = "pending"
    }
}

crate::open_string_enum! {
    /// Invite resource discriminator.
    pub enum InviteObject {
        Invite = "organization.invite"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub object: InviteObject,
    pub id: String,
    pub email: String,
    pub role: InviteRole,
    pub status: InviteStatus,
    pub created_at: u64,
    pub projects: Vec<AdminJsonObject>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_at: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub accepted_at: Omittable<Nullable<u64>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InviteRequest {
    pub email: String,
    pub role: InviteRole,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub projects: Omittable<Vec<AdminJsonObject>>,
}

pub type InviteListResponse = AdminCursorPage<Invite>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InviteDeleteResponse {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Project residency configuration.
    pub enum ProjectResidency {
        Global = "GLOBAL",
        UsStorageProcessing = "US_STORAGE_PROCESSING",
        EuStorageProcessing = "EU_STORAGE_PROCESSING",
        JpStorage = "JP_STORAGE",
        KrStorage = "KR_STORAGE",
        CaStorage = "CA_STORAGE",
        SgStorage = "SG_STORAGE",
        InStorage = "IN_STORAGE",
        AuStorage = "AU_STORAGE",
        GbStorage = "GB_STORAGE",
        AeStorage = "AE_STORAGE",
        AeStorageProcessing = "AE_STORAGE_PROCESSING"
    }
}

crate::open_string_enum! {
    /// Project resource discriminator.
    pub enum ProjectObject {
        Project = "organization.project"
    }
}

/// Organization project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub object: ProjectObject,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub archived_at: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub status: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub external_key_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub residency: Omittable<ProjectResidency>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Project {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub geography: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub residency: Omittable<Nullable<ProjectResidency>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub external_key_id: Omittable<Nullable<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectUpdateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub geography: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub external_key_id: Omittable<Nullable<String>>,
}

pub type ProjectListResponse = AdminCursorPage<Project>;

crate::open_string_enum! {
    /// Project group type.
    pub enum ProjectGroupType {
        Group = "group",
        TenantGroup = "tenant_group"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub object: String,
    pub project_id: String,
    pub group_id: String,
    pub group_name: String,
    pub group_type: ProjectGroupType,
    pub created_at: u64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ProjectGroupListResource = AdminNextPage<ProjectGroup>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteProjectGroupBody {
    pub group_id: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroupDeletedResource {
    pub object: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// User membership in a project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectUser {
    pub object: UserObject,
    pub id: String,
    pub role: String,
    pub added_at: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub email: Omittable<Nullable<String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectUserCreateRequest {
    pub role: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub email: Omittable<Nullable<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectUserUpdateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<String>,
}

pub type ProjectUserListResponse = AdminCursorPage<ProjectUser>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectUserDeleteResponse {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Project service-account role.
    pub enum ProjectServiceAccountRole {
        Owner = "owner",
        Member = "member",
        None = "none"
    }
}

crate::open_string_enum! {
    /// Project service-account discriminator.
    pub enum ProjectServiceAccountObject {
        Account = "organization.project.service_account"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectServiceAccount {
    pub object: ProjectServiceAccountObject,
    pub id: String,
    pub name: String,
    pub role: ProjectServiceAccountRole,
    pub created_at: u64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectServiceAccountCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub create_service_account_only: Omittable<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateProjectServiceAccountBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<ProjectServiceAccountRole>,
}

crate::open_string_enum! {
    /// Service-account API key discriminator.
    pub enum ServiceAccountApiKeyObject {
        Key = "organization.project.service_account.api_key"
    }
}

/// Unredacted service-account API key, returned only at creation.
#[derive(Clone, Serialize, Deserialize)]
pub struct ServiceAccountApiKeyBody {
    pub object: ServiceAccountApiKeyObject,
    pub value: WireSecret,
    pub name: String,
    pub created_at: u64,
    pub id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl fmt::Debug for ServiceAccountApiKeyBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceAccountApiKeyBody")
            .field("id", &self.id)
            .field("value", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

pub type ProjectServiceAccountApiKey = ServiceAccountApiKeyBody;

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectServiceAccountCreateResponse {
    pub object: ProjectServiceAccountObject,
    pub id: String,
    pub name: String,
    pub role: ProjectServiceAccountRole,
    pub created_at: u64,
    pub api_key: Nullable<ProjectServiceAccountApiKey>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl fmt::Debug for ProjectServiceAccountCreateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProjectServiceAccountCreateResponse")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("api_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateProjectServiceAccountApiKeyBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub scopes: Omittable<Vec<String>>,
}

pub type ProjectServiceAccountListResponse = AdminCursorPage<ProjectServiceAccount>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectServiceAccountDeleteResponse {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Project API-key owner access state.
    pub enum ProjectAccessState {
        Active = "active",
        Inactive = "inactive"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKey {
    pub object: String,
    pub redacted_value: String,
    pub name: String,
    pub created_at: u64,
    pub last_used_at: Nullable<u64>,
    pub id: String,
    pub owner_project_access: ProjectAccessState,
    pub owner: AdminJsonObject,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ProjectApiKeyListResponse = AdminCursorPage<ProjectApiKey>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKeyDeleteResponse {
    pub object: String,
    pub id: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Project model permission mode.
    pub enum ProjectModelPermissionMode {
        AllowList = "allow_list",
        DenyList = "deny_list"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectModelPermissions {
    pub object: String,
    pub mode: ProjectModelPermissionMode,
    pub model_ids: Vec<ModelId>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectModelPermissionsUpdateRequest {
    pub mode: ProjectModelPermissionMode,
    pub model_ids: Vec<ModelId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectModelPermissionsDeleteResponse {
    pub object: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Permission state for one hosted tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedToolPermission {
    pub enabled: bool,
}

pub type HostedToolPermissionUpdate = HostedToolPermission;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectHostedToolPermissions {
    pub file_search: HostedToolPermission,
    pub web_search: HostedToolPermission,
    pub image_generation: HostedToolPermission,
    pub mcp: HostedToolPermission,
    pub code_interpreter: HostedToolPermission,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectHostedToolPermissionsUpdateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_search: Omittable<Nullable<HostedToolPermissionUpdate>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub web_search: Omittable<Nullable<HostedToolPermissionUpdate>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_generation: Omittable<Nullable<HostedToolPermissionUpdate>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub mcp: Omittable<Nullable<HostedToolPermissionUpdate>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub code_interpreter: Omittable<Nullable<HostedToolPermissionUpdate>>,
}

crate::open_string_enum! {
    /// Project rate-limit discriminator.
    pub enum ProjectRateLimitObject {
        RateLimit = "project.rate_limit"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectRateLimit {
    pub object: ProjectRateLimitObject,
    pub id: String,
    pub model: ModelId,
    pub max_requests_per_1_minute: u64,
    pub max_tokens_per_1_minute: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_images_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_audio_megabytes_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_requests_per_1_day: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_1_day_max_input_tokens: Omittable<u64>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectRateLimitUpdateRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_requests_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_tokens_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_images_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_audio_megabytes_per_1_minute: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_requests_per_1_day: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_1_day_max_input_tokens: Omittable<u64>,
}

pub type ProjectRateLimitListResponse = AdminCursorPage<ProjectRateLimit>;

crate::open_string_enum! {
    /// Spend currency.
    pub enum SpendCurrency {
        Usd = "USD"
    }
}

crate::open_string_enum! {
    /// Spend evaluation interval.
    pub enum SpendInterval {
        Month = "month"
    }
}

crate::open_string_enum! {
    /// Spend alert notification type.
    pub enum SpendNotificationType {
        Email = "email"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendAlertNotificationChannel {
    #[serde(rename = "type")]
    pub kind: SpendNotificationType,
    pub recipients: Vec<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub subject_prefix: Omittable<Nullable<String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateSpendAlertBody {
    /// Threshold in cents.
    pub threshold_amount: u64,
    pub currency: SpendCurrency,
    pub interval: SpendInterval,
    pub notification_channel: SpendAlertNotificationChannel,
}

crate::open_string_enum! {
    /// Spend alert discriminator.
    pub enum SpendAlertObject {
        Organization = "organization.spend_alert",
        Project = "project.spend_alert"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendAlert {
    pub id: String,
    pub object: SpendAlertObject,
    pub threshold_amount: u64,
    pub currency: SpendCurrency,
    pub interval: SpendInterval,
    pub notification_channel: SpendAlertNotificationChannel,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type OrganizationSpendAlert = SpendAlert;
pub type ProjectSpendAlert = SpendAlert;
pub type OrganizationSpendAlertListResource = AdminCursorPage<SpendAlert>;
pub type ProjectSpendAlertListResource = AdminCursorPage<SpendAlert>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendAlertDeletedResource {
    pub id: String,
    pub object: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type OrganizationSpendAlertDeletedResource = SpendAlertDeletedResource;
pub type ProjectSpendAlertDeletedResource = SpendAlertDeletedResource;

crate::open_string_enum! {
    /// Spend-limit enforcement state.
    pub enum SpendLimitEnforcementStatus {
        Inactive = "inactive",
        Enforcing = "enforcing"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendLimitEnforcement {
    pub status: SpendLimitEnforcementStatus,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Spend-limit discriminator.
    pub enum SpendLimitObject {
        Organization = "organization.spend_limit",
        Project = "project.spend_limit"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendLimitResource {
    pub object: SpendLimitObject,
    pub threshold_amount: u64,
    pub currency: SpendCurrency,
    pub interval: SpendInterval,
    pub enforcement: SpendLimitEnforcement,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type OrganizationSpendLimitResource = SpendLimitResource;
pub type ProjectSpendLimitResource = SpendLimitResource;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateSpendLimitBody {
    pub threshold_amount: u64,
    pub currency: SpendCurrency,
    pub interval: SpendInterval,
}

pub type UpdateOrganizationSpendLimitBody = UpdateSpendLimitBody;
pub type UpdateProjectSpendLimitBody = UpdateSpendLimitBody;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendLimitDeletedResource {
    pub object: String,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type OrganizationSpendLimitDeletedResource = SpendLimitDeletedResource;
pub type ProjectSpendLimitDeletedResource = SpendLimitDeletedResource;
