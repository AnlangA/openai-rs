//! Stable Administration API wire DTOs.
//!
//! This module is exposed only by the crate's `admin` feature. It covers the
//! organization/project administration surface and provides a frozen operation
//! manifest so every supported route has explicit request and response schema
//! identities.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{ExtraFields, ModelId, Nullable, Omittable, WireSecret};

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

/// Cursor page whose frozen schema requires non-null first/last IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminRequiredCursorPage<T> {
    pub object: AdminListObject,
    pub data: Vec<T>,
    pub first_id: String,
    pub last_id: String,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<T> AdminRequiredCursorPage<T> {
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        self.has_more.then_some(self.last_id.as_str())
    }

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
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
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

crate::open_string_enum! {
    /// Deleted Admin API key discriminator.
    pub enum AdminApiKeyDeleteObject {
        Deleted = "organization.admin_api_key.deleted"
    }
}

/// JSON confirmation returned by `admin-api-keys-delete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminApiKeyDeleteResponse {
    pub id: String,
    pub object: AdminApiKeyDeleteObject,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AdminApiKeyDeleteResponse {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
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
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
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
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CertificateScopeResponse {
    pub object: AdminListObject,
    pub data: Vec<Certificate>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ListCertificatesResponse = AdminRequiredCursorPage<Certificate>;
pub type ListProjectCertificatesResponse = AdminRequiredCursorPage<Certificate>;
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
pub type OrganizationSpendAlertListResource = AdminRequiredCursorPage<SpendAlert>;
pub type ProjectSpendAlertListResource = AdminRequiredCursorPage<SpendAlert>;

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

crate::open_string_enum! {
    /// Usage bucket width.
    pub enum UsageBucketWidth {
        Minute = "1m",
        Hour = "1h",
        Day = "1d"
    }
}

/// Superset of stable dimensions returned when usage endpoints group results.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageDimensions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<Nullable<ModelId>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_tier: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub source: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub size: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub vector_store_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub context_level: Omittable<Nullable<String>>,
}

/// Shared query superset for Usage endpoints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageQueryParams {
    pub start_time: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub end_time: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub bucket_width: Omittable<UsageBucketWidth>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub models: Omittable<Vec<ModelId>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub group_by: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub page: Omittable<String>,
}

impl UsageQueryParams {
    /// Construct the required inclusive start timestamp.
    #[must_use]
    pub fn new(start_time: u64) -> Self {
        Self {
            start_time,
            end_time: Omittable::Omitted,
            bucket_width: Omittable::Omitted,
            project_ids: Omittable::Omitted,
            user_ids: Omittable::Omitted,
            api_key_ids: Omittable::Omitted,
            models: Omittable::Omitted,
            batch: Omittable::Omitted,
            group_by: Omittable::Omitted,
            limit: Omittable::Omitted,
            page: Omittable::Omitted,
        }
    }
}

literal_tag!(
    UsageCompletionsTag,
    Value,
    "organization.usage.completions.result"
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageCompletionsResult {
    #[serde(rename = "object")]
    kind: UsageCompletionsTag,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub num_model_requests: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_cached_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_cache_write_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_uncached_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_text_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_text_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_cached_text_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_audio_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_cached_audio_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_audio_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_image_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub input_cached_image_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub output_image_tokens: Omittable<u64>,
    #[serde(flatten)]
    pub dimensions: UsageDimensions,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

macro_rules! simple_usage_result {
    ($name:ident, $tag:ident, $wire:literal, $metric:ident, requests) => {
        literal_tag!($tag, Value, $wire);
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "object")]
            kind: $tag,
            pub $metric: u64,
            pub num_model_requests: u64,
            #[serde(flatten)]
            pub dimensions: UsageDimensions,
            #[serde(default, flatten)]
            extra: ExtraFields,
        }
    };
    ($name:ident, $tag:ident, $wire:literal, $metric:ident) => {
        literal_tag!($tag, Value, $wire);
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "object")]
            kind: $tag,
            pub $metric: u64,
            #[serde(flatten)]
            pub dimensions: UsageDimensions,
            #[serde(default, flatten)]
            extra: ExtraFields,
        }
    };
}

simple_usage_result!(
    UsageEmbeddingsResult,
    UsageEmbeddingsTag,
    "organization.usage.embeddings.result",
    input_tokens,
    requests
);
simple_usage_result!(
    UsageModerationsResult,
    UsageModerationsTag,
    "organization.usage.moderations.result",
    input_tokens,
    requests
);
simple_usage_result!(
    UsageImagesResult,
    UsageImagesTag,
    "organization.usage.images.result",
    images,
    requests
);
simple_usage_result!(
    UsageAudioSpeechesResult,
    UsageAudioSpeechesTag,
    "organization.usage.audio_speeches.result",
    characters,
    requests
);
simple_usage_result!(
    UsageAudioTranscriptionsResult,
    UsageAudioTranscriptionsTag,
    "organization.usage.audio_transcriptions.result",
    seconds,
    requests
);
simple_usage_result!(
    UsageVectorStoresResult,
    UsageVectorStoresTag,
    "organization.usage.vector_stores.result",
    usage_bytes
);
simple_usage_result!(
    UsageCodeInterpreterSessionsResult,
    UsageCodeInterpreterTag,
    "organization.usage.code_interpreter_sessions.result",
    num_sessions
);
simple_usage_result!(
    UsageFileSearchCallsResult,
    UsageFileSearchTag,
    "organization.usage.file_search_calls.result",
    num_requests
);

literal_tag!(
    UsageWebSearchTag,
    Value,
    "organization.usage.web_search_calls.result"
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageWebSearchCallsResult {
    #[serde(rename = "object")]
    kind: UsageWebSearchTag,
    pub num_model_requests: u64,
    pub num_requests: u64,
    #[serde(flatten)]
    pub dimensions: UsageDimensions,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Monetary amount in a costs result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CostAmount {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub value: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub currency: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Unit attached to an aggregated cost quantity.
    pub enum CostQuantityUnit {
        Tokens = "tokens",
        ThousandTokens = "1000_tokens",
        DurationSeconds = "duration_seconds",
        DurationMinutes = "duration_minutes",
        DurationHours = "duration_hours",
        GibibyteHours = "gibibyte_hours",
        Images = "images",
        Characters = "characters"
    }
}

literal_tag!(CostsResultTag, Value, "organization.costs.result");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostsResult {
    #[serde(rename = "object")]
    kind: CostsResultTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub amount: Omittable<CostAmount>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub line_item: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quantity: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub quantity_unit: Omittable<Nullable<CostQuantityUnit>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

strict_tagged_union! {
    field = "object";
    /// One usage/cost result inside a time bucket.
    pub enum UsageResult {
        Completions(Box<UsageCompletionsResult>) = "organization.usage.completions.result",
        Embeddings(UsageEmbeddingsResult) = "organization.usage.embeddings.result",
        Moderations(UsageModerationsResult) = "organization.usage.moderations.result",
        Images(UsageImagesResult) = "organization.usage.images.result",
        AudioSpeeches(UsageAudioSpeechesResult) = "organization.usage.audio_speeches.result",
        AudioTranscriptions(UsageAudioTranscriptionsResult) = "organization.usage.audio_transcriptions.result",
        VectorStores(UsageVectorStoresResult) = "organization.usage.vector_stores.result",
        CodeInterpreterSessions(UsageCodeInterpreterSessionsResult) = "organization.usage.code_interpreter_sessions.result",
        FileSearchCalls(UsageFileSearchCallsResult) = "organization.usage.file_search_calls.result",
        WebSearchCalls(UsageWebSearchCallsResult) = "organization.usage.web_search_calls.result",
        Costs(CostsResult) = "organization.costs.result"
    }
}

literal_tag!(UsageBucketTag, Value, "bucket");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageTimeBucket {
    #[serde(rename = "object")]
    kind: UsageBucketTag,
    pub start_time: u64,
    pub end_time: u64,
    pub results: Vec<UsageResult>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

literal_tag!(UsagePageTag, Value, "page");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageResponse {
    #[serde(rename = "object")]
    kind: UsagePageTag,
    pub data: Vec<UsageTimeBucket>,
    pub has_more: bool,
    pub next_page: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl UsageResponse {
    /// Cursor for the next usage page.
    #[must_use]
    pub fn next_page(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.next_page {
            Nullable::Value(page) => Some(page),
            Nullable::Null => None,
        }
    }

    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Standard administration error envelope used by non-success responses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: AdminJsonObject,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type SpendLimitCurrency = SpendCurrency;
pub type SpendLimitInterval = SpendInterval;

/// One frozen stable Administration operation and its body/success DTO names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdminOperationDto {
    pub operation_id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub request_schema: &'static str,
    pub response_schema: &'static str,
    pub request_mode: &'static str,
    pub response_mode: &'static str,
    pub success_statuses: &'static [u16],
    pub response_content_types: &'static [&'static str],
    pub request_schema_refs: &'static [&'static str],
    pub response_schema_refs: &'static [&'static str],
}

macro_rules! admin_op {
    ("admin-api-keys-create", $method:literal, $path:literal, $request:literal, $response:literal) => {
        AdminOperationDto {
            operation_id: "admin-api-keys-create",
            method: $method,
            path: $path,
            request_schema: $request,
            response_schema: $response,
            request_mode: "json",
            response_mode: "json",
            success_statuses: &[200],
            response_content_types: &["application/json"],
            request_schema_refs: &[],
            response_schema_refs: &[concat!("#/components/schemas/", $response)],
        }
    };
    ("admin-api-keys-delete", $method:literal, $path:literal, "()", $response:literal) => {
        AdminOperationDto {
            operation_id: "admin-api-keys-delete",
            method: $method,
            path: $path,
            request_schema: "()",
            response_schema: $response,
            request_mode: "none",
            response_mode: "json",
            success_statuses: &[200],
            response_content_types: &["application/json"],
            request_schema_refs: &[],
            response_schema_refs: &[],
        }
    };
    ($id:literal, $method:literal, $path:literal, "AdminListParams", $response:literal) => {
        admin_op!($id, $method, $path, "()", $response)
    };
    ($id:literal, $method:literal, $path:literal, "AuditLogListParams", $response:literal) => {
        admin_op!($id, $method, $path, "()", $response)
    };
    ($id:literal, $method:literal, $path:literal, "UsageQueryParams", $response:literal) => {
        admin_op!($id, $method, $path, "()", $response)
    };
    ($id:literal, $method:literal, $path:literal, "()", $response:literal) => {
        AdminOperationDto {
            operation_id: $id,
            method: $method,
            path: $path,
            request_schema: "()",
            response_schema: $response,
            request_mode: "none",
            response_mode: "json",
            success_statuses: &[200],
            response_content_types: &["application/json"],
            request_schema_refs: &[],
            response_schema_refs: &[concat!("#/components/schemas/", $response)],
        }
    };
    ($id:literal, $method:literal, $path:literal, $request:literal, $response:literal) => {
        AdminOperationDto {
            operation_id: $id,
            method: $method,
            path: $path,
            request_schema: $request,
            response_schema: $response,
            request_mode: "json",
            response_mode: "json",
            success_statuses: &[200],
            response_content_types: &["application/json"],
            request_schema_refs: &[concat!("#/components/schemas/", $request)],
            response_schema_refs: &[concat!("#/components/schemas/", $response)],
        }
    };
}

/// Complete frozen Administration operation-to-DTO manifest.
///
/// Generated from `spec/contracts/operations.json` SHA-256
/// `789d8e83ac0ac8ad2d5d44b88b4496b463bda56df730ecade45d8789429cc061`.
pub const ADMIN_OPERATION_CONTRACT_SOURCE_SHA256: &str =
    "789d8e83ac0ac8ad2d5d44b88b4496b463bda56df730ecade45d8789429cc061";

/// SHA-256 of the normalized 119-row method/path/mode/status/schema projection.
pub const ADMIN_OPERATION_CONTRACT_NORMALIZED_SHA256: &str =
    "595e0eba78c7f7a0e31f46d2eeb5f52140159491d06d284cd3c5b5882b748cf3";

pub const ADMIN_OPERATION_MANIFEST: &[AdminOperationDto] = &[
    admin_op!(
        "CreateanAPIkeyforaserviceaccount",
        "POST",
        "/organization/projects/{project_id}/service_accounts/{service_account_id}/api_keys",
        "CreateProjectServiceAccountApiKeyBody",
        "ServiceAccountApiKeyBody"
    ),
    admin_op!(
        "Deleteorganizationspendlimit",
        "DELETE",
        "/organization/spend_limit",
        "()",
        "OrganizationSpendLimitDeletedResource"
    ),
    admin_op!(
        "Deleteprojectspendlimit",
        "DELETE",
        "/organization/projects/{project_id}/spend_limit",
        "()",
        "ProjectSpendLimitDeletedResource"
    ),
    admin_op!(
        "Getorganizationspendlimit",
        "GET",
        "/organization/spend_limit",
        "()",
        "OrganizationSpendLimitResource"
    ),
    admin_op!(
        "Getprojectspendlimit",
        "GET",
        "/organization/projects/{project_id}/spend_limit",
        "()",
        "ProjectSpendLimitResource"
    ),
    admin_op!(
        "Updateorganizationspendlimit",
        "POST",
        "/organization/spend_limit",
        "UpdateOrganizationSpendLimitBody",
        "OrganizationSpendLimitResource"
    ),
    admin_op!(
        "Updateprojectspendlimit",
        "POST",
        "/organization/projects/{project_id}/spend_limit",
        "UpdateProjectSpendLimitBody",
        "ProjectSpendLimitResource"
    ),
    admin_op!(
        "activateOrganizationCertificates",
        "POST",
        "/organization/certificates/activate",
        "ToggleCertificatesRequest",
        "OrganizationCertificateActivationResponse"
    ),
    admin_op!(
        "activateProjectCertificates",
        "POST",
        "/organization/projects/{project_id}/certificates/activate",
        "ToggleCertificatesRequest",
        "OrganizationProjectCertificateActivationResponse"
    ),
    admin_op!(
        "add-group-user",
        "POST",
        "/organization/groups/{group_id}/users",
        "CreateGroupUserBody",
        "GroupUserAssignment"
    ),
    admin_op!(
        "add-project-group",
        "POST",
        "/organization/projects/{project_id}/groups",
        "InviteProjectGroupBody",
        "ProjectGroup"
    ),
    admin_op!(
        "admin-api-keys-create",
        "POST",
        "/organization/admin_api_keys",
        "AdminApiKeyCreateRequest",
        "AdminApiKeyCreateResponse"
    ),
    admin_op!(
        "admin-api-keys-delete",
        "DELETE",
        "/organization/admin_api_keys/{key_id}",
        "()",
        "AdminApiKeyDeleteResponse"
    ),
    admin_op!(
        "admin-api-keys-get",
        "GET",
        "/organization/admin_api_keys/{key_id}",
        "()",
        "AdminApiKey"
    ),
    admin_op!(
        "admin-api-keys-list",
        "GET",
        "/organization/admin_api_keys",
        "AdminListParams",
        "ApiKeyList"
    ),
    admin_op!(
        "archive-project",
        "POST",
        "/organization/projects/{project_id}/archive",
        "()",
        "Project"
    ),
    admin_op!(
        "assign-group-role",
        "POST",
        "/organization/groups/{group_id}/roles",
        "PublicAssignOrganizationGroupRoleBody",
        "GroupRoleAssignment"
    ),
    admin_op!(
        "assign-project-group-role",
        "POST",
        "/projects/{project_id}/groups/{group_id}/roles",
        "PublicAssignOrganizationGroupRoleBody",
        "GroupRoleAssignment"
    ),
    admin_op!(
        "assign-project-user-role",
        "POST",
        "/projects/{project_id}/users/{user_id}/roles",
        "PublicAssignOrganizationGroupRoleBody",
        "UserRoleAssignment"
    ),
    admin_op!(
        "assign-user-role",
        "POST",
        "/organization/users/{user_id}/roles",
        "PublicAssignOrganizationGroupRoleBody",
        "UserRoleAssignment"
    ),
    admin_op!(
        "create-group",
        "POST",
        "/organization/groups",
        "CreateGroupBody",
        "GroupResponse"
    ),
    admin_op!(
        "create-organization-spend-alert",
        "POST",
        "/organization/spend_alerts",
        "CreateSpendAlertBody",
        "OrganizationSpendAlert"
    ),
    admin_op!(
        "create-project",
        "POST",
        "/organization/projects",
        "ProjectCreateRequest",
        "Project"
    ),
    admin_op!(
        "create-project-role",
        "POST",
        "/projects/{project_id}/roles",
        "PublicCreateOrganizationRoleBody",
        "Role"
    ),
    admin_op!(
        "create-project-service-account",
        "POST",
        "/organization/projects/{project_id}/service_accounts",
        "ProjectServiceAccountCreateRequest",
        "ProjectServiceAccountCreateResponse"
    ),
    admin_op!(
        "create-project-spend-alert",
        "POST",
        "/organization/projects/{project_id}/spend_alerts",
        "CreateSpendAlertBody",
        "ProjectSpendAlert"
    ),
    admin_op!(
        "create-project-user",
        "POST",
        "/organization/projects/{project_id}/users",
        "ProjectUserCreateRequest",
        "ProjectUser"
    ),
    admin_op!(
        "create-role",
        "POST",
        "/organization/roles",
        "PublicCreateOrganizationRoleBody",
        "Role"
    ),
    admin_op!(
        "deactivateOrganizationCertificates",
        "POST",
        "/organization/certificates/deactivate",
        "ToggleCertificatesRequest",
        "OrganizationCertificateDeactivationResponse"
    ),
    admin_op!(
        "deactivateProjectCertificates",
        "POST",
        "/organization/projects/{project_id}/certificates/deactivate",
        "ToggleCertificatesRequest",
        "OrganizationProjectCertificateDeactivationResponse"
    ),
    admin_op!(
        "delete-group",
        "DELETE",
        "/organization/groups/{group_id}",
        "()",
        "GroupDeletedResource"
    ),
    admin_op!(
        "delete-invite",
        "DELETE",
        "/organization/invites/{invite_id}",
        "()",
        "InviteDeleteResponse"
    ),
    admin_op!(
        "delete-organization-spend-alert",
        "DELETE",
        "/organization/spend_alerts/{alert_id}",
        "()",
        "OrganizationSpendAlertDeletedResource"
    ),
    admin_op!(
        "delete-project-api-key",
        "DELETE",
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        "()",
        "ProjectApiKeyDeleteResponse"
    ),
    admin_op!(
        "delete-project-model-permissions",
        "DELETE",
        "/organization/projects/{project_id}/model_permissions",
        "()",
        "ProjectModelPermissionsDeleteResponse"
    ),
    admin_op!(
        "delete-project-role",
        "DELETE",
        "/projects/{project_id}/roles/{role_id}",
        "()",
        "RoleDeletedResource"
    ),
    admin_op!(
        "delete-project-service-account",
        "DELETE",
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        "()",
        "ProjectServiceAccountDeleteResponse"
    ),
    admin_op!(
        "delete-project-spend-alert",
        "DELETE",
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        "()",
        "ProjectSpendAlertDeletedResource"
    ),
    admin_op!(
        "delete-project-user",
        "DELETE",
        "/organization/projects/{project_id}/users/{user_id}",
        "()",
        "ProjectUserDeleteResponse"
    ),
    admin_op!(
        "delete-role",
        "DELETE",
        "/organization/roles/{role_id}",
        "()",
        "RoleDeletedResource"
    ),
    admin_op!(
        "delete-user",
        "DELETE",
        "/organization/users/{user_id}",
        "()",
        "UserDeleteResponse"
    ),
    admin_op!(
        "deleteCertificate",
        "DELETE",
        "/organization/certificates/{certificate_id}",
        "()",
        "DeleteCertificateResponse"
    ),
    admin_op!(
        "getCertificate",
        "GET",
        "/organization/certificates/{certificate_id}",
        "()",
        "Certificate"
    ),
    admin_op!(
        "inviteUser",
        "POST",
        "/organization/invites",
        "InviteRequest",
        "Invite"
    ),
    admin_op!(
        "list-audit-logs",
        "GET",
        "/organization/audit_logs",
        "AuditLogListParams",
        "ListAuditLogsResponse"
    ),
    admin_op!(
        "list-group-role-assignments",
        "GET",
        "/organization/groups/{group_id}/roles",
        "AdminListParams",
        "RoleListResource"
    ),
    admin_op!(
        "list-group-users",
        "GET",
        "/organization/groups/{group_id}/users",
        "AdminListParams",
        "UserListResource"
    ),
    admin_op!(
        "list-groups",
        "GET",
        "/organization/groups",
        "AdminListParams",
        "GroupListResource"
    ),
    admin_op!(
        "list-invites",
        "GET",
        "/organization/invites",
        "AdminListParams",
        "InviteListResponse"
    ),
    admin_op!(
        "list-organization-spend-alerts",
        "GET",
        "/organization/spend_alerts",
        "AdminListParams",
        "OrganizationSpendAlertListResource"
    ),
    admin_op!(
        "list-project-api-keys",
        "GET",
        "/organization/projects/{project_id}/api_keys",
        "AdminListParams",
        "ProjectApiKeyListResponse"
    ),
    admin_op!(
        "list-project-group-role-assignments",
        "GET",
        "/projects/{project_id}/groups/{group_id}/roles",
        "AdminListParams",
        "RoleListResource"
    ),
    admin_op!(
        "list-project-groups",
        "GET",
        "/organization/projects/{project_id}/groups",
        "AdminListParams",
        "ProjectGroupListResource"
    ),
    admin_op!(
        "list-project-rate-limits",
        "GET",
        "/organization/projects/{project_id}/rate_limits",
        "AdminListParams",
        "ProjectRateLimitListResponse"
    ),
    admin_op!(
        "list-project-roles",
        "GET",
        "/projects/{project_id}/roles",
        "AdminListParams",
        "PublicRoleListResource"
    ),
    admin_op!(
        "list-project-service-accounts",
        "GET",
        "/organization/projects/{project_id}/service_accounts",
        "AdminListParams",
        "ProjectServiceAccountListResponse"
    ),
    admin_op!(
        "list-project-spend-alerts",
        "GET",
        "/organization/projects/{project_id}/spend_alerts",
        "AdminListParams",
        "ProjectSpendAlertListResource"
    ),
    admin_op!(
        "list-project-user-role-assignments",
        "GET",
        "/projects/{project_id}/users/{user_id}/roles",
        "AdminListParams",
        "RoleListResource"
    ),
    admin_op!(
        "list-project-users",
        "GET",
        "/organization/projects/{project_id}/users",
        "AdminListParams",
        "ProjectUserListResponse"
    ),
    admin_op!(
        "list-projects",
        "GET",
        "/organization/projects",
        "AdminListParams",
        "ProjectListResponse"
    ),
    admin_op!(
        "list-roles",
        "GET",
        "/organization/roles",
        "AdminListParams",
        "PublicRoleListResource"
    ),
    admin_op!(
        "list-user-role-assignments",
        "GET",
        "/organization/users/{user_id}/roles",
        "AdminListParams",
        "RoleListResource"
    ),
    admin_op!(
        "list-users",
        "GET",
        "/organization/users",
        "AdminListParams",
        "UserListResponse"
    ),
    admin_op!(
        "listOrganizationCertificates",
        "GET",
        "/organization/certificates",
        "AdminListParams",
        "ListCertificatesResponse"
    ),
    admin_op!(
        "listProjectCertificates",
        "GET",
        "/organization/projects/{project_id}/certificates",
        "AdminListParams",
        "ListProjectCertificatesResponse"
    ),
    admin_op!(
        "modify-project",
        "POST",
        "/organization/projects/{project_id}",
        "ProjectUpdateRequest",
        "Project"
    ),
    admin_op!(
        "modify-project-user",
        "POST",
        "/organization/projects/{project_id}/users/{user_id}",
        "ProjectUserUpdateRequest",
        "ProjectUser"
    ),
    admin_op!(
        "modify-user",
        "POST",
        "/organization/users/{user_id}",
        "UserRoleUpdateRequest",
        "User"
    ),
    admin_op!(
        "modifyCertificate",
        "POST",
        "/organization/certificates/{certificate_id}",
        "ModifyCertificateRequest",
        "Certificate"
    ),
    admin_op!(
        "remove-group-user",
        "DELETE",
        "/organization/groups/{group_id}/users/{user_id}",
        "()",
        "GroupUserDeletedResource"
    ),
    admin_op!(
        "remove-project-group",
        "DELETE",
        "/organization/projects/{project_id}/groups/{group_id}",
        "()",
        "ProjectGroupDeletedResource"
    ),
    admin_op!(
        "retrieve-group",
        "GET",
        "/organization/groups/{group_id}",
        "()",
        "GroupResponse"
    ),
    admin_op!(
        "retrieve-group-role",
        "GET",
        "/organization/groups/{group_id}/roles/{role_id}",
        "()",
        "AssignedRoleDetails"
    ),
    admin_op!(
        "retrieve-group-user",
        "GET",
        "/organization/groups/{group_id}/users/{user_id}",
        "()",
        "GroupMemberUser"
    ),
    admin_op!(
        "retrieve-invite",
        "GET",
        "/organization/invites/{invite_id}",
        "()",
        "Invite"
    ),
    admin_op!(
        "retrieve-organization-data-retention",
        "GET",
        "/organization/data_retention",
        "()",
        "OrganizationDataRetention"
    ),
    admin_op!(
        "retrieve-organization-spend-alert",
        "GET",
        "/organization/spend_alerts/{alert_id}",
        "()",
        "OrganizationSpendAlert"
    ),
    admin_op!(
        "retrieve-project",
        "GET",
        "/organization/projects/{project_id}",
        "()",
        "Project"
    ),
    admin_op!(
        "retrieve-project-api-key",
        "GET",
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        "()",
        "ProjectApiKey"
    ),
    admin_op!(
        "retrieve-project-data-retention",
        "GET",
        "/organization/projects/{project_id}/data_retention",
        "()",
        "ProjectDataRetention"
    ),
    admin_op!(
        "retrieve-project-group",
        "GET",
        "/organization/projects/{project_id}/groups/{group_id}",
        "()",
        "ProjectGroup"
    ),
    admin_op!(
        "retrieve-project-group-role",
        "GET",
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        "()",
        "AssignedRoleDetails"
    ),
    admin_op!(
        "retrieve-project-hosted-tool-permissions",
        "GET",
        "/organization/projects/{project_id}/hosted_tool_permissions",
        "()",
        "ProjectHostedToolPermissions"
    ),
    admin_op!(
        "retrieve-project-model-permissions",
        "GET",
        "/organization/projects/{project_id}/model_permissions",
        "()",
        "ProjectModelPermissions"
    ),
    admin_op!(
        "retrieve-project-role",
        "GET",
        "/projects/{project_id}/roles/{role_id}",
        "()",
        "Role"
    ),
    admin_op!(
        "retrieve-project-service-account",
        "GET",
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        "()",
        "ProjectServiceAccount"
    ),
    admin_op!(
        "retrieve-project-spend-alert",
        "GET",
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        "()",
        "ProjectSpendAlert"
    ),
    admin_op!(
        "retrieve-project-user",
        "GET",
        "/organization/projects/{project_id}/users/{user_id}",
        "()",
        "ProjectUser"
    ),
    admin_op!(
        "retrieve-project-user-role",
        "GET",
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        "()",
        "AssignedRoleDetails"
    ),
    admin_op!(
        "retrieve-role",
        "GET",
        "/organization/roles/{role_id}",
        "()",
        "Role"
    ),
    admin_op!(
        "retrieve-user",
        "GET",
        "/organization/users/{user_id}",
        "()",
        "User"
    ),
    admin_op!(
        "retrieve-user-role",
        "GET",
        "/organization/users/{user_id}/roles/{role_id}",
        "()",
        "AssignedRoleDetails"
    ),
    admin_op!(
        "unassign-group-role",
        "DELETE",
        "/organization/groups/{group_id}/roles/{role_id}",
        "()",
        "DeletedRoleAssignmentResource"
    ),
    admin_op!(
        "unassign-project-group-role",
        "DELETE",
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        "()",
        "DeletedRoleAssignmentResource"
    ),
    admin_op!(
        "unassign-project-user-role",
        "DELETE",
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        "()",
        "DeletedRoleAssignmentResource"
    ),
    admin_op!(
        "unassign-user-role",
        "DELETE",
        "/organization/users/{user_id}/roles/{role_id}",
        "()",
        "DeletedRoleAssignmentResource"
    ),
    admin_op!(
        "update-group",
        "POST",
        "/organization/groups/{group_id}",
        "UpdateGroupBody",
        "GroupResourceWithSuccess"
    ),
    admin_op!(
        "update-organization-data-retention",
        "POST",
        "/organization/data_retention",
        "UpdateOrganizationDataRetentionBody",
        "OrganizationDataRetention"
    ),
    admin_op!(
        "update-organization-spend-alert",
        "POST",
        "/organization/spend_alerts/{alert_id}",
        "CreateSpendAlertBody",
        "OrganizationSpendAlert"
    ),
    admin_op!(
        "update-project-data-retention",
        "POST",
        "/organization/projects/{project_id}/data_retention",
        "UpdateProjectDataRetentionBody",
        "ProjectDataRetention"
    ),
    admin_op!(
        "update-project-hosted-tool-permissions",
        "POST",
        "/organization/projects/{project_id}/hosted_tool_permissions",
        "ProjectHostedToolPermissionsUpdateRequest",
        "ProjectHostedToolPermissions"
    ),
    admin_op!(
        "update-project-model-permissions",
        "POST",
        "/organization/projects/{project_id}/model_permissions",
        "ProjectModelPermissionsUpdateRequest",
        "ProjectModelPermissions"
    ),
    admin_op!(
        "update-project-rate-limits",
        "POST",
        "/organization/projects/{project_id}/rate_limits/{rate_limit_id}",
        "ProjectRateLimitUpdateRequest",
        "ProjectRateLimit"
    ),
    admin_op!(
        "update-project-role",
        "POST",
        "/projects/{project_id}/roles/{role_id}",
        "PublicUpdateOrganizationRoleBody",
        "Role"
    ),
    admin_op!(
        "update-project-service-account",
        "POST",
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        "UpdateProjectServiceAccountBody",
        "ProjectServiceAccount"
    ),
    admin_op!(
        "update-project-spend-alert",
        "POST",
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        "CreateSpendAlertBody",
        "ProjectSpendAlert"
    ),
    admin_op!(
        "update-role",
        "POST",
        "/organization/roles/{role_id}",
        "PublicUpdateOrganizationRoleBody",
        "Role"
    ),
    admin_op!(
        "uploadCertificate",
        "POST",
        "/organization/certificates",
        "UploadCertificateRequest",
        "Certificate"
    ),
    admin_op!(
        "usage-audio-speeches",
        "GET",
        "/organization/usage/audio_speeches",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-audio-transcriptions",
        "GET",
        "/organization/usage/audio_transcriptions",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-code-interpreter-sessions",
        "GET",
        "/organization/usage/code_interpreter_sessions",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-completions",
        "GET",
        "/organization/usage/completions",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-costs",
        "GET",
        "/organization/costs",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-embeddings",
        "GET",
        "/organization/usage/embeddings",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-file-search-calls",
        "GET",
        "/organization/usage/file_search_calls",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-images",
        "GET",
        "/organization/usage/images",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-moderations",
        "GET",
        "/organization/usage/moderations",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-vector-stores",
        "GET",
        "/organization/usage/vector_stores",
        "UsageQueryParams",
        "UsageResponse"
    ),
    admin_op!(
        "usage-web-search-calls",
        "GET",
        "/organization/usage/web_search_calls",
        "UsageQueryParams",
        "UsageResponse"
    ),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(AdminApiKey: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(AdminApiKeyCreateResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(AuditLog: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Certificate: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(User: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Role: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Invite: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Project: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ProjectRateLimit: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ProjectHostedToolPermissions: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(SpendAlert: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(UsageResponse: Serialize, DeserializeOwned, Send, Sync);

    fn ok<T, E: fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    #[test]
    fn operation_manifest_covers_every_frozen_admin_operation_once() {
        assert_eq!(ADMIN_OPERATION_MANIFEST.len(), 119);
        let mut ids = HashSet::new();
        let mut method_paths = HashSet::new();
        for operation in ADMIN_OPERATION_MANIFEST {
            assert!(ids.insert(operation.operation_id));
            assert!(method_paths.insert((operation.method, operation.path)));
            assert!(matches!(operation.method, "GET" | "POST" | "DELETE"));
            assert!(operation.path.starts_with('/'));
            assert!(!operation.request_schema.is_empty());
            assert!(!operation.response_schema.is_empty());
            assert!(matches!(operation.request_mode, "none" | "json"));
            assert_eq!(operation.response_mode, "json");
            assert_eq!(operation.success_statuses, &[200]);
            assert_eq!(operation.response_content_types, &["application/json"]);
            if operation.request_mode == "none" || operation.operation_id == "admin-api-keys-create"
            {
                assert!(operation.request_schema_refs.is_empty());
            } else {
                assert_eq!(operation.request_schema_refs.len(), 1);
            }
            if operation.operation_id == "admin-api-keys-delete" {
                assert!(operation.response_schema_refs.is_empty());
            } else {
                assert_eq!(operation.response_schema_refs.len(), 1);
            }
        }
        assert!(ids.contains("admin-api-keys-create"));
        assert!(ids.contains("list-audit-logs"));
        assert!(ids.contains("usage-costs"));
        assert!(ids.contains("update-project-hosted-tool-permissions"));
        let delete_key = ADMIN_OPERATION_MANIFEST
            .iter()
            .find(|operation| operation.operation_id == "admin-api-keys-delete")
            .expect("delete-key contract");
        assert_eq!(delete_key.response_schema, "AdminApiKeyDeleteResponse");
    }

    #[test]
    fn admin_key_requiredness_and_secret_redaction_are_exact() {
        let fixture = json!({
            "object": "organization.admin_api_key",
            "id": "key_1",
            "name": null,
            "redacted_value": "sk-admin...xyz",
            "created_at": 1,
            "expires_at": null,
            "owner": {},
            "value": "sk-admin-secret",
            "key_future": true
        });
        let response = ok(serde_json::from_value::<AdminApiKeyCreateResponse>(
            fixture.clone(),
        ));
        let debug = format!("{response:?}");
        assert!(!debug.contains("sk-admin-secret"));
        assert_eq!(ok(serde_json::to_value(response)), fixture);

        let mut missing = fixture;
        match &mut missing {
            Value::Object(object) => {
                object.remove("expires_at");
            }
            _ => panic!("fixture must be object"),
        }
        assert!(serde_json::from_value::<AdminApiKeyCreateResponse>(missing).is_err());
    }

    #[test]
    fn certificate_pem_is_wire_serializable_but_never_debugged() {
        let upload = UploadCertificateRequest {
            certificate: WireSecret::from("-----BEGIN CERTIFICATE----- secret"),
            name: Omittable::Value("client".to_owned()),
        };
        let encoded = ok(serde_json::to_value(&upload));
        assert_eq!(encoded["certificate"], "-----BEGIN CERTIFICATE----- secret");
        assert!(!format!("{upload:?}").contains("secret"));

        let fixture = json!({
            "object": "certificate",
            "id": "cert_1",
            "name": null,
            "created_at": 1,
            "certificate_details": {
                "valid_at": 1,
                "expires_at": 2,
                "content": "PEM-secret",
                "details_future": true
            },
            "certificate_future": true
        });
        let certificate = ok(serde_json::from_value::<Certificate>(fixture.clone()));
        assert!(!format!("{certificate:?}").contains("PEM-secret"));
        assert!(certificate.extra().contains_key("certificate_future"));
        assert_eq!(ok(serde_json::to_value(certificate)), fixture);
    }

    #[test]
    fn audit_common_envelope_and_event_specific_payload_are_lossless() {
        let fixture = json!({
            "id": "audit_1",
            "type": "tenant.policy.updated",
            "effective_at": 10,
            "actor": null,
            "project": {"id": "proj_1", "name": "default"},
            "tenant.policy.updated": {
                "id": "policy_1",
                "changes_requested": {"mode": "strict"}
            },
            "audit_future": true
        });
        let audit = ok(serde_json::from_value::<AuditLog>(fixture.clone()));
        assert_eq!(audit.kind.as_str(), "tenant.policy.updated");
        assert!(matches!(audit.actor, Omittable::Value(Nullable::Null)));
        assert!(audit.extra().contains_key("tenant.policy.updated"));
        assert!(audit.extra().contains_key("audit_future"));
        assert_eq!(ok(serde_json::to_value(audit)), fixture);
    }

    #[test]
    fn projects_permissions_and_updates_preserve_three_states() {
        let fixture = json!({
            "id": "proj_1",
            "object": "organization.project",
            "created_at": 1,
            "name": null,
            "status": "future_status",
            "residency": "MARS_STORAGE",
            "project_future": 1
        });
        let project = ok(serde_json::from_value::<Project>(fixture.clone()));
        assert!(matches!(project.name, Omittable::Value(Nullable::Null)));
        match &project.residency {
            Omittable::Value(value) => assert_eq!(value.as_str(), "MARS_STORAGE"),
            Omittable::Omitted => panic!("fixture must contain residency"),
        }
        assert!(project.extra().contains_key("project_future"));
        assert_eq!(ok(serde_json::to_value(project)), fixture);

        let missing = ok(serde_json::from_value::<
            ProjectHostedToolPermissionsUpdateRequest,
        >(json!({})));
        assert!(missing.file_search.is_omitted());
        let null = ok(serde_json::from_value::<
            ProjectHostedToolPermissionsUpdateRequest,
        >(json!({"file_search": null})));
        assert!(matches!(null.file_search, Omittable::Value(Nullable::Null)));

        assert!(
            serde_json::from_value::<ProjectModelPermissionsUpdateRequest>(json!({
                "mode": "allow_list"
            }))
            .is_err()
        );
    }

    #[test]
    fn role_invite_and_rate_limit_requiredness_are_typed() {
        let role_fixture = json!({
            "object": "role",
            "id": "role_1",
            "name": "auditor",
            "description": null,
            "permissions": ["api.usage.read"],
            "resource_type": "organization",
            "predefined_role": false,
            "role_future": true
        });
        let role = ok(serde_json::from_value::<Role>(role_fixture.clone()));
        assert!(role.extra().contains_key("role_future"));
        assert_eq!(ok(serde_json::to_value(role)), role_fixture);

        assert!(
            serde_json::from_value::<Invite>(json!({
                "object": "organization.invite",
                "id": "inv_1"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ProjectRateLimit>(json!({
                "object": "project.rate_limit",
                "id": "rl_1",
                "model": "gpt-5.6",
                "max_requests_per_1_minute": 10
            }))
            .is_err()
        );
    }

    #[test]
    fn usage_bucket_routes_known_results_strictly_and_future_results_losslessly() {
        let fixture = json!({
            "object": "page",
            "data": [{
                "object": "bucket",
                "start_time": 0,
                "end_time": 3600,
                "results": [
                    {
                        "object": "organization.usage.completions.result",
                        "input_tokens": 10,
                        "output_tokens": 2,
                        "num_model_requests": 1,
                        "project_id": "proj_1",
                        "usage_future": true
                    },
                    {
                        "object": "organization.costs.result",
                        "amount": {"value": 0.01, "currency": "usd"},
                        "line_item": null
                    },
                    {
                        "object": "organization.usage.future.result",
                        "units": 7
                    }
                ],
                "bucket_future": true
            }],
            "has_more": true,
            "next_page": "cursor_2",
            "page_future": true
        });
        let page = ok(serde_json::from_value::<UsageResponse>(fixture.clone()));
        assert_eq!(page.next_page(), Some("cursor_2"));
        assert!(matches!(
            page.data[0].results[0],
            UsageResult::Completions(_)
        ));
        assert!(matches!(page.data[0].results[1], UsageResult::Costs(_)));
        match &page.data[0].results[2] {
            UsageResult::Unknown(value) => {
                assert_eq!(value.discriminator(), "organization.usage.future.result");
            }
            _ => panic!("future usage result must remain unknown"),
        }
        assert!(page.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(page)), fixture);

        assert!(
            serde_json::from_value::<UsageResult>(json!({
                "object": "organization.usage.completions.result",
                "output_tokens": 2,
                "num_model_requests": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn usage_query_requires_start_time_and_preserves_pagination() {
        assert!(serde_json::from_value::<UsageQueryParams>(json!({})).is_err());
        let params = UsageQueryParams::new(100);
        assert_eq!(ok(serde_json::to_value(params)), json!({"start_time": 100}));
    }
}
