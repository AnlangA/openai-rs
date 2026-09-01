//! Stable Administration API wire DTOs.
//!
//! This module is exposed only by the crate's `admin` feature. It covers the
//! organization/project administration surface and provides a frozen operation
//! manifest so every supported route has explicit request and response schema
//! identities.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{ExtraFields, ModelId, Nullable, Omittable, WireSecret};

/// Inclusive minimum for `AdminApiKeyCreateRequest.expires_in_seconds`.
pub const MIN_ADMIN_API_KEY_EXPIRES_IN_SECONDS: u64 = 1;
/// Inclusive maximum for `AdminApiKeyCreateRequest.expires_in_seconds`.
pub const MAX_ADMIN_API_KEY_EXPIRES_IN_SECONDS: u64 = 31_536_000;
/// Inclusive minimum for organization group `name`.
pub const MIN_ADMIN_GROUP_NAME_CHARS: usize = 1;
/// Inclusive maximum for organization group `name`.
pub const MAX_ADMIN_GROUP_NAME_CHARS: usize = 255;
/// Inclusive minimum for `ToggleCertificatesRequest.certificate_ids`.
pub const MIN_TOGGLE_CERTIFICATE_IDS: usize = 1;
/// Inclusive maximum for `ToggleCertificatesRequest.certificate_ids`.
pub const MAX_TOGGLE_CERTIFICATE_IDS: usize = 10;
/// Inclusive minimum for spend-limit `threshold_amount`.
pub const MIN_SPEND_LIMIT_THRESHOLD: u64 = 1;

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
    ///
    /// Every pinned Administration list envelope pins `object` to the constant
    /// `list`; the usage/costs `page` envelope has its own [`UsagePageTag`].
    pub enum AdminListObject {
        List = "list"
    }
}

/// Query parameters used by cursor-based administration listings.
///
/// This is a shared send-side bag. Official list operations expose overlapping
/// pagination plus a few operation-specific filters (`before`, `emails`,
/// `include_archived`, `owner_project_access`). Omitted fields are not sent.
/// Not every key is defined on every route — for example, the audit-log route
/// defines only `after`/`before`/`limit` of these fields (its own filters live
/// on [`AuditLogListParams`]); sending an undefined key is the caller's
/// responsibility.
///
/// # `limit` value domains
///
/// `limit` is not validated here (6-R1: the shared bag is a deliberate
/// superset, D0059); the pinned domains differ per route family:
///
/// | Route family | Domain | Default | Source |
/// |---|---|---|---|
/// | Roles and role assignments (`list-roles`, `list-project-roles`, group/user/project role-assignment lists) | `0..=1000` | 1000 for the role lists, unset for assignments | schema `minimum`/`maximum` |
/// | Groups and group users (`list-groups`, `list-group-users`) | `0..=1000` | 100 | schema `minimum`/`maximum` |
/// | Project groups (`list-project-groups`) | `0..=100` | 20 | schema `minimum`/`maximum` |
/// | Spend alerts (`list-organization/project-spend-alerts`) | `0..=100` | 20 | schema `minimum`/`maximum` |
/// | Most remaining lists (admin API keys, audit logs, certificates, invites, projects, project API keys/certificates/service accounts/users) | `1..=100` (prose) | 20 | description text only — no schema bounds (the D0154 "prose-backed lower bound" family) |
///
/// The usage/costs bucket limits do not use this bag at all — they live on
/// [`UsageQueryParams`] with per-`bucket_width` defaults. Sending a `limit`
/// outside the target route's domain is the caller's responsibility; the
/// service, not this bag, rejects it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub before: Omittable<String>,
    /// Page size. Unvalidated shared-bag value; see the `limit` value domains
    /// table in the type documentation for the per-route bounds and defaults.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<AdminListOrder>,
    /// Official `list-users` filter.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub emails: Omittable<Vec<String>>,
    /// Official `list-projects` filter.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include_archived: Omittable<bool>,
    /// Official `list-project-api-keys` filter (`active` / `inactive` / `any`).
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub owner_project_access: Omittable<ProjectAccessFilter>,
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
    ///
    /// An empty `last_id` yields `None` (D0145): it would otherwise be dropped
    /// by the query encoder and silently re-request the first page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        self.last_id_str().filter(|id| !id.is_empty())
    }

    /// Cursor for another page with the D0147 last-item fallback.
    ///
    /// Resolution order mirrors `pagination::next_cursor` (D0147): when
    /// `has_more` is set, a non-empty envelope `last_id` wins; otherwise the
    /// caller-supplied identifier of the page's final element is used (the
    /// openai-python `data[-1].id` rule); when both are absent or empty there
    /// is no forward cursor. The Administration channel pages by hand (it has
    /// no `list_pages` stream), so callers pass the last item's id explicitly,
    /// e.g. `page.data.last().and_then(|item| item.id.as_deref())`.
    #[must_use]
    pub fn next_after_with<'a>(&'a self, last_item_id: Option<&'a str>) -> Option<&'a str> {
        if !self.has_more {
            return None;
        }
        self.next_after()
            .or_else(|| last_item_id.filter(|id| !id.is_empty()))
    }

    /// The envelope `last_id` when present and non-null.
    #[must_use]
    fn last_id_str(&self) -> Option<&str> {
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

/// Cursor page whose frozen schema requires first/last IDs that may be null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminRequiredCursorPage<T> {
    pub object: AdminListObject,
    pub data: Vec<T>,
    pub first_id: Nullable<String>,
    pub last_id: Nullable<String>,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl<T> AdminRequiredCursorPage<T> {
    /// Server-provided cursor for another page.
    ///
    /// An empty `last_id` yields `None` (D0145): it would otherwise be dropped
    /// by the query encoder and silently re-request the first page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Nullable::Value(id) => Some(id.as_str()).filter(|id| !id.is_empty()),
            Nullable::Null => None,
        }
    }

    /// Cursor for another page with the D0147 last-item fallback.
    ///
    /// Same resolution order as [`AdminCursorPage::next_after_with`]: a
    /// non-empty envelope `last_id` wins, then the caller-supplied last item
    /// id, then no cursor.
    #[must_use]
    pub fn next_after_with<'a>(&'a self, last_item_id: Option<&'a str>) -> Option<&'a str> {
        if !self.has_more {
            return None;
        }
        self.next_after()
            .or_else(|| last_item_id.filter(|id| !id.is_empty()))
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
    ///
    /// An empty `next` yields `None` (D0145): it would otherwise be dropped
    /// by the query encoder and silently re-request the first page, exactly
    /// like the empty `last_id` of [`AdminCursorPage::next_after`].
    #[must_use]
    pub fn next_cursor(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.next {
            Nullable::Value(next) => Some(next.as_str()).filter(|next| !next.is_empty()),
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

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), AdminConstraintError> {
        if let Omittable::Value(actual) = self.expires_in_seconds
            && !(MIN_ADMIN_API_KEY_EXPIRES_IN_SECONDS..=MAX_ADMIN_API_KEY_EXPIRES_IN_SECONDS)
                .contains(&actual)
        {
            return Err(AdminConstraintError::ApiKeyExpiresInSeconds {
                actual,
                minimum: MIN_ADMIN_API_KEY_EXPIRES_IN_SECONDS,
                maximum: MAX_ADMIN_API_KEY_EXPIRES_IN_SECONDS,
            });
        }
        Ok(())
    }
}

/// An Administration create/update value that violates a pinned OpenAPI constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AdminConstraintError {
    /// `expires_in_seconds` is outside `1..=31536000`.
    #[error("admin API key expires_in_seconds must be {minimum}..={maximum}, got {actual}")]
    ApiKeyExpiresInSeconds {
        /// Rejected value.
        actual: u64,
        /// Contract minimum.
        minimum: u64,
        /// Contract maximum.
        maximum: u64,
    },
    /// Group `name` is empty or longer than 255 characters.
    #[error("admin group name has {actual} characters; must be {minimum}..={maximum}")]
    GroupName {
        /// Observed character count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// `certificate_ids` length is outside `1..=10`.
    #[error("certificate_ids must contain {minimum}..={maximum} entries, got {actual}")]
    CertificateIds {
        /// Observed count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// Spend-limit `threshold_amount` is less than 1.
    #[error("spend-limit threshold_amount must be at least {minimum}, got {actual}")]
    SpendThreshold {
        /// Rejected value.
        actual: u64,
        /// Contract minimum.
        minimum: u64,
    },
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

mod admin_audit;
pub use admin_audit::*;

crate::open_string_enum! {
    /// Official `AuditLogEventType`.
    pub enum AuditEventType {
        ApiKeyCreated = "api_key.created",
        ApiKeyUpdated = "api_key.updated",
        ApiKeyDeleted = "api_key.deleted",
        CertificateCreated = "certificate.created",
        CertificateUpdated = "certificate.updated",
        CertificateDeleted = "certificate.deleted",
        CertificatesActivated = "certificates.activated",
        CertificatesDeactivated = "certificates.deactivated",
        CheckpointPermissionCreated = "checkpoint.permission.created",
        CheckpointPermissionDeleted = "checkpoint.permission.deleted",
        ExternalKeyRegistered = "external_key.registered",
        ExternalKeyRemoved = "external_key.removed",
        GroupCreated = "group.created",
        GroupUpdated = "group.updated",
        GroupDeleted = "group.deleted",
        InviteSent = "invite.sent",
        InviteAccepted = "invite.accepted",
        InviteDeleted = "invite.deleted",
        IpAllowlistCreated = "ip_allowlist.created",
        IpAllowlistUpdated = "ip_allowlist.updated",
        IpAllowlistDeleted = "ip_allowlist.deleted",
        IpAllowlistConfigActivated = "ip_allowlist.config.activated",
        IpAllowlistConfigDeactivated = "ip_allowlist.config.deactivated",
        LoginSucceeded = "login.succeeded",
        LoginFailed = "login.failed",
        LogoutSucceeded = "logout.succeeded",
        LogoutFailed = "logout.failed",
        OrganizationUpdated = "organization.updated",
        ProjectCreated = "project.created",
        ProjectUpdated = "project.updated",
        ProjectArchived = "project.archived",
        ProjectDeleted = "project.deleted",
        RateLimitUpdated = "rate_limit.updated",
        RateLimitDeleted = "rate_limit.deleted",
        ResourceDeleted = "resource.deleted",
        TunnelCreated = "tunnel.created",
        TunnelUpdated = "tunnel.updated",
        TunnelDeleted = "tunnel.deleted",
        WorkloadIdentityProviderCreated = "workload_identity_provider.created",
        WorkloadIdentityProviderUpdated = "workload_identity_provider.updated",
        WorkloadIdentityProviderDeleted = "workload_identity_provider.deleted",
        WorkloadIdentityProviderMappingCreated = "workload_identity_provider_mapping.created",
        WorkloadIdentityProviderMappingUpdated = "workload_identity_provider_mapping.updated",
        WorkloadIdentityProviderMappingDeleted = "workload_identity_provider_mapping.deleted",
        RoleCreated = "role.created",
        RoleUpdated = "role.updated",
        RoleDeleted = "role.deleted",
        RoleAssignmentCreated = "role.assignment.created",
        RoleAssignmentDeleted = "role.assignment.deleted",
        RoleBoundToResource = "role.bound_to_resource",
        RoleUnboundFromResource = "role.unbound_from_resource",
        ScimEnabled = "scim.enabled",
        ScimDisabled = "scim.disabled",
        ServiceAccountCreated = "service_account.created",
        ServiceAccountUpdated = "service_account.updated",
        ServiceAccountDeleted = "service_account.deleted",
        UserAdded = "user.added",
        UserUpdated = "user.updated",
        UserDeleted = "user.deleted",
        TenantMetadataUpdated = "tenant.metadata.updated",
        TenantMicrosoftEntraMappingUpserted = "tenant.microsoft_entra_mapping.upserted",
        TenantMicrosoftEntraMappingDeleted = "tenant.microsoft_entra_mapping.deleted",
        TenantWorkloadIdentityProviderCreated = "tenant.workload_identity.provider.created",
        TenantWorkloadIdentityProviderUpdated = "tenant.workload_identity.provider.updated",
        TenantWorkloadIdentityProviderArchived = "tenant.workload_identity.provider.archived",
        TenantWorkloadIdentityMappingCreated = "tenant.workload_identity.mapping.created",
        TenantWorkloadIdentityMappingUpdated = "tenant.workload_identity.mapping.updated",
        TenantWorkloadIdentityMappingArchived = "tenant.workload_identity.mapping.archived",
        TenantWorkloadIdentityBindingCreated = "tenant.workload_identity.binding.created",
        TenantWorkloadIdentityPrincipalProvisioned = "tenant.workload_identity.principal.provisioned",
        TenantWorkloadIdentityAccessTokenIssued = "tenant.workload_identity.access_token.issued",
        TenantAdminApiKeyCreated = "tenant.admin_api_key.created",
        TenantAdminApiKeyUpdated = "tenant.admin_api_key.updated",
        TenantAdminApiKeyDeleted = "tenant.admin_api_key.deleted",
        TenantProjectApiKeyCreated = "tenant.project_api_key.created",
        TenantTrustedAccessBusinessVerificationStarted = "tenant.trusted_access.business_verification.started",
        TenantTrustedAccessApplicationSubmitted = "tenant.trusted_access.application.submitted",
        TenantChatgptAccessTokenRevoked = "tenant.chatgpt_access_token.revoked",
        TenantMigrationCompleted = "tenant.migration.completed",
        TenantSsoMigrated = "tenant.sso.migrated",
        TenantDomainsMigrated = "tenant.domains.migrated",
        TenantSsoConnectionCreated = "tenant.sso_connection.created",
        TenantSsoConnectionUpdated = "tenant.sso_connection.updated",
        TenantSsoConnectionDeleted = "tenant.sso_connection.deleted",
        TenantSsoConnectionSetupStarted = "tenant.sso_connection.setup.started",
        TenantPolicyCreated = "tenant.policy.created",
        TenantPolicyUpdated = "tenant.policy.updated",
        TenantPolicyDeleted = "tenant.policy.deleted",
        TenantPolicyAttached = "tenant.policy.attached",
        TenantPolicyDetached = "tenant.policy.detached",
        TenantPrincipalAuthenticationPolicyResolved = "tenant.principal_authentication_policy.resolved",
        TenantScimSetupStarted = "tenant.scim.setup.started",
        TenantScimDeletionRequested = "tenant.scim.deletion.requested",
        TenantScimDirectoryCreated = "tenant.scim.directory.created",
        TenantProductAccessPolicyUpdated = "tenant.product_access_policy.updated",
        TenantResourceShareGrantCreated = "tenant.resource_share_grant.created",
        TenantResourceShareGrantUpdated = "tenant.resource_share_grant.updated",
        TenantResourceShareGrantAccepted = "tenant.resource_share_grant.accepted",
        TenantResourceShareGrantDeclined = "tenant.resource_share_grant.declined",
        TenantResourceShareGrantRevoked = "tenant.resource_share_grant.revoked",
        TenantResourceShareGrantDeleted = "tenant.resource_share_grant.deleted",
        TenantServiceAccountUpdated = "tenant.service_account.updated",
        TenantServiceAccountDeleted = "tenant.service_account.deleted",
        TenantServiceAccountTokenRevoked = "tenant.service_account.token.revoked",
        TenantBillingOverageLimitUpdated = "tenant.billing.overage_limit.updated",
        TenantBillingAlertsUpdated = "tenant.billing.alerts.updated",
        TenantBillingInfoUpdated = "tenant.billing.info.updated",
        TenantUsageLimitWorkspaceUpdated = "tenant.usage_limit.workspace.updated",
        TenantUsageLimitGroupUpdated = "tenant.usage_limit.group.updated",
        TenantUsageLimitUserUpdated = "tenant.usage_limit.user.updated",
        TenantUsageLimitIncreaseRequestUpdated = "tenant.usage_limit.increase_request.updated",
        TenantUsageLimitIncreaseRequestResolved = "tenant.usage_limit.increase_request.resolved",
        TenantGroupCreated = "tenant.group.created",
        TenantGroupUpdated = "tenant.group.updated",
        TenantGroupDeleted = "tenant.group.deleted",
        TenantGroupMemberAdded = "tenant.group.member.added",
        TenantGroupMemberRemoved = "tenant.group.member.removed",
        TenantMigrationRolloutStatusUpdated = "tenant.migration_rollout.status.updated",
        TenantMigrationRolloutTierUpdated = "tenant.migration_rollout.tier.updated",
        TenantRoleMetadataUpdated = "tenant.role.metadata.updated",
        TenantCustomRoleCreated = "tenant.custom_role.created",
        TenantCustomRoleUpdated = "tenant.custom_role.updated",
        TenantCustomRoleDeleted = "tenant.custom_role.deleted",
        TenantRoleAssignmentCreated = "tenant.role_assignment.created",
        TenantRoleAssignmentDeleted = "tenant.role_assignment.deleted",
        TenantResourceRoleAssignmentCreated = "tenant.resource_role_assignment.created",
        TenantResourceRoleAssignmentDeleted = "tenant.resource_role_assignment.deleted",
        TenantResourceAccessUpdated = "tenant.resource_access.updated",
        TenantResourceAccessDeleted = "tenant.resource_access.deleted",
        TenantAdsAccountOnboardingRedemption = "tenant.ads_account.onboarding.redemption",
        TenantSessionPolicyCreated = "tenant.session_policy.created",
        TenantSessionPolicyUpdated = "tenant.session_policy.updated",
        TenantSessionPolicyDeleted = "tenant.session_policy.deleted",
        TenantSessionRevocationStarted = "tenant.session_revocation.started",
        TenantThirdPartyAppPolicyUpdated = "tenant.third_party_app_policy.updated",
        TenantUserAdded = "tenant.user.added",
        TenantUserUpdated = "tenant.user.updated",
        TenantUserRemoved = "tenant.user.removed",
        TenantUserLookedUp = "tenant.user.looked_up",
        TenantUserInvited = "tenant.user.invited",
        TenantMembershipRevoked = "tenant.membership.revoked",
        TenantApiOrganizationInviteUpserted = "tenant.api_organization_invite.upserted",
        TenantApiOrganizationInviteDeleted = "tenant.api_organization_invite.deleted",
        TenantChatgptWorkspaceInviteUpserted = "tenant.chatgpt_workspace_invite.upserted",
        TenantMembershipAccepted = "tenant.membership.accepted",
        TenantMembershipDeclined = "tenant.membership.declined",
        TenantWorkspaceInviteEmailSettingsUpdated = "tenant.workspace_invite_email_settings.updated",
    }
}

crate::open_string_enum! {
    /// Official `AuditLogActorApiKey.type`.
    pub enum AuditActorApiKeyType {
        User = "user",
        ServiceAccount = "service_account"
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

impl AuditActorUser {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
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

impl AuditActorSession {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Official `AuditLogActorServiceAccount`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditActorServiceAccount {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AuditActorServiceAccount {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// API key audit actor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditActorApiKey {
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<AuditActorApiKeyType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<AuditActorUser>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_account: Omittable<AuditActorServiceAccount>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AuditActorApiKey {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
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

impl AuditLogActor {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
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

impl AuditProject {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Organization audit log entry.
///
/// Official event-specific keys such as `api_key.created` are typed. Tenant
/// events and future keys remain in `extra`.
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
    #[serde(
        rename = "api_key.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub api_key_created: Omittable<AuditPayloadApiKeyCreated>,
    #[serde(
        rename = "api_key.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub api_key_updated: Omittable<AuditPayloadApiKeyUpdated>,
    #[serde(
        rename = "api_key.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub api_key_deleted: Omittable<AuditPayloadApiKeyDeleted>,
    #[serde(
        rename = "checkpoint.permission.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub checkpoint_permission_created: Omittable<AuditPayloadCheckpointPermissionCreated>,
    #[serde(
        rename = "checkpoint.permission.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub checkpoint_permission_deleted: Omittable<AuditPayloadCheckpointPermissionDeleted>,
    #[serde(
        rename = "external_key.registered",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub external_key_registered: Omittable<AuditPayloadExternalKeyRegistered>,
    #[serde(
        rename = "external_key.removed",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub external_key_removed: Omittable<AuditPayloadExternalKeyRemoved>,
    #[serde(
        rename = "group.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub group_created: Omittable<AuditPayloadGroupCreated>,
    #[serde(
        rename = "group.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub group_updated: Omittable<AuditPayloadGroupUpdated>,
    #[serde(
        rename = "group.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub group_deleted: Omittable<AuditPayloadGroupDeleted>,
    #[serde(
        rename = "scim.enabled",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub scim_enabled: Omittable<AuditPayloadScimEnabled>,
    #[serde(
        rename = "scim.disabled",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub scim_disabled: Omittable<AuditPayloadScimDisabled>,
    #[serde(
        rename = "invite.sent",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub invite_sent: Omittable<AuditPayloadInviteSent>,
    #[serde(
        rename = "invite.accepted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub invite_accepted: Omittable<AuditPayloadInviteAccepted>,
    #[serde(
        rename = "invite.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub invite_deleted: Omittable<AuditPayloadInviteDeleted>,
    #[serde(
        rename = "ip_allowlist.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub ip_allowlist_created: Omittable<AuditPayloadIpAllowlistCreated>,
    #[serde(
        rename = "ip_allowlist.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub ip_allowlist_updated: Omittable<AuditPayloadIpAllowlistUpdated>,
    #[serde(
        rename = "ip_allowlist.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub ip_allowlist_deleted: Omittable<AuditPayloadIpAllowlistDeleted>,
    #[serde(
        rename = "ip_allowlist.config.activated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub ip_allowlist_config_activated: Omittable<AuditPayloadIpAllowlistConfigActivated>,
    #[serde(
        rename = "ip_allowlist.config.deactivated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub ip_allowlist_config_deactivated: Omittable<AuditPayloadIpAllowlistConfigDeactivated>,
    #[serde(
        rename = "login.succeeded",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub login_succeeded: Omittable<AdminJsonObject>,
    #[serde(
        rename = "login.failed",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub login_failed: Omittable<AuditPayloadLoginFailed>,
    #[serde(
        rename = "logout.succeeded",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub logout_succeeded: Omittable<AdminJsonObject>,
    #[serde(
        rename = "logout.failed",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub logout_failed: Omittable<AuditPayloadLogoutFailed>,
    #[serde(
        rename = "organization.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub organization_updated: Omittable<AuditPayloadOrganizationUpdated>,
    #[serde(
        rename = "project.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub project_created: Omittable<AuditPayloadProjectCreated>,
    #[serde(
        rename = "project.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub project_updated: Omittable<AuditPayloadProjectUpdated>,
    #[serde(
        rename = "project.archived",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub project_archived: Omittable<AuditPayloadProjectArchived>,
    #[serde(
        rename = "project.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub project_deleted: Omittable<AuditPayloadProjectDeleted>,
    #[serde(
        rename = "rate_limit.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub rate_limit_updated: Omittable<AuditPayloadRateLimitUpdated>,
    #[serde(
        rename = "rate_limit.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub rate_limit_deleted: Omittable<AuditPayloadRateLimitDeleted>,
    #[serde(
        rename = "role.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_created: Omittable<AuditPayloadRoleCreated>,
    #[serde(
        rename = "role.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_updated: Omittable<AuditPayloadRoleUpdated>,
    #[serde(
        rename = "role.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_deleted: Omittable<AuditPayloadRoleDeleted>,
    #[serde(
        rename = "role.assignment.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_assignment_created: Omittable<AuditPayloadRoleAssignmentCreated>,
    #[serde(
        rename = "role.assignment.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_assignment_deleted: Omittable<AuditPayloadRoleAssignmentDeleted>,
    #[serde(
        rename = "role.bound_to_resource",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_bound_to_resource: Omittable<AuditPayloadRoleBoundToResource>,
    #[serde(
        rename = "role.unbound_from_resource",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub role_unbound_from_resource: Omittable<AuditPayloadRoleUnboundFromResource>,
    #[serde(
        rename = "service_account.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub service_account_created: Omittable<AuditPayloadServiceAccountCreated>,
    #[serde(
        rename = "service_account.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub service_account_updated: Omittable<AuditPayloadServiceAccountUpdated>,
    #[serde(
        rename = "service_account.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub service_account_deleted: Omittable<AuditPayloadServiceAccountDeleted>,
    #[serde(
        rename = "workload_identity_provider.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_created: Omittable<AuditPayloadWorkloadIdentityProviderCreated>,
    #[serde(
        rename = "workload_identity_provider.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_updated: Omittable<AuditPayloadWorkloadIdentityProviderUpdated>,
    #[serde(
        rename = "workload_identity_provider.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_deleted: Omittable<AuditPayloadWorkloadIdentityProviderDeleted>,
    #[serde(
        rename = "workload_identity_provider_mapping.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_mapping_created:
        Omittable<AuditPayloadWorkloadIdentityProviderMappingCreated>,
    #[serde(
        rename = "workload_identity_provider_mapping.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_mapping_updated:
        Omittable<AuditPayloadWorkloadIdentityProviderMappingUpdated>,
    #[serde(
        rename = "workload_identity_provider_mapping.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub workload_identity_provider_mapping_deleted:
        Omittable<AuditPayloadWorkloadIdentityProviderMappingDeleted>,
    #[serde(
        rename = "user.added",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub user_added: Omittable<AuditPayloadUserAdded>,
    #[serde(
        rename = "user.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub user_updated: Omittable<AuditPayloadUserUpdated>,
    #[serde(
        rename = "user.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub user_deleted: Omittable<AuditPayloadUserDeleted>,
    #[serde(
        rename = "certificate.created",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub certificate_created: Omittable<AuditPayloadCertificateCreated>,
    #[serde(
        rename = "certificate.updated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub certificate_updated: Omittable<AuditPayloadCertificateUpdated>,
    #[serde(
        rename = "certificate.deleted",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub certificate_deleted: Omittable<AuditPayloadCertificateDeleted>,
    #[serde(
        rename = "certificates.activated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub certificates_activated: Omittable<AuditPayloadCertificatesActivated>,
    #[serde(
        rename = "certificates.deactivated",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub certificates_deactivated: Omittable<AuditPayloadCertificatesDeactivated>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AuditLog {
    /// Tenant events and other fields not named on the pinned `AuditLog` schema.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

pub type ListAuditLogsResponse = AdminCursorPage<AuditLog>;

/// `effective_at` bounds for `GET /organization/audit_logs`.
///
/// The pinned `effective_at` query parameter is an object with exactly these
/// four comparison keys (`gt`/`gte`/`lt`/`lte`, Unix seconds); openai-python
/// and openai-node model it as the same four-key literal type. Omitted fields
/// are not sent, and the deep-object encoder emits them as
/// `effective_at[gt]=…` style pairs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditEffectiveAt {
    /// Return only events whose `effective_at` is greater than this value.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub gt: Omittable<u64>,
    /// Return only events whose `effective_at` is greater than or equal to
    /// this value.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub gte: Omittable<u64>,
    /// Return only events whose `effective_at` is less than this value.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub lt: Omittable<u64>,
    /// Return only events whose `effective_at` is less than or equal to this
    /// value.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub lte: Omittable<u64>,
}

impl AuditEffectiveAt {
    /// Sets the exclusive lower bound (`effective_at > value`).
    #[must_use]
    pub fn with_gt(mut self, gt: u64) -> Self {
        self.gt = Omittable::Value(gt);
        self
    }

    /// Sets the inclusive lower bound (`effective_at >= value`).
    #[must_use]
    pub fn with_gte(mut self, gte: u64) -> Self {
        self.gte = Omittable::Value(gte);
        self
    }

    /// Sets the exclusive upper bound (`effective_at < value`).
    #[must_use]
    pub fn with_lt(mut self, lt: u64) -> Self {
        self.lt = Omittable::Value(lt);
        self
    }

    /// Sets the inclusive upper bound (`effective_at <= value`).
    #[must_use]
    pub fn with_lte(mut self, lte: u64) -> Self {
        self.lte = Omittable::Value(lte);
        self
    }
}

/// Audit log filters and pagination.
///
/// The pinned `GET /organization/audit_logs` defines exactly `effective_at`,
/// `project_ids`, `event_types`, `actor_ids`, `actor_emails`, `resource_ids`,
/// `tenant_only`, `limit`, `after`, and `before`. The flattened
/// [`AdminListParams`] bag is shared across the Administration list endpoints,
/// so it also carries `order`, `emails`, `include_archived`, and
/// `owner_project_access` — four filters the audit route does not define. They
/// stay omitted unless explicitly set; sending one is the caller's
/// responsibility, mirroring how the shared superset is documented on
/// [`AdminListParams`] (see 5-K4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditLogListParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub effective_at: Omittable<AuditEffectiveAt>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub event_types: Omittable<Vec<AuditEventType>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub actor_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub actor_emails: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub resource_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tenant_only: Omittable<bool>,
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

crate::open_string_enum! {
    /// Official `getCertificate` `include` query values.
    pub enum CertificateInclude {
        Content = "content"
    }
}

/// Official `getCertificate` query parameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CertificateGetParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub include: Omittable<Vec<CertificateInclude>>,
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

impl ToggleCertificatesRequest {
    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), AdminConstraintError> {
        let actual = self.certificate_ids.len();
        if !(MIN_TOGGLE_CERTIFICATE_IDS..=MAX_TOGGLE_CERTIFICATE_IDS).contains(&actual) {
            return Err(AdminConstraintError::CertificateIds {
                actual,
                minimum: MIN_TOGGLE_CERTIFICATE_IDS,
                maximum: MAX_TOGGLE_CERTIFICATE_IDS,
            });
        }
        Ok(())
    }
}

crate::open_string_enum! {
    /// Official certificate activate/deactivate result discriminator.
    pub enum CertificateScopeObject {
        OrganizationActivation = "organization.certificate.activation",
        OrganizationDeactivation = "organization.certificate.deactivation",
        ProjectActivation = "organization.project.certificate.activation",
        ProjectDeactivation = "organization.project.certificate.deactivation"
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CertificateScopeResponse {
    pub object: CertificateScopeObject,
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

crate::open_string_enum! {
    /// Deleted certificate discriminator.
    pub enum DeleteCertificateObject {
        Deleted = "certificate.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteCertificateResponse {
    pub object: DeleteCertificateObject,
    pub id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Project data-retention mode, which is the pinned superset domain.
    ///
    /// This enum also backs the shared resource side ([`DataRetentionResource`]):
    /// it is open, so the four organization values, the two project-only
    /// values (`organization_default`, `none`), and any future service value
    /// all decode losslessly.
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
    /// Organization data-retention mode.
    ///
    /// The pinned `UpdateOrganizationDataRetentionBody.retention_type` and
    /// `OrganizationDataRetention.type` enumerate exactly these four values;
    /// `organization_default` and `none` are project-only and decode as
    /// `Unknown` rather than named variants.
    pub enum OrganizationDataRetentionType {
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
    pub retention_type: OrganizationDataRetentionType,
}

/// Project data-retention update body. Distinct from
/// [`UpdateOrganizationDataRetentionBody`] because the pinned project domain
/// additionally allows `organization_default` and `none`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateProjectDataRetentionBody {
    pub retention_type: DataRetentionType,
}

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

impl GroupMemberUser {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Official `UserListResource` item (`list-group-users`). Retrieve uses [`GroupMemberUser`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupUser {
    pub id: String,
    pub name: String,
    pub email: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl GroupUser {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupBody {
    pub name: String,
}

impl CreateGroupBody {
    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), AdminConstraintError> {
        let actual = self.name.chars().count();
        if !(MIN_ADMIN_GROUP_NAME_CHARS..=MAX_ADMIN_GROUP_NAME_CHARS).contains(&actual) {
            return Err(AdminConstraintError::GroupName {
                actual,
                minimum: MIN_ADMIN_GROUP_NAME_CHARS,
                maximum: MAX_ADMIN_GROUP_NAME_CHARS,
            });
        }
        Ok(())
    }
}

pub type UpdateGroupBody = CreateGroupBody;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGroupUserBody {
    pub user_id: String,
}

crate::open_string_enum! {
    /// Assignment/deletion object discriminator.
    ///
    /// `group.user`/`group.role`/`user.role`/`group.user.deleted` are pinned
    /// schema constants; `group.role.deleted`/`user.role.deleted` appear in the
    /// pinned `DeletedRoleAssignmentResource.object` description ("such as …")
    /// whose free-form string stays lossless through [`Unknown`].
    pub enum AssignmentObject {
        GroupUser = "group.user",
        GroupRole = "group.role",
        UserRole = "user.role",
        GroupUserDeleted = "group.user.deleted",
        GroupRoleDeleted = "group.role.deleted",
        UserRoleDeleted = "user.role.deleted"
    }
}

crate::open_string_enum! {
    /// Official `Group.object` discriminator.
    pub enum GroupObject {
        Group = "group"
    }
}

/// Official `Group` summary embedded in role-assignment responses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub object: GroupObject,
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub scim_managed: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl GroupSummary {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
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
    pub group: GroupSummary,
    pub role: Role,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRoleAssignment {
    pub object: AssignmentObject,
    pub user: User,
    pub role: Role,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

crate::open_string_enum! {
    /// Deleted group discriminator.
    pub enum GroupDeletedObject {
        Deleted = "group.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupDeletedResource {
    pub object: GroupDeletedObject,
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
pub type UserListResource = AdminNextPage<GroupUser>;

crate::open_string_enum! {
    /// Organization user object discriminator.
    pub enum UserObject {
        User = "organization.user",
        ProjectUser = "organization.project.user"
    }
}

crate::open_string_enum! {
    /// Official nested `User.user.object` discriminator.
    pub enum NestedUserObject {
        User = "user"
    }
}

/// Official nested `User.user` details.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestedUserDetails {
    pub object: NestedUserObject,
    pub id: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub email: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub picture: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub enabled: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub banned: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub banned_at: Omittable<Nullable<u64>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl NestedUserDetails {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// One project listed on official `User.projects.data`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserProjectListItem {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<Nullable<String>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl UserProjectListItem {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Official `User.projects` list envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserProjectList {
    pub object: AdminListObject,
    pub data: Vec<UserProjectListItem>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl UserProjectList {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
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
    pub user: Omittable<NestedUserDetails>,
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
    pub projects: Omittable<Nullable<UserProjectList>>,
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
    pub role: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub technical_level: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub developer_persona: Omittable<Nullable<String>>,
}

crate::open_string_enum! {
    /// Deleted organization user discriminator.
    pub enum UserDeleteObject {
        Deleted = "organization.user.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserDeleteResponse {
    pub object: UserDeleteObject,
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
    pub description: Omittable<Nullable<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublicUpdateOrganizationRoleBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role_name: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub permissions: Omittable<Nullable<Vec<String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub description: Omittable<Nullable<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAssignOrganizationGroupRoleBody {
    pub role_id: String,
}

/// Official `AssignedRoleDetails.assignment_sources` item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleAssignmentSource {
    pub principal_id: String,
    pub principal_type: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl RoleAssignmentSource {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignedRoleDetails {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub resource_type: String,
    pub predefined_role: bool,
    pub description: Nullable<String>,
    pub created_at: Nullable<u64>,
    pub updated_at: Nullable<u64>,
    pub created_by: Nullable<String>,
    pub created_by_user_obj: Nullable<AdminJsonObject>,
    pub metadata: Nullable<AdminJsonObject>,
    pub assignment_sources: Nullable<Vec<RoleAssignmentSource>>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type PublicRoleListResource = AdminNextPage<Role>;
pub type RoleListResource = AdminNextPage<AssignedRoleDetails>;

crate::open_string_enum! {
    /// Deleted role discriminator.
    pub enum RoleDeletedObject {
        Deleted = "role.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleDeletedResource {
    pub object: RoleDeletedObject,
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
    ///
    /// The pinned `Invite.role` / `InviteRequest.role` enum is exactly
    /// `owner` / `reader`. Project memberships use [`InviteProjectRole`],
    /// which is where `member` lives.
    pub enum InviteRole {
        Owner = "owner",
        Reader = "reader"
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

crate::open_string_enum! {
    /// Official invite-project membership role.
    pub enum InviteProjectRole {
        Member = "member",
        Owner = "owner"
    }
}

/// Official `Invite.projects[]` / `InviteRequest.projects[]` item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InviteProjectMembership {
    pub id: String,
    pub role: InviteProjectRole,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl InviteProjectMembership {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
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
    pub projects: Vec<InviteProjectMembership>,
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
    pub projects: Omittable<Vec<InviteProjectMembership>>,
}

pub type InviteListResponse = AdminCursorPage<Invite>;

crate::open_string_enum! {
    /// Deleted invite discriminator.
    pub enum InviteDeleteObject {
        Deleted = "organization.invite.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InviteDeleteResponse {
    pub object: InviteDeleteObject,
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
    pub name: Omittable<Nullable<String>>,
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

crate::open_string_enum! {
    /// Project group discriminator.
    pub enum ProjectGroupObject {
        Group = "project.group"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub object: ProjectGroupObject,
    pub project_id: String,
    pub group_id: String,
    pub group_name: String,
    pub group_type: ProjectGroupType,
    pub created_at: u64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ProjectGroupListResource = AdminNextPage<ProjectGroup>;

/// Official `retrieve-project-group` query parameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroupGetParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub group_type: Omittable<GroupType>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteProjectGroupBody {
    pub group_id: String,
    pub role: String,
}

crate::open_string_enum! {
    /// Removed project-group discriminator.
    pub enum ProjectGroupDeletedObject {
        Deleted = "project.group.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectGroupDeletedResource {
    pub object: ProjectGroupDeletedObject,
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
    pub role: Omittable<Nullable<String>>,
}

pub type ProjectUserListResponse = AdminCursorPage<ProjectUser>;

crate::open_string_enum! {
    /// Removed project-user discriminator.
    pub enum ProjectUserDeleteObject {
        Deleted = "organization.project.user.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectUserDeleteResponse {
    pub object: ProjectUserDeleteObject,
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
    /// Service-account role accepted by the update body.
    ///
    /// The pinned `UpdateProjectServiceAccountBody.role` enumerates exactly
    /// `member`/`owner` (openai-python and openai-node agree); the
    /// resource-side [`ProjectServiceAccountRole`] additionally carries `none`,
    /// which decodes as `Unknown` here.
    pub enum ProjectServiceAccountUpdateRole {
        Member = "member",
        Owner = "owner"
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
    pub create_service_account_only: Omittable<Nullable<bool>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateProjectServiceAccountBody {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub role: Omittable<ProjectServiceAccountUpdateRole>,
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

crate::open_string_enum! {
    /// Deleted project service-account discriminator.
    pub enum ProjectServiceAccountDeleteObject {
        Deleted = "organization.project.service_account.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectServiceAccountDeleteResponse {
    pub object: ProjectServiceAccountDeleteObject,
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

crate::open_string_enum! {
    /// Official `list-project-api-keys` `owner_project_access` query filter.
    pub enum ProjectAccessFilter {
        Active = "active",
        Inactive = "inactive",
        Any = "any"
    }
}

crate::open_string_enum! {
    /// Official `ProjectApiKey.owner.type`.
    pub enum ProjectApiKeyOwnerType {
        User = "user",
        ServiceAccount = "service_account"
    }
}

/// Official `ProjectApiKeyOwnerUser`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKeyOwnerUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub created_at: u64,
    pub role: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ProjectApiKeyOwnerUser {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Official `ProjectApiKeyOwnerServiceAccount`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKeyOwnerServiceAccount {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub role: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ProjectApiKeyOwnerServiceAccount {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Official `ProjectApiKey.owner`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKeyOwner {
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<ProjectApiKeyOwnerType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<ProjectApiKeyOwnerUser>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub service_account: Omittable<ProjectApiKeyOwnerServiceAccount>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ProjectApiKeyOwner {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Project API-key discriminator.
    pub enum ProjectApiKeyObject {
        Key = "organization.project.api_key"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKey {
    pub object: ProjectApiKeyObject,
    pub redacted_value: String,
    pub name: String,
    pub created_at: u64,
    pub last_used_at: Nullable<u64>,
    pub id: String,
    pub owner_project_access: ProjectAccessState,
    pub owner: ProjectApiKeyOwner,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

pub type ProjectApiKeyListResponse = AdminCursorPage<ProjectApiKey>;

crate::open_string_enum! {
    /// Deleted project API-key discriminator.
    pub enum ProjectApiKeyDeleteObject {
        Deleted = "organization.project.api_key.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectApiKeyDeleteResponse {
    pub object: ProjectApiKeyDeleteObject,
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

crate::open_string_enum! {
    /// Project model-permissions discriminator.
    pub enum ProjectModelPermissionsObject {
        Permissions = "project.model_permissions"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectModelPermissions {
    pub object: ProjectModelPermissionsObject,
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

crate::open_string_enum! {
    /// Deleted project model-permissions discriminator.
    pub enum ProjectModelPermissionsDeleteObject {
        Deleted = "project.model_permissions.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectModelPermissionsDeleteResponse {
    pub object: ProjectModelPermissionsDeleteObject,
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

crate::open_string_enum! {
    /// Deleted spend-alert discriminator.
    ///
    /// The organization and project delete routes each pin their own constant,
    /// so one open enum carries both (the shared struct is aliased per route).
    pub enum SpendAlertDeletedObject {
        Organization = "organization.spend_alert.deleted",
        Project = "project.spend_alert.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendAlertDeletedResource {
    pub id: String,
    pub object: SpendAlertDeletedObject,
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

impl UpdateSpendLimitBody {
    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), AdminConstraintError> {
        if self.threshold_amount < MIN_SPEND_LIMIT_THRESHOLD {
            return Err(AdminConstraintError::SpendThreshold {
                actual: self.threshold_amount,
                minimum: MIN_SPEND_LIMIT_THRESHOLD,
            });
        }
        Ok(())
    }
}

pub type UpdateOrganizationSpendLimitBody = UpdateSpendLimitBody;
pub type UpdateProjectSpendLimitBody = UpdateSpendLimitBody;

crate::open_string_enum! {
    /// Deleted spend-limit discriminator.
    ///
    /// The organization and project delete routes each pin their own constant,
    /// so one open enum carries both (the shared struct is aliased per route).
    pub enum SpendLimitDeletedObject {
        Organization = "organization.spend_limit.deleted",
        Project = "project.spend_limit.deleted"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendLimitDeletedResource {
    pub object: SpendLimitDeletedObject,
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

crate::open_string_enum! {
    /// Cost-query bucket width.
    ///
    /// The pinned `GET /organization/costs` `bucket_width` enumerates only `1d`
    /// (openai-python and openai-node both use `Literal["1d"]`); the usage-side
    /// `1m`/`1h` widths decode as `Unknown` here.
    pub enum UsageCostsBucketWidth {
        Day = "1d"
    }
}

crate::open_string_enum! {
    /// Cost-query grouping dimension.
    ///
    /// The pinned `GET /organization/costs` `group_by` items enumerate exactly
    /// these three values; the usage-side dimensions (`user_id`, `model`,
    /// `batch`, `service_tier`, ...) decode as `Unknown` here.
    pub enum UsageCostsGroupBy {
        ProjectId = "project_id",
        LineItem = "line_item",
        ApiKeyId = "api_key_id"
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

crate::open_string_enum! {
    /// Usage-query grouping dimension.
    ///
    /// Union of the per-endpoint `group_by` item enums across the pinned usage
    /// routes: `completions` adds `batch`/`service_tier`, `images` adds
    /// `size`/`source`, `file_search_calls` adds `vector_store_id`, and
    /// `web_search_calls` adds `context_level` over the shared
    /// `project_id`/`user_id`/`api_key_id`/`model` core. The field set equals
    /// [`UsageDimensions`]. The costs-only `line_item` and any future value
    /// decode as `Unknown`; construct via [`UsageGroupBy::from_raw`] when an
    /// endpoint-scoped domain is required.
    pub enum UsageGroupBy {
        ProjectId = "project_id",
        UserId = "user_id",
        ApiKeyId = "api_key_id",
        Model = "model",
        Batch = "batch",
        ServiceTier = "service_tier",
        Source = "source",
        Size = "size",
        VectorStoreId = "vector_store_id",
        ContextLevel = "context_level"
    }
}

crate::open_string_enum! {
    /// Image-usage query source filter.
    ///
    /// The pinned `GET /organization/usage/images` `sources` item enum lists
    /// exactly these three values; any future service value decodes as
    /// `Unknown` and re-encodes verbatim.
    pub enum UsageImageSource {
        Generation = "image.generation",
        Edit = "image.edit",
        Variation = "image.variation"
    }
}

crate::open_string_enum! {
    /// Image-usage query size filter.
    ///
    /// The pinned `GET /organization/usage/images` `sizes` item enum lists
    /// exactly these five values — note the pinned `1792x1792` square, which
    /// the generation-side [`crate::media::ImageSize`] does not carry (that
    /// domain has `1792x1024` instead). Any future service value decodes as
    /// `Unknown` and re-encodes verbatim.
    pub enum UsageImageSize {
        Square256 = "256x256",
        Square512 = "512x512",
        Square1024 = "1024x1024",
        Square1792 = "1792x1792",
        Portrait1024x1792 = "1024x1792"
    }
}

crate::open_string_enum! {
    /// Web-search usage context-level filter.
    ///
    /// The pinned `GET /organization/usage/web_search_calls` `context_levels`
    /// item enum lists exactly these three values; any future service value
    /// decodes as `Unknown` and re-encodes verbatim.
    pub enum UsageContextLevel {
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

/// Shared query superset for Usage endpoints.
///
/// This is a send-side superset (D0059): individual endpoints accept only a
/// subset of these filters, per the pinned OpenAPI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageQueryParams {
    /// Unix seconds; buckets with start timestamps at or after this value
    /// are included.
    pub start_time: u64,
    /// Unix seconds, exclusive upper bound on bucket start timestamps.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub end_time: Omittable<u64>,
    /// Bucket width; defaults to `1d`. The per-bucket `limit` defaults and
    /// ceilings vary with it (`1d`: default 7, max 31; `1h`: 24/168;
    /// `1m`: 60/1440) per the official docs.
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
    /// Restricts results to batch usage only.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch: Omittable<bool>,
    /// Images endpoints only.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub sources: Omittable<Vec<UsageImageSource>>,
    /// Images endpoints only.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub sizes: Omittable<Vec<UsageImageSize>>,
    /// File-search endpoints only.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub vector_store_ids: Omittable<Vec<String>>,
    /// Web-search endpoints only.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub context_levels: Omittable<Vec<UsageContextLevel>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub group_by: Omittable<Vec<UsageGroupBy>>,
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
            sources: Omittable::Omitted,
            sizes: Omittable::Omitted,
            vector_store_ids: Omittable::Omitted,
            context_levels: Omittable::Omitted,
            group_by: Omittable::Omitted,
            limit: Omittable::Omitted,
            page: Omittable::Omitted,
        }
    }
}

/// Query parameters for `GET /organization/costs`.
///
/// Distinct from [`UsageQueryParams`] because the pinned costs route accepts
/// only these eight parameters: `bucket_width` supports `1d` alone and
/// `group_by` enumerates `project_id`/`line_item`/`api_key_id`. The usage-side
/// filters (`user_ids`, `models`, `batch`, `sources`, `sizes`,
/// `vector_store_ids`, `context_levels`) are not defined for costs and are
/// absent here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageCostsQueryParams {
    pub start_time: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub end_time: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub bucket_width: Omittable<UsageCostsBucketWidth>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub api_key_ids: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub group_by: Omittable<Vec<UsageCostsGroupBy>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub page: Omittable<String>,
}

impl UsageCostsQueryParams {
    /// Construct the required inclusive start timestamp.
    #[must_use]
    pub fn new(start_time: u64) -> Self {
        Self {
            start_time,
            end_time: Omittable::Omitted,
            bucket_width: Omittable::Omitted,
            project_ids: Omittable::Omitted,
            api_key_ids: Omittable::Omitted,
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
    "organization.usage.file_searches.result",
    num_requests
);

literal_tag!(
    UsageWebSearchTag,
    Value,
    "organization.usage.web_searches.result"
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
        FileSearchCalls(UsageFileSearchCallsResult) = "organization.usage.file_searches.result",
        WebSearchCalls(UsageWebSearchCallsResult) = "organization.usage.web_searches.result",
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
    ///
    /// An empty `next_page` yields `None` (D0145): it would otherwise be
    /// dropped by the query encoder and silently re-request the first page,
    /// exactly like the empty `last_id` of [`AdminCursorPage::next_after`].
    #[must_use]
    pub fn next_page(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.next_page {
            Nullable::Value(page) => Some(page.as_str()).filter(|page| !page.is_empty()),
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
    ($id:literal, $method:literal, $path:literal, "UsageCostsQueryParams", $response:literal) => {
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
/// Generated from `spec/contracts/operations.json`. Spec provenance is
/// enforced at runtime rather than by a recorded digest: the
/// `operation_manifest_rows_match_the_pinned_spec_projection` test decodes the
/// pinned spec file directly (`include_str`) and compares it row-by-row
/// against this manifest, so the manifest cannot drift from the spec it was
/// generated from. An earlier pair of recorded SHA-256 constants was removed
/// (11-06): the digests had gone stale as the spec evolved, had no consumers
/// to catch the drift, and offered only false provenance.
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
        "UsageCostsQueryParams",
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
    use serde_json::{Map, Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(AdminApiKey: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(AdminApiKeyCreateResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(AuditLog: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(AuditEffectiveAt: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Certificate: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(CertificateScopeResponse: Serialize, DeserializeOwned, Send, Sync);
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

    /// Count of Administration routes in the pinned spec source.
    ///
    /// Organization and project routes only — the three Administration-only
    /// fine-tuning checkpoint-permission operations belong to the dedicated
    /// Administration client channel, not to this manifest.
    fn pinned_admin_operation_count() -> usize {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/operations.json"))
                .expect("operation manifest JSON");
        manifest["client_operations"]
            .as_array()
            .expect("client operation array")
            .iter()
            .filter(|operation| {
                operation["path"].as_str().is_some_and(|path| {
                    path.starts_with("/organization/") || path.starts_with("/projects/")
                })
            })
            .count()
    }

    #[test]
    fn operation_manifest_covers_every_frozen_admin_operation_once() {
        assert_eq!(
            ADMIN_OPERATION_MANIFEST.len(),
            pinned_admin_operation_count(),
            "the manifest must cover exactly the pinned Administration routes"
        );
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

    /// One normalized manifest row: the method/path/mode/status/content-type/
    /// schema-ref projection shared by the pinned spec and the manifest.
    type OperationProjectionRow = (
        String,
        String,
        String,
        String,
        String,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Vec<String>,
    );

    #[test]
    fn operation_manifest_rows_match_the_pinned_spec_projection() {
        // Replaces the removed recorded-digest provenance constants (11-06):
        // the manifest is compared row-by-row against the pinned spec
        // projection itself, so it cannot drift from the file it was
        // generated from without failing here.
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/operations.json"))
                .expect("operation manifest JSON");
        let string_array = |value: &[Value]| -> Vec<String> {
            value
                .iter()
                .map(|item| item.as_str().expect("string entry").to_owned())
                .collect()
        };
        let mut spec_rows = Vec::new();
        for operation in manifest["client_operations"]
            .as_array()
            .expect("client operation array")
        {
            let path = operation["path"].as_str().expect("operation path");
            if !(path.starts_with("/organization/") || path.starts_with("/projects/")) {
                continue;
            }
            let request_refs = operation["request"]["body"]["schema_refs"]
                .as_array()
                .map(|refs| string_array(refs))
                .unwrap_or_default();
            spec_rows.push((
                operation["operation_id"]
                    .as_str()
                    .expect("operation id")
                    .to_owned(),
                operation["method"].as_str().expect("method").to_owned(),
                path.to_owned(),
                operation["request"]["mode"]
                    .as_str()
                    .expect("request mode")
                    .to_owned(),
                operation["response"]["mode"]
                    .as_str()
                    .expect("response mode")
                    .to_owned(),
                string_array(
                    operation["response"]["success_statuses"]
                        .as_array()
                        .expect("success statuses"),
                ),
                string_array(
                    operation["response"]["content_types"]
                        .as_array()
                        .expect("content types"),
                ),
                request_refs,
                string_array(
                    operation["response"]["schema_refs"]
                        .as_array()
                        .expect("response schema refs"),
                ),
            ));
        }
        spec_rows.sort_unstable();

        let mut manifest_rows: Vec<OperationProjectionRow> = ADMIN_OPERATION_MANIFEST
            .iter()
            .map(|operation| {
                (
                    operation.operation_id.to_owned(),
                    operation.method.to_owned(),
                    operation.path.to_owned(),
                    operation.request_mode.to_owned(),
                    operation.response_mode.to_owned(),
                    operation
                        .success_statuses
                        .iter()
                        .map(|status| status.to_string())
                        .collect(),
                    operation
                        .response_content_types
                        .iter()
                        .map(|content_type| (*content_type).to_owned())
                        .collect(),
                    operation
                        .request_schema_refs
                        .iter()
                        .map(|reference| (*reference).to_owned())
                        .collect(),
                    operation
                        .response_schema_refs
                        .iter()
                        .map(|reference| (*reference).to_owned())
                        .collect(),
                )
            })
            .collect();
        manifest_rows.sort_unstable();

        assert_eq!(
            manifest_rows, spec_rows,
            "the manifest must be the exact row projection of the pinned spec"
        );
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
        assert!(matches!(audit.kind, AuditEventType::TenantPolicyUpdated));
        assert!(audit.api_key_created.is_omitted());
        assert_eq!(ok(serde_json::to_value(audit)), fixture);
    }

    #[test]
    fn audit_effective_at_pins_the_four_comparison_keys() {
        // The pinned `effective_at` query parameter is an object with exactly
        // gt/gte/lt/lte (Unix seconds); openai-python and openai-node type it
        // the same way, so the previous free-form string map is gone.
        let bounds = AuditEffectiveAt::default()
            .with_gt(100)
            .with_gte(101)
            .with_lt(200)
            .with_lte(201);
        assert_eq!(
            ok(serde_json::to_value(&bounds)),
            json!({"gt": 100, "gte": 101, "lt": 200, "lte": 201})
        );

        // A partially filled filter only emits the keys that were set.
        assert_eq!(
            ok(serde_json::to_value(
                AuditEffectiveAt::default().with_gte(100)
            )),
            json!({"gte": 100})
        );

        let audit = AuditLogListParams {
            effective_at: Omittable::Value(AuditEffectiveAt::default().with_gt(10).with_lt(20)),
            ..AuditLogListParams::default()
        };
        assert_eq!(
            ok(serde_json::to_value(&audit)),
            json!({"effective_at": {"gt": 10, "lt": 20}})
        );

        let decoded = ok(serde_json::from_value::<AuditLogListParams>(json!({
            "effective_at": {"gte": 1, "lte": 2}
        })));
        match decoded.effective_at {
            Omittable::Value(bounds) => {
                assert_eq!(bounds.gte, Omittable::Value(1));
                assert_eq!(bounds.lte, Omittable::Value(2));
                assert!(bounds.gt.is_omitted());
                assert!(bounds.lt.is_omitted());
            }
            Omittable::Omitted => panic!("official effective_at bounds must decode"),
        }

        // Three-state rule: a bound is either absent or an integer, never null.
        assert!(
            serde_json::from_value::<AuditEffectiveAt>(json!({"gt": null})).is_err(),
            "explicit null is not an effective_at bound"
        );
        assert!(
            serde_json::from_value::<AuditLogListParams>(json!({"effective_at": null})).is_err(),
            "explicit null is not an effective_at filter"
        );
    }

    /// Pinned audit wire inventory derived from the frozen OpenAPI document.
    ///
    /// Mirrors the `openapi_tags` derivation in `realtime.rs`: the official
    /// event-type strings come from `AuditLogEventType.enum` and the dotted
    /// payload keys from `AuditLog.properties`, so the parity test below
    /// asserts against the pin itself instead of a hand-copied list (7-20).
    fn pinned_audit_wire_inventory() -> (Vec<String>, Vec<String>, Vec<String>) {
        let openapi: Value = serde_json::from_str(include_str!(
            "../../../spec/upstream/openapi-2026-08-29.json"
        ))
        .expect("pinned OpenAPI is valid JSON");
        let schemas = openapi["components"]["schemas"]
            .as_object()
            .expect("OpenAPI schemas are an object");
        let mut event_types: Vec<String> = schemas["AuditLogEventType"]["enum"]
            .as_array()
            .expect("AuditLogEventType carries an enum array")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("AuditLogEventType enum values are strings")
                    .to_owned()
            })
            .collect();
        event_types.sort();
        let properties = schemas["AuditLog"]["properties"]
            .as_object()
            .expect("AuditLog properties are an object");
        let mut payload_keys: Vec<String> = properties
            .keys()
            .filter(|key| key.contains('.'))
            .cloned()
            .collect();
        payload_keys.sort();
        let mut envelope_keys: Vec<String> = properties
            .keys()
            .filter(|key| !key.contains('.'))
            .cloned()
            .collect();
        envelope_keys.sort();
        (event_types, payload_keys, envelope_keys)
    }

    /// Pinned per-payload property schemas: `AuditLog.properties[<key>].properties`.
    ///
    /// Each dotted `AuditLog` property maps to the field schemas of its typed
    /// Rust payload, so the parity test can derive both a complete sample
    /// fixture and a type-mismatched probe per field straight from the pin
    /// (8-08) instead of hand-copied field lists.
    fn pinned_audit_payload_properties() -> Map<String, Value> {
        let openapi: Value = serde_json::from_str(include_str!(
            "../../../spec/upstream/openapi-2026-08-29.json"
        ))
        .expect("pinned OpenAPI is valid JSON");
        openapi["components"]["schemas"]["AuditLog"]["properties"]
            .as_object()
            .expect("AuditLog properties are an object")
            .iter()
            .filter(|(key, _)| key.contains('.'))
            .map(|(key, schema)| {
                // `login.succeeded`/`logout.succeeded` pin an explicitly empty
                // object ("no fields beyond the standard attributes"), which
                // the Rust side types as free-form `AdminJsonObject`; their
                // field set is empty rather than absent.
                let fields = schema["properties"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                (key.clone(), Value::Object(fields))
            })
            .collect()
    }

    /// Sample value matching a pinned payload property schema, recursively for
    /// nested objects and array items.
    fn audit_sample_value(schema: &Value) -> Value {
        if let Some(values) = schema["enum"].as_array() {
            return values
                .first()
                .cloned()
                .expect("pinned enum property carries a value");
        }
        match schema["type"].as_str().expect("payload property is typed") {
            "string" => json!("sample"),
            "integer" => json!(1),
            "number" => json!(0.5),
            "boolean" => json!(true),
            "array" => json!([audit_sample_value(&schema["items"])]),
            "object" => match schema["properties"].as_object() {
                Some(properties) => Value::Object(
                    properties
                        .iter()
                        .map(|(field, sub)| (field.clone(), audit_sample_value(sub)))
                        .collect(),
                ),
                // A propertyless pinned object is free-form on the Rust side
                // (`AdminJsonObject`); the empty object is its neutral value.
                None => json!({}),
            },
            other => panic!("no audit sample for pinned type {other}"),
        }
    }

    /// Value whose JSON type mismatches a pinned payload property schema.
    ///
    /// A typed Rust field must reject it at decode time; only the lossless
    /// `extra` fallback accepts anything, and it never errors — so a decode
    /// failure proves the pinned wire name is consumed by a typed field with
    /// exactly that spelling.
    fn audit_mismatched_value(schema: &Value) -> Value {
        let typed = if schema["enum"].as_array().is_some() {
            "string"
        } else {
            schema["type"].as_str().expect("payload property is typed")
        };
        match typed {
            "string" => json!(false),
            "integer" | "number" => json!("not-a-number"),
            "boolean" => json!("not-a-boolean"),
            "array" | "object" => json!(1),
            other => panic!("no mismatched value for pinned type {other}"),
        }
    }

    /// Minimal official audit envelope carrying one typed payload.
    fn audit_envelope(key: &str, payload: Value) -> Value {
        let mut object = Map::new();
        object.insert("id".to_owned(), json!("audit_parity"));
        object.insert("type".to_owned(), Value::from(key));
        object.insert("effective_at".to_owned(), json!(0));
        object.insert(key.to_owned(), payload);
        Value::Object(object)
    }

    #[test]
    fn admin_audit_event_payloads_match_openapi() {
        let (event_types, payload_keys, envelope_keys) = pinned_audit_wire_inventory();
        // The pin carries 147 official event types and 55 dotted payload keys;
        // the shared envelope owns every remaining `AuditLog` property. These
        // counts guard a pin swap from silently rescoping the inventory.
        assert_eq!(event_types.len(), 147);
        assert_eq!(payload_keys.len(), 55);
        assert_eq!(
            envelope_keys,
            ["actor", "effective_at", "id", "project", "type"]
        );

        // OpenAPI -> Rust: every pinned event type is known and round-trips
        // through the exact wire spelling.
        for wire in &event_types {
            let parsed = AuditEventType::from_raw(wire.as_str());
            assert!(parsed.is_known(), "{wire} must be a known official type");
            assert_eq!(parsed.as_str(), wire.as_str());
        }
        assert!(!AuditEventType::from_raw("audit.future.event").is_known());

        // Every dotted payload key is itself an official event type, and must
        // be consumed by a typed `AuditLog` field rather than `extra`. The
        // field-level probe below is decisive in both directions: decoding a
        // complete pin-derived fixture proves each typed field accepts the
        // pinned JSON type (a wrong Rust type rejects the sample), while a
        // type-mismatched value under a pinned field name can only survive
        // decode by falling into the lossless `extra` fallback — so a decode
        // error proves the exact wire spelling is typed (8-08).
        let event_set: HashSet<&str> = event_types.iter().map(String::as_str).collect();
        let payload_properties = pinned_audit_payload_properties();
        let mut probed_fields = 0_usize;
        for key in &payload_keys {
            assert!(
                event_set.contains(key.as_str()),
                "{key} must be an official event type"
            );
            let fields = payload_properties[key]
                .as_object()
                .unwrap_or_else(|| panic!("{key} carries pinned payload properties"));
            let mut payload = Map::new();
            for (field, schema) in fields {
                payload.insert(field.clone(), audit_sample_value(schema));
            }
            let envelope = audit_envelope(key, Value::Object(payload));
            let audit = ok(serde_json::from_value::<AuditLog>(envelope.clone()));
            assert_eq!(audit.kind.as_str(), key.as_str());
            assert!(
                !audit.extra().contains_key(key),
                "{key} must be typed on AuditLog, not extra"
            );
            assert_eq!(ok(serde_json::to_value(&audit)), envelope);

            for (field, schema) in fields {
                probed_fields += 1;
                let mut probe = Map::new();
                probe.insert(field.clone(), audit_mismatched_value(schema));
                assert!(
                    serde_json::from_value::<AuditLog>(audit_envelope(key, Value::Object(probe)))
                        .is_err(),
                    "{key}.{field} must be a typed field with this exact wire spelling"
                );
            }
        }
        // The pin carries 120 payload fields across the 55 typed payloads;
        // this guards a pin swap from silently narrowing the probe.
        assert_eq!(probed_fields, 120);

        // Rust -> OpenAPI control: a dotted key the pin does not carry is not
        // typed and stays in `extra` losslessly, so the typed set observed by
        // decode is exactly the pinned 55 keys.
        let future = json!({
            "id": "audit_future",
            "type": "api_key.created",
            "effective_at": 0,
            "audit.future.payload": {"future": true}
        });
        let decoded = ok(serde_json::from_value::<AuditLog>(future.clone()));
        assert!(decoded.extra().contains_key("audit.future.payload"));
        assert_eq!(ok(serde_json::to_value(decoded)), future);

        let fixture = json!({
            "id": "req_xxx_20240101",
            "type": "api_key.created",
            "effective_at": 1_720_804_090,
            "actor": {
                "type": "session",
                "session": {
                    "user": {
                        "id": "user-xxx",
                        "email": "user@example.com"
                    },
                    "ip_address": "127.0.0.1",
                    "user_agent": "Mozilla/5.0"
                }
            },
            "api_key.created": {
                "id": "key_xxxx",
                "data": {
                    "scopes": ["resource.operation"],
                    "data_future": true
                }
            }
        });
        let audit = ok(serde_json::from_value::<AuditLog>(fixture.clone()));
        assert!(matches!(audit.kind, AuditEventType::ApiKeyCreated));
        match &audit.api_key_created {
            Omittable::Value(payload) => {
                assert_eq!(payload.id, Omittable::Value("key_xxxx".to_owned()));
                match &payload.data {
                    Omittable::Value(data) => {
                        assert_eq!(
                            data.scopes,
                            Omittable::Value(vec!["resource.operation".to_owned()])
                        );
                        assert!(data.extra().contains_key("data_future"));
                    }
                    Omittable::Omitted => panic!("official api_key.created.data must decode"),
                }
            }
            Omittable::Omitted => panic!("official api_key.created must be typed"),
        }
        match &audit.actor {
            Omittable::Value(Nullable::Value(actor)) => match &actor.session {
                Omittable::Value(session) => {
                    assert!(session.extra().contains_key("user_agent"));
                }
                Omittable::Omitted => panic!("official session actor must decode"),
            },
            other => panic!("official actor must decode, got {other:?}"),
        }
        assert!(!audit.extra().contains_key("api_key.created"));
        assert_eq!(ok(serde_json::to_value(&audit)), fixture);

        let archived = ok(serde_json::from_value::<AuditLog>(json!({
            "id": "audit_2",
            "type": "project.archived",
            "effective_at": 11,
            "project.archived": {"id": "proj_1"}
        })));
        match archived.project_archived {
            Omittable::Value(payload) => {
                assert_eq!(payload.id, Omittable::Value("proj_1".to_owned()));
            }
            Omittable::Omitted => panic!("official project.archived must be typed"),
        }

        let bound = ok(serde_json::from_value::<AuditLog>(json!({
            "id": "audit_3",
            "type": "role.bound_to_resource",
            "effective_at": 12,
            "role.bound_to_resource": {
                "source": "connector_publish",
                "enabled": true
            }
        })));
        match bound.role_bound_to_resource {
            Omittable::Value(payload) => {
                assert!(matches!(
                    payload.source,
                    Omittable::Value(AuditRoleBindingSource::ConnectorPublish)
                ));
                assert_eq!(payload.enabled, Omittable::Value(true));
            }
            Omittable::Omitted => panic!("official role.bound_to_resource must be typed"),
        }

        let deleted = ok(serde_json::from_value::<AuditLog>(json!({
            "id": "audit_4",
            "type": "certificate.deleted",
            "effective_at": 13,
            "certificate.deleted": {
                "id": "cert_1",
                "name": "leaf",
                "certificate": "-----BEGIN CERTIFICATE----- secret"
            }
        })));
        assert!(!format!("{deleted:?}").contains("secret"));
        match &deleted.certificate_deleted {
            Omittable::Value(payload) => match &payload.certificate {
                Omittable::Value(pem) => {
                    assert_eq!(
                        pem.with_exposed(ToOwned::to_owned),
                        "-----BEGIN CERTIFICATE----- secret"
                    );
                }
                Omittable::Omitted => panic!("official PEM must decode"),
            },
            Omittable::Omitted => panic!("official certificate.deleted must be typed"),
        }

        assert!(
            serde_json::from_value::<AuditLog>(json!({
                "id": "audit_5",
                "type": "api_key.created",
                "effective_at": 14,
                "api_key.created": {"id": null}
            }))
            .is_err(),
            "official api_key.created.id is a non-null string"
        );

        let api_actor = ok(serde_json::from_value::<AuditLog>(json!({
            "id": "audit_6",
            "type": "login.succeeded",
            "effective_at": 15,
            "actor": {
                "type": "api_key",
                "api_key": {
                    "type": "service_account",
                    "id": "key_sa"
                }
            },
            "login.succeeded": {}
        })));
        match api_actor.actor {
            Omittable::Value(Nullable::Value(actor)) => match actor.api_key {
                Omittable::Value(key) => {
                    assert!(matches!(
                        key.kind,
                        Omittable::Value(AuditActorApiKeyType::ServiceAccount)
                    ));
                }
                Omittable::Omitted => panic!("official api_key actor must decode"),
            },
            other => panic!("official actor must decode, got {other:?}"),
        }
        assert!(matches!(api_actor.login_succeeded, Omittable::Value(_)));
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
                "model": "gpt-5.6-sol",
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
                        "object": "organization.usage.file_searches.result",
                        "num_requests": 2,
                        "vector_store_id": null
                    },
                    {
                        "object": "organization.usage.web_searches.result",
                        "num_model_requests": 1,
                        "num_requests": 3
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
        assert!(matches!(
            page.data[0].results[2],
            UsageResult::FileSearchCalls(_)
        ));
        assert!(matches!(
            page.data[0].results[3],
            UsageResult::WebSearchCalls(_)
        ));
        match &page.data[0].results[4] {
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

    /// Pinned usage-result branch inventory derived from the frozen
    /// discriminator manifest and OpenAPI document.
    ///
    /// Mirrors the realtime parity derivation (`realtime.rs`): the eleven
    /// `UsageTimeBucket` branch references come from
    /// `spec/contracts/discriminators.json`, and each wire tag plus required
    /// field set from the pinned schema itself, so the parity test below
    /// asserts against the pins instead of a hand-copied list (8-07).
    fn pinned_usage_result_branches() -> Vec<(String, Vec<String>)> {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/discriminators.json"))
                .expect("pinned discriminator manifest is valid JSON");
        let openapi: Value = serde_json::from_str(include_str!(
            "../../../spec/upstream/openapi-2026-08-29.json"
        ))
        .expect("pinned OpenAPI is valid JSON");
        let schemas = openapi["components"]["schemas"]
            .as_object()
            .expect("OpenAPI schemas are an object");
        let entry = manifest["entries"]
            .as_array()
            .expect("manifest entries are an array")
            .iter()
            .find(|entry| entry["schema"].as_str() == Some("UsageTimeBucket"))
            .expect("UsageTimeBucket discriminator entry is present");
        entry["branch_refs"]
            .as_array()
            .expect("branch_refs is an array")
            .iter()
            .map(|reference| {
                let branch = reference
                    .as_str()
                    .and_then(|reference| reference.rsplit('/').next())
                    .expect("branch ref has a final component");
                let schema = &schemas[branch];
                let tag = schema["properties"]["object"]["enum"]
                    .as_array()
                    .and_then(|values| values.first())
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{branch} has one object enum"))
                    .to_owned();
                let required = schema["required"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{branch} carries a required array"))
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .unwrap_or_else(|| panic!("{branch} required entries are strings"))
                            .to_owned()
                    })
                    .collect();
                (tag, required)
            })
            .collect()
    }

    /// Wire tag routed by a decoded usage result, including the future lane.
    fn usage_result_variant_tag(result: &UsageResult) -> String {
        match result {
            UsageResult::Completions(_) => "organization.usage.completions.result",
            UsageResult::Embeddings(_) => "organization.usage.embeddings.result",
            UsageResult::Moderations(_) => "organization.usage.moderations.result",
            UsageResult::Images(_) => "organization.usage.images.result",
            UsageResult::AudioSpeeches(_) => "organization.usage.audio_speeches.result",
            UsageResult::AudioTranscriptions(_) => "organization.usage.audio_transcriptions.result",
            UsageResult::VectorStores(_) => "organization.usage.vector_stores.result",
            UsageResult::CodeInterpreterSessions(_) => {
                "organization.usage.code_interpreter_sessions.result"
            }
            UsageResult::FileSearchCalls(_) => "organization.usage.file_searches.result",
            UsageResult::WebSearchCalls(_) => "organization.usage.web_searches.result",
            UsageResult::Costs(_) => "organization.costs.result",
            UsageResult::Unknown(value) => return value.discriminator().to_owned(),
        }
        .to_owned()
    }

    /// Sample value for a pinned required metric; every non-`object` required
    /// field across the eleven branches is an integer count.
    fn usage_required_value(field: &str) -> Value {
        match field {
            "characters" | "seconds" | "num_model_requests" | "num_sessions" | "input_tokens"
            | "output_tokens" | "num_requests" | "images" | "usage_bytes" => json!(1),
            other => panic!("no sample value for pinned usage field {other}"),
        }
    }

    #[test]
    fn usage_result_routes_every_pinned_branch_and_futures_to_unknown() {
        let branches = pinned_usage_result_branches();
        // Ten usage metrics plus the shared costs result.
        assert_eq!(branches.len(), 11, "the pin carries eleven result branches");
        let mut routed_tags = HashSet::new();

        for (tag, required) in &branches {
            let mut object = Map::new();
            object.insert("object".to_owned(), Value::from(tag.clone()));
            for field in required.iter().filter(|field| field.as_str() != "object") {
                object.insert(field.clone(), usage_required_value(field));
            }
            let fixture = Value::Object(object);
            let decoded = ok(serde_json::from_value::<UsageResult>(fixture.clone()));
            assert_eq!(
                usage_result_variant_tag(&decoded),
                tag.as_str(),
                "the pinned wire tag must route to its typed variant"
            );
            assert_eq!(ok(serde_json::to_value(&decoded)), fixture);
            assert!(routed_tags.insert(tag.clone()), "branch tags are unique");

            // The Rust required-field set may not be looser than the pin:
            // dropping any pinned required metric must fail the decode
            // instead of silently defaulting to zero.
            for field in required.iter().filter(|field| field.as_str() != "object") {
                let mut reduced = fixture.as_object().expect("usage fixture object").clone();
                assert!(reduced.remove(field).is_some());
                assert!(
                    serde_json::from_value::<UsageResult>(Value::Object(reduced)).is_err(),
                    "{tag} must require {field}"
                );
            }
        }

        // A future discriminator stays `Unknown`, keeps its exact tag, and
        // round-trips verbatim.
        let future = json!({"object": "organization.usage.future.result", "units": 7});
        match ok(serde_json::from_value::<UsageResult>(future.clone())) {
            UsageResult::Unknown(value) => {
                assert_eq!(value.discriminator(), "organization.usage.future.result");
            }
            _ => panic!("future usage result must remain unknown"),
        }
        assert_eq!(
            ok(serde_json::to_value(ok(serde_json::from_value::<
                UsageResult,
            >(future.clone())))),
            future
        );
    }

    #[test]
    fn usage_query_requires_start_time_and_preserves_pagination() {
        assert!(serde_json::from_value::<UsageQueryParams>(json!({})).is_err());
        let params = UsageQueryParams::new(100);
        assert_eq!(ok(serde_json::to_value(params)), json!({"start_time": 100}));
    }

    #[test]
    fn usage_group_by_pins_the_ten_value_union_of_the_usage_endpoints() {
        // Union of the pinned per-endpoint `group_by` item enums; the field
        // set matches `UsageDimensions`, and the costs-only `line_item` stays
        // outside this domain (it belongs to `UsageCostsGroupBy`).
        const OFFICIAL_USAGE_GROUP_BY: [(&str, UsageGroupBy); 10] = [
            ("project_id", UsageGroupBy::ProjectId),
            ("user_id", UsageGroupBy::UserId),
            ("api_key_id", UsageGroupBy::ApiKeyId),
            ("model", UsageGroupBy::Model),
            ("batch", UsageGroupBy::Batch),
            ("service_tier", UsageGroupBy::ServiceTier),
            ("source", UsageGroupBy::Source),
            ("size", UsageGroupBy::Size),
            ("vector_store_id", UsageGroupBy::VectorStoreId),
            ("context_level", UsageGroupBy::ContextLevel),
        ];
        for (value, expected) in OFFICIAL_USAGE_GROUP_BY {
            let decoded = UsageGroupBy::from_raw(value);
            assert!(
                decoded.is_known(),
                "official usage group_by {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }
        for costs_only in ["line_item", "cost_center"] {
            let decoded = UsageGroupBy::from_raw(costs_only);
            assert!(
                !decoded.is_known(),
                "usage group_by `{costs_only}` is outside the pinned usage union"
            );
            assert_eq!(decoded.as_str(), costs_only);
        }

        let usage = UsageQueryParams {
            group_by: Omittable::Value(vec![
                UsageGroupBy::ProjectId,
                UsageGroupBy::Model,
                UsageGroupBy::ServiceTier,
                UsageGroupBy::from_raw("future_dimension"),
            ]),
            ..UsageQueryParams::new(100)
        };
        assert_eq!(
            ok(serde_json::to_value(&usage)),
            json!({
                "start_time": 100,
                "group_by": ["project_id", "model", "service_tier", "future_dimension"]
            })
        );

        // Decode stays lossless for known and future dimensions alike.
        let decoded = ok(serde_json::from_value::<UsageQueryParams>(json!({
            "start_time": 100,
            "group_by": ["batch", "context_level", "line_item"]
        })));
        match decoded.group_by {
            Omittable::Value(groups) => {
                assert!(groups[0].is_known());
                assert!(groups[1].is_known());
                assert!(!groups[2].is_known());
                assert_eq!(groups[2].as_str(), "line_item");
            }
            Omittable::Omitted => panic!("usage group_by must decode"),
        }
        assert!(
            serde_json::from_value::<UsageQueryParams>(json!({
                "start_time": 100,
                "group_by": null
            }))
            .is_err()
        );
    }

    #[test]
    fn usage_query_pins_image_and_web_search_filter_enums() {
        const OFFICIAL_IMAGE_SOURCES: [(&str, UsageImageSource); 3] = [
            ("image.generation", UsageImageSource::Generation),
            ("image.edit", UsageImageSource::Edit),
            ("image.variation", UsageImageSource::Variation),
        ];
        for (value, expected) in OFFICIAL_IMAGE_SOURCES {
            let decoded = UsageImageSource::from_raw(value);
            assert!(
                decoded.is_known(),
                "official image source {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }

        // The pinned `sizes` enum carries `1792x1792` (a square), unlike the
        // generation-side `crate::media::ImageSize` which has `1792x1024`.
        const OFFICIAL_IMAGE_SIZES: [(&str, UsageImageSize); 5] = [
            ("256x256", UsageImageSize::Square256),
            ("512x512", UsageImageSize::Square512),
            ("1024x1024", UsageImageSize::Square1024),
            ("1792x1792", UsageImageSize::Square1792),
            ("1024x1792", UsageImageSize::Portrait1024x1792),
        ];
        for (value, expected) in OFFICIAL_IMAGE_SIZES {
            let decoded = UsageImageSize::from_raw(value);
            assert!(
                decoded.is_known(),
                "official usage size {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }
        assert!(
            !UsageImageSize::from_raw("1792x1024").is_known(),
            "1792x1024 belongs to the generation-side ImageSize domain"
        );

        const OFFICIAL_CONTEXT_LEVELS: [(&str, UsageContextLevel); 3] = [
            ("low", UsageContextLevel::Low),
            ("medium", UsageContextLevel::Medium),
            ("high", UsageContextLevel::High),
        ];
        for (value, expected) in OFFICIAL_CONTEXT_LEVELS {
            let decoded = UsageContextLevel::from_raw(value);
            assert!(
                decoded.is_known(),
                "official context level {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }

        // Future service values stay lossless through the open enums.
        let usage = UsageQueryParams {
            sources: Omittable::Value(vec![
                UsageImageSource::Edit,
                UsageImageSource::from_raw("image.future"),
            ]),
            sizes: Omittable::Value(vec![UsageImageSize::from_raw("2048x2048")]),
            context_levels: Omittable::Value(vec![
                UsageContextLevel::Low,
                UsageContextLevel::from_raw("auto"),
            ]),
            ..UsageQueryParams::new(100)
        };
        assert_eq!(
            ok(serde_json::to_value(&usage)),
            json!({
                "start_time": 100,
                "sources": ["image.edit", "image.future"],
                "sizes": ["2048x2048"],
                "context_levels": ["low", "auto"]
            })
        );

        let decoded = ok(serde_json::from_value::<UsageQueryParams>(json!({
            "start_time": 100,
            "sources": ["image.generation"],
            "sizes": ["1792x1792"],
            "context_levels": ["medium"]
        })));
        match (decoded.sources, decoded.sizes, decoded.context_levels) {
            (
                Omittable::Value(sources),
                Omittable::Value(sizes),
                Omittable::Value(context_levels),
            ) => {
                assert!(sources[0].is_known());
                assert!(sizes[0].is_known());
                assert_eq!(sizes[0].as_str(), "1792x1792");
                assert!(context_levels[0].is_known());
            }
            _ => panic!("official usage image/web-search filters must decode"),
        }
        assert!(
            serde_json::from_value::<UsageQueryParams>(json!({
                "start_time": 100,
                "context_levels": null
            }))
            .is_err()
        );
    }

    #[test]
    fn admin_list_object_pins_the_single_list_constant() {
        assert_eq!(AdminListObject::from_raw("list"), AdminListObject::List);
        // Every pinned Administration list envelope uses the `list` constant;
        // the usage/costs `page` envelope is carried by `UsagePageTag`, so
        // `page` must not be a named `AdminListObject` variant anymore.
        assert!(!AdminListObject::from_raw("page").is_known());
        assert_eq!(AdminListObject::from_raw("page").as_str(), "page");
    }

    #[test]
    fn usage_costs_query_pins_one_day_bucket_and_three_value_group_by() {
        assert!(serde_json::from_value::<UsageCostsQueryParams>(json!({})).is_err());
        let params = UsageCostsQueryParams::new(100);
        assert_eq!(ok(serde_json::to_value(params)), json!({"start_time": 100}));

        // GET /organization/costs supports only `1d`; the usage-side 1m/1h
        // widths fall to Unknown on the costs domain but stay known on the
        // shared usage domain.
        assert!(UsageCostsBucketWidth::from_raw("1d").is_known());
        for usage_only in ["1m", "1h"] {
            assert!(
                !UsageCostsBucketWidth::from_raw(usage_only).is_known(),
                "costs bucket width `{usage_only}` belongs to the usage endpoints only"
            );
            assert!(UsageBucketWidth::from_raw(usage_only).is_known());
        }

        const OFFICIAL_COSTS_GROUP_BY: [(&str, UsageCostsGroupBy); 3] = [
            ("project_id", UsageCostsGroupBy::ProjectId),
            ("line_item", UsageCostsGroupBy::LineItem),
            ("api_key_id", UsageCostsGroupBy::ApiKeyId),
        ];
        for (value, expected) in OFFICIAL_COSTS_GROUP_BY {
            let decoded = UsageCostsGroupBy::from_raw(value);
            assert!(
                decoded.is_known(),
                "official costs group_by {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }
        for usage_only in ["user_id", "model", "batch", "service_tier"] {
            assert!(
                !UsageCostsGroupBy::from_raw(usage_only).is_known(),
                "costs group_by `{usage_only}` belongs to the usage endpoints only"
            );
        }

        let costs = UsageCostsQueryParams {
            start_time: 100,
            end_time: Omittable::Value(200),
            bucket_width: Omittable::Value(UsageCostsBucketWidth::Day),
            project_ids: Omittable::Value(vec!["proj_1".to_owned()]),
            api_key_ids: Omittable::Value(vec!["key_1".to_owned()]),
            group_by: Omittable::Value(vec![
                UsageCostsGroupBy::ProjectId,
                UsageCostsGroupBy::LineItem,
                UsageCostsGroupBy::ApiKeyId,
            ]),
            limit: Omittable::Value(7),
            page: Omittable::Value("cursor_1".to_owned()),
        };
        assert_eq!(
            ok(serde_json::to_value(&costs)),
            json!({
                "start_time": 100,
                "end_time": 200,
                "bucket_width": "1d",
                "project_ids": ["proj_1"],
                "api_key_ids": ["key_1"],
                "group_by": ["project_id", "line_item", "api_key_id"],
                "limit": 7,
                "page": "cursor_1"
            })
        );
        assert!(
            serde_json::from_value::<UsageCostsQueryParams>(json!({
                "start_time": 100,
                "group_by": null
            }))
            .is_err()
        );

        // Future service values still decode losslessly through the open enums.
        let future = ok(serde_json::from_value::<UsageCostsQueryParams>(json!({
            "start_time": 100,
            "bucket_width": "1w",
            "group_by": ["cost_center"]
        })));
        match future.bucket_width {
            Omittable::Value(width) => {
                assert!(!width.is_known());
                assert_eq!(width.as_str(), "1w");
            }
            _ => panic!("costs bucket width must decode"),
        }
        match future.group_by {
            Omittable::Value(groups) => {
                assert_eq!(groups.len(), 1);
                assert!(!groups[0].is_known());
                assert_eq!(groups[0].as_str(), "cost_center");
            }
            _ => panic!("costs group_by must decode"),
        }
    }

    #[test]
    fn admin_query_filters_match_openapi() {
        let page = AdminListParams {
            before: Omittable::Value("cursor_0".to_owned()),
            emails: Omittable::Value(vec!["user@example.com".to_owned()]),
            include_archived: Omittable::Value(true),
            owner_project_access: Omittable::Value(ProjectAccessFilter::Any),
            ..AdminListParams::default()
        };
        let encoded = ok(serde_json::to_value(&page));
        assert_eq!(
            encoded,
            json!({
                "before": "cursor_0",
                "emails": ["user@example.com"],
                "include_archived": true,
                "owner_project_access": "any"
            })
        );
        assert_eq!(ProjectAccessFilter::Any.as_str(), "any");
        assert!(serde_json::from_value::<AdminListParams>(json!({"before": null})).is_err());
        assert!(serde_json::from_value::<AdminListParams>(json!({"emails": null})).is_err());
        assert!(
            serde_json::from_value::<AdminListParams>(json!({"include_archived": null})).is_err()
        );
        assert!(
            serde_json::from_value::<AdminListParams>(json!({"owner_project_access": null}))
                .is_err()
        );

        let audit = AuditLogListParams {
            actor_emails: Omittable::Value(vec!["actor@example.com".to_owned()]),
            resource_ids: Omittable::Value(vec!["proj_1".to_owned()]),
            tenant_only: Omittable::Value(true),
            ..AuditLogListParams::default()
        };
        let encoded = ok(serde_json::to_value(&audit));
        assert_eq!(
            encoded,
            json!({
                "actor_emails": ["actor@example.com"],
                "resource_ids": ["proj_1"],
                "tenant_only": true
            })
        );
        assert!(
            serde_json::from_value::<AuditLogListParams>(json!({"actor_emails": null})).is_err()
        );
        assert!(
            serde_json::from_value::<AuditLogListParams>(json!({"resource_ids": null})).is_err()
        );
        assert!(
            serde_json::from_value::<AuditLogListParams>(json!({"tenant_only": null})).is_err()
        );

        let usage = UsageQueryParams {
            sources: Omittable::Value(vec![UsageImageSource::Generation]),
            sizes: Omittable::Value(vec![UsageImageSize::Square1024]),
            vector_store_ids: Omittable::Value(vec!["vs_1".to_owned()]),
            context_levels: Omittable::Value(vec![UsageContextLevel::High]),
            ..UsageQueryParams::new(100)
        };
        let encoded = ok(serde_json::to_value(&usage));
        assert_eq!(encoded["start_time"], 100);
        assert_eq!(encoded["sources"], json!(["image.generation"]));
        assert_eq!(encoded["sizes"], json!(["1024x1024"]));
        assert_eq!(encoded["vector_store_ids"], json!(["vs_1"]));
        assert_eq!(encoded["context_levels"], json!(["high"]));
        assert!(
            serde_json::from_value::<UsageQueryParams>(json!({
                "start_time": 100,
                "sources": null
            }))
            .is_err()
        );

        let certificate = CertificateGetParams {
            include: Omittable::Value(vec![CertificateInclude::Content]),
        };
        assert_eq!(
            ok(serde_json::to_value(&certificate)),
            json!({"include": ["content"]})
        );
        assert!(serde_json::from_value::<CertificateGetParams>(json!({"include": null})).is_err());

        let group = ProjectGroupGetParams {
            group_type: Omittable::Value(GroupType::TenantGroup),
        };
        assert_eq!(
            ok(serde_json::to_value(&group)),
            json!({"group_type": "tenant_group"})
        );
        assert!(
            serde_json::from_value::<ProjectGroupGetParams>(json!({"group_type": null})).is_err()
        );
    }

    #[test]
    fn admin_typed_objects_match_openapi() {
        let owner = ok(serde_json::from_value::<ProjectApiKeyOwner>(json!({
            "type": "user",
            "user": {
                "id": "user_1",
                "email": "owner@example.com",
                "name": "Owner",
                "created_at": 1,
                "role": "owner"
            }
        })));
        match &owner.kind {
            Omittable::Value(kind) => assert_eq!(kind.as_str(), "user"),
            Omittable::Omitted => panic!("official owner.type must decode"),
        }
        match &owner.user {
            Omittable::Value(user) => assert_eq!(user.email, "owner@example.com"),
            Omittable::Omitted => panic!("official owner.user must decode"),
        }
        assert!(
            serde_json::from_value::<ProjectApiKeyOwnerUser>(json!({
                "id": "user_1",
                "email": null,
                "name": "Owner",
                "created_at": 1,
                "role": "owner"
            }))
            .is_err()
        );

        let invite = ok(serde_json::from_value::<Invite>(json!({
            "object": "organization.invite",
            "id": "inv_1",
            "email": "user@example.com",
            "role": "owner",
            "status": "pending",
            "created_at": 1,
            "projects": [{"id": "proj_1", "role": "member"}]
        })));
        assert_eq!(invite.projects[0].id, "proj_1");
        assert_eq!(invite.projects[0].role.as_str(), "member");

        let request = InviteRequest {
            email: "user@example.com".to_owned(),
            role: InviteRole::Owner,
            projects: Omittable::Value(vec![InviteProjectMembership {
                id: "proj_1".to_owned(),
                role: InviteProjectRole::Owner,
                extra: ExtraFields::default(),
            }]),
        };
        assert_eq!(
            ok(serde_json::to_value(&request)),
            json!({
                "email": "user@example.com",
                "role": "owner",
                "projects": [{"id": "proj_1", "role": "owner"}]
            })
        );

        let group = ok(serde_json::from_value::<GroupRoleAssignment>(json!({
            "object": "group.role",
            "group": {
                "object": "group",
                "id": "group_1",
                "name": "Support",
                "created_at": 1,
                "scim_managed": false
            },
            "role": {
                "object": "role",
                "id": "role_1",
                "name": "auditor",
                "description": null,
                "permissions": [],
                "resource_type": "organization",
                "predefined_role": false
            }
        })));
        assert_eq!(group.object.as_str(), "group.role");
        assert!(!group.group.scim_managed);

        let user = ok(serde_json::from_value::<User>(json!({
            "object": "organization.user",
            "id": "user_1",
            "added_at": 1,
            "user": {
                "object": "user",
                "id": "nested_1",
                "email": null,
                "picture": null
            },
            "projects": {
                "object": "list",
                "data": [{"id": null, "name": "proj", "role": "member"}]
            }
        })));
        match &user.user {
            Omittable::Value(nested) => {
                assert!(matches!(nested.email, Omittable::Value(Nullable::Null)));
            }
            Omittable::Omitted => panic!("official User.user must decode"),
        }
        match &user.projects {
            Omittable::Value(Nullable::Value(projects)) => {
                assert!(matches!(
                    projects.data[0].id,
                    Omittable::Value(Nullable::Null)
                ));
            }
            _ => panic!("official User.projects list must decode"),
        }
        assert!(
            serde_json::from_value::<User>(json!({
                "object": "organization.user",
                "id": "user_1",
                "added_at": 1,
                "user": {"object": "user", "id": "nested_1", "email": null, "email_future": true}
            }))
            .is_ok()
        );

        let details = ok(serde_json::from_value::<AssignedRoleDetails>(json!({
            "id": "role_1",
            "name": "auditor",
            "permissions": [],
            "resource_type": "organization",
            "predefined_role": false,
            "description": null,
            "created_at": null,
            "updated_at": null,
            "created_by": null,
            "created_by_user_obj": null,
            "metadata": null,
            "assignment_sources": null
        })));
        assert!(matches!(details.created_at, Nullable::Null));
        assert!(matches!(details.assignment_sources, Nullable::Null));

        let listed_users = ok(serde_json::from_value::<UserListResource>(json!({
            "object": "list",
            "data": [{
                "id": "user_abc123",
                "name": "Ada Lovelace",
                "email": "ada@example.com"
            }],
            "has_more": false,
            "next": null
        })));
        assert_eq!(listed_users.data[0].id, "user_abc123");
        assert!(matches!(
            listed_users.data[0].email,
            Nullable::Value(ref email) if email == "ada@example.com"
        ));
        assert!(
            serde_json::from_value::<UserListResource>(json!({
                "object": "list",
                "data": [{
                    "id": "user_abc123",
                    "name": "Ada Lovelace",
                    "email": null,
                    "picture": null,
                    "is_service_account": false,
                    "user_type": "user"
                }],
                "has_more": false,
                "next": null
            }))
            .is_ok(),
            "official retrieve-only GroupMemberUser fields stay lossless extras on list items"
        );
        assert!(
            serde_json::from_value::<GroupMemberUser>(json!({
                "id": "user_abc123",
                "name": "Ada Lovelace",
                "email": "ada@example.com"
            }))
            .is_err(),
            "retrieve-group-user still requires official GroupMemberUser fields"
        );

        let assigned_roles = ok(serde_json::from_value::<RoleListResource>(json!({
            "object": "list",
            "data": [{
                "id": "role_01J1F8ROLE01",
                "name": "API Group Manager",
                "permissions": ["api.groups.read", "api.groups.write"],
                "resource_type": "api.organization",
                "predefined_role": false,
                "description": "Allows managing organization groups",
                "created_at": 1711471533,
                "updated_at": 1711472599,
                "created_by": "user_abc123",
                "created_by_user_obj": {
                    "id": "user_abc123",
                    "name": "Ada Lovelace",
                    "email": "ada@example.com"
                },
                "metadata": {},
                "assignment_sources": null
            }],
            "has_more": false,
            "next": null
        })));
        assert_eq!(assigned_roles.data[0].id, "role_01J1F8ROLE01");
        assert!(matches!(
            assigned_roles.data[0].assignment_sources,
            Nullable::Null
        ));
        assert!(
            serde_json::from_value::<PublicRoleListResource>(json!({
                "object": "list",
                "data": [{
                    "object": "role",
                    "id": "role_01J1F8ROLE01",
                    "name": "API Group Manager",
                    "description": "Allows managing organization groups",
                    "permissions": ["api.groups.read", "api.groups.write"],
                    "resource_type": "api.organization",
                    "predefined_role": false
                }],
                "has_more": false,
                "next": null
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<PublicRoleListResource>(json!({
                "object": "list",
                "data": [{
                    "id": "role_01J1F8ROLE01",
                    "name": "API Group Manager",
                    "permissions": ["api.groups.read", "api.groups.write"],
                    "resource_type": "api.organization",
                    "predefined_role": false,
                    "description": "Allows managing organization groups",
                    "created_at": 1711471533,
                    "updated_at": 1711472599,
                    "created_by": "user_abc123",
                    "created_by_user_obj": null,
                    "metadata": {},
                    "assignment_sources": null
                }],
                "has_more": false,
                "next": null
            }))
            .is_err(),
            "public role lists still require official Role.object"
        );

        let actor = ok(serde_json::from_value::<AuditActorApiKey>(json!({
            "type": "service_account",
            "id": "key_1",
            "service_account": {"id": "sa_1"}
        })));
        match &actor.service_account {
            Omittable::Value(account) => match &account.id {
                Omittable::Value(id) => assert_eq!(id, "sa_1"),
                Omittable::Omitted => panic!("official service_account.id must decode"),
            },
            Omittable::Omitted => panic!("official service_account must decode"),
        }

        assert_eq!(AssignmentObject::GroupRole.as_str(), "group.role");
        assert_eq!(AssignmentObject::UserRole.as_str(), "user.role");
        assert_eq!(AssignmentObject::GroupUser.as_str(), "group.user");
        assert_eq!(
            AssignmentObject::GroupUserDeleted.as_str(),
            "group.user.deleted"
        );
        // The pinned `DeletedRoleAssignmentResource.object` description cites
        // `group.role.deleted` / `user.role.deleted` as examples, so they stay
        // named variants on the open enum.
        assert_eq!(
            AssignmentObject::GroupRoleDeleted.as_str(),
            "group.role.deleted"
        );
        assert_eq!(
            AssignmentObject::UserRoleDeleted.as_str(),
            "user.role.deleted"
        );
        // The `organization.*.assignment` discriminators exist nowhere in the
        // pin and were fabricated; they must decode as Unknown, losslessly.
        for phantom in [
            "organization.group.user.assignment",
            "organization.group.role.assignment",
            "organization.user.role.assignment",
            "organization.role.assignment.deleted",
        ] {
            let decoded = AssignmentObject::from_raw(phantom);
            assert!(
                !decoded.is_known(),
                "assignment object {phantom} has no baseline and must not be a named variant"
            );
            assert_eq!(decoded.as_str(), phantom);
        }
    }

    #[test]
    fn official_certificate_scope_object_names_all_pin_members() {
        const OFFICIAL_SCOPE_OBJECTS: [(&str, CertificateScopeObject); 4] = [
            (
                "organization.certificate.activation",
                CertificateScopeObject::OrganizationActivation,
            ),
            (
                "organization.certificate.deactivation",
                CertificateScopeObject::OrganizationDeactivation,
            ),
            (
                "organization.project.certificate.activation",
                CertificateScopeObject::ProjectActivation,
            ),
            (
                "organization.project.certificate.deactivation",
                CertificateScopeObject::ProjectDeactivation,
            ),
        ];
        for (value, expected) in OFFICIAL_SCOPE_OBJECTS {
            let decoded = CertificateScopeObject::from_raw(value);
            assert!(
                decoded.is_known(),
                "official certificate scope object {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
            assert!(
                !AdminListObject::from_raw(value).is_known(),
                "list-page object must not absorb official certificate scope discriminators"
            );

            let response = ok(serde_json::from_value::<CertificateScopeResponse>(json!({
                "object": value,
                "data": []
            })));
            assert_eq!(response.object, expected);
            assert_eq!(ok(serde_json::to_value(&response))["object"], value);
        }
    }

    #[test]
    fn official_invite_role_pins_owner_and_reader_only() {
        const OFFICIAL_INVITE_ROLES: [(&str, InviteRole); 2] =
            [("owner", InviteRole::Owner), ("reader", InviteRole::Reader)];
        for (value, expected) in OFFICIAL_INVITE_ROLES {
            let decoded = InviteRole::from_raw(value);
            assert!(
                decoded.is_known(),
                "official invite role {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }

        // `member` belongs to InviteProjectRole only; an org-level `member`
        // is not a named InviteRole variant anymore.
        let project_only = InviteRole::from_raw("member");
        assert!(
            !project_only.is_known(),
            "invite role `member` is outside the pinned owner/reader domain"
        );
        assert_eq!(project_only.as_str(), "member");
        assert!(
            !InviteRole::from_raw("").is_known(),
            "empty invite role must stay unknown"
        );

        let request = InviteRequest {
            email: "user@example.com".to_owned(),
            role: InviteRole::Reader,
            projects: Omittable::Omitted,
        };
        assert_eq!(
            ok(serde_json::to_value(&request)),
            json!({"email": "user@example.com", "role": "reader"})
        );

        // Resource-side losslessness: an invite echoing an unofficial role
        // still decodes verbatim through the open-enum fallback.
        let invite = ok(serde_json::from_value::<Invite>(json!({
            "object": "organization.invite",
            "id": "inv_1",
            "email": "user@example.com",
            "role": "member",
            "status": "pending",
            "created_at": 1,
            "projects": []
        })));
        assert!(!invite.role.is_known());
        assert_eq!(invite.role.as_str(), "member");
        assert_eq!(
            ok(serde_json::to_value(&invite))["role"],
            json!("member"),
            "unofficial invite role must round-trip losslessly"
        );
    }

    #[test]
    fn official_org_data_retention_pins_four_value_domain() {
        const OFFICIAL_ORG_TYPES: [(&str, OrganizationDataRetentionType); 4] = [
            (
                "zero_data_retention",
                OrganizationDataRetentionType::ZeroDataRetention,
            ),
            (
                "modified_abuse_monitoring",
                OrganizationDataRetentionType::ModifiedAbuseMonitoring,
            ),
            (
                "enhanced_zero_data_retention",
                OrganizationDataRetentionType::EnhancedZeroDataRetention,
            ),
            (
                "enhanced_modified_abuse_monitoring",
                OrganizationDataRetentionType::EnhancedModifiedAbuseMonitoring,
            ),
        ];
        for (value, expected) in OFFICIAL_ORG_TYPES {
            let decoded = OrganizationDataRetentionType::from_raw(value);
            assert!(
                decoded.is_known(),
                "official organization data-retention type {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);

            let body = UpdateOrganizationDataRetentionBody {
                retention_type: expected,
            };
            assert_eq!(
                ok(serde_json::to_value(&body)),
                json!({"retention_type": value})
            );
        }

        // Project-only values are rejected from the organization domain: they
        // carry no named variant on OrganizationDataRetentionType.
        for project_only in ["organization_default", "none"] {
            assert!(
                !OrganizationDataRetentionType::from_raw(project_only).is_known(),
                "organization data-retention type `{project_only}` is project-only"
            );
        }

        // The project body keeps the full six-value pinned domain.
        for value in [
            "organization_default",
            "none",
            "zero_data_retention",
            "modified_abuse_monitoring",
            "enhanced_zero_data_retention",
            "enhanced_modified_abuse_monitoring",
        ] {
            let body = UpdateProjectDataRetentionBody {
                retention_type: DataRetentionType::from_raw(value),
            };
            assert!(
                body.retention_type.is_known(),
                "project data-retention type {value} must be a named variant"
            );
            assert_eq!(
                ok(serde_json::to_value(&body)),
                json!({"retention_type": value})
            );
        }

        // The shared resource side stays the open superset: organization and
        // project payloads both decode losslessly.
        let organization = ok(serde_json::from_value::<OrganizationDataRetention>(json!({
            "object": "organization.data_retention",
            "type": "modified_abuse_monitoring"
        })));
        assert_eq!(
            organization.retention_type,
            DataRetentionType::ModifiedAbuseMonitoring
        );
        let project = ok(serde_json::from_value::<ProjectDataRetention>(json!({
            "object": "project.data_retention",
            "type": "organization_default"
        })));
        assert_eq!(
            project.retention_type,
            DataRetentionType::OrganizationDefault
        );
    }

    #[test]
    fn project_service_account_update_role_pins_member_and_owner() {
        const OFFICIAL_UPDATE_ROLES: [(&str, ProjectServiceAccountUpdateRole); 2] = [
            ("member", ProjectServiceAccountUpdateRole::Member),
            ("owner", ProjectServiceAccountUpdateRole::Owner),
        ];
        for (value, expected) in OFFICIAL_UPDATE_ROLES {
            let decoded = ProjectServiceAccountUpdateRole::from_raw(value);
            assert!(
                decoded.is_known(),
                "official update role {value} must be a named variant"
            );
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), value);
        }

        // `none` exists only on the resource side: the update body has no
        // named variant for it and cannot ask for it.
        let resource_only = ProjectServiceAccountUpdateRole::from_raw("none");
        assert!(
            !resource_only.is_known(),
            "update role `none` is resource-side only"
        );
        assert_eq!(resource_only.as_str(), "none");

        let body = UpdateProjectServiceAccountBody {
            name: Omittable::Omitted,
            role: Omittable::Value(ProjectServiceAccountUpdateRole::Member),
        };
        assert_eq!(ok(serde_json::to_value(&body)), json!({"role": "member"}));

        // The resource and create-response sides keep the three-value superset.
        let account = ok(serde_json::from_value::<ProjectServiceAccount>(json!({
            "object": "organization.project.service_account",
            "id": "sa_1",
            "name": "bot",
            "role": "none",
            "created_at": 1
        })));
        assert_eq!(account.role, ProjectServiceAccountRole::None);
        let created = ok(
            serde_json::from_value::<ProjectServiceAccountCreateResponse>(json!({
                "object": "organization.project.service_account",
                "id": "sa_1",
                "name": "bot",
                "role": "owner",
                "created_at": 1,
                "api_key": null
            })),
        );
        assert_eq!(created.role, ProjectServiceAccountRole::Owner);
    }

    #[test]
    fn admin_required_cursor_page_accepts_official_null_ids() {
        let fixture = json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": null,
            "has_more": false
        });
        let page = ok(serde_json::from_value::<ListCertificatesResponse>(
            fixture.clone(),
        ));
        assert!(matches!(page.first_id, Nullable::Null));
        assert!(matches!(page.last_id, Nullable::Null));
        assert_eq!(page.next_after(), None);
        assert_eq!(page.next_after_with(Some("cert_1")), None);
        assert_eq!(ok(serde_json::to_value(page)), fixture);
        assert!(
            serde_json::from_value::<ListCertificatesResponse>(json!({
                "object": "list",
                "data": [],
                "has_more": false
            }))
            .is_err()
        );
    }

    #[test]
    fn admin_cursor_pages_fall_back_to_the_last_item_id() {
        // D0147 resolution order on the optional-id envelope: a non-empty
        // `last_id` wins, then the caller-supplied last item id, then nothing.
        let page = ok(serde_json::from_value::<ListAuditLogsResponse>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "first_id": null,
            "last_id": null
        })));
        assert_eq!(page.next_after(), None);
        assert_eq!(page.next_after_with(None), None);
        assert_eq!(page.next_after_with(Some("audit_9")), Some("audit_9"));
        // Empty fallback ids never become cursors.
        assert_eq!(page.next_after_with(Some("")), None);

        let with_last = ok(serde_json::from_value::<ListAuditLogsResponse>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "last_id": "audit_7"
        })));
        assert_eq!(with_last.next_after(), Some("audit_7"));
        assert_eq!(
            with_last.next_after_with(Some("audit_9")),
            Some("audit_7"),
            "the envelope last_id outranks the last item id"
        );

        // D0145: an empty-string last_id would be dropped by the query encoder
        // and silently re-request the first page, so both getters treat it as
        // absent — `next_after_with` still recovers via the fallback.
        let empty_last = ok(serde_json::from_value::<ListAuditLogsResponse>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "last_id": ""
        })));
        assert_eq!(empty_last.next_after(), None);
        assert_eq!(empty_last.next_after_with(Some("audit_9")), Some("audit_9"));

        // Without `has_more` there is no next page regardless of ids.
        let done = ok(serde_json::from_value::<ListAuditLogsResponse>(json!({
            "object": "list",
            "data": [],
            "has_more": false,
            "last_id": "audit_7"
        })));
        assert_eq!(done.next_after(), None);
        assert_eq!(done.next_after_with(Some("audit_9")), None);

        // The required-id envelope shares the rule.
        let required = ok(serde_json::from_value::<ListCertificatesResponse>(json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": null,
            "has_more": true
        })));
        assert_eq!(required.next_after_with(Some("cert_1")), Some("cert_1"));
        let required_empty = ok(serde_json::from_value::<ListCertificatesResponse>(json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": "",
            "has_more": true
        })));
        assert_eq!(required_empty.next_after(), None);
        assert_eq!(required_empty.next_after_with(Some("")), None);
        assert_eq!(
            required_empty.next_after_with(Some("cert_1")),
            Some("cert_1")
        );
    }

    #[test]
    fn admin_next_and_usage_page_cursors_drop_empty_strings() {
        // D0145 (third family member): an empty cursor would be dropped by
        // the query encoder and silently re-request the first page, so the
        // `next`-style getters treat it as absent even when `has_more` is
        // set — mirroring `AdminCursorPage::next_after`.
        let group_page = ok(serde_json::from_value::<GroupListResource>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "next": ""
        })));
        assert_eq!(group_page.next_cursor(), None);

        let group_null = ok(serde_json::from_value::<GroupListResource>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "next": null
        })));
        assert_eq!(group_null.next_cursor(), None);

        let group_done = ok(serde_json::from_value::<GroupListResource>(json!({
            "object": "list",
            "data": [],
            "has_more": false,
            "next": "group_cursor"
        })));
        assert_eq!(group_done.next_cursor(), None);

        let group_more = ok(serde_json::from_value::<GroupListResource>(json!({
            "object": "list",
            "data": [],
            "has_more": true,
            "next": "group_cursor"
        })));
        assert_eq!(group_more.next_cursor(), Some("group_cursor"));

        let usage = ok(serde_json::from_value::<UsageResponse>(json!({
            "object": "page",
            "data": [],
            "has_more": true,
            "next_page": ""
        })));
        assert_eq!(usage.next_page(), None);

        let usage_null = ok(serde_json::from_value::<UsageResponse>(json!({
            "object": "page",
            "data": [],
            "has_more": true,
            "next_page": null
        })));
        assert_eq!(usage_null.next_page(), None);
    }

    #[test]
    fn delete_and_resource_object_discriminators_are_pinned_open_enums() {
        // 6-14: every `object` below is a pinned single-constant string that
        // openai-python and openai-node model as a Literal; the Rust side now
        // mirrors the sibling open enums (e.g. `AdminApiKeyDeleteObject`) so a
        // known discriminator decodes to a named variant while a future one
        // stays lossless through `Unknown`.
        macro_rules! discriminator_case {
            ($ty:ty, $fixture:expr, $expected:literal) => {{
                let decoded = ok(serde_json::from_value::<$ty>($fixture.clone()));
                assert_eq!(
                    decoded.object.as_str(),
                    $expected,
                    concat!(stringify!($ty), " discriminator")
                );
                assert!(
                    decoded.object.is_known(),
                    concat!(stringify!($ty), " must decode as a named variant")
                );
                assert_eq!(ok(serde_json::to_value(decoded)), $fixture);
            }};
        }

        discriminator_case!(
            DeleteCertificateResponse,
            json!({"object": "certificate.deleted", "id": "cert_1"}),
            "certificate.deleted"
        );
        discriminator_case!(
            GroupDeletedResource,
            json!({"object": "group.deleted", "id": "grp_1", "deleted": true}),
            "group.deleted"
        );
        discriminator_case!(
            UserDeleteResponse,
            json!({"object": "organization.user.deleted", "id": "user_1", "deleted": true}),
            "organization.user.deleted"
        );
        discriminator_case!(
            RoleDeletedResource,
            json!({"object": "role.deleted", "id": "role_1", "deleted": true}),
            "role.deleted"
        );
        discriminator_case!(
            InviteDeleteResponse,
            json!({"object": "organization.invite.deleted", "id": "inv_1", "deleted": true}),
            "organization.invite.deleted"
        );
        discriminator_case!(
            ProjectGroup,
            json!({
                "object": "project.group",
                "project_id": "proj_1",
                "group_id": "grp_1",
                "group_name": "ops",
                "group_type": "group",
                "created_at": 1
            }),
            "project.group"
        );
        discriminator_case!(
            ProjectGroupDeletedResource,
            json!({"object": "project.group.deleted", "deleted": true}),
            "project.group.deleted"
        );
        discriminator_case!(
            ProjectUserDeleteResponse,
            json!({
                "object": "organization.project.user.deleted",
                "id": "user_1",
                "deleted": true
            }),
            "organization.project.user.deleted"
        );
        discriminator_case!(
            ProjectServiceAccountDeleteResponse,
            json!({
                "object": "organization.project.service_account.deleted",
                "id": "sa_1",
                "deleted": true
            }),
            "organization.project.service_account.deleted"
        );
        discriminator_case!(
            ProjectApiKey,
            json!({
                "object": "organization.project.api_key",
                "redacted_value": "sk-proj...",
                "name": "ci",
                "created_at": 1,
                "last_used_at": null,
                "id": "key_1",
                "owner_project_access": "active",
                "owner": {}
            }),
            "organization.project.api_key"
        );
        discriminator_case!(
            ProjectApiKeyDeleteResponse,
            json!({
                "object": "organization.project.api_key.deleted",
                "id": "key_1",
                "deleted": true
            }),
            "organization.project.api_key.deleted"
        );
        discriminator_case!(
            ProjectModelPermissions,
            json!({
                "object": "project.model_permissions",
                "mode": "allow_list",
                "model_ids": []
            }),
            "project.model_permissions"
        );
        discriminator_case!(
            ProjectModelPermissionsDeleteResponse,
            json!({"object": "project.model_permissions.deleted", "deleted": true}),
            "project.model_permissions.deleted"
        );
        // The two spend families each pin an organization and a project
        // constant on the same shared struct.
        discriminator_case!(
            OrganizationSpendAlertDeletedResource,
            json!({
                "id": "alert_1",
                "object": "organization.spend_alert.deleted",
                "deleted": true
            }),
            "organization.spend_alert.deleted"
        );
        discriminator_case!(
            ProjectSpendAlertDeletedResource,
            json!({
                "id": "alert_1",
                "object": "project.spend_alert.deleted",
                "deleted": true
            }),
            "project.spend_alert.deleted"
        );
        discriminator_case!(
            OrganizationSpendLimitDeletedResource,
            json!({"object": "organization.spend_limit.deleted", "deleted": true}),
            "organization.spend_limit.deleted"
        );
        discriminator_case!(
            ProjectSpendLimitDeletedResource,
            json!({"object": "project.spend_limit.deleted", "deleted": true}),
            "project.spend_limit.deleted"
        );

        // A discriminator minted after this release decodes losslessly.
        let future = ok(serde_json::from_value::<UserDeleteResponse>(json!({
            "object": "organization.user.future_deleted",
            "id": "user_1",
            "deleted": true
        })));
        assert_eq!(
            future.object.unknown_value(),
            Some("organization.user.future_deleted")
        );
        assert_eq!(
            ok(serde_json::to_value(future))["object"],
            "organization.user.future_deleted"
        );
    }

    #[test]
    fn admin_request_nulls_and_limits_match_python_and_openapi_inventory() {
        let project = ok(serde_json::from_value::<ProjectUpdateRequest>(json!({
            "name": null
        })));
        assert!(matches!(project.name, Omittable::Value(Nullable::Null)));

        let user = ok(serde_json::from_value::<UserRoleUpdateRequest>(json!({
            "role": null,
            "role_id": null,
            "technical_level": null,
            "developer_persona": null
        })));
        assert!(matches!(user.role, Omittable::Value(Nullable::Null)));

        let service = ok(
            serde_json::from_value::<ProjectServiceAccountCreateRequest>(json!({
                "name": "bot",
                "create_service_account_only": null
            })),
        );
        assert!(matches!(
            service.create_service_account_only,
            Omittable::Value(Nullable::Null)
        ));

        AdminApiKeyCreateRequest::new("ops")
            .validate()
            .expect("omitted expiry is accepted");
        let mut over_expiry = AdminApiKeyCreateRequest::new("ops");
        over_expiry.expires_in_seconds = Omittable::Value(MAX_ADMIN_API_KEY_EXPIRES_IN_SECONDS + 1);
        assert!(matches!(
            over_expiry.validate(),
            Err(AdminConstraintError::ApiKeyExpiresInSeconds { actual, .. })
                if actual == MAX_ADMIN_API_KEY_EXPIRES_IN_SECONDS + 1
        ));

        assert!(matches!(
            CreateGroupBody {
                name: String::new()
            }
            .validate(),
            Err(AdminConstraintError::GroupName { actual: 0, .. })
        ));
        assert!(matches!(
            ToggleCertificatesRequest {
                certificate_ids: Vec::new()
            }
            .validate(),
            Err(AdminConstraintError::CertificateIds { actual: 0, .. })
        ));
    }
}
