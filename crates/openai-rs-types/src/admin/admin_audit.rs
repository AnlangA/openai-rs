//! Official `AuditLog` event-specific payloads from the pinned OpenAPI.

use serde::{Deserialize, Serialize};

use crate::{ExtraFields, Omittable, WireSecret};

use super::AdminJsonObject;

crate::open_string_enum! {
    /// Official `role.bound_to_resource` / `role.unbound_from_resource` `source`.
    pub enum AuditRoleBindingSource {
        RoleToggle = "role_toggle",
        RoleConnectorUpdate = "role_connector_update",
        RoleDelete = "role_delete",
        WorkspacePermissions = "workspace_permissions",
        ConnectorPublish = "connector_publish"
    }
}

macro_rules! audit_object {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($field:ident : $ty:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        $vis struct $name {
            $(
                #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
                pub $field: Omittable<$ty>,
            )*
            #[serde(default, flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Future official or vendor fields on this payload object.
            #[must_use]
            pub const fn extra(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

audit_object! {
    /// Official `{id, name}` pair used by certificate and IP-allowlist items.
    pub struct AuditNamedId {
        id: String,
        name: String,
    }
}

audit_object! {
    /// The payload used to create the API key.
    pub struct AuditPayloadApiKeyCreatedData {
        scopes: Vec<String>,
    }
}

audit_object! {
    /// Official `AuditLog` `api_key.created` object.
    pub struct AuditPayloadApiKeyCreated {
        id: String,
        data: AuditPayloadApiKeyCreatedData,
    }
}

audit_object! {
    /// The payload used to update the API key.
    pub struct AuditPayloadApiKeyUpdatedChangesRequested {
        scopes: Vec<String>,
    }
}

audit_object! {
    /// Official `AuditLog` `api_key.updated` object.
    pub struct AuditPayloadApiKeyUpdated {
        id: String,
        changes_requested: AuditPayloadApiKeyUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `api_key.deleted` object.
    pub struct AuditPayloadApiKeyDeleted {
        id: String,
    }
}

audit_object! {
    /// The payload used to create the checkpoint permission.
    pub struct AuditPayloadCheckpointPermissionCreatedData {
        project_id: String,
        fine_tuned_model_checkpoint: String,
    }
}

audit_object! {
    /// Official `AuditLog` `checkpoint.permission.created` object.
    pub struct AuditPayloadCheckpointPermissionCreated {
        id: String,
        data: AuditPayloadCheckpointPermissionCreatedData,
    }
}

audit_object! {
    /// Official `AuditLog` `checkpoint.permission.deleted` object.
    pub struct AuditPayloadCheckpointPermissionDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `external_key.registered` object.
    pub struct AuditPayloadExternalKeyRegistered {
        id: String,
        data: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `external_key.removed` object.
    pub struct AuditPayloadExternalKeyRemoved {
        id: String,
    }
}

audit_object! {
    /// Information about the created group.
    pub struct AuditPayloadGroupCreatedData {
        group_name: String,
    }
}

audit_object! {
    /// Official `AuditLog` `group.created` object.
    pub struct AuditPayloadGroupCreated {
        id: String,
        data: AuditPayloadGroupCreatedData,
    }
}

audit_object! {
    /// The payload used to update the group.
    pub struct AuditPayloadGroupUpdatedChangesRequested {
        group_name: String,
    }
}

audit_object! {
    /// Official `AuditLog` `group.updated` object.
    pub struct AuditPayloadGroupUpdated {
        id: String,
        changes_requested: AuditPayloadGroupUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `group.deleted` object.
    pub struct AuditPayloadGroupDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `scim.enabled` object.
    pub struct AuditPayloadScimEnabled {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `scim.disabled` object.
    pub struct AuditPayloadScimDisabled {
        id: String,
    }
}

audit_object! {
    /// The payload used to create the invite.
    pub struct AuditPayloadInviteSentData {
        email: String,
        role: String,
    }
}

audit_object! {
    /// Official `AuditLog` `invite.sent` object.
    pub struct AuditPayloadInviteSent {
        id: String,
        data: AuditPayloadInviteSentData,
    }
}

audit_object! {
    /// Official `AuditLog` `invite.accepted` object.
    pub struct AuditPayloadInviteAccepted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `invite.deleted` object.
    pub struct AuditPayloadInviteDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `ip_allowlist.created` object.
    pub struct AuditPayloadIpAllowlistCreated {
        id: String,
        name: String,
        allowed_ips: Vec<String>,
    }
}

audit_object! {
    /// Official `AuditLog` `ip_allowlist.updated` object.
    pub struct AuditPayloadIpAllowlistUpdated {
        id: String,
        allowed_ips: Vec<String>,
    }
}

audit_object! {
    /// Official `AuditLog` `ip_allowlist.deleted` object.
    pub struct AuditPayloadIpAllowlistDeleted {
        id: String,
        name: String,
        allowed_ips: Vec<String>,
    }
}

audit_object! {
    /// Official `AuditLog` `ip_allowlist.config.activated` object.
    pub struct AuditPayloadIpAllowlistConfigActivated {
        configs: Vec<AuditNamedId>,
    }
}

audit_object! {
    /// Official `AuditLog` `ip_allowlist.config.deactivated` object.
    pub struct AuditPayloadIpAllowlistConfigDeactivated {
        configs: Vec<AuditNamedId>,
    }
}

audit_object! {
    /// Official `AuditLog` `login.failed` object.
    pub struct AuditPayloadLoginFailed {
        error_code: String,
        error_message: String,
    }
}

audit_object! {
    /// Official `AuditLog` `logout.failed` object.
    pub struct AuditPayloadLogoutFailed {
        error_code: String,
        error_message: String,
    }
}

audit_object! {
    /// The payload used to update the organization settings.
    pub struct AuditPayloadOrganizationUpdatedChangesRequested {
        title: String,
        description: String,
        name: String,
        threads_ui_visibility: String,
        usage_dashboard_visibility: String,
        api_call_logging: String,
        api_call_logging_project_ids: String,
    }
}

audit_object! {
    /// Official `AuditLog` `organization.updated` object.
    pub struct AuditPayloadOrganizationUpdated {
        id: String,
        changes_requested: AuditPayloadOrganizationUpdatedChangesRequested,
    }
}

audit_object! {
    /// The payload used to create the project.
    pub struct AuditPayloadProjectCreatedData {
        name: String,
        title: String,
    }
}

audit_object! {
    /// Official `AuditLog` `project.created` object.
    pub struct AuditPayloadProjectCreated {
        id: String,
        data: AuditPayloadProjectCreatedData,
    }
}

audit_object! {
    /// The payload used to update the project.
    pub struct AuditPayloadProjectUpdatedChangesRequested {
        title: String,
    }
}

audit_object! {
    /// Official `AuditLog` `project.updated` object.
    pub struct AuditPayloadProjectUpdated {
        id: String,
        changes_requested: AuditPayloadProjectUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `project.archived` object.
    pub struct AuditPayloadProjectArchived {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `project.deleted` object.
    pub struct AuditPayloadProjectDeleted {
        id: String,
    }
}

audit_object! {
    /// The payload used to update the rate limits.
    pub struct AuditPayloadRateLimitUpdatedChangesRequested {
        max_requests_per_1_minute: u64,
        max_tokens_per_1_minute: u64,
        max_images_per_1_minute: u64,
        max_audio_megabytes_per_1_minute: u64,
        max_requests_per_1_day: u64,
        batch_1_day_max_input_tokens: u64,
    }
}

audit_object! {
    /// Official `AuditLog` `rate_limit.updated` object.
    pub struct AuditPayloadRateLimitUpdated {
        id: String,
        changes_requested: AuditPayloadRateLimitUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `rate_limit.deleted` object.
    pub struct AuditPayloadRateLimitDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `role.created` object.
    pub struct AuditPayloadRoleCreated {
        id: String,
        role_name: String,
        permissions: Vec<String>,
        resource_type: String,
        resource_id: String,
    }
}

audit_object! {
    /// The payload used to update the role.
    pub struct AuditPayloadRoleUpdatedChangesRequested {
        role_name: String,
        resource_id: String,
        resource_type: String,
        permissions_added: Vec<String>,
        permissions_removed: Vec<String>,
        description: String,
        metadata: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `role.updated` object.
    pub struct AuditPayloadRoleUpdated {
        id: String,
        changes_requested: AuditPayloadRoleUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `role.deleted` object.
    pub struct AuditPayloadRoleDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `role.assignment.created` object.
    pub struct AuditPayloadRoleAssignmentCreated {
        id: String,
        principal_id: String,
        principal_type: String,
        resource_id: String,
        resource_type: String,
    }
}

audit_object! {
    /// Official `AuditLog` `role.assignment.deleted` object.
    pub struct AuditPayloadRoleAssignmentDeleted {
        id: String,
        principal_id: String,
        principal_type: String,
        resource_id: String,
        resource_type: String,
    }
}

audit_object! {
    /// Official `AuditLog` `role.bound_to_resource` object.
    pub struct AuditPayloadRoleBoundToResource {
        id: String,
        role_id: String,
        resource_id: String,
        resource_type: String,
        permissions: Vec<String>,
        workspace_id: String,
        connector_id: String,
        connector_name: String,
        enabled: bool,
        source: AuditRoleBindingSource,
    }
}

audit_object! {
    /// Official `AuditLog` `role.unbound_from_resource` object.
    pub struct AuditPayloadRoleUnboundFromResource {
        id: String,
        role_id: String,
        resource_id: String,
        resource_type: String,
        permissions: Vec<String>,
        workspace_id: String,
        connector_id: String,
        connector_name: String,
        enabled: bool,
        source: AuditRoleBindingSource,
    }
}

audit_object! {
    /// The payload used to create the service account.
    pub struct AuditPayloadServiceAccountCreatedData {
        role: String,
    }
}

audit_object! {
    /// Official `AuditLog` `service_account.created` object.
    pub struct AuditPayloadServiceAccountCreated {
        id: String,
        data: AuditPayloadServiceAccountCreatedData,
    }
}

audit_object! {
    /// The payload used to updated the service account.
    pub struct AuditPayloadServiceAccountUpdatedChangesRequested {
        role: String,
    }
}

audit_object! {
    /// Official `AuditLog` `service_account.updated` object.
    pub struct AuditPayloadServiceAccountUpdated {
        id: String,
        changes_requested: AuditPayloadServiceAccountUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `service_account.deleted` object.
    pub struct AuditPayloadServiceAccountDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider.created` object.
    pub struct AuditPayloadWorkloadIdentityProviderCreated {
        id: String,
        data: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider.updated` object.
    pub struct AuditPayloadWorkloadIdentityProviderUpdated {
        id: String,
        changes_requested: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider.deleted` object.
    pub struct AuditPayloadWorkloadIdentityProviderDeleted {
        id: String,
        name: String,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider_mapping.created` object.
    pub struct AuditPayloadWorkloadIdentityProviderMappingCreated {
        id: String,
        identity_provider_id: String,
        data: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider_mapping.updated` object.
    pub struct AuditPayloadWorkloadIdentityProviderMappingUpdated {
        id: String,
        identity_provider_id: String,
        changes_requested: AdminJsonObject,
    }
}

audit_object! {
    /// Official `AuditLog` `workload_identity_provider_mapping.deleted` object.
    pub struct AuditPayloadWorkloadIdentityProviderMappingDeleted {
        id: String,
        identity_provider_id: String,
        project_id: String,
        service_account_id: String,
    }
}

audit_object! {
    /// The payload used to add the user to the project.
    pub struct AuditPayloadUserAddedData {
        role: String,
    }
}

audit_object! {
    /// Official `AuditLog` `user.added` object.
    pub struct AuditPayloadUserAdded {
        id: String,
        data: AuditPayloadUserAddedData,
    }
}

audit_object! {
    /// The payload used to update the user.
    pub struct AuditPayloadUserUpdatedChangesRequested {
        role: String,
    }
}

audit_object! {
    /// Official `AuditLog` `user.updated` object.
    pub struct AuditPayloadUserUpdated {
        id: String,
        changes_requested: AuditPayloadUserUpdatedChangesRequested,
    }
}

audit_object! {
    /// Official `AuditLog` `user.deleted` object.
    pub struct AuditPayloadUserDeleted {
        id: String,
    }
}

audit_object! {
    /// Official `AuditLog` `certificate.created` object.
    pub struct AuditPayloadCertificateCreated {
        id: String,
        name: String,
    }
}

audit_object! {
    /// Official `AuditLog` `certificate.updated` object.
    pub struct AuditPayloadCertificateUpdated {
        id: String,
        name: String,
    }
}

/// Official `AuditLog` `certificate.deleted` object.
///
/// PEM content uses explicit wire-secret redaction and is compared only
/// through `WireSecret::with_exposed`, matching `CertificateDetails`.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AuditPayloadCertificateDeleted {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub certificate: Omittable<WireSecret>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl AuditPayloadCertificateDeleted {
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

impl PartialEq for AuditPayloadCertificateDeleted {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.extra == other.extra
            && match (&self.certificate, &other.certificate) {
                (Omittable::Omitted, Omittable::Omitted) => true,
                (Omittable::Value(left), Omittable::Value(right)) => {
                    left.with_exposed(|left| right.with_exposed(|right| left == right))
                }
                _ => false,
            }
    }
}

impl std::fmt::Debug for AuditPayloadCertificateDeleted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuditPayloadCertificateDeleted")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("certificate", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

audit_object! {
    /// Official `AuditLog` `certificates.activated` object.
    pub struct AuditPayloadCertificatesActivated {
        certificates: Vec<AuditNamedId>,
    }
}

audit_object! {
    /// Official `AuditLog` `certificates.deactivated` object.
    pub struct AuditPayloadCertificatesDeactivated {
        certificates: Vec<AuditNamedId>,
    }
}
