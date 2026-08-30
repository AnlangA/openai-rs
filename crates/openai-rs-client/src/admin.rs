//! Dedicated Administration API client.
//!
//! [`AdminClient`] cannot be constructed from or converted into the ordinary
//! Platform [`crate::Client`]. Its sealed operation markers all carry
//! [`AdminAuthScope::Admin`], and request URLs are assembled only from frozen
//! route templates.

use std::{fmt, marker::PhantomData, sync::Arc, time::Duration};

use futures_util::StreamExt;
use http::{HeaderValue, Method, header};
use openai_rs_types::admin::*;
use secrecy::{ExposeSecret, SecretString};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error as ThisError;
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::{ApiError, ApiResponse, BodyPreview, Error, ResponseMeta, TlsBackend};

const DEFAULT_ADMIN_BASE_URL: &str = "https://api.openai.com/v1/";
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_MAX_JSON_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const DECODE_PREVIEW_BYTES: usize = 8 * 1024;

/// Administration credential that is intentionally incompatible with
/// [`crate::ApiKey`] and has no Serde implementation.
#[derive(Clone)]
pub struct AdminApiKey(SecretString);

impl AdminApiKey {
    /// Validate an Administration API key.
    pub fn new(key: impl Into<String>) -> Result<Self, AdminApiKeyError> {
        let key = key.into();
        if key.is_empty() {
            return Err(AdminApiKeyError::Empty);
        }
        if key.trim() != key {
            return Err(AdminApiKeyError::SurroundingWhitespace);
        }
        if key.chars().any(char::is_whitespace) {
            return Err(AdminApiKeyError::Whitespace);
        }
        if key.chars().any(char::is_control) {
            return Err(AdminApiKeyError::ControlCharacter);
        }
        if !key.is_ascii() {
            return Err(AdminApiKeyError::NonAscii);
        }
        Ok(Self(SecretString::from(key)))
    }

    fn authorization_header(&self) -> Result<HeaderValue, AdminApiKeyError> {
        let value = Zeroizing::new(format!("Bearer {}", self.0.expose_secret()));
        let mut header =
            HeaderValue::from_str(&value).map_err(|_| AdminApiKeyError::InvalidHeaderValue)?;
        header.set_sensitive(true);
        Ok(header)
    }
}

impl fmt::Debug for AdminApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdminApiKey([REDACTED])")
    }
}

/// Validation failure for [`AdminApiKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, ThisError)]
pub enum AdminApiKeyError {
    #[error("the Administration API key is empty")]
    Empty,
    #[error("the Administration API key has surrounding whitespace")]
    SurroundingWhitespace,
    #[error("the Administration API key contains whitespace")]
    Whitespace,
    #[error("the Administration API key contains a control character")]
    ControlCharacter,
    #[error("the Administration API key contains non-ASCII characters")]
    NonAscii,
    #[error("the Administration API key cannot be represented as an HTTP header")]
    InvalidHeaderValue,
}

/// Credential scope for sealed Administration operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminAuthScope {
    /// Organization/project Administration credential.
    Admin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminRequestEncoding {
    None,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminResponseMode {
    Json,
    Empty,
}

/// Sealed static contract for one Administration operation.
pub trait AdminOperation: admin_operation_private::Sealed + Send + Sync + 'static {
    type Request: Serialize + Send + Sync + 'static;
    type Response: DeserializeOwned + Send + 'static;
    const ID: &'static str;
    const METHOD: Method;
    const ROUTE: &'static str;
    const AUTH: AdminAuthScope = AdminAuthScope::Admin;
    const REQUEST_ENCODING: AdminRequestEncoding;
    const RESPONSE_MODE: AdminResponseMode;
}

mod admin_operation_private {
    pub trait Sealed {}
    pub trait QuerySealed {}
}

/// Sealed typed query accepted by Administration requests.
pub trait AdminQuery: admin_operation_private::QuerySealed + Serialize {}

macro_rules! admin_query {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl admin_operation_private::QuerySealed for $ty {}
            impl AdminQuery for $ty {}
        )+
    };
}

admin_query!(AdminListParams, AuditLogListParams, UsageQueryParams);

macro_rules! admin_operation {
    ($name:ident, $id:literal, $method:ident, $route:literal, $request:ty, $response:ty, $encoding:ident, $mode:ident) => {
        #[derive(Clone, Copy, Debug)]
        pub struct $name;
        impl admin_operation_private::Sealed for $name {}
        impl AdminOperation for $name {
            type Request = $request;
            type Response = $response;
            const ID: &'static str = $id;
            const METHOD: Method = admin_method!($method);
            const ROUTE: &'static str = $route;
            const REQUEST_ENCODING: AdminRequestEncoding = AdminRequestEncoding::$encoding;
            const RESPONSE_MODE: AdminResponseMode = AdminResponseMode::$mode;
        }
    };
}

macro_rules! admin_method {
    (Get) => {
        Method::GET
    };
    (Post) => {
        Method::POST
    };
    (Delete) => {
        Method::DELETE
    };
}

/// Sealed Administration operation markers generated from the frozen manifest.
pub mod operations {
    use super::*;

    admin_operation!(
        OpCreateanAPIkeyforaserviceaccount,
        "CreateanAPIkeyforaserviceaccount",
        Post,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}/api_keys",
        CreateProjectServiceAccountApiKeyBody,
        ServiceAccountApiKeyBody,
        Json,
        Json
    );
    admin_operation!(
        OpDeleteorganizationspendlimit,
        "Deleteorganizationspendlimit",
        Delete,
        "/organization/spend_limit",
        (),
        OrganizationSpendLimitDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteprojectspendlimit,
        "Deleteprojectspendlimit",
        Delete,
        "/organization/projects/{project_id}/spend_limit",
        (),
        ProjectSpendLimitDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpGetorganizationspendlimit,
        "Getorganizationspendlimit",
        Get,
        "/organization/spend_limit",
        (),
        OrganizationSpendLimitResource,
        None,
        Json
    );
    admin_operation!(
        OpGetprojectspendlimit,
        "Getprojectspendlimit",
        Get,
        "/organization/projects/{project_id}/spend_limit",
        (),
        ProjectSpendLimitResource,
        None,
        Json
    );
    admin_operation!(
        OpUpdateorganizationspendlimit,
        "Updateorganizationspendlimit",
        Post,
        "/organization/spend_limit",
        UpdateOrganizationSpendLimitBody,
        OrganizationSpendLimitResource,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateprojectspendlimit,
        "Updateprojectspendlimit",
        Post,
        "/organization/projects/{project_id}/spend_limit",
        UpdateProjectSpendLimitBody,
        ProjectSpendLimitResource,
        Json,
        Json
    );
    admin_operation!(
        OpActivateOrganizationCertificates,
        "activateOrganizationCertificates",
        Post,
        "/organization/certificates/activate",
        ToggleCertificatesRequest,
        OrganizationCertificateActivationResponse,
        Json,
        Json
    );
    admin_operation!(
        OpActivateProjectCertificates,
        "activateProjectCertificates",
        Post,
        "/organization/projects/{project_id}/certificates/activate",
        ToggleCertificatesRequest,
        OrganizationProjectCertificateActivationResponse,
        Json,
        Json
    );
    admin_operation!(
        OpAddGroupUser,
        "add-group-user",
        Post,
        "/organization/groups/{group_id}/users",
        CreateGroupUserBody,
        GroupUserAssignment,
        Json,
        Json
    );
    admin_operation!(
        OpAddProjectGroup,
        "add-project-group",
        Post,
        "/organization/projects/{project_id}/groups",
        InviteProjectGroupBody,
        ProjectGroup,
        Json,
        Json
    );
    admin_operation!(
        OpAdminApiKeysCreate,
        "admin-api-keys-create",
        Post,
        "/organization/admin_api_keys",
        AdminApiKeyCreateRequest,
        AdminApiKeyCreateResponse,
        Json,
        Json
    );
    admin_operation!(
        OpAdminApiKeysDelete,
        "admin-api-keys-delete",
        Delete,
        "/organization/admin_api_keys/{key_id}",
        (),
        (),
        None,
        Empty
    );
    admin_operation!(
        OpAdminApiKeysGet,
        "admin-api-keys-get",
        Get,
        "/organization/admin_api_keys/{key_id}",
        (),
        openai_rs_types::admin::AdminApiKey,
        None,
        Json
    );
    admin_operation!(
        OpAdminApiKeysList,
        "admin-api-keys-list",
        Get,
        "/organization/admin_api_keys",
        (),
        ApiKeyList,
        None,
        Json
    );
    admin_operation!(
        OpArchiveProject,
        "archive-project",
        Post,
        "/organization/projects/{project_id}/archive",
        (),
        Project,
        None,
        Json
    );
    admin_operation!(
        OpAssignGroupRole,
        "assign-group-role",
        Post,
        "/organization/groups/{group_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        GroupRoleAssignment,
        Json,
        Json
    );
    admin_operation!(
        OpAssignProjectGroupRole,
        "assign-project-group-role",
        Post,
        "/projects/{project_id}/groups/{group_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        GroupRoleAssignment,
        Json,
        Json
    );
    admin_operation!(
        OpAssignProjectUserRole,
        "assign-project-user-role",
        Post,
        "/projects/{project_id}/users/{user_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        UserRoleAssignment,
        Json,
        Json
    );
    admin_operation!(
        OpAssignUserRole,
        "assign-user-role",
        Post,
        "/organization/users/{user_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        UserRoleAssignment,
        Json,
        Json
    );
    admin_operation!(
        OpCreateGroup,
        "create-group",
        Post,
        "/organization/groups",
        CreateGroupBody,
        GroupResponse,
        Json,
        Json
    );
    admin_operation!(
        OpCreateOrganizationSpendAlert,
        "create-organization-spend-alert",
        Post,
        "/organization/spend_alerts",
        CreateSpendAlertBody,
        OrganizationSpendAlert,
        Json,
        Json
    );
    admin_operation!(
        OpCreateProject,
        "create-project",
        Post,
        "/organization/projects",
        ProjectCreateRequest,
        Project,
        Json,
        Json
    );
    admin_operation!(
        OpCreateProjectRole,
        "create-project-role",
        Post,
        "/projects/{project_id}/roles",
        PublicCreateOrganizationRoleBody,
        Role,
        Json,
        Json
    );
    admin_operation!(
        OpCreateProjectServiceAccount,
        "create-project-service-account",
        Post,
        "/organization/projects/{project_id}/service_accounts",
        ProjectServiceAccountCreateRequest,
        ProjectServiceAccountCreateResponse,
        Json,
        Json
    );
    admin_operation!(
        OpCreateProjectSpendAlert,
        "create-project-spend-alert",
        Post,
        "/organization/projects/{project_id}/spend_alerts",
        CreateSpendAlertBody,
        ProjectSpendAlert,
        Json,
        Json
    );
    admin_operation!(
        OpCreateProjectUser,
        "create-project-user",
        Post,
        "/organization/projects/{project_id}/users",
        ProjectUserCreateRequest,
        ProjectUser,
        Json,
        Json
    );
    admin_operation!(
        OpCreateRole,
        "create-role",
        Post,
        "/organization/roles",
        PublicCreateOrganizationRoleBody,
        Role,
        Json,
        Json
    );
    admin_operation!(
        OpDeactivateOrganizationCertificates,
        "deactivateOrganizationCertificates",
        Post,
        "/organization/certificates/deactivate",
        ToggleCertificatesRequest,
        OrganizationCertificateDeactivationResponse,
        Json,
        Json
    );
    admin_operation!(
        OpDeactivateProjectCertificates,
        "deactivateProjectCertificates",
        Post,
        "/organization/projects/{project_id}/certificates/deactivate",
        ToggleCertificatesRequest,
        OrganizationProjectCertificateDeactivationResponse,
        Json,
        Json
    );
    admin_operation!(
        OpDeleteGroup,
        "delete-group",
        Delete,
        "/organization/groups/{group_id}",
        (),
        GroupDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteInvite,
        "delete-invite",
        Delete,
        "/organization/invites/{invite_id}",
        (),
        InviteDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteOrganizationSpendAlert,
        "delete-organization-spend-alert",
        Delete,
        "/organization/spend_alerts/{alert_id}",
        (),
        OrganizationSpendAlertDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectApiKey,
        "delete-project-api-key",
        Delete,
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        (),
        ProjectApiKeyDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectModelPermissions,
        "delete-project-model-permissions",
        Delete,
        "/organization/projects/{project_id}/model_permissions",
        (),
        ProjectModelPermissionsDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectRole,
        "delete-project-role",
        Delete,
        "/projects/{project_id}/roles/{role_id}",
        (),
        RoleDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectServiceAccount,
        "delete-project-service-account",
        Delete,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        (),
        ProjectServiceAccountDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectSpendAlert,
        "delete-project-spend-alert",
        Delete,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        (),
        ProjectSpendAlertDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteProjectUser,
        "delete-project-user",
        Delete,
        "/organization/projects/{project_id}/users/{user_id}",
        (),
        ProjectUserDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteRole,
        "delete-role",
        Delete,
        "/organization/roles/{role_id}",
        (),
        RoleDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpDeleteUser,
        "delete-user",
        Delete,
        "/organization/users/{user_id}",
        (),
        UserDeleteResponse,
        None,
        Json
    );
    admin_operation!(
        OpDeleteCertificate,
        "deleteCertificate",
        Delete,
        "/organization/certificates/{certificate_id}",
        (),
        DeleteCertificateResponse,
        None,
        Json
    );
    admin_operation!(
        OpGetCertificate,
        "getCertificate",
        Get,
        "/organization/certificates/{certificate_id}",
        (),
        Certificate,
        None,
        Json
    );
    admin_operation!(
        OpInviteUser,
        "inviteUser",
        Post,
        "/organization/invites",
        InviteRequest,
        Invite,
        Json,
        Json
    );
    admin_operation!(
        OpListAuditLogs,
        "list-audit-logs",
        Get,
        "/organization/audit_logs",
        (),
        ListAuditLogsResponse,
        None,
        Json
    );
    admin_operation!(
        OpListGroupRoleAssignments,
        "list-group-role-assignments",
        Get,
        "/organization/groups/{group_id}/roles",
        (),
        RoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListGroupUsers,
        "list-group-users",
        Get,
        "/organization/groups/{group_id}/users",
        (),
        UserListResource,
        None,
        Json
    );
    admin_operation!(
        OpListGroups,
        "list-groups",
        Get,
        "/organization/groups",
        (),
        GroupListResource,
        None,
        Json
    );
    admin_operation!(
        OpListInvites,
        "list-invites",
        Get,
        "/organization/invites",
        (),
        InviteListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListOrganizationSpendAlerts,
        "list-organization-spend-alerts",
        Get,
        "/organization/spend_alerts",
        (),
        OrganizationSpendAlertListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectApiKeys,
        "list-project-api-keys",
        Get,
        "/organization/projects/{project_id}/api_keys",
        (),
        ProjectApiKeyListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListProjectGroupRoleAssignments,
        "list-project-group-role-assignments",
        Get,
        "/projects/{project_id}/groups/{group_id}/roles",
        (),
        RoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectGroups,
        "list-project-groups",
        Get,
        "/organization/projects/{project_id}/groups",
        (),
        ProjectGroupListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectRateLimits,
        "list-project-rate-limits",
        Get,
        "/organization/projects/{project_id}/rate_limits",
        (),
        ProjectRateLimitListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListProjectRoles,
        "list-project-roles",
        Get,
        "/projects/{project_id}/roles",
        (),
        PublicRoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectServiceAccounts,
        "list-project-service-accounts",
        Get,
        "/organization/projects/{project_id}/service_accounts",
        (),
        ProjectServiceAccountListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListProjectSpendAlerts,
        "list-project-spend-alerts",
        Get,
        "/organization/projects/{project_id}/spend_alerts",
        (),
        ProjectSpendAlertListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectUserRoleAssignments,
        "list-project-user-role-assignments",
        Get,
        "/projects/{project_id}/users/{user_id}/roles",
        (),
        RoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListProjectUsers,
        "list-project-users",
        Get,
        "/organization/projects/{project_id}/users",
        (),
        ProjectUserListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListProjects,
        "list-projects",
        Get,
        "/organization/projects",
        (),
        ProjectListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListRoles,
        "list-roles",
        Get,
        "/organization/roles",
        (),
        PublicRoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListUserRoleAssignments,
        "list-user-role-assignments",
        Get,
        "/organization/users/{user_id}/roles",
        (),
        RoleListResource,
        None,
        Json
    );
    admin_operation!(
        OpListUsers,
        "list-users",
        Get,
        "/organization/users",
        (),
        UserListResponse,
        None,
        Json
    );
    admin_operation!(
        OpListOrganizationCertificates,
        "listOrganizationCertificates",
        Get,
        "/organization/certificates",
        (),
        ListCertificatesResponse,
        None,
        Json
    );
    admin_operation!(
        OpListProjectCertificates,
        "listProjectCertificates",
        Get,
        "/organization/projects/{project_id}/certificates",
        (),
        ListProjectCertificatesResponse,
        None,
        Json
    );
    admin_operation!(
        OpModifyProject,
        "modify-project",
        Post,
        "/organization/projects/{project_id}",
        ProjectUpdateRequest,
        Project,
        Json,
        Json
    );
    admin_operation!(
        OpModifyProjectUser,
        "modify-project-user",
        Post,
        "/organization/projects/{project_id}/users/{user_id}",
        ProjectUserUpdateRequest,
        ProjectUser,
        Json,
        Json
    );
    admin_operation!(
        OpModifyUser,
        "modify-user",
        Post,
        "/organization/users/{user_id}",
        UserRoleUpdateRequest,
        User,
        Json,
        Json
    );
    admin_operation!(
        OpModifyCertificate,
        "modifyCertificate",
        Post,
        "/organization/certificates/{certificate_id}",
        ModifyCertificateRequest,
        Certificate,
        Json,
        Json
    );
    admin_operation!(
        OpRemoveGroupUser,
        "remove-group-user",
        Delete,
        "/organization/groups/{group_id}/users/{user_id}",
        (),
        GroupUserDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpRemoveProjectGroup,
        "remove-project-group",
        Delete,
        "/organization/projects/{project_id}/groups/{group_id}",
        (),
        ProjectGroupDeletedResource,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveGroup,
        "retrieve-group",
        Get,
        "/organization/groups/{group_id}",
        (),
        GroupResponse,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveGroupRole,
        "retrieve-group-role",
        Get,
        "/organization/groups/{group_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveGroupUser,
        "retrieve-group-user",
        Get,
        "/organization/groups/{group_id}/users/{user_id}",
        (),
        GroupMemberUser,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveInvite,
        "retrieve-invite",
        Get,
        "/organization/invites/{invite_id}",
        (),
        Invite,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveOrganizationDataRetention,
        "retrieve-organization-data-retention",
        Get,
        "/organization/data_retention",
        (),
        OrganizationDataRetention,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveOrganizationSpendAlert,
        "retrieve-organization-spend-alert",
        Get,
        "/organization/spend_alerts/{alert_id}",
        (),
        OrganizationSpendAlert,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProject,
        "retrieve-project",
        Get,
        "/organization/projects/{project_id}",
        (),
        Project,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectApiKey,
        "retrieve-project-api-key",
        Get,
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        (),
        ProjectApiKey,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectDataRetention,
        "retrieve-project-data-retention",
        Get,
        "/organization/projects/{project_id}/data_retention",
        (),
        ProjectDataRetention,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectGroup,
        "retrieve-project-group",
        Get,
        "/organization/projects/{project_id}/groups/{group_id}",
        (),
        ProjectGroup,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectGroupRole,
        "retrieve-project-group-role",
        Get,
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectHostedToolPermissions,
        "retrieve-project-hosted-tool-permissions",
        Get,
        "/organization/projects/{project_id}/hosted_tool_permissions",
        (),
        ProjectHostedToolPermissions,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectModelPermissions,
        "retrieve-project-model-permissions",
        Get,
        "/organization/projects/{project_id}/model_permissions",
        (),
        ProjectModelPermissions,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectRole,
        "retrieve-project-role",
        Get,
        "/projects/{project_id}/roles/{role_id}",
        (),
        Role,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectServiceAccount,
        "retrieve-project-service-account",
        Get,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        (),
        ProjectServiceAccount,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectSpendAlert,
        "retrieve-project-spend-alert",
        Get,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        (),
        ProjectSpendAlert,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectUser,
        "retrieve-project-user",
        Get,
        "/organization/projects/{project_id}/users/{user_id}",
        (),
        ProjectUser,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveProjectUserRole,
        "retrieve-project-user-role",
        Get,
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveRole,
        "retrieve-role",
        Get,
        "/organization/roles/{role_id}",
        (),
        Role,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveUser,
        "retrieve-user",
        Get,
        "/organization/users/{user_id}",
        (),
        User,
        None,
        Json
    );
    admin_operation!(
        OpRetrieveUserRole,
        "retrieve-user-role",
        Get,
        "/organization/users/{user_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json
    );
    admin_operation!(
        OpUnassignGroupRole,
        "unassign-group-role",
        Delete,
        "/organization/groups/{group_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json
    );
    admin_operation!(
        OpUnassignProjectGroupRole,
        "unassign-project-group-role",
        Delete,
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json
    );
    admin_operation!(
        OpUnassignProjectUserRole,
        "unassign-project-user-role",
        Delete,
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json
    );
    admin_operation!(
        OpUnassignUserRole,
        "unassign-user-role",
        Delete,
        "/organization/users/{user_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json
    );
    admin_operation!(
        OpUpdateGroup,
        "update-group",
        Post,
        "/organization/groups/{group_id}",
        UpdateGroupBody,
        GroupResourceWithSuccess,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateOrganizationDataRetention,
        "update-organization-data-retention",
        Post,
        "/organization/data_retention",
        UpdateOrganizationDataRetentionBody,
        OrganizationDataRetention,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateOrganizationSpendAlert,
        "update-organization-spend-alert",
        Post,
        "/organization/spend_alerts/{alert_id}",
        CreateSpendAlertBody,
        OrganizationSpendAlert,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectDataRetention,
        "update-project-data-retention",
        Post,
        "/organization/projects/{project_id}/data_retention",
        UpdateProjectDataRetentionBody,
        ProjectDataRetention,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectHostedToolPermissions,
        "update-project-hosted-tool-permissions",
        Post,
        "/organization/projects/{project_id}/hosted_tool_permissions",
        ProjectHostedToolPermissionsUpdateRequest,
        ProjectHostedToolPermissions,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectModelPermissions,
        "update-project-model-permissions",
        Post,
        "/organization/projects/{project_id}/model_permissions",
        ProjectModelPermissionsUpdateRequest,
        ProjectModelPermissions,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectRateLimits,
        "update-project-rate-limits",
        Post,
        "/organization/projects/{project_id}/rate_limits/{rate_limit_id}",
        ProjectRateLimitUpdateRequest,
        ProjectRateLimit,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectRole,
        "update-project-role",
        Post,
        "/projects/{project_id}/roles/{role_id}",
        PublicUpdateOrganizationRoleBody,
        Role,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectServiceAccount,
        "update-project-service-account",
        Post,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        UpdateProjectServiceAccountBody,
        ProjectServiceAccount,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateProjectSpendAlert,
        "update-project-spend-alert",
        Post,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        CreateSpendAlertBody,
        ProjectSpendAlert,
        Json,
        Json
    );
    admin_operation!(
        OpUpdateRole,
        "update-role",
        Post,
        "/organization/roles/{role_id}",
        PublicUpdateOrganizationRoleBody,
        Role,
        Json,
        Json
    );
    admin_operation!(
        OpUploadCertificate,
        "uploadCertificate",
        Post,
        "/organization/certificates",
        UploadCertificateRequest,
        Certificate,
        Json,
        Json
    );
    admin_operation!(
        OpUsageAudioSpeeches,
        "usage-audio-speeches",
        Get,
        "/organization/usage/audio_speeches",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageAudioTranscriptions,
        "usage-audio-transcriptions",
        Get,
        "/organization/usage/audio_transcriptions",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageCodeInterpreterSessions,
        "usage-code-interpreter-sessions",
        Get,
        "/organization/usage/code_interpreter_sessions",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageCompletions,
        "usage-completions",
        Get,
        "/organization/usage/completions",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageCosts,
        "usage-costs",
        Get,
        "/organization/costs",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageEmbeddings,
        "usage-embeddings",
        Get,
        "/organization/usage/embeddings",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageFileSearchCalls,
        "usage-file-search-calls",
        Get,
        "/organization/usage/file_search_calls",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageImages,
        "usage-images",
        Get,
        "/organization/usage/images",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageModerations,
        "usage-moderations",
        Get,
        "/organization/usage/moderations",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageVectorStores,
        "usage-vector-stores",
        Get,
        "/organization/usage/vector_stores",
        (),
        UsageResponse,
        None,
        Json
    );
    admin_operation!(
        OpUsageWebSearchCalls,
        "usage-web-search-calls",
        Get,
        "/organization/usage/web_search_calls",
        (),
        UsageResponse,
        None,
        Json
    );
}

/// Cheap-to-clone, Administration-only client.
#[derive(Clone)]
pub struct AdminClient {
    inner: Arc<AdminInner>,
}

struct AdminInner {
    http: reqwest::Client,
    base_url: Url,
    authorization: HeaderValue,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
}

impl AdminClient {
    /// Start a secure builder from a dedicated Administration key.
    #[must_use]
    pub fn builder(api_key: AdminApiKey) -> AdminClientBuilder {
        AdminClientBuilder::new(api_key)
    }

    /// Build with the official base URL and secure defaults.
    pub fn new(api_key: AdminApiKey) -> Result<Self, Error> {
        Self::builder(api_key).build()
    }

    /// Configured base URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.inner.base_url
    }

    /// Create a typed request for any sealed Administration operation.
    #[must_use]
    pub fn request<O: AdminOperation>(&self) -> AdminRequest<O> {
        AdminRequest {
            client: self.clone(),
            path_parameters: Vec::new(),
            query: Vec::new(),
            body: None,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn api_keys(&self) -> AdminApiKeys {
        AdminApiKeys(self.clone())
    }

    #[must_use]
    pub fn audit_logs(&self) -> AdminAuditLogs {
        AdminAuditLogs(self.clone())
    }

    #[must_use]
    pub fn certificates(&self) -> AdminCertificates {
        AdminCertificates(self.clone())
    }

    #[must_use]
    pub fn data_retention(&self) -> AdminDataRetention {
        AdminDataRetention(self.clone())
    }

    #[must_use]
    pub fn groups(&self) -> AdminGroups {
        AdminGroups(self.clone())
    }

    #[must_use]
    pub fn users(&self) -> AdminUsers {
        AdminUsers(self.clone())
    }

    #[must_use]
    pub fn roles(&self) -> AdminRoles {
        AdminRoles(self.clone())
    }

    #[must_use]
    pub fn invites(&self) -> AdminInvites {
        AdminInvites(self.clone())
    }

    #[must_use]
    pub fn projects(&self) -> AdminProjects {
        AdminProjects(self.clone())
    }

    #[must_use]
    pub fn usage(&self) -> AdminUsage {
        AdminUsage(self.clone())
    }
}

impl fmt::Debug for AdminClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminClient")
            .field(
                "base_origin",
                &self.base_url().origin().ascii_serialization(),
            )
            .finish_non_exhaustive()
    }
}

/// Builder enforcing HTTPS and redirect-free Administration transport.
pub struct AdminClientBuilder {
    api_key: AdminApiKey,
    base_url: Option<Url>,
    allow_insecure_loopback: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    tls_backend: Option<TlsBackend>,
}

impl AdminClientBuilder {
    #[must_use]
    pub fn new(api_key: AdminApiKey) -> Self {
        Self {
            api_key,
            base_url: None,
            allow_insecure_loopback: false,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_json_body_bytes: DEFAULT_MAX_JSON_BODY_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
            tls_backend: default_tls_backend(),
        }
    }

    /// Replace the origin used by sealed routes. This never enables raw URLs.
    #[must_use]
    pub fn base_url(mut self, base_url: Url) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Permit HTTP only for a literal loopback address.
    #[must_use]
    pub const fn allow_insecure_loopback(mut self, allow: bool) -> Self {
        self.allow_insecure_loopback = allow;
        self
    }

    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn max_json_body_bytes(mut self, limit: usize) -> Self {
        self.max_json_body_bytes = limit;
        self
    }

    #[must_use]
    pub const fn max_error_body_bytes(mut self, limit: usize) -> Self {
        self.max_error_body_bytes = limit;
        self
    }

    #[must_use]
    pub const fn tls_backend(mut self, backend: TlsBackend) -> Self {
        self.tls_backend = Some(backend);
        self
    }

    pub fn build(self) -> Result<AdminClient, Error> {
        if self.connect_timeout.is_zero() || self.request_timeout.is_zero() {
            return Err(invalid_configuration("timeouts must be non-zero"));
        }
        if self.max_json_body_bytes == 0 || self.max_error_body_bytes == 0 {
            return Err(invalid_configuration(
                "response body limits must be non-zero",
            ));
        }
        let mut base_url = match self.base_url {
            Some(url) => url,
            None => Url::parse(DEFAULT_ADMIN_BASE_URL)
                .map_err(|error| invalid_configuration(error.to_string()))?,
        };
        validate_base_url(&base_url, self.allow_insecure_loopback)?;
        if base_url.scheme() == "https" && self.tls_backend.is_none() {
            return Err(invalid_configuration(
                "HTTPS requires a compiled TLS backend",
            ));
        }
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_owned();
            path.push('/');
            base_url.set_path(&path);
        }

        let authorization = self
            .api_key
            .authorization_header()
            .map_err(|error| invalid_configuration(error.to_string()))?;
        let builder = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(concat!("openai-rs-admin/", env!("CARGO_PKG_VERSION")));
        let builder = match self.tls_backend {
            #[cfg(feature = "rustls-tls")]
            Some(TlsBackend::Rustls) => builder.use_rustls_tls(),
            #[cfg(feature = "native-tls")]
            Some(TlsBackend::Native) => builder.use_native_tls(),
            None => builder,
        };
        let http = builder.build().map_err(Error::from_reqwest)?;
        Ok(AdminClient {
            inner: Arc::new(AdminInner {
                http,
                base_url,
                authorization,
                request_timeout: self.request_timeout,
                max_json_body_bytes: self.max_json_body_bytes,
                max_error_body_bytes: self.max_error_body_bytes,
            }),
        })
    }
}

impl fmt::Debug for AdminClientBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminClientBuilder")
            .field("api_key", &"[REDACTED]")
            .field(
                "base_origin",
                &self
                    .base_url
                    .as_ref()
                    .map(|url| url.origin().ascii_serialization()),
            )
            .field("allow_insecure_loopback", &self.allow_insecure_loopback)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .field("tls_backend", &self.tls_backend)
            .finish()
    }
}

/// Typed request bound to one sealed Administration operation.
pub struct AdminRequest<O: AdminOperation> {
    client: AdminClient,
    path_parameters: Vec<String>,
    query: Vec<(String, String)>,
    body: Option<O::Request>,
    marker: PhantomData<fn() -> O>,
}

impl<O: AdminOperation> AdminRequest<O> {
    /// Fill the next path placeholder. Values are always encoded as one segment.
    pub fn path_parameter(mut self, value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        validate_path_parameter(&value)?;
        self.path_parameters.push(value);
        Ok(self)
    }

    /// Encode a typed query object.
    pub fn query<Q: AdminQuery + ?Sized>(mut self, query: &Q) -> Result<Self, Error> {
        self.query = encode_query(query)?;
        Ok(self)
    }

    /// Attach the associated typed JSON request body.
    #[must_use]
    pub fn body(mut self, body: O::Request) -> Self {
        self.body = Some(body);
        self
    }

    /// Send and decode the associated response type.
    pub async fn send(self) -> Result<ApiResponse<O::Response>, Error> {
        self.client.clone().send(self).await
    }
}

impl<O: AdminOperation> fmt::Debug for AdminRequest<O> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminRequest")
            .field("operation", &O::ID)
            .field("path_parameter_count", &self.path_parameters.len())
            .field("query_parameter_count", &self.query.len())
            .field("has_body", &self.body.is_some())
            .finish()
    }
}

impl AdminClient {
    async fn send<O: AdminOperation>(
        &self,
        request: AdminRequest<O>,
    ) -> Result<ApiResponse<O::Response>, Error> {
        if O::AUTH != AdminAuthScope::Admin {
            return Err(invalid_configuration(
                "operation is not authorized for Administration credentials",
            ));
        }
        match (O::REQUEST_ENCODING, request.body.as_ref()) {
            (AdminRequestEncoding::Json, None) => {
                return Err(invalid_configuration(
                    "Administration JSON operation is missing its body",
                ));
            }
            (AdminRequestEncoding::None, Some(_)) => {
                return Err(invalid_configuration(
                    "bodyless Administration operation received a body",
                ));
            }
            (AdminRequestEncoding::None, None) | (AdminRequestEncoding::Json, Some(_)) => {}
        }

        let mut url = render_route(&self.inner.base_url, O::ROUTE, &request.path_parameters)?;
        if !request.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in request.query {
                pairs.append_pair(&name, &value);
            }
        }
        if !same_origin(&url, &self.inner.base_url) {
            return Err(invalid_configuration(
                "Administration route escaped its configured credential origin",
            ));
        }

        let mut builder = self
            .inner
            .http
            .request(O::METHOD.clone(), url)
            .timeout(self.inner.request_timeout)
            .header(header::AUTHORIZATION, self.inner.authorization.clone())
            .header(header::ACCEPT, "application/json");
        if let Some(body) = request.body.as_ref() {
            let encoded = serde_json::to_vec(body).map_err(Error::Encode)?;
            builder = builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(encoded);
        }
        let response = builder.send().await.map_err(Error::from_reqwest)?;
        if !response.status().is_success() {
            return Err(self.error_from_response(response).await);
        }
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_limited(response, self.inner.max_json_body_bytes, &meta).await?;
        match O::RESPONSE_MODE {
            AdminResponseMode::Empty => {
                let decoded =
                    serde_json::from_value::<O::Response>(Value::Null).map_err(|source| {
                        Error::Decode {
                            source,
                            path: None,
                            meta_status: meta.status(),
                            request_id: meta.request_id().map(Box::<str>::from),
                            body: BodyPreview::from_bytes(&body, false),
                        }
                    })?;
                Ok(ApiResponse::new(decoded, meta))
            }
            AdminResponseMode::Json => {
                let decoded = serde_json::from_slice::<O::Response>(&body).map_err(|source| {
                    Error::Decode {
                        source,
                        path: None,
                        meta_status: meta.status(),
                        request_id: meta.request_id().map(Box::<str>::from),
                        body: BodyPreview::from_bytes(
                            &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                            body.len() > DECODE_PREVIEW_BYTES,
                        ),
                    }
                })?;
                Ok(ApiResponse::new(decoded, meta))
            }
        }
    }

    async fn error_from_response(&self, response: reqwest::Response) -> Error {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        match read_limited(response, self.inner.max_error_body_bytes, &meta).await {
            Ok(body) => ApiError::from_body(meta, &body, false).into(),
            Err(error) => error,
        }
    }
}

fn validate_base_url(base_url: &Url, allow_insecure_loopback: bool) -> Result<(), Error> {
    if !base_url.username().is_empty() || base_url.password().is_some() {
        return Err(invalid_configuration(
            "base URL must not contain user information",
        ));
    }
    if base_url.query().is_some() || base_url.fragment().is_some() {
        return Err(invalid_configuration(
            "base URL must not contain query or fragment",
        ));
    }
    if base_url.cannot_be_a_base() || base_url.host().is_none() {
        return Err(invalid_configuration(
            "base URL must be absolute and hierarchical",
        ));
    }
    match base_url.scheme() {
        "https" => Ok(()),
        "http" if allow_insecure_loopback && is_literal_loopback(base_url) => Ok(()),
        "http" => Err(invalid_configuration(
            "HTTP requires allow_insecure_loopback(true) and a literal loopback IP",
        )),
        _ => Err(invalid_configuration("base URL scheme must be HTTPS")),
    }
}

fn is_literal_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(_)) | None => false,
    }
}

fn validate_path_parameter(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::InvalidPathParameter {
            name: "admin_path_parameter",
            reason: "must not be empty",
        });
    }
    if value == "." || value == ".." {
        return Err(Error::InvalidPathParameter {
            name: "admin_path_parameter",
            reason: "must not be a dot segment",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidPathParameter {
            name: "admin_path_parameter",
            reason: "must not contain control characters",
        });
    }
    Ok(())
}

fn render_route(base_url: &Url, route: &str, parameters: &[String]) -> Result<Url, Error> {
    if !(route.starts_with("/organization/") || route.starts_with("/projects/")) {
        return Err(invalid_configuration(
            "Administration route is outside its sealed prefixes",
        ));
    }
    let placeholders = route
        .split('/')
        .filter(|segment| segment.starts_with('{') && segment.ends_with('}'))
        .count();
    if placeholders != parameters.len() {
        return Err(invalid_configuration(format!(
            "Administration route expected {placeholders} path parameters but received {}",
            parameters.len()
        )));
    }

    let mut url = base_url.clone();
    let mut parameter = parameters.iter();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| invalid_configuration("base URL cannot accept path segments"))?;
        segments.pop_if_empty();
        for segment in route.trim_start_matches('/').split('/') {
            if segment.starts_with('{') && segment.ends_with('}') {
                let value = parameter
                    .next()
                    .ok_or_else(|| invalid_configuration("missing Administration path value"))?;
                segments.push(value);
            } else {
                segments.push(segment);
            }
        }
    }
    Ok(url)
}

fn encode_query<Q: Serialize + ?Sized>(query: &Q) -> Result<Vec<(String, String)>, Error> {
    let Value::Object(fields) = serde_json::to_value(query)
        .map_err(|error| Error::EncodeQuery(error.to_string().into()))?
    else {
        return Err(Error::EncodeQuery(
            "Administration query must serialize as an object".into(),
        ));
    };
    let mut pairs = Vec::new();
    for (name, value) in fields {
        append_query_value(&mut pairs, &name, value)?;
    }
    Ok(pairs)
}

fn append_query_value(
    pairs: &mut Vec<(String, String)>,
    name: &str,
    value: Value,
) -> Result<(), Error> {
    match value {
        Value::Null => pairs.push((name.to_owned(), String::new())),
        Value::Bool(value) => pairs.push((name.to_owned(), value.to_string())),
        Value::Number(value) => pairs.push((name.to_owned(), value.to_string())),
        Value::String(value) => pairs.push((name.to_owned(), value)),
        Value::Array(values) => {
            for value in values {
                append_query_value(pairs, name, value)?;
            }
        }
        Value::Object(fields) => {
            for (child, value) in fields {
                let nested = format!("{name}[{child}]");
                append_query_value(pairs, &nested, value)?;
            }
        }
    }
    Ok(())
}

async fn read_limited(
    response: reqwest::Response,
    limit: usize,
    meta: &ResponseMeta,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        });
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(Error::from_reqwest)?;
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(Error::BodyTooLarge {
                limit,
                status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn invalid_configuration(message: impl Into<Box<str>>) -> Error {
    Error::InvalidConfiguration(message.into())
}

const fn default_tls_backend() -> Option<TlsBackend> {
    #[cfg(feature = "rustls-tls")]
    {
        Some(TlsBackend::Rustls)
    }
    #[cfg(all(not(feature = "rustls-tls"), feature = "native-tls"))]
    {
        Some(TlsBackend::Native)
    }
    #[cfg(all(not(feature = "rustls-tls"), not(feature = "native-tls")))]
    {
        None
    }
}

#[derive(Clone, Debug)]
pub struct AdminApiKeys(AdminClient);

impl AdminApiKeys {
    pub async fn list(&self, params: &AdminListParams) -> Result<ApiResponse<ApiKeyList>, Error> {
        self.0
            .request::<operations::OpAdminApiKeysList>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(
        &self,
        key_id: &str,
    ) -> Result<ApiResponse<openai_rs_types::admin::AdminApiKey>, Error> {
        self.0
            .request::<operations::OpAdminApiKeysGet>()
            .path_parameter(key_id)?
            .send()
            .await
    }

    pub async fn create(
        &self,
        request: AdminApiKeyCreateRequest,
    ) -> Result<ApiResponse<AdminApiKeyCreateResponse>, Error> {
        self.0
            .request::<operations::OpAdminApiKeysCreate>()
            .body(request)
            .send()
            .await
    }

    pub async fn delete(&self, key_id: &str) -> Result<ApiResponse<()>, Error> {
        self.0
            .request::<operations::OpAdminApiKeysDelete>()
            .path_parameter(key_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminAuditLogs(AdminClient);

impl AdminAuditLogs {
    pub async fn list(
        &self,
        params: &AuditLogListParams,
    ) -> Result<ApiResponse<ListAuditLogsResponse>, Error> {
        self.0
            .request::<operations::OpListAuditLogs>()
            .query(params)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminCertificates(AdminClient);

impl AdminCertificates {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<ListCertificatesResponse>, Error> {
        self.0
            .request::<operations::OpListOrganizationCertificates>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, certificate_id: &str) -> Result<ApiResponse<Certificate>, Error> {
        self.0
            .request::<operations::OpGetCertificate>()
            .path_parameter(certificate_id)?
            .send()
            .await
    }

    pub async fn upload(
        &self,
        request: UploadCertificateRequest,
    ) -> Result<ApiResponse<Certificate>, Error> {
        self.0
            .request::<operations::OpUploadCertificate>()
            .body(request)
            .send()
            .await
    }

    pub async fn modify(
        &self,
        certificate_id: &str,
        request: ModifyCertificateRequest,
    ) -> Result<ApiResponse<Certificate>, Error> {
        self.0
            .request::<operations::OpModifyCertificate>()
            .path_parameter(certificate_id)?
            .body(request)
            .send()
            .await
    }

    pub async fn delete(
        &self,
        certificate_id: &str,
    ) -> Result<ApiResponse<DeleteCertificateResponse>, Error> {
        self.0
            .request::<operations::OpDeleteCertificate>()
            .path_parameter(certificate_id)?
            .send()
            .await
    }

    pub async fn activate(
        &self,
        request: ToggleCertificatesRequest,
    ) -> Result<ApiResponse<OrganizationCertificateActivationResponse>, Error> {
        self.0
            .request::<operations::OpActivateOrganizationCertificates>()
            .body(request)
            .send()
            .await
    }

    pub async fn deactivate(
        &self,
        request: ToggleCertificatesRequest,
    ) -> Result<ApiResponse<OrganizationCertificateDeactivationResponse>, Error> {
        self.0
            .request::<operations::OpDeactivateOrganizationCertificates>()
            .body(request)
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminDataRetention(AdminClient);

impl AdminDataRetention {
    pub async fn organization(&self) -> Result<ApiResponse<OrganizationDataRetention>, Error> {
        self.0
            .request::<operations::OpRetrieveOrganizationDataRetention>()
            .send()
            .await
    }

    pub async fn update_organization(
        &self,
        request: UpdateOrganizationDataRetentionBody,
    ) -> Result<ApiResponse<OrganizationDataRetention>, Error> {
        self.0
            .request::<operations::OpUpdateOrganizationDataRetention>()
            .body(request)
            .send()
            .await
    }

    pub async fn project(
        &self,
        project_id: &str,
    ) -> Result<ApiResponse<ProjectDataRetention>, Error> {
        self.0
            .request::<operations::OpRetrieveProjectDataRetention>()
            .path_parameter(project_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminGroups(AdminClient);

impl AdminGroups {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<GroupListResource>, Error> {
        self.0
            .request::<operations::OpListGroups>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, group_id: &str) -> Result<ApiResponse<GroupResponse>, Error> {
        self.0
            .request::<operations::OpRetrieveGroup>()
            .path_parameter(group_id)?
            .send()
            .await
    }

    pub async fn create(
        &self,
        request: CreateGroupBody,
    ) -> Result<ApiResponse<GroupResponse>, Error> {
        self.0
            .request::<operations::OpCreateGroup>()
            .body(request)
            .send()
            .await
    }

    pub async fn update(
        &self,
        group_id: &str,
        request: UpdateGroupBody,
    ) -> Result<ApiResponse<GroupResourceWithSuccess>, Error> {
        self.0
            .request::<operations::OpUpdateGroup>()
            .path_parameter(group_id)?
            .body(request)
            .send()
            .await
    }

    pub async fn delete(&self, group_id: &str) -> Result<ApiResponse<GroupDeletedResource>, Error> {
        self.0
            .request::<operations::OpDeleteGroup>()
            .path_parameter(group_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminUsers(AdminClient);

impl AdminUsers {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<UserListResponse>, Error> {
        self.0
            .request::<operations::OpListUsers>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, user_id: &str) -> Result<ApiResponse<User>, Error> {
        self.0
            .request::<operations::OpRetrieveUser>()
            .path_parameter(user_id)?
            .send()
            .await
    }

    pub async fn update(
        &self,
        user_id: &str,
        request: UserRoleUpdateRequest,
    ) -> Result<ApiResponse<User>, Error> {
        self.0
            .request::<operations::OpModifyUser>()
            .path_parameter(user_id)?
            .body(request)
            .send()
            .await
    }

    pub async fn delete(&self, user_id: &str) -> Result<ApiResponse<UserDeleteResponse>, Error> {
        self.0
            .request::<operations::OpDeleteUser>()
            .path_parameter(user_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminRoles(AdminClient);

impl AdminRoles {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<PublicRoleListResource>, Error> {
        self.0
            .request::<operations::OpListRoles>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, role_id: &str) -> Result<ApiResponse<Role>, Error> {
        self.0
            .request::<operations::OpRetrieveRole>()
            .path_parameter(role_id)?
            .send()
            .await
    }

    pub async fn create(
        &self,
        request: PublicCreateOrganizationRoleBody,
    ) -> Result<ApiResponse<Role>, Error> {
        self.0
            .request::<operations::OpCreateRole>()
            .body(request)
            .send()
            .await
    }

    pub async fn update(
        &self,
        role_id: &str,
        request: PublicUpdateOrganizationRoleBody,
    ) -> Result<ApiResponse<Role>, Error> {
        self.0
            .request::<operations::OpUpdateRole>()
            .path_parameter(role_id)?
            .body(request)
            .send()
            .await
    }

    pub async fn delete(&self, role_id: &str) -> Result<ApiResponse<RoleDeletedResource>, Error> {
        self.0
            .request::<operations::OpDeleteRole>()
            .path_parameter(role_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminInvites(AdminClient);

impl AdminInvites {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<InviteListResponse>, Error> {
        self.0
            .request::<operations::OpListInvites>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, invite_id: &str) -> Result<ApiResponse<Invite>, Error> {
        self.0
            .request::<operations::OpRetrieveInvite>()
            .path_parameter(invite_id)?
            .send()
            .await
    }

    pub async fn create(&self, request: InviteRequest) -> Result<ApiResponse<Invite>, Error> {
        self.0
            .request::<operations::OpInviteUser>()
            .body(request)
            .send()
            .await
    }

    pub async fn delete(
        &self,
        invite_id: &str,
    ) -> Result<ApiResponse<InviteDeleteResponse>, Error> {
        self.0
            .request::<operations::OpDeleteInvite>()
            .path_parameter(invite_id)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminProjects(AdminClient);

impl AdminProjects {
    pub async fn list(
        &self,
        params: &AdminListParams,
    ) -> Result<ApiResponse<ProjectListResponse>, Error> {
        self.0
            .request::<operations::OpListProjects>()
            .query(params)?
            .send()
            .await
    }

    pub async fn retrieve(&self, project_id: &str) -> Result<ApiResponse<Project>, Error> {
        self.0
            .request::<operations::OpRetrieveProject>()
            .path_parameter(project_id)?
            .send()
            .await
    }

    pub async fn create(
        &self,
        request: ProjectCreateRequest,
    ) -> Result<ApiResponse<Project>, Error> {
        self.0
            .request::<operations::OpCreateProject>()
            .body(request)
            .send()
            .await
    }

    pub async fn update(
        &self,
        project_id: &str,
        request: ProjectUpdateRequest,
    ) -> Result<ApiResponse<Project>, Error> {
        self.0
            .request::<operations::OpModifyProject>()
            .path_parameter(project_id)?
            .body(request)
            .send()
            .await
    }

    pub async fn archive(&self, project_id: &str) -> Result<ApiResponse<Project>, Error> {
        self.0
            .request::<operations::OpArchiveProject>()
            .path_parameter(project_id)?
            .send()
            .await
    }

    pub async fn model_permissions(
        &self,
        project_id: &str,
    ) -> Result<ApiResponse<ProjectModelPermissions>, Error> {
        self.0
            .request::<operations::OpRetrieveProjectModelPermissions>()
            .path_parameter(project_id)?
            .send()
            .await
    }

    pub async fn hosted_tool_permissions(
        &self,
        project_id: &str,
    ) -> Result<ApiResponse<ProjectHostedToolPermissions>, Error> {
        self.0
            .request::<operations::OpRetrieveProjectHostedToolPermissions>()
            .path_parameter(project_id)?
            .send()
            .await
    }

    pub async fn rate_limits(
        &self,
        project_id: &str,
        params: &AdminListParams,
    ) -> Result<ApiResponse<ProjectRateLimitListResponse>, Error> {
        self.0
            .request::<operations::OpListProjectRateLimits>()
            .path_parameter(project_id)?
            .query(params)?
            .send()
            .await
    }
}

#[derive(Clone, Debug)]
pub struct AdminUsage(AdminClient);

macro_rules! usage_method {
    ($name:ident, $operation:ty) => {
        pub async fn $name(
            &self,
            params: &UsageQueryParams,
        ) -> Result<ApiResponse<UsageResponse>, Error> {
            self.0.request::<$operation>().query(params)?.send().await
        }
    };
}

impl AdminUsage {
    usage_method!(completions, operations::OpUsageCompletions);
    usage_method!(embeddings, operations::OpUsageEmbeddings);
    usage_method!(moderations, operations::OpUsageModerations);
    usage_method!(images, operations::OpUsageImages);
    usage_method!(audio_speeches, operations::OpUsageAudioSpeeches);
    usage_method!(audio_transcriptions, operations::OpUsageAudioTranscriptions);
    usage_method!(vector_stores, operations::OpUsageVectorStores);
    usage_method!(
        code_interpreter_sessions,
        operations::OpUsageCodeInterpreterSessions
    );
    usage_method!(file_search_calls, operations::OpUsageFileSearchCalls);
    usage_method!(web_search_calls, operations::OpUsageWebSearchCalls);
    usage_method!(costs, operations::OpUsageCosts);
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, Response, body::Incoming, service::service_fn};
    use hyper_util::rt::TokioIo;
    use static_assertions::{assert_impl_all, assert_not_impl_any};
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;

    assert_impl_all!(AdminApiKey: Clone, Send, Sync);
    assert_not_impl_any!(AdminApiKey: Serialize, DeserializeOwned);
    assert_not_impl_any!(AdminApiKey: Into<crate::ApiKey>);
    assert_not_impl_any!(crate::ApiKey: Into<AdminApiKey>);
    assert_impl_all!(AdminClient: Clone, Send, Sync);

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    async fn spawn_server() -> (Url, Arc<Mutex<Vec<CapturedRequest>>>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let task_captured = Arc::clone(&captured);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let connection_captured = Arc::clone(&task_captured);
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let request_captured = Arc::clone(&connection_captured);
                        async move {
                            let method = request.method().clone();
                            let path_and_query = request.uri().path_and_query().map_or_else(
                                || request.uri().path().to_owned(),
                                ToString::to_string,
                            );
                            let authorization = request
                                .headers()
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_owned);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect request")
                                .to_bytes()
                                .to_vec();
                            request_captured
                                .lock()
                                .expect("capture lock")
                                .push(CapturedRequest {
                                    method,
                                    path_and_query: path_and_query.clone(),
                                    authorization,
                                    body,
                                });

                            let (status, response_body) = if path_and_query
                                .starts_with("/v1/organization/users")
                            {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"list","data":[],"has_more":false}"#,
                                )
                            } else if path_and_query == "/v1/organization/groups" {
                                (
                                    StatusCode::OK,
                                    r#"{"id":"group_1","name":"engineering","created_at":1,"is_scim_managed":false,"group_type":"group"}"#,
                                )
                            } else if path_and_query == "/v1/organization/admin_api_keys/key_1" {
                                (StatusCode::NO_CONTENT, "")
                            } else if path_and_query == "/v1/organization/admin_api_keys" {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"organization.admin_api_key","id":"key_1","redacted_value":"sk-admin...","created_at":1,"expires_at":null,"owner":{},"value":"sk-admin-new-secret"}"#,
                                )
                            } else if path_and_query.starts_with("/v1/organization/roles") {
                                (
                                    StatusCode::UNAUTHORIZED,
                                    r#"{"error":{"message":"secret failure body","type":"auth_error","code":"invalid_admin_key"}}"#,
                                )
                            } else {
                                (StatusCode::NOT_FOUND, r#"{"error":{"message":"missing"}}"#)
                            };
                            let response = Response::builder()
                                .status(status)
                                .header(header::CONTENT_TYPE, "application/json")
                                .header("x-request-id", "req_admin_test")
                                .body(Full::new(Bytes::copy_from_slice(response_body.as_bytes())))
                                .expect("response");
                            Ok::<_, std::convert::Infallible>(response)
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let url = Url::parse(&format!("http://{address}/v1/")).expect("server URL");
        (url, captured, task)
    }

    fn key() -> AdminApiKey {
        AdminApiKey::new("admin-test-placeholder-key").expect("valid admin key")
    }

    #[test]
    fn credential_and_builder_debug_are_redacted_and_base_is_strict() {
        assert!(!format!("{:?}", key()).contains("placeholder"));
        let builder = AdminClient::builder(key());
        assert!(!format!("{builder:?}").contains("placeholder"));

        let loopback = Url::parse("http://127.0.0.1:1234/v1/").expect("test URL");
        assert!(
            AdminClient::builder(key())
                .base_url(loopback.clone())
                .build()
                .is_err()
        );
        assert!(
            AdminClient::builder(key())
                .base_url(loopback)
                .allow_insecure_loopback(true)
                .build()
                .is_ok()
        );
        let localhost = Url::parse("http://localhost:1234/v1/").expect("test URL");
        assert!(
            AdminClient::builder(key())
                .base_url(localhost)
                .allow_insecure_loopback(true)
                .build()
                .is_err()
        );
    }

    #[test]
    fn query_encoder_supports_arrays_null_and_deep_objects() {
        let query = serde_json::json!({
            "project_ids": ["proj_1", "proj_2"],
            "metadata": {"team": "sdk"},
            "after": null
        });
        let pairs = encode_query(&query).expect("encode query");
        assert!(pairs.contains(&("project_ids".to_owned(), "proj_1".to_owned())));
        assert!(pairs.contains(&("project_ids".to_owned(), "proj_2".to_owned())));
        assert!(pairs.contains(&("metadata[team]".to_owned(), "sdk".to_owned())));
        assert!(pairs.contains(&("after".to_owned(), String::new())));

        let base = Url::parse("https://api.openai.com/v1/").expect("base URL");
        let route = render_route(
            &base,
            "/organization/projects/{project_id}/api_keys/{api_key_id}",
            &["proj/a b".to_owned(), "key?1".to_owned()],
        )
        .expect("render sealed route");
        assert_eq!(
            route.as_str(),
            "https://api.openai.com/v1/organization/projects/proj%2Fa%20b/api_keys/key%3F1"
        );
    }

    #[tokio::test]
    async fn loopback_get_post_delete_and_secret_response_use_admin_auth() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");

        let users = client
            .users()
            .list(&AdminListParams::default())
            .await
            .expect("list users");
        assert!(users.data.is_empty());

        let group = client
            .groups()
            .create(CreateGroupBody {
                name: "engineering".to_owned(),
            })
            .await
            .expect("create group");
        assert_eq!(group.name, "engineering");

        client.api_keys().delete("key_1").await.expect("delete key");

        let created = client
            .api_keys()
            .create(AdminApiKeyCreateRequest::new("automation"))
            .await
            .expect("create key");
        assert!(!format!("{:?}", created.body()).contains("new-secret"));

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 4);
        assert_eq!(captured[0].method, Method::GET);
        assert_eq!(captured[1].method, Method::POST);
        assert_eq!(captured[2].method, Method::DELETE);
        assert_eq!(captured[3].method, Method::POST);
        assert_eq!(captured[0].path_and_query, "/v1/organization/users");
        assert_eq!(captured[1].path_and_query, "/v1/organization/groups");
        assert_eq!(
            captured[2].path_and_query,
            "/v1/organization/admin_api_keys/key_1"
        );
        assert_eq!(
            captured[3].path_and_query,
            "/v1/organization/admin_api_keys"
        );
        for request in captured.iter() {
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer admin-test-placeholder-key")
            );
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&captured[1].body).expect("group JSON")["name"],
            "engineering"
        );
        task.abort();
    }

    #[tokio::test]
    async fn loopback_errors_are_typed_and_redacted() {
        let (base_url, _captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");
        let error = client
            .roles()
            .list(&AdminListParams::default())
            .await
            .expect_err("server returns an error");
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.request_id(), Some("req_admin_test"));
        assert!(!format!("{error:?}").contains("secret failure body"));
        task.abort();
    }

    fn assert_operation<O: AdminOperation>() -> &'static str {
        assert_eq!(O::AUTH, AdminAuthScope::Admin);
        assert!(!O::ID.is_empty());
        assert!(O::ROUTE.starts_with('/'));
        O::ID
    }

    #[test]
    fn every_manifest_entry_has_a_unique_compiling_operation_marker() {
        assert_eq!(ADMIN_OPERATION_MANIFEST.len(), 119);
        let mut bound_ids: HashSet<&'static str> = HashSet::new();
        assert!(bound_ids.insert(assert_operation::<
            operations::OpCreateanAPIkeyforaserviceaccount,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteorganizationspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteprojectspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpGetorganizationspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpGetprojectspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateorganizationspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateprojectspendlimit>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpActivateOrganizationCertificates,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpActivateProjectCertificates>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAddGroupUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAddProjectGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAdminApiKeysCreate>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAdminApiKeysDelete>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAdminApiKeysGet>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAdminApiKeysList>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpArchiveProject>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAssignGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAssignProjectGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAssignProjectUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpAssignUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateOrganizationSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateProject>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateProjectRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateProjectServiceAccount>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateProjectSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateProjectUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpCreateRole>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpDeactivateOrganizationCertificates,
        >()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpDeactivateProjectCertificates,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteInvite>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteOrganizationSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteProjectApiKey>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpDeleteProjectModelPermissions,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteProjectRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteProjectServiceAccount>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteProjectSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteProjectUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpDeleteCertificate>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpGetCertificate>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpInviteUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListAuditLogs>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListGroupRoleAssignments>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListGroupUsers>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListGroups>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListInvites>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListOrganizationSpendAlerts>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectApiKeys>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpListProjectGroupRoleAssignments,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectGroups>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectRateLimits>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectRoles>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectServiceAccounts>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectSpendAlerts>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpListProjectUserRoleAssignments,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectUsers>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjects>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListRoles>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListUserRoleAssignments>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListUsers>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListOrganizationCertificates>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpListProjectCertificates>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpModifyProject>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpModifyProjectUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpModifyUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpModifyCertificate>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRemoveGroupUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRemoveProjectGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveGroupUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveInvite>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpRetrieveOrganizationDataRetention,
        >()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpRetrieveOrganizationSpendAlert,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProject>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectApiKey>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectDataRetention>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectGroup>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpRetrieveProjectHostedToolPermissions,
        >()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpRetrieveProjectModelPermissions,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectRole>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpRetrieveProjectServiceAccount,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveProjectUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveUser>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpRetrieveUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUnassignGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUnassignProjectGroupRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUnassignProjectUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUnassignUserRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateGroup>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpUpdateOrganizationDataRetention,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateOrganizationSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateProjectDataRetention>()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpUpdateProjectHostedToolPermissions,
        >()));
        assert!(bound_ids.insert(assert_operation::<
            operations::OpUpdateProjectModelPermissions,
        >()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateProjectRateLimits>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateProjectRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateProjectServiceAccount>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateProjectSpendAlert>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUpdateRole>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUploadCertificate>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageAudioSpeeches>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageAudioTranscriptions>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageCodeInterpreterSessions>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageCompletions>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageCosts>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageEmbeddings>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageFileSearchCalls>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageImages>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageModerations>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageVectorStores>()));
        assert!(bound_ids.insert(assert_operation::<operations::OpUsageWebSearchCalls>()));
        assert_eq!(bound_ids.len(), ADMIN_OPERATION_MANIFEST.len());
        for operation in ADMIN_OPERATION_MANIFEST {
            assert!(bound_ids.contains(operation.operation_id));
        }
    }
}
