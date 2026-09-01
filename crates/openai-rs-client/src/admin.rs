//! Dedicated Administration API client.
//!
//! [`AdminClient`] cannot be constructed from or converted into the ordinary
//! Platform [`crate::Client`]. Its sealed operation markers all carry
//! [`AdminAuthScope::Admin`], and request URLs are assembled only from frozen
//! route templates.

use std::{
    fmt,
    marker::PhantomData,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use futures_util::StreamExt;
use http::{HeaderValue, Method, StatusCode, header};
use openai_rs_types::admin::*;
use openai_rs_types::fine_tuning::{
    CreateFineTuningCheckpointPermissionRequest, DeleteFineTuningCheckpointPermissionResponse,
    ListFineTuningCheckpointPermissionResponse, ListFineTuningCheckpointPermissionsParams,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error as ThisError;
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::operation::RetryClass;
use crate::transport::should_retry_response;
use crate::{
    ApiError, ApiResponse, BodyPreview, Error, ResponseMeta, RetryPolicy, TlsBackend, trace,
};

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
    const SUCCESS_STATUSES: &'static [StatusCode];
    const RESPONSE_CONTENT_TYPES: &'static [&'static str];
    const REQUEST_TYPE: &'static str;
    const RESPONSE_TYPE: &'static str;
    const REQUEST_SCHEMA_REFS: &'static [&'static str];
    const RESPONSE_SCHEMA_REFS: &'static [&'static str];
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

admin_query!(
    AdminListParams,
    AuditLogListParams,
    UsageQueryParams,
    UsageCostsQueryParams,
    ListFineTuningCheckpointPermissionsParams,
    CertificateGetParams,
    ProjectGroupGetParams,
);

macro_rules! admin_operation {
    ($name:ident, $id:literal, $method:ident, $route:literal, $request:ty, $response:ty, $encoding:ident, $mode:ident, $request_refs:expr, $response_refs:expr) => {
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
            const SUCCESS_STATUSES: &'static [StatusCode] = &[StatusCode::OK];
            const RESPONSE_CONTENT_TYPES: &'static [&'static str] = &["application/json"];
            const REQUEST_TYPE: &'static str = stringify!($request);
            const RESPONSE_TYPE: &'static str = stringify!($response);
            const REQUEST_SCHEMA_REFS: &'static [&'static str] = $request_refs;
            const RESPONSE_SCHEMA_REFS: &'static [&'static str] = $response_refs;
        }
        impl $name {
            pub const CONTRACT: AdminClientOperationContract = AdminClientOperationContract {
                operation_id: $id,
                method: admin_method_name!($method),
                path: $route,
                request_mode: admin_encoding_name!($encoding),
                response_mode: admin_response_name!($mode),
                success_statuses: &[200],
                response_content_types: &["application/json"],
                request_type: stringify!($request),
                response_type: stringify!($response),
                request_schema_refs: $request_refs,
                response_schema_refs: $response_refs,
            };
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

macro_rules! admin_method_name {
    (Get) => {
        "GET"
    };
    (Post) => {
        "POST"
    };
    (Delete) => {
        "DELETE"
    };
}

macro_rules! admin_encoding_name {
    (None) => {
        "none"
    };
    (Json) => {
        "json"
    };
}

macro_rules! admin_response_name {
    (Json) => {
        "json"
    };
}

/// Reviewable effective wire contract generated by sealed operation bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdminClientOperationContract {
    pub operation_id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub request_mode: &'static str,
    pub response_mode: &'static str,
    pub success_statuses: &'static [u16],
    pub response_content_types: &'static [&'static str],
    pub request_type: &'static str,
    pub response_type: &'static str,
    pub request_schema_refs: &'static [&'static str],
    pub response_schema_refs: &'static [&'static str],
}

/// Sealed Administration operation markers generated from the frozen manifest.
///
/// Three of these operations are one-shot secret mints — the response carries
/// a credential `value` exactly once and never again:
///
/// - [`OpAdminApiKeysCreate`] (`admin-api-keys-create`) mints an organization
///   Admin API key;
/// - [`OpCreateanAPIkeyforaserviceaccount`] mints a project service-account
///   API key;
/// - [`OpCreateProjectServiceAccount`] mints a service account together with
///   its first API key.
///
/// Replay risk: under the default [`RetryPolicy::openai_compatible`] these
/// `POST`s are classified `Replayable`, so a timeout after the server already
/// minted the credential can trigger a retry that mints a second, unobserved
/// secret. Callers that must avoid orphaned credentials should build the
/// client with [`RetryPolicy::conservative`] (read-only retries only) or
/// [`RetryPolicy::disabled`] — see [`AdminClientBuilder::with_retry_policy`].
pub mod operations {
    use http::{Method, StatusCode};
    use openai_rs_types::admin::*;
    use openai_rs_types::fine_tuning::{
        CreateFineTuningCheckpointPermissionRequest, DeleteFineTuningCheckpointPermissionResponse,
        ListFineTuningCheckpointPermissionResponse,
    };

    use super::{
        AdminClientOperationContract, AdminOperation, AdminRequestEncoding, AdminResponseMode,
        admin_operation_private,
    };

    admin_operation!(
        OpCreateanAPIkeyforaserviceaccount,
        "CreateanAPIkeyforaserviceaccount",
        Post,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}/api_keys",
        CreateProjectServiceAccountApiKeyBody,
        ServiceAccountApiKeyBody,
        Json,
        Json,
        &["#/components/schemas/CreateProjectServiceAccountApiKeyBody"],
        &["#/components/schemas/ServiceAccountApiKeyBody"]
    );
    admin_operation!(
        OpDeleteorganizationspendlimit,
        "Deleteorganizationspendlimit",
        Delete,
        "/organization/spend_limit",
        (),
        OrganizationSpendLimitDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationSpendLimitDeletedResource"]
    );
    admin_operation!(
        OpDeleteprojectspendlimit,
        "Deleteprojectspendlimit",
        Delete,
        "/organization/projects/{project_id}/spend_limit",
        (),
        ProjectSpendLimitDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectSpendLimitDeletedResource"]
    );
    admin_operation!(
        OpGetorganizationspendlimit,
        "Getorganizationspendlimit",
        Get,
        "/organization/spend_limit",
        (),
        OrganizationSpendLimitResource,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationSpendLimitResource"]
    );
    admin_operation!(
        OpGetprojectspendlimit,
        "Getprojectspendlimit",
        Get,
        "/organization/projects/{project_id}/spend_limit",
        (),
        ProjectSpendLimitResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectSpendLimitResource"]
    );
    admin_operation!(
        OpUpdateorganizationspendlimit,
        "Updateorganizationspendlimit",
        Post,
        "/organization/spend_limit",
        UpdateOrganizationSpendLimitBody,
        OrganizationSpendLimitResource,
        Json,
        Json,
        &["#/components/schemas/UpdateOrganizationSpendLimitBody"],
        &["#/components/schemas/OrganizationSpendLimitResource"]
    );
    admin_operation!(
        OpUpdateprojectspendlimit,
        "Updateprojectspendlimit",
        Post,
        "/organization/projects/{project_id}/spend_limit",
        UpdateProjectSpendLimitBody,
        ProjectSpendLimitResource,
        Json,
        Json,
        &["#/components/schemas/UpdateProjectSpendLimitBody"],
        &["#/components/schemas/ProjectSpendLimitResource"]
    );
    admin_operation!(
        OpActivateOrganizationCertificates,
        "activateOrganizationCertificates",
        Post,
        "/organization/certificates/activate",
        ToggleCertificatesRequest,
        OrganizationCertificateActivationResponse,
        Json,
        Json,
        &["#/components/schemas/ToggleCertificatesRequest"],
        &["#/components/schemas/OrganizationCertificateActivationResponse"]
    );
    admin_operation!(
        OpActivateProjectCertificates,
        "activateProjectCertificates",
        Post,
        "/organization/projects/{project_id}/certificates/activate",
        ToggleCertificatesRequest,
        OrganizationProjectCertificateActivationResponse,
        Json,
        Json,
        &["#/components/schemas/ToggleCertificatesRequest"],
        &["#/components/schemas/OrganizationProjectCertificateActivationResponse"]
    );
    admin_operation!(
        OpAddGroupUser,
        "add-group-user",
        Post,
        "/organization/groups/{group_id}/users",
        CreateGroupUserBody,
        GroupUserAssignment,
        Json,
        Json,
        &["#/components/schemas/CreateGroupUserBody"],
        &["#/components/schemas/GroupUserAssignment"]
    );
    admin_operation!(
        OpAddProjectGroup,
        "add-project-group",
        Post,
        "/organization/projects/{project_id}/groups",
        InviteProjectGroupBody,
        ProjectGroup,
        Json,
        Json,
        &["#/components/schemas/InviteProjectGroupBody"],
        &["#/components/schemas/ProjectGroup"]
    );
    admin_operation!(
        OpAdminApiKeysCreate,
        "admin-api-keys-create",
        Post,
        "/organization/admin_api_keys",
        AdminApiKeyCreateRequest,
        AdminApiKeyCreateResponse,
        Json,
        Json,
        &[],
        &["#/components/schemas/AdminApiKeyCreateResponse"]
    );
    admin_operation!(
        OpAdminApiKeysDelete,
        "admin-api-keys-delete",
        Delete,
        "/organization/admin_api_keys/{key_id}",
        (),
        AdminApiKeyDeleteResponse,
        None,
        Json,
        &[],
        &[]
    );
    admin_operation!(
        OpAdminApiKeysGet,
        "admin-api-keys-get",
        Get,
        "/organization/admin_api_keys/{key_id}",
        (),
        AdminApiKey,
        None,
        Json,
        &[],
        &["#/components/schemas/AdminApiKey"]
    );
    admin_operation!(
        OpAdminApiKeysList,
        "admin-api-keys-list",
        Get,
        "/organization/admin_api_keys",
        (),
        ApiKeyList,
        None,
        Json,
        &[],
        &["#/components/schemas/ApiKeyList"]
    );
    admin_operation!(
        OpArchiveProject,
        "archive-project",
        Post,
        "/organization/projects/{project_id}/archive",
        (),
        Project,
        None,
        Json,
        &[],
        &["#/components/schemas/Project"]
    );
    admin_operation!(
        OpAssignGroupRole,
        "assign-group-role",
        Post,
        "/organization/groups/{group_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        GroupRoleAssignment,
        Json,
        Json,
        &["#/components/schemas/PublicAssignOrganizationGroupRoleBody"],
        &["#/components/schemas/GroupRoleAssignment"]
    );
    admin_operation!(
        OpAssignProjectGroupRole,
        "assign-project-group-role",
        Post,
        "/projects/{project_id}/groups/{group_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        GroupRoleAssignment,
        Json,
        Json,
        &["#/components/schemas/PublicAssignOrganizationGroupRoleBody"],
        &["#/components/schemas/GroupRoleAssignment"]
    );
    admin_operation!(
        OpAssignProjectUserRole,
        "assign-project-user-role",
        Post,
        "/projects/{project_id}/users/{user_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        UserRoleAssignment,
        Json,
        Json,
        &["#/components/schemas/PublicAssignOrganizationGroupRoleBody"],
        &["#/components/schemas/UserRoleAssignment"]
    );
    admin_operation!(
        OpAssignUserRole,
        "assign-user-role",
        Post,
        "/organization/users/{user_id}/roles",
        PublicAssignOrganizationGroupRoleBody,
        UserRoleAssignment,
        Json,
        Json,
        &["#/components/schemas/PublicAssignOrganizationGroupRoleBody"],
        &["#/components/schemas/UserRoleAssignment"]
    );
    admin_operation!(
        OpCreateGroup,
        "create-group",
        Post,
        "/organization/groups",
        CreateGroupBody,
        GroupResponse,
        Json,
        Json,
        &["#/components/schemas/CreateGroupBody"],
        &["#/components/schemas/GroupResponse"]
    );
    admin_operation!(
        OpCreateOrganizationSpendAlert,
        "create-organization-spend-alert",
        Post,
        "/organization/spend_alerts",
        CreateSpendAlertBody,
        OrganizationSpendAlert,
        Json,
        Json,
        &["#/components/schemas/CreateSpendAlertBody"],
        &["#/components/schemas/OrganizationSpendAlert"]
    );
    admin_operation!(
        OpCreateProject,
        "create-project",
        Post,
        "/organization/projects",
        ProjectCreateRequest,
        Project,
        Json,
        Json,
        &["#/components/schemas/ProjectCreateRequest"],
        &["#/components/schemas/Project"]
    );
    admin_operation!(
        OpCreateProjectRole,
        "create-project-role",
        Post,
        "/projects/{project_id}/roles",
        PublicCreateOrganizationRoleBody,
        Role,
        Json,
        Json,
        &["#/components/schemas/PublicCreateOrganizationRoleBody"],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpCreateProjectServiceAccount,
        "create-project-service-account",
        Post,
        "/organization/projects/{project_id}/service_accounts",
        ProjectServiceAccountCreateRequest,
        ProjectServiceAccountCreateResponse,
        Json,
        Json,
        &["#/components/schemas/ProjectServiceAccountCreateRequest"],
        &["#/components/schemas/ProjectServiceAccountCreateResponse"]
    );
    admin_operation!(
        OpCreateProjectSpendAlert,
        "create-project-spend-alert",
        Post,
        "/organization/projects/{project_id}/spend_alerts",
        CreateSpendAlertBody,
        ProjectSpendAlert,
        Json,
        Json,
        &["#/components/schemas/CreateSpendAlertBody"],
        &["#/components/schemas/ProjectSpendAlert"]
    );
    admin_operation!(
        OpCreateProjectUser,
        "create-project-user",
        Post,
        "/organization/projects/{project_id}/users",
        ProjectUserCreateRequest,
        ProjectUser,
        Json,
        Json,
        &["#/components/schemas/ProjectUserCreateRequest"],
        &["#/components/schemas/ProjectUser"]
    );
    admin_operation!(
        OpCreateRole,
        "create-role",
        Post,
        "/organization/roles",
        PublicCreateOrganizationRoleBody,
        Role,
        Json,
        Json,
        &["#/components/schemas/PublicCreateOrganizationRoleBody"],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpDeactivateOrganizationCertificates,
        "deactivateOrganizationCertificates",
        Post,
        "/organization/certificates/deactivate",
        ToggleCertificatesRequest,
        OrganizationCertificateDeactivationResponse,
        Json,
        Json,
        &["#/components/schemas/ToggleCertificatesRequest"],
        &["#/components/schemas/OrganizationCertificateDeactivationResponse"]
    );
    admin_operation!(
        OpDeactivateProjectCertificates,
        "deactivateProjectCertificates",
        Post,
        "/organization/projects/{project_id}/certificates/deactivate",
        ToggleCertificatesRequest,
        OrganizationProjectCertificateDeactivationResponse,
        Json,
        Json,
        &["#/components/schemas/ToggleCertificatesRequest"],
        &["#/components/schemas/OrganizationProjectCertificateDeactivationResponse"]
    );
    admin_operation!(
        OpDeleteGroup,
        "delete-group",
        Delete,
        "/organization/groups/{group_id}",
        (),
        GroupDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/GroupDeletedResource"]
    );
    admin_operation!(
        OpDeleteInvite,
        "delete-invite",
        Delete,
        "/organization/invites/{invite_id}",
        (),
        InviteDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/InviteDeleteResponse"]
    );
    admin_operation!(
        OpDeleteOrganizationSpendAlert,
        "delete-organization-spend-alert",
        Delete,
        "/organization/spend_alerts/{alert_id}",
        (),
        OrganizationSpendAlertDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationSpendAlertDeletedResource"]
    );
    admin_operation!(
        OpDeleteProjectApiKey,
        "delete-project-api-key",
        Delete,
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        (),
        ProjectApiKeyDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectApiKeyDeleteResponse"]
    );
    admin_operation!(
        OpDeleteProjectModelPermissions,
        "delete-project-model-permissions",
        Delete,
        "/organization/projects/{project_id}/model_permissions",
        (),
        ProjectModelPermissionsDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectModelPermissionsDeleteResponse"]
    );
    admin_operation!(
        OpDeleteProjectRole,
        "delete-project-role",
        Delete,
        "/projects/{project_id}/roles/{role_id}",
        (),
        RoleDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleDeletedResource"]
    );
    admin_operation!(
        OpDeleteProjectServiceAccount,
        "delete-project-service-account",
        Delete,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        (),
        ProjectServiceAccountDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectServiceAccountDeleteResponse"]
    );
    admin_operation!(
        OpDeleteProjectSpendAlert,
        "delete-project-spend-alert",
        Delete,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        (),
        ProjectSpendAlertDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectSpendAlertDeletedResource"]
    );
    admin_operation!(
        OpDeleteProjectUser,
        "delete-project-user",
        Delete,
        "/organization/projects/{project_id}/users/{user_id}",
        (),
        ProjectUserDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectUserDeleteResponse"]
    );
    admin_operation!(
        OpDeleteRole,
        "delete-role",
        Delete,
        "/organization/roles/{role_id}",
        (),
        RoleDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleDeletedResource"]
    );
    admin_operation!(
        OpDeleteUser,
        "delete-user",
        Delete,
        "/organization/users/{user_id}",
        (),
        UserDeleteResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UserDeleteResponse"]
    );
    admin_operation!(
        OpDeleteCertificate,
        "deleteCertificate",
        Delete,
        "/organization/certificates/{certificate_id}",
        (),
        DeleteCertificateResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/DeleteCertificateResponse"]
    );
    admin_operation!(
        OpGetCertificate,
        "getCertificate",
        Get,
        "/organization/certificates/{certificate_id}",
        (),
        Certificate,
        None,
        Json,
        &[],
        &["#/components/schemas/Certificate"]
    );
    admin_operation!(
        OpInviteUser,
        "inviteUser",
        Post,
        "/organization/invites",
        InviteRequest,
        Invite,
        Json,
        Json,
        &["#/components/schemas/InviteRequest"],
        &["#/components/schemas/Invite"]
    );
    admin_operation!(
        OpListAuditLogs,
        "list-audit-logs",
        Get,
        "/organization/audit_logs",
        (),
        ListAuditLogsResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ListAuditLogsResponse"]
    );
    admin_operation!(
        OpListGroupRoleAssignments,
        "list-group-role-assignments",
        Get,
        "/organization/groups/{group_id}/roles",
        (),
        RoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleListResource"]
    );
    admin_operation!(
        OpListGroupUsers,
        "list-group-users",
        Get,
        "/organization/groups/{group_id}/users",
        (),
        UserListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/UserListResource"]
    );
    admin_operation!(
        OpListGroups,
        "list-groups",
        Get,
        "/organization/groups",
        (),
        GroupListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/GroupListResource"]
    );
    admin_operation!(
        OpListInvites,
        "list-invites",
        Get,
        "/organization/invites",
        (),
        InviteListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/InviteListResponse"]
    );
    admin_operation!(
        OpListOrganizationSpendAlerts,
        "list-organization-spend-alerts",
        Get,
        "/organization/spend_alerts",
        (),
        OrganizationSpendAlertListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationSpendAlertListResource"]
    );
    admin_operation!(
        OpListProjectApiKeys,
        "list-project-api-keys",
        Get,
        "/organization/projects/{project_id}/api_keys",
        (),
        ProjectApiKeyListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectApiKeyListResponse"]
    );
    admin_operation!(
        OpListProjectGroupRoleAssignments,
        "list-project-group-role-assignments",
        Get,
        "/projects/{project_id}/groups/{group_id}/roles",
        (),
        RoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleListResource"]
    );
    admin_operation!(
        OpListProjectGroups,
        "list-project-groups",
        Get,
        "/organization/projects/{project_id}/groups",
        (),
        ProjectGroupListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectGroupListResource"]
    );
    admin_operation!(
        OpListProjectRateLimits,
        "list-project-rate-limits",
        Get,
        "/organization/projects/{project_id}/rate_limits",
        (),
        ProjectRateLimitListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectRateLimitListResponse"]
    );
    admin_operation!(
        OpListProjectRoles,
        "list-project-roles",
        Get,
        "/projects/{project_id}/roles",
        (),
        PublicRoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/PublicRoleListResource"]
    );
    admin_operation!(
        OpListProjectServiceAccounts,
        "list-project-service-accounts",
        Get,
        "/organization/projects/{project_id}/service_accounts",
        (),
        ProjectServiceAccountListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectServiceAccountListResponse"]
    );
    admin_operation!(
        OpListProjectSpendAlerts,
        "list-project-spend-alerts",
        Get,
        "/organization/projects/{project_id}/spend_alerts",
        (),
        ProjectSpendAlertListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectSpendAlertListResource"]
    );
    admin_operation!(
        OpListProjectUserRoleAssignments,
        "list-project-user-role-assignments",
        Get,
        "/projects/{project_id}/users/{user_id}/roles",
        (),
        RoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleListResource"]
    );
    admin_operation!(
        OpListProjectUsers,
        "list-project-users",
        Get,
        "/organization/projects/{project_id}/users",
        (),
        ProjectUserListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectUserListResponse"]
    );
    admin_operation!(
        OpListProjects,
        "list-projects",
        Get,
        "/organization/projects",
        (),
        ProjectListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectListResponse"]
    );
    admin_operation!(
        OpListRoles,
        "list-roles",
        Get,
        "/organization/roles",
        (),
        PublicRoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/PublicRoleListResource"]
    );
    admin_operation!(
        OpListUserRoleAssignments,
        "list-user-role-assignments",
        Get,
        "/organization/users/{user_id}/roles",
        (),
        RoleListResource,
        None,
        Json,
        &[],
        &["#/components/schemas/RoleListResource"]
    );
    admin_operation!(
        OpListUsers,
        "list-users",
        Get,
        "/organization/users",
        (),
        UserListResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UserListResponse"]
    );
    admin_operation!(
        OpListOrganizationCertificates,
        "listOrganizationCertificates",
        Get,
        "/organization/certificates",
        (),
        ListCertificatesResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ListCertificatesResponse"]
    );
    admin_operation!(
        OpListProjectCertificates,
        "listProjectCertificates",
        Get,
        "/organization/projects/{project_id}/certificates",
        (),
        ListProjectCertificatesResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ListProjectCertificatesResponse"]
    );
    admin_operation!(
        OpModifyProject,
        "modify-project",
        Post,
        "/organization/projects/{project_id}",
        ProjectUpdateRequest,
        Project,
        Json,
        Json,
        &["#/components/schemas/ProjectUpdateRequest"],
        &["#/components/schemas/Project"]
    );
    admin_operation!(
        OpModifyProjectUser,
        "modify-project-user",
        Post,
        "/organization/projects/{project_id}/users/{user_id}",
        ProjectUserUpdateRequest,
        ProjectUser,
        Json,
        Json,
        &["#/components/schemas/ProjectUserUpdateRequest"],
        &["#/components/schemas/ProjectUser"]
    );
    admin_operation!(
        OpModifyUser,
        "modify-user",
        Post,
        "/organization/users/{user_id}",
        UserRoleUpdateRequest,
        User,
        Json,
        Json,
        &["#/components/schemas/UserRoleUpdateRequest"],
        &["#/components/schemas/User"]
    );
    admin_operation!(
        OpModifyCertificate,
        "modifyCertificate",
        Post,
        "/organization/certificates/{certificate_id}",
        ModifyCertificateRequest,
        Certificate,
        Json,
        Json,
        &["#/components/schemas/ModifyCertificateRequest"],
        &["#/components/schemas/Certificate"]
    );
    admin_operation!(
        OpRemoveGroupUser,
        "remove-group-user",
        Delete,
        "/organization/groups/{group_id}/users/{user_id}",
        (),
        GroupUserDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/GroupUserDeletedResource"]
    );
    admin_operation!(
        OpRemoveProjectGroup,
        "remove-project-group",
        Delete,
        "/organization/projects/{project_id}/groups/{group_id}",
        (),
        ProjectGroupDeletedResource,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectGroupDeletedResource"]
    );
    admin_operation!(
        OpRetrieveGroup,
        "retrieve-group",
        Get,
        "/organization/groups/{group_id}",
        (),
        GroupResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/GroupResponse"]
    );
    admin_operation!(
        OpRetrieveGroupRole,
        "retrieve-group-role",
        Get,
        "/organization/groups/{group_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json,
        &[],
        &["#/components/schemas/AssignedRoleDetails"]
    );
    admin_operation!(
        OpRetrieveGroupUser,
        "retrieve-group-user",
        Get,
        "/organization/groups/{group_id}/users/{user_id}",
        (),
        GroupMemberUser,
        None,
        Json,
        &[],
        &["#/components/schemas/GroupMemberUser"]
    );
    admin_operation!(
        OpRetrieveInvite,
        "retrieve-invite",
        Get,
        "/organization/invites/{invite_id}",
        (),
        Invite,
        None,
        Json,
        &[],
        &["#/components/schemas/Invite"]
    );
    admin_operation!(
        OpRetrieveOrganizationDataRetention,
        "retrieve-organization-data-retention",
        Get,
        "/organization/data_retention",
        (),
        OrganizationDataRetention,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationDataRetention"]
    );
    admin_operation!(
        OpRetrieveOrganizationSpendAlert,
        "retrieve-organization-spend-alert",
        Get,
        "/organization/spend_alerts/{alert_id}",
        (),
        OrganizationSpendAlert,
        None,
        Json,
        &[],
        &["#/components/schemas/OrganizationSpendAlert"]
    );
    admin_operation!(
        OpRetrieveProject,
        "retrieve-project",
        Get,
        "/organization/projects/{project_id}",
        (),
        Project,
        None,
        Json,
        &[],
        &["#/components/schemas/Project"]
    );
    admin_operation!(
        OpRetrieveProjectApiKey,
        "retrieve-project-api-key",
        Get,
        "/organization/projects/{project_id}/api_keys/{api_key_id}",
        (),
        ProjectApiKey,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectApiKey"]
    );
    admin_operation!(
        OpRetrieveProjectDataRetention,
        "retrieve-project-data-retention",
        Get,
        "/organization/projects/{project_id}/data_retention",
        (),
        ProjectDataRetention,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectDataRetention"]
    );
    admin_operation!(
        OpRetrieveProjectGroup,
        "retrieve-project-group",
        Get,
        "/organization/projects/{project_id}/groups/{group_id}",
        (),
        ProjectGroup,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectGroup"]
    );
    admin_operation!(
        OpRetrieveProjectGroupRole,
        "retrieve-project-group-role",
        Get,
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json,
        &[],
        &["#/components/schemas/AssignedRoleDetails"]
    );
    admin_operation!(
        OpRetrieveProjectHostedToolPermissions,
        "retrieve-project-hosted-tool-permissions",
        Get,
        "/organization/projects/{project_id}/hosted_tool_permissions",
        (),
        ProjectHostedToolPermissions,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectHostedToolPermissions"]
    );
    admin_operation!(
        OpRetrieveProjectModelPermissions,
        "retrieve-project-model-permissions",
        Get,
        "/organization/projects/{project_id}/model_permissions",
        (),
        ProjectModelPermissions,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectModelPermissions"]
    );
    admin_operation!(
        OpRetrieveProjectRole,
        "retrieve-project-role",
        Get,
        "/projects/{project_id}/roles/{role_id}",
        (),
        Role,
        None,
        Json,
        &[],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpRetrieveProjectServiceAccount,
        "retrieve-project-service-account",
        Get,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        (),
        ProjectServiceAccount,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectServiceAccount"]
    );
    admin_operation!(
        OpRetrieveProjectSpendAlert,
        "retrieve-project-spend-alert",
        Get,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        (),
        ProjectSpendAlert,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectSpendAlert"]
    );
    admin_operation!(
        OpRetrieveProjectUser,
        "retrieve-project-user",
        Get,
        "/organization/projects/{project_id}/users/{user_id}",
        (),
        ProjectUser,
        None,
        Json,
        &[],
        &["#/components/schemas/ProjectUser"]
    );
    admin_operation!(
        OpRetrieveProjectUserRole,
        "retrieve-project-user-role",
        Get,
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json,
        &[],
        &["#/components/schemas/AssignedRoleDetails"]
    );
    admin_operation!(
        OpRetrieveRole,
        "retrieve-role",
        Get,
        "/organization/roles/{role_id}",
        (),
        Role,
        None,
        Json,
        &[],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpRetrieveUser,
        "retrieve-user",
        Get,
        "/organization/users/{user_id}",
        (),
        User,
        None,
        Json,
        &[],
        &["#/components/schemas/User"]
    );
    admin_operation!(
        OpRetrieveUserRole,
        "retrieve-user-role",
        Get,
        "/organization/users/{user_id}/roles/{role_id}",
        (),
        AssignedRoleDetails,
        None,
        Json,
        &[],
        &["#/components/schemas/AssignedRoleDetails"]
    );
    admin_operation!(
        OpUnassignGroupRole,
        "unassign-group-role",
        Delete,
        "/organization/groups/{group_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json,
        &[],
        &["#/components/schemas/DeletedRoleAssignmentResource"]
    );
    admin_operation!(
        OpUnassignProjectGroupRole,
        "unassign-project-group-role",
        Delete,
        "/projects/{project_id}/groups/{group_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json,
        &[],
        &["#/components/schemas/DeletedRoleAssignmentResource"]
    );
    admin_operation!(
        OpUnassignProjectUserRole,
        "unassign-project-user-role",
        Delete,
        "/projects/{project_id}/users/{user_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json,
        &[],
        &["#/components/schemas/DeletedRoleAssignmentResource"]
    );
    admin_operation!(
        OpUnassignUserRole,
        "unassign-user-role",
        Delete,
        "/organization/users/{user_id}/roles/{role_id}",
        (),
        DeletedRoleAssignmentResource,
        None,
        Json,
        &[],
        &["#/components/schemas/DeletedRoleAssignmentResource"]
    );
    admin_operation!(
        OpUpdateGroup,
        "update-group",
        Post,
        "/organization/groups/{group_id}",
        UpdateGroupBody,
        GroupResourceWithSuccess,
        Json,
        Json,
        &["#/components/schemas/UpdateGroupBody"],
        &["#/components/schemas/GroupResourceWithSuccess"]
    );
    admin_operation!(
        OpUpdateOrganizationDataRetention,
        "update-organization-data-retention",
        Post,
        "/organization/data_retention",
        UpdateOrganizationDataRetentionBody,
        OrganizationDataRetention,
        Json,
        Json,
        &["#/components/schemas/UpdateOrganizationDataRetentionBody"],
        &["#/components/schemas/OrganizationDataRetention"]
    );
    admin_operation!(
        OpUpdateOrganizationSpendAlert,
        "update-organization-spend-alert",
        Post,
        "/organization/spend_alerts/{alert_id}",
        CreateSpendAlertBody,
        OrganizationSpendAlert,
        Json,
        Json,
        &["#/components/schemas/CreateSpendAlertBody"],
        &["#/components/schemas/OrganizationSpendAlert"]
    );
    admin_operation!(
        OpUpdateProjectDataRetention,
        "update-project-data-retention",
        Post,
        "/organization/projects/{project_id}/data_retention",
        UpdateProjectDataRetentionBody,
        ProjectDataRetention,
        Json,
        Json,
        &["#/components/schemas/UpdateProjectDataRetentionBody"],
        &["#/components/schemas/ProjectDataRetention"]
    );
    admin_operation!(
        OpUpdateProjectHostedToolPermissions,
        "update-project-hosted-tool-permissions",
        Post,
        "/organization/projects/{project_id}/hosted_tool_permissions",
        ProjectHostedToolPermissionsUpdateRequest,
        ProjectHostedToolPermissions,
        Json,
        Json,
        &["#/components/schemas/ProjectHostedToolPermissionsUpdateRequest"],
        &["#/components/schemas/ProjectHostedToolPermissions"]
    );
    admin_operation!(
        OpUpdateProjectModelPermissions,
        "update-project-model-permissions",
        Post,
        "/organization/projects/{project_id}/model_permissions",
        ProjectModelPermissionsUpdateRequest,
        ProjectModelPermissions,
        Json,
        Json,
        &["#/components/schemas/ProjectModelPermissionsUpdateRequest"],
        &["#/components/schemas/ProjectModelPermissions"]
    );
    admin_operation!(
        OpUpdateProjectRateLimits,
        "update-project-rate-limits",
        Post,
        "/organization/projects/{project_id}/rate_limits/{rate_limit_id}",
        ProjectRateLimitUpdateRequest,
        ProjectRateLimit,
        Json,
        Json,
        &["#/components/schemas/ProjectRateLimitUpdateRequest"],
        &["#/components/schemas/ProjectRateLimit"]
    );
    admin_operation!(
        OpUpdateProjectRole,
        "update-project-role",
        Post,
        "/projects/{project_id}/roles/{role_id}",
        PublicUpdateOrganizationRoleBody,
        Role,
        Json,
        Json,
        &["#/components/schemas/PublicUpdateOrganizationRoleBody"],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpUpdateProjectServiceAccount,
        "update-project-service-account",
        Post,
        "/organization/projects/{project_id}/service_accounts/{service_account_id}",
        UpdateProjectServiceAccountBody,
        ProjectServiceAccount,
        Json,
        Json,
        &["#/components/schemas/UpdateProjectServiceAccountBody"],
        &["#/components/schemas/ProjectServiceAccount"]
    );
    admin_operation!(
        OpUpdateProjectSpendAlert,
        "update-project-spend-alert",
        Post,
        "/organization/projects/{project_id}/spend_alerts/{alert_id}",
        CreateSpendAlertBody,
        ProjectSpendAlert,
        Json,
        Json,
        &["#/components/schemas/CreateSpendAlertBody"],
        &["#/components/schemas/ProjectSpendAlert"]
    );
    admin_operation!(
        OpUpdateRole,
        "update-role",
        Post,
        "/organization/roles/{role_id}",
        PublicUpdateOrganizationRoleBody,
        Role,
        Json,
        Json,
        &["#/components/schemas/PublicUpdateOrganizationRoleBody"],
        &["#/components/schemas/Role"]
    );
    admin_operation!(
        OpUploadCertificate,
        "uploadCertificate",
        Post,
        "/organization/certificates",
        UploadCertificateRequest,
        Certificate,
        Json,
        Json,
        &["#/components/schemas/UploadCertificateRequest"],
        &["#/components/schemas/Certificate"]
    );
    admin_operation!(
        OpUsageAudioSpeeches,
        "usage-audio-speeches",
        Get,
        "/organization/usage/audio_speeches",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageAudioTranscriptions,
        "usage-audio-transcriptions",
        Get,
        "/organization/usage/audio_transcriptions",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageCodeInterpreterSessions,
        "usage-code-interpreter-sessions",
        Get,
        "/organization/usage/code_interpreter_sessions",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageCompletions,
        "usage-completions",
        Get,
        "/organization/usage/completions",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageCosts,
        "usage-costs",
        Get,
        "/organization/costs",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageEmbeddings,
        "usage-embeddings",
        Get,
        "/organization/usage/embeddings",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageFileSearchCalls,
        "usage-file-search-calls",
        Get,
        "/organization/usage/file_search_calls",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageImages,
        "usage-images",
        Get,
        "/organization/usage/images",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageModerations,
        "usage-moderations",
        Get,
        "/organization/usage/moderations",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageVectorStores,
        "usage-vector-stores",
        Get,
        "/organization/usage/vector_stores",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpUsageWebSearchCalls,
        "usage-web-search-calls",
        Get,
        "/organization/usage/web_search_calls",
        (),
        UsageResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/UsageResponse"]
    );
    admin_operation!(
        OpListFineTuningCheckpointPermissions,
        "listFineTuningCheckpointPermissions",
        Get,
        "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions",
        (),
        ListFineTuningCheckpointPermissionResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/ListFineTuningCheckpointPermissionResponse"]
    );
    admin_operation!(
        OpCreateFineTuningCheckpointPermission,
        "createFineTuningCheckpointPermission",
        Post,
        "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions",
        CreateFineTuningCheckpointPermissionRequest,
        ListFineTuningCheckpointPermissionResponse,
        Json,
        Json,
        &["#/components/schemas/CreateFineTuningCheckpointPermissionRequest"],
        &["#/components/schemas/ListFineTuningCheckpointPermissionResponse"]
    );
    admin_operation!(
        OpDeleteFineTuningCheckpointPermission,
        "deleteFineTuningCheckpointPermission",
        Delete,
        "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions/{permission_id}",
        (),
        DeleteFineTuningCheckpointPermissionResponse,
        None,
        Json,
        &[],
        &["#/components/schemas/DeleteFineTuningCheckpointPermissionResponse"]
    );
}

/// Effective 119-entry client binding manifest generated from sealed markers.
pub const ADMIN_CLIENT_OPERATION_MANIFEST: &[AdminClientOperationContract] = &[
    operations::OpCreateanAPIkeyforaserviceaccount::CONTRACT,
    operations::OpDeleteorganizationspendlimit::CONTRACT,
    operations::OpDeleteprojectspendlimit::CONTRACT,
    operations::OpGetorganizationspendlimit::CONTRACT,
    operations::OpGetprojectspendlimit::CONTRACT,
    operations::OpUpdateorganizationspendlimit::CONTRACT,
    operations::OpUpdateprojectspendlimit::CONTRACT,
    operations::OpActivateOrganizationCertificates::CONTRACT,
    operations::OpActivateProjectCertificates::CONTRACT,
    operations::OpAddGroupUser::CONTRACT,
    operations::OpAddProjectGroup::CONTRACT,
    operations::OpAdminApiKeysCreate::CONTRACT,
    operations::OpAdminApiKeysDelete::CONTRACT,
    operations::OpAdminApiKeysGet::CONTRACT,
    operations::OpAdminApiKeysList::CONTRACT,
    operations::OpArchiveProject::CONTRACT,
    operations::OpAssignGroupRole::CONTRACT,
    operations::OpAssignProjectGroupRole::CONTRACT,
    operations::OpAssignProjectUserRole::CONTRACT,
    operations::OpAssignUserRole::CONTRACT,
    operations::OpCreateGroup::CONTRACT,
    operations::OpCreateOrganizationSpendAlert::CONTRACT,
    operations::OpCreateProject::CONTRACT,
    operations::OpCreateProjectRole::CONTRACT,
    operations::OpCreateProjectServiceAccount::CONTRACT,
    operations::OpCreateProjectSpendAlert::CONTRACT,
    operations::OpCreateProjectUser::CONTRACT,
    operations::OpCreateRole::CONTRACT,
    operations::OpDeactivateOrganizationCertificates::CONTRACT,
    operations::OpDeactivateProjectCertificates::CONTRACT,
    operations::OpDeleteGroup::CONTRACT,
    operations::OpDeleteInvite::CONTRACT,
    operations::OpDeleteOrganizationSpendAlert::CONTRACT,
    operations::OpDeleteProjectApiKey::CONTRACT,
    operations::OpDeleteProjectModelPermissions::CONTRACT,
    operations::OpDeleteProjectRole::CONTRACT,
    operations::OpDeleteProjectServiceAccount::CONTRACT,
    operations::OpDeleteProjectSpendAlert::CONTRACT,
    operations::OpDeleteProjectUser::CONTRACT,
    operations::OpDeleteRole::CONTRACT,
    operations::OpDeleteUser::CONTRACT,
    operations::OpDeleteCertificate::CONTRACT,
    operations::OpGetCertificate::CONTRACT,
    operations::OpInviteUser::CONTRACT,
    operations::OpListAuditLogs::CONTRACT,
    operations::OpListGroupRoleAssignments::CONTRACT,
    operations::OpListGroupUsers::CONTRACT,
    operations::OpListGroups::CONTRACT,
    operations::OpListInvites::CONTRACT,
    operations::OpListOrganizationSpendAlerts::CONTRACT,
    operations::OpListProjectApiKeys::CONTRACT,
    operations::OpListProjectGroupRoleAssignments::CONTRACT,
    operations::OpListProjectGroups::CONTRACT,
    operations::OpListProjectRateLimits::CONTRACT,
    operations::OpListProjectRoles::CONTRACT,
    operations::OpListProjectServiceAccounts::CONTRACT,
    operations::OpListProjectSpendAlerts::CONTRACT,
    operations::OpListProjectUserRoleAssignments::CONTRACT,
    operations::OpListProjectUsers::CONTRACT,
    operations::OpListProjects::CONTRACT,
    operations::OpListRoles::CONTRACT,
    operations::OpListUserRoleAssignments::CONTRACT,
    operations::OpListUsers::CONTRACT,
    operations::OpListOrganizationCertificates::CONTRACT,
    operations::OpListProjectCertificates::CONTRACT,
    operations::OpModifyProject::CONTRACT,
    operations::OpModifyProjectUser::CONTRACT,
    operations::OpModifyUser::CONTRACT,
    operations::OpModifyCertificate::CONTRACT,
    operations::OpRemoveGroupUser::CONTRACT,
    operations::OpRemoveProjectGroup::CONTRACT,
    operations::OpRetrieveGroup::CONTRACT,
    operations::OpRetrieveGroupRole::CONTRACT,
    operations::OpRetrieveGroupUser::CONTRACT,
    operations::OpRetrieveInvite::CONTRACT,
    operations::OpRetrieveOrganizationDataRetention::CONTRACT,
    operations::OpRetrieveOrganizationSpendAlert::CONTRACT,
    operations::OpRetrieveProject::CONTRACT,
    operations::OpRetrieveProjectApiKey::CONTRACT,
    operations::OpRetrieveProjectDataRetention::CONTRACT,
    operations::OpRetrieveProjectGroup::CONTRACT,
    operations::OpRetrieveProjectGroupRole::CONTRACT,
    operations::OpRetrieveProjectHostedToolPermissions::CONTRACT,
    operations::OpRetrieveProjectModelPermissions::CONTRACT,
    operations::OpRetrieveProjectRole::CONTRACT,
    operations::OpRetrieveProjectServiceAccount::CONTRACT,
    operations::OpRetrieveProjectSpendAlert::CONTRACT,
    operations::OpRetrieveProjectUser::CONTRACT,
    operations::OpRetrieveProjectUserRole::CONTRACT,
    operations::OpRetrieveRole::CONTRACT,
    operations::OpRetrieveUser::CONTRACT,
    operations::OpRetrieveUserRole::CONTRACT,
    operations::OpUnassignGroupRole::CONTRACT,
    operations::OpUnassignProjectGroupRole::CONTRACT,
    operations::OpUnassignProjectUserRole::CONTRACT,
    operations::OpUnassignUserRole::CONTRACT,
    operations::OpUpdateGroup::CONTRACT,
    operations::OpUpdateOrganizationDataRetention::CONTRACT,
    operations::OpUpdateOrganizationSpendAlert::CONTRACT,
    operations::OpUpdateProjectDataRetention::CONTRACT,
    operations::OpUpdateProjectHostedToolPermissions::CONTRACT,
    operations::OpUpdateProjectModelPermissions::CONTRACT,
    operations::OpUpdateProjectRateLimits::CONTRACT,
    operations::OpUpdateProjectRole::CONTRACT,
    operations::OpUpdateProjectServiceAccount::CONTRACT,
    operations::OpUpdateProjectSpendAlert::CONTRACT,
    operations::OpUpdateRole::CONTRACT,
    operations::OpUploadCertificate::CONTRACT,
    operations::OpUsageAudioSpeeches::CONTRACT,
    operations::OpUsageAudioTranscriptions::CONTRACT,
    operations::OpUsageCodeInterpreterSessions::CONTRACT,
    operations::OpUsageCompletions::CONTRACT,
    operations::OpUsageCosts::CONTRACT,
    operations::OpUsageEmbeddings::CONTRACT,
    operations::OpUsageFileSearchCalls::CONTRACT,
    operations::OpUsageImages::CONTRACT,
    operations::OpUsageModerations::CONTRACT,
    operations::OpUsageVectorStores::CONTRACT,
    operations::OpUsageWebSearchCalls::CONTRACT,
];

/// Effective client binding manifest for the three Administration-only
/// fine-tuning checkpoint-permission operations.
pub const ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST: &[AdminClientOperationContract] = &[
    operations::OpListFineTuningCheckpointPermissions::CONTRACT,
    operations::OpCreateFineTuningCheckpointPermission::CONTRACT,
    operations::OpDeleteFineTuningCheckpointPermission::CONTRACT,
];

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
    retry_policy: RetryPolicy,
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

    /// Organization spend alerts, with project-scoped sub-resources.
    #[must_use]
    pub fn spend_alerts(&self) -> AdminSpendAlerts {
        AdminSpendAlerts(self.clone())
    }

    /// Organization spend limit, with project-scoped sub-resources.
    #[must_use]
    pub fn spend_limits(&self) -> AdminSpendLimits {
        AdminSpendLimits(self.clone())
    }

    /// Administration-only fine-tuning checkpoint permissions.
    #[must_use]
    pub fn checkpoint_permissions(&self) -> AdminCheckpointPermissions {
        AdminCheckpointPermissions(self.clone())
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
    retry_policy: RetryPolicy,
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
            retry_policy: RetryPolicy::openai_compatible(),
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

    /// Sets the per-attempt connection budget (TCP plus TLS handshake).
    ///
    /// The default is 10s, matching the platform transport's
    /// [`crate::ClientBuilder::connect_timeout`] middle ground between the two
    /// official baselines (openai-python 5s, openai-node transport default 10s;
    /// decisions D0163/D0199). The budget is independent of
    /// [`AdminClientBuilder::request_timeout`] and applies to every dial,
    /// including retried attempts. Must be non-zero; zero values are rejected
    /// by [`AdminClientBuilder::build`].
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the total budget for one logical Administration request.
    ///
    /// This budget covers connection, request write, server processing, body
    /// read, and *all* retries with their backoff delays from start to finish:
    /// every attempt is issued with the remaining slice of the same deadline,
    /// and a retry that cannot fit inside the remainder fails fast with
    /// `Error::DeadlineExceeded` instead of extending the operation (matching
    /// the platform transport's `overall_timeout` semantics, D0199). The
    /// default is 600s, identical to the platform default. Must be non-zero;
    /// zero values are rejected by [`AdminClientBuilder::build`].
    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the cap on buffered success-response bodies, in bytes.
    ///
    /// The default is 16 MiB (matching the platform transport). Administration
    /// responses are always JSON, so this bounds the largest decoded envelope
    /// (a paginated `list` or usage-bucket page); bodies that exceed it fail
    /// with a transport error rather than growing unbounded. Must be non-zero;
    /// zero values are rejected by [`AdminClientBuilder::build`].
    #[must_use]
    pub const fn max_json_body_bytes(mut self, limit: usize) -> Self {
        self.max_json_body_bytes = limit;
        self
    }

    /// Sets the cap on buffered error-response bodies, in bytes.
    ///
    /// The default is 64 KiB. Unlike the success cap, an oversized error body
    /// is *truncated and flagged* on the resulting [`crate::Error`], not fatal,
    /// so the typed envelope and request id survive for diagnostics (D0176).
    /// Must be non-zero; zero values are rejected by
    /// [`AdminClientBuilder::build`].
    #[must_use]
    pub const fn max_error_body_bytes(mut self, limit: usize) -> Self {
        self.max_error_body_bytes = limit;
        self
    }

    /// Selects one of the TLS backends compiled into this crate.
    ///
    /// The default is the platform default backend (the first of
    /// rustls/native-TLS enabled by the crate's feature set). Selecting a
    /// backend that was not compiled in leaves the client without TLS, which
    /// [`AdminClientBuilder::build`] rejects for the default HTTPS base URL
    /// ("HTTPS requires a compiled TLS backend").
    #[must_use]
    pub const fn tls_backend(mut self, backend: TlsBackend) -> Self {
        self.tls_backend = Some(backend);
        self
    }

    /// Replaces the automatic retry policy.
    ///
    /// The Administration transport derives a retry class per operation:
    /// `GET`/`DELETE` are read-only and always retryable (`Safe`), while `POST`
    /// mutations retry only when the policy enables `retry_replayable_mutations`
    /// (`Replayable`, the [`RetryPolicy::openai_compatible`] default), matching
    /// the platform transport's semantics.
    ///
    /// Interaction with one-shot secrets (4-26): the minting endpoints —
    /// [`AdminApiKeys::create`] (organization admin keys), project API-key
    /// creation, and project service-account creation — each return their
    /// secret `value` exactly once. Under a policy that replays `POST`s, a
    /// timeout after the server already minted the key can replay the request
    /// and mint a second, unobserved credential. Callers that must avoid
    /// orphaned secrets can pass [`RetryPolicy::conservative`] (only read-only
    /// retries) or [`RetryPolicy::disabled`].
    #[must_use]
    pub const fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
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
            // Proxy stance (5-25): aligned with openai-node, this channel never
            // reads HTTP(S)_PROXY/ALL_PROXY-style environment configuration, so
            // an administrator credential cannot be routed through an invisible
            // on-host hop the caller never opted into. The Administration
            // channel deliberately exposes no proxy knob and cannot borrow the
            // platform client's proxy surface either — it is not constructible
            // from or convertible into a platform [`crate::Client`] — so
            // proxied egress for Administration traffic is simply unavailable
            // here (fail-closed by design).
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
                retry_policy: self.retry_policy,
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
            .field("retry_policy", &self.retry_policy)
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
    #[tracing::instrument(
        level = "debug",
        name = "openai.http_request",
        skip_all,
        fields(
            operation.id = O::ID,
            http.request.method = %O::METHOD,
            http.route = O::ROUTE,
            http.response.status_code = tracing::field::Empty,
            openai.request_id = tracing::field::Empty,
            retry.count = tracing::field::Empty,
        )
    )]
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

        let encoded_body = request
            .body
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(Error::Encode)?;
        let retry_class = admin_retry_class(&O::METHOD);
        let policy = self.inner.retry_policy;
        let started = Instant::now();
        let mut retries = 0;

        let response = loop {
            // The per-request timeout doubles as the whole-operation budget,
            // exactly like the platform transport's `overall_timeout`, so a
            // retry never extends past the configured deadline.
            let remaining = self
                .inner
                .request_timeout
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    trace::emit_deadline_exceeded();
                    trace::record_retry_count(retries);
                    Error::DeadlineExceeded
                })?;
            let mut builder = self
                .inner
                .http
                .request(O::METHOD.clone(), url.clone())
                .timeout(remaining)
                .header(header::AUTHORIZATION, self.inner.authorization.clone())
                .header(header::ACCEPT, "application/json");
            if let Some(encoded) = &encoded_body {
                builder = builder
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(encoded.clone());
            }
            let response = match builder.send().await {
                Ok(response) => response,
                // Connection failures and timeouts are retryable for any
                // retryable operation class, before a status is known.
                Err(error)
                    if retryable_operation(retry_class, policy)
                        && retries < policy.max_retries
                        && (error.is_connect() || error.is_timeout()) =>
                {
                    let delay = local_retry_delay(retries);
                    if !can_wait(started, delay, self.inner.request_timeout) {
                        trace::record_retry_count(retries);
                        return Err(Error::from_reqwest(error));
                    }
                    retries += 1;
                    trace::emit_retry(retries, delay, trace::RetryReason::from_reqwest(&error));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(error) => {
                    trace::record_retry_count(retries);
                    return Err(Error::from_reqwest(error));
                }
            };

            if O::SUCCESS_STATUSES.contains(&response.status()) {
                trace::record_http_outcome(retries, &response);
                break response;
            }

            if retryable_operation(retry_class, policy)
                && retries < policy.max_retries
                && should_retry_response(&response)
            {
                let delay = match server_retry_delay(response.headers(), policy.max_server_delay) {
                    ServerDelay::Valid(delay) => delay,
                    // A missing, non-positive, or over-bound server delay all
                    // fall back to local exponential backoff; the retry budget
                    // above still caps the total number of attempts.
                    ServerDelay::TooLong | ServerDelay::Absent => local_retry_delay(retries),
                };
                if can_wait(started, delay, self.inner.request_timeout) {
                    retries += 1;
                    trace::emit_retry(retries, delay, trace::RetryReason::HttpStatus);
                    drop(response);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
            trace::record_http_outcome(retries, &response);
            return Err(self.error_from_response(response).await);
        };
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        if O::RESPONSE_MODE == AdminResponseMode::Json {
            let actual = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok());
            let media_type = actual
                .and_then(|value| value.split(';').next())
                .map(str::trim);
            if !O::RESPONSE_CONTENT_TYPES
                .iter()
                .any(|expected| Some(*expected) == media_type)
            {
                return Err(Error::UnexpectedContentType {
                    expected: "application/json",
                    actual: actual.map(Box::<str>::from),
                    status: meta.status(),
                    request_id: meta.request_id().map(Box::<str>::from),
                });
            }
        }
        let body = read_limited(response, self.inner.max_json_body_bytes, &meta).await?;
        let decoded =
            serde_json::from_slice::<O::Response>(&body).map_err(|source| Error::Decode {
                source,
                path: None,
                meta_status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
                body: BodyPreview::from_bytes(
                    &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                    body.len() > DECODE_PREVIEW_BYTES,
                ),
            })?;
        Ok(ApiResponse::new(decoded, meta))
    }

    async fn error_from_response(&self, response: reqwest::Response) -> Error {
        // Same-source semantics as `transport.rs::error_from_response` (4-25,
        // D0176): an oversized error body is truncated and flagged instead of
        // failing, so the typed envelope and request id survive; an
        // interrupted read surfaces as `Error::ResponseBody` carrying the
        // status and request id rather than a bare transport error.
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        match read_up_to(response, self.inner.max_error_body_bytes).await {
            Ok((body, truncated)) => ApiError::from_body(meta, &body, truncated).into(),
            Err(error) => Error::from_response_body(error, &meta),
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
    if !(route.starts_with("/organization/")
        || route.starts_with("/projects/")
        || route.starts_with("/fine_tuning/checkpoints/"))
    {
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

/// Appends one serialized query value as `name=value` pairs.
///
/// Mirrors the official client-level query serializers rather than
/// openai-python's `_qs.py::_stringify_item` defaults: the D0145 drop rule
/// (`null` and the empty string serialize to nothing, so the key is
/// omitted entirely rather than sent as `key=`; other falsy scalars like
/// `0`/`false` still encode) comes from `_qs.py`, while arrays take the
/// bracketed spelling `name[]` from openai-node's client-level
/// `stringifyQuery` (`qs.stringify(query, { arrayFormat: 'brackets' })`)
/// and from the pinned OpenAPI's own spelling of the five audit-log
/// filters (`actor_emails[]`/`actor_ids[]`/`event_types[]`/
/// `project_ids[]`/`resource_ids[]`; the pin spells the remaining
/// Administration/Usage array filters — usage `project_ids`/`sources`/
/// `sizes`/`vector_store_ids`/`context_levels`, users `emails`,
/// certificates `include` — without the suffix, and openai-python's
/// client-level `Querystring()` still repeats those plain keys, so the
/// two official SDKs disagree; this channel follows node and the audit
/// spelling, uniformly bracketed). Nested object leaves keep the
/// `name[child]` form (`effective_at[gt]`). Arrays recurse through the
/// same leaf rule, so `null`/`""` items inside an array are dropped just
/// like top-level fields.
fn append_query_value(
    pairs: &mut Vec<(String, String)>,
    name: &str,
    value: Value,
) -> Result<(), Error> {
    match value {
        Value::Null => {}
        Value::Bool(value) => pairs.push((name.to_owned(), value.to_string())),
        Value::Number(value) => pairs.push((name.to_owned(), value.to_string())),
        Value::String(value) => {
            if !value.is_empty() {
                pairs.push((name.to_owned(), value));
            }
        }
        Value::Array(values) => {
            let bracketed = format!("{name}[]");
            for value in values {
                append_query_value(pairs, &bracketed, value)?;
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

/// Reads the success body, failing with `BodyTooLarge` past `limit`.
///
/// Same-source twin of `transport.rs::read_success` (4-25, D0176): a
/// declared or streamed body longer than `limit` is a hard failure, and a
/// read interruption surfaces as `Error::ResponseBody` (preserving status and
/// request id) instead of a bare transport error.
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
    let (body, truncated) = read_up_to(response, limit)
        .await
        .map_err(|error| Error::from_response_body(error, meta))?;
    if truncated {
        Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        })
    } else {
        Ok(body)
    }
}

/// Reads up to `limit` bytes of a body, reporting whether the wire body was
/// longer.
///
/// Verbatim twin of `transport.rs::read_up_to` (4-25, D0176): the helpers are
/// private there, so this channel duplicates the body; the two copies must
/// stay behaviorally identical. Truncation is a *reported* outcome rather
/// than an error because the error-body channel keeps decoding the truncated
/// prefix into an `ApiError`.
async fn read_up_to(
    response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

/// Retry classification for sealed Administration operations.
///
/// `GET`/`DELETE` are read-only and classified `Safe`; every `POST` mutation
/// is `Replayable` (its bodies are fully buffered, so a retry resends the
/// identical bytes) and only retries when the policy opts in.
fn admin_retry_class(method: &Method) -> RetryClass {
    match *method {
        Method::GET | Method::DELETE => RetryClass::Safe,
        _ => RetryClass::Replayable,
    }
}

// The retry helpers below are minimal copies of the private helpers in
// `transport.rs` (`retryable_operation`, `server_retry_delay`,
// `bounded_delay`, `local_retry_delay`, `can_wait`). They are private there, so
// this channel duplicates them verbatim; the two copies must stay behaviorally
// identical — the delay semantics are pinned by decision D0131 and the
// response gating by the platform transport. `should_retry_response` is the
// one exception: it is shared `pub(crate)` from `transport.rs` (8-10), so the
// Administration channel classifies responses through the identical
// `x-should-retry` / status truth table.

fn retryable_operation(class: RetryClass, policy: RetryPolicy) -> bool {
    match class {
        RetryClass::Safe => true,
        RetryClass::Replayable => policy.retry_replayable_mutations,
        #[cfg(any(feature = "realtime", feature = "legacy-realtime"))]
        RetryClass::Never => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerDelay {
    Absent,
    Valid(Duration),
    TooLong,
}

fn server_retry_delay(headers: &http::HeaderMap, maximum: Duration) -> ServerDelay {
    if let Some(value) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        && let Ok(milliseconds) = value.parse::<f64>()
    {
        // A *parseable* `retry-after-ms` decides the delay on its own, exactly
        // like openai-python's `_parse_retry_after_header` and the sibling
        // copies (`transport.rs::server_retry_delay`,
        // `multipart.rs::retry_delay`): a positive, in-bound value wins, while
        // zero, negative, non-finite (`nan`/`inf`), and over-bound values all
        // map to local exponential backoff without ever consulting
        // `Retry-After`, so a stale coarse header cannot override the
        // millisecond header the server actually emitted. Only an unparseable
        // value falls through to `Retry-After`. The zero/negative guards live
        // inside `bounded_delay`.
        return bounded_delay(milliseconds / 1000.0, maximum);
    }

    let Some(value) = headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
    else {
        return ServerDelay::Absent;
    };
    if let Ok(seconds) = value.parse::<f64>()
        && seconds.is_finite()
        && seconds >= 0.0
    {
        return bounded_delay(seconds, maximum);
    }
    match httpdate::parse_http_date(value) {
        Ok(time) => {
            let delay = time
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO);
            if delay.is_zero() {
                // A date already in the past carries a non-positive delay, so
                // it falls back to local exponential backoff like the numeric
                // forms above.
                ServerDelay::Absent
            } else if delay <= maximum {
                ServerDelay::Valid(delay)
            } else {
                ServerDelay::TooLong
            }
        }
        Err(_) => ServerDelay::Absent,
    }
}

fn bounded_delay(seconds: f64, maximum: Duration) -> ServerDelay {
    if seconds <= 0.0 {
        // Only strictly positive delays are honored, matching openai-python's
        // `0 < retry_after` gate; zero or negative values fall back to local
        // exponential backoff rather than triggering an immediate retry.
        ServerDelay::Absent
    } else if seconds > maximum.as_secs_f64() {
        ServerDelay::TooLong
    } else {
        match Duration::try_from_secs_f64(seconds) {
            Ok(delay) => ServerDelay::Valid(delay),
            // The only in-bound value that fails to convert is `nan`, which
            // carries no usable delay and lands on the same local-backoff
            // fallback as the over-bound branch above.
            Err(_) => ServerDelay::TooLong,
        }
    }
}

fn local_retry_delay(retries: u32) -> Duration {
    let exponent = retries.min(4) as i32;
    let base_seconds = (0.5_f64 * 2_f64.powi(exponent)).min(8.0);
    let fraction = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => f64::from(duration.subsec_nanos()) / 1_000_000_000.0,
        Err(_) => 0.5,
    };
    Duration::from_secs_f64(base_seconds * (0.75 + fraction * 0.25))
}

fn can_wait(started: Instant, delay: Duration, overall_timeout: Duration) -> bool {
    started
        .elapsed()
        .checked_add(delay)
        .is_some_and(|elapsed| elapsed < overall_timeout)
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

    /// Creates one admin API key, returning its unredacted `value` exactly
    /// once.
    ///
    /// Automatic-retry interaction (4-26): this `POST` is classified
    /// `Replayable`, so with the default [`RetryPolicy::openai_compatible`]
    /// a timeout after the server already created the key can replay the
    /// request and mint a second, orphaned key whose `value` this call never
    /// observes. The default matches openai-python, which retries every
    /// request. To guarantee a single mint attempt, build the client with
    /// [`RetryPolicy::conservative`] (only read-only retries) or
    /// [`RetryPolicy::disabled`].
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

    pub async fn delete(
        &self,
        key_id: &str,
    ) -> Result<ApiResponse<AdminApiKeyDeleteResponse>, Error> {
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
        self.retrieve_with(certificate_id, &CertificateGetParams::default())
            .await
    }

    /// Retrieve a certificate, optionally including PEM `content`.
    pub async fn retrieve_with(
        &self,
        certificate_id: &str,
        params: &CertificateGetParams,
    ) -> Result<ApiResponse<Certificate>, Error> {
        self.0
            .request::<operations::OpGetCertificate>()
            .path_parameter(certificate_id)?
            .query(params)?
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

    /// Update a project's data-retention setting via
    /// `POST /organization/projects/{project_id}/data_retention`.
    pub async fn update_project(
        &self,
        project_id: &str,
        request: UpdateProjectDataRetentionBody,
    ) -> Result<ApiResponse<ProjectDataRetention>, Error> {
        self.0
            .request::<operations::OpUpdateProjectDataRetention>()
            .path_parameter(project_id)?
            .body(request)
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

    /// List project API keys, including the official `owner_project_access` filter.
    pub async fn api_keys(
        &self,
        project_id: &str,
        params: &AdminListParams,
    ) -> Result<ApiResponse<ProjectApiKeyListResponse>, Error> {
        self.0
            .request::<operations::OpListProjectApiKeys>()
            .path_parameter(project_id)?
            .query(params)?
            .send()
            .await
    }

    /// Retrieve a project group, optionally selecting `group_type`.
    pub async fn group(
        &self,
        project_id: &str,
        group_id: &str,
        params: &ProjectGroupGetParams,
    ) -> Result<ApiResponse<ProjectGroup>, Error> {
        self.0
            .request::<operations::OpRetrieveProjectGroup>()
            .path_parameter(project_id)?
            .path_parameter(group_id)?
            .query(params)?
            .send()
            .await
    }
}

/// Administration-only access management for fine-tuned model checkpoints.
#[derive(Clone, Debug)]
pub struct AdminCheckpointPermissions(AdminClient);

impl AdminCheckpointPermissions {
    /// List project permissions for a fine-tuned model checkpoint.
    pub async fn list(
        &self,
        fine_tuned_model_checkpoint: &str,
        params: &ListFineTuningCheckpointPermissionsParams,
    ) -> Result<ApiResponse<ListFineTuningCheckpointPermissionResponse>, Error> {
        self.0
            .request::<operations::OpListFineTuningCheckpointPermissions>()
            .path_parameter(fine_tuned_model_checkpoint)?
            .query(params)?
            .send()
            .await
    }

    /// Grant projects access to a fine-tuned model checkpoint.
    pub async fn create(
        &self,
        fine_tuned_model_checkpoint: &str,
        request: CreateFineTuningCheckpointPermissionRequest,
    ) -> Result<ApiResponse<ListFineTuningCheckpointPermissionResponse>, Error> {
        self.0
            .request::<operations::OpCreateFineTuningCheckpointPermission>()
            .path_parameter(fine_tuned_model_checkpoint)?
            .body(request)
            .send()
            .await
    }

    /// Delete one project permission from a fine-tuned model checkpoint.
    pub async fn delete(
        &self,
        fine_tuned_model_checkpoint: &str,
        permission_id: &str,
    ) -> Result<ApiResponse<DeleteFineTuningCheckpointPermissionResponse>, Error> {
        self.0
            .request::<operations::OpDeleteFineTuningCheckpointPermission>()
            .path_parameter(fine_tuned_model_checkpoint)?
            .path_parameter(permission_id)?
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

    /// Query costs with the pinned `GET /organization/costs` parameters.
    ///
    /// Unlike the shared [`UsageQueryParams`] used by the usage endpoints,
    /// [`UsageCostsQueryParams`] pins `bucket_width` to `1d` and `group_by` to
    /// `project_id`/`line_item`/`api_key_id`.
    pub async fn costs(
        &self,
        params: &UsageCostsQueryParams,
    ) -> Result<ApiResponse<UsageResponse>, Error> {
        self.0
            .request::<operations::OpUsageCosts>()
            .query(params)?
            .send()
            .await
    }
}

/// Organization-level spend alerts, with project-scoped sub-resources.
#[derive(Clone, Debug)]
pub struct AdminSpendAlerts(AdminClient);

impl AdminSpendAlerts {
    /// List organization spend alerts.
    pub async fn list(&self) -> Result<ApiResponse<OrganizationSpendAlertListResource>, Error> {
        self.0
            .request::<operations::OpListOrganizationSpendAlerts>()
            .send()
            .await
    }

    /// Create an organization spend alert.
    pub async fn create(
        &self,
        request: CreateSpendAlertBody,
    ) -> Result<ApiResponse<OrganizationSpendAlert>, Error> {
        self.0
            .request::<operations::OpCreateOrganizationSpendAlert>()
            .body(request)
            .send()
            .await
    }

    /// Retrieve one organization spend alert.
    pub async fn retrieve(
        &self,
        alert_id: &str,
    ) -> Result<ApiResponse<OrganizationSpendAlert>, Error> {
        self.0
            .request::<operations::OpRetrieveOrganizationSpendAlert>()
            .path_parameter(alert_id)?
            .send()
            .await
    }

    /// Update one organization spend alert.
    ///
    /// The pinned route reuses the create body schema
    /// (`CreateSpendAlertBody`), matching openai-python and openai-node.
    pub async fn update(
        &self,
        alert_id: &str,
        request: CreateSpendAlertBody,
    ) -> Result<ApiResponse<OrganizationSpendAlert>, Error> {
        self.0
            .request::<operations::OpUpdateOrganizationSpendAlert>()
            .path_parameter(alert_id)?
            .body(request)
            .send()
            .await
    }

    /// Delete one organization spend alert.
    pub async fn delete(
        &self,
        alert_id: &str,
    ) -> Result<ApiResponse<OrganizationSpendAlertDeletedResource>, Error> {
        self.0
            .request::<operations::OpDeleteOrganizationSpendAlert>()
            .path_parameter(alert_id)?
            .send()
            .await
    }

    /// Project-scoped spend-alert sub-resource.
    #[must_use]
    pub fn project(&self, project_id: impl Into<String>) -> AdminProjectSpendAlerts {
        AdminProjectSpendAlerts {
            client: self.0.clone(),
            project_id: project_id.into(),
        }
    }
}

/// Project-scoped spend alerts.
#[derive(Clone, Debug)]
pub struct AdminProjectSpendAlerts {
    client: AdminClient,
    project_id: String,
}

impl AdminProjectSpendAlerts {
    /// List project spend alerts.
    pub async fn list(&self) -> Result<ApiResponse<ProjectSpendAlertListResource>, Error> {
        self.client
            .request::<operations::OpListProjectSpendAlerts>()
            .path_parameter(&self.project_id)?
            .send()
            .await
    }

    /// Create a project spend alert.
    pub async fn create(
        &self,
        request: CreateSpendAlertBody,
    ) -> Result<ApiResponse<ProjectSpendAlert>, Error> {
        self.client
            .request::<operations::OpCreateProjectSpendAlert>()
            .path_parameter(&self.project_id)?
            .body(request)
            .send()
            .await
    }

    /// Retrieve one project spend alert.
    pub async fn retrieve(&self, alert_id: &str) -> Result<ApiResponse<ProjectSpendAlert>, Error> {
        self.client
            .request::<operations::OpRetrieveProjectSpendAlert>()
            .path_parameter(&self.project_id)?
            .path_parameter(alert_id)?
            .send()
            .await
    }

    /// Update one project spend alert.
    pub async fn update(
        &self,
        alert_id: &str,
        request: CreateSpendAlertBody,
    ) -> Result<ApiResponse<ProjectSpendAlert>, Error> {
        self.client
            .request::<operations::OpUpdateProjectSpendAlert>()
            .path_parameter(&self.project_id)?
            .path_parameter(alert_id)?
            .body(request)
            .send()
            .await
    }

    /// Delete one project spend alert.
    pub async fn delete(
        &self,
        alert_id: &str,
    ) -> Result<ApiResponse<ProjectSpendAlertDeletedResource>, Error> {
        self.client
            .request::<operations::OpDeleteProjectSpendAlert>()
            .path_parameter(&self.project_id)?
            .path_parameter(alert_id)?
            .send()
            .await
    }
}

/// Organization-level spend limit, with project-scoped sub-resources.
#[derive(Clone, Debug)]
pub struct AdminSpendLimits(AdminClient);

impl AdminSpendLimits {
    /// Retrieve the organization spend limit.
    pub async fn get(&self) -> Result<ApiResponse<OrganizationSpendLimitResource>, Error> {
        self.0
            .request::<operations::OpGetorganizationspendlimit>()
            .send()
            .await
    }

    /// Update the organization spend limit.
    ///
    /// `UpdateSpendLimitBody::validate` checks the pinned
    /// `threshold_amount` minimum without sending the request.
    pub async fn update(
        &self,
        request: UpdateOrganizationSpendLimitBody,
    ) -> Result<ApiResponse<OrganizationSpendLimitResource>, Error> {
        self.0
            .request::<operations::OpUpdateorganizationspendlimit>()
            .body(request)
            .send()
            .await
    }

    /// Delete the organization spend limit.
    pub async fn delete(
        &self,
    ) -> Result<ApiResponse<OrganizationSpendLimitDeletedResource>, Error> {
        self.0
            .request::<operations::OpDeleteorganizationspendlimit>()
            .send()
            .await
    }

    /// Project-scoped spend-limit sub-resource.
    #[must_use]
    pub fn project(&self, project_id: impl Into<String>) -> AdminProjectSpendLimits {
        AdminProjectSpendLimits {
            client: self.0.clone(),
            project_id: project_id.into(),
        }
    }
}

/// Project-scoped spend limit.
#[derive(Clone, Debug)]
pub struct AdminProjectSpendLimits {
    client: AdminClient,
    project_id: String,
}

impl AdminProjectSpendLimits {
    /// Retrieve the project spend limit.
    pub async fn get(&self) -> Result<ApiResponse<ProjectSpendLimitResource>, Error> {
        self.client
            .request::<operations::OpGetprojectspendlimit>()
            .path_parameter(&self.project_id)?
            .send()
            .await
    }

    /// Update the project spend limit.
    pub async fn update(
        &self,
        request: UpdateProjectSpendLimitBody,
    ) -> Result<ApiResponse<ProjectSpendLimitResource>, Error> {
        self.client
            .request::<operations::OpUpdateprojectspendlimit>()
            .path_parameter(&self.project_id)?
            .body(request)
            .send()
            .await
    }

    /// Delete the project spend limit.
    pub async fn delete(&self) -> Result<ApiResponse<ProjectSpendLimitDeletedResource>, Error> {
        self.client
            .request::<operations::OpDeleteprojectspendlimit>()
            .path_parameter(&self.project_id)?
            .send()
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        convert::Infallible,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, Response, body::Incoming, service::service_fn};
    use hyper_util::rt::TokioIo;
    use static_assertions::{assert_impl_all, assert_not_impl_any};
    use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener, task::JoinHandle};

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
                                    method: method.clone(),
                                    path_and_query: path_and_query.clone(),
                                    authorization,
                                    body,
                                });

                            let checkpoint_permissions_path =
                                "/v1/fine_tuning/checkpoints/ft:model%2Fcheckpoint/permissions";
                            let (status, response_body) = if (method == Method::GET
                                && path_and_query.starts_with(checkpoint_permissions_path))
                                || (method == Method::POST
                                    && path_and_query == checkpoint_permissions_path)
                            {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"list","data":[{"id":"perm_1","created_at":1,"project_id":"proj_1","object":"checkpoint.permission"}],"has_more":false}"#,
                                )
                            } else if method == Method::DELETE
                                && path_and_query
                                    == format!("{checkpoint_permissions_path}/perm%2F1")
                            {
                                (
                                    StatusCode::OK,
                                    r#"{"id":"perm/1","object":"checkpoint.permission","deleted":true}"#,
                                )
                            } else if method == Method::POST
                                && path_and_query
                                    == "/v1/organization/projects/proj_1/data_retention"
                            {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"project.data_retention","type":"none"}"#,
                                )
                            } else if path_and_query.starts_with("/v1/organization/usage/") {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700003600,"results":[{"object":"organization.usage.completions.result","input_tokens":10,"output_tokens":2,"num_model_requests":1,"project_id":"proj_1"}]}],"has_more":false,"next_page":null}"#,
                                )
                            } else if path_and_query.starts_with("/v1/organization/costs") {
                                (
                                    StatusCode::OK,
                                    r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700086400,"results":[{"object":"organization.costs.result","amount":{"value":0.01,"currency":"usd"},"line_item":null,"project_id":"proj_1"}]}],"has_more":false,"next_page":null}"#,
                                )
                            } else if path_and_query
                                .starts_with("/v1/organization/projects/proj_1/spend_alerts")
                            {
                                let body = match (method, path_and_query.ends_with("alert%2F1")) {
                                    (Method::DELETE, _) => {
                                        r#"{"id":"alert/1","object":"project.spend_alert.deleted","deleted":true}"#
                                    }
                                    (Method::GET, false) => {
                                        r#"{"object":"list","data":[{"id":"alert/1","object":"project.spend_alert","threshold_amount":100,"currency":"USD","interval":"month","notification_channel":{"type":"email","recipients":["ops@example.com"]}}],"first_id":"alert/1","last_id":"alert/1","has_more":false}"#
                                    }
                                    _ => {
                                        r#"{"id":"alert/1","object":"project.spend_alert","threshold_amount":100,"currency":"USD","interval":"month","notification_channel":{"type":"email","recipients":["ops@example.com"]}}"#
                                    }
                                };
                                (StatusCode::OK, body)
                            } else if path_and_query.starts_with("/v1/organization/spend_alerts") {
                                let body = match (method, path_and_query.ends_with("alert%2F1")) {
                                    (Method::DELETE, _) => {
                                        r#"{"id":"alert/1","object":"organization.spend_alert.deleted","deleted":true}"#
                                    }
                                    (Method::GET, false) => {
                                        r#"{"object":"list","data":[{"id":"alert/1","object":"organization.spend_alert","threshold_amount":100,"currency":"USD","interval":"month","notification_channel":{"type":"email","recipients":["ops@example.com"]}}],"first_id":"alert/1","last_id":"alert/1","has_more":false}"#
                                    }
                                    _ => {
                                        r#"{"id":"alert/1","object":"organization.spend_alert","threshold_amount":100,"currency":"USD","interval":"month","notification_channel":{"type":"email","recipients":["ops@example.com"]}}"#
                                    }
                                };
                                (StatusCode::OK, body)
                            } else if path_and_query
                                .starts_with("/v1/organization/projects/proj_1/spend_limit")
                            {
                                if method == Method::DELETE {
                                    (
                                        StatusCode::OK,
                                        r#"{"object":"project.spend_limit.deleted","deleted":true}"#,
                                    )
                                } else {
                                    (
                                        StatusCode::OK,
                                        r#"{"object":"project.spend_limit","threshold_amount":100,"currency":"USD","interval":"month","enforcement":{"status":"inactive"}}"#,
                                    )
                                }
                            } else if path_and_query.starts_with("/v1/organization/spend_limit") {
                                if method == Method::DELETE {
                                    (
                                        StatusCode::OK,
                                        r#"{"object":"organization.spend_limit.deleted","deleted":true}"#,
                                    )
                                } else {
                                    (
                                        StatusCode::OK,
                                        r#"{"object":"organization.spend_limit","threshold_amount":100,"currency":"USD","interval":"month","enforcement":{"status":"inactive"}}"#,
                                    )
                                }
                            } else if path_and_query.starts_with("/v1/organization/audit_logs") {
                                // has_more with a null last_id: the D0147
                                // last-item fallback must recover the cursor.
                                (
                                    StatusCode::OK,
                                    r#"{"object":"list","data":[{"id":"audit_1","type":"api_key.created","effective_at":10},{"id":"audit_2","type":"api_key.deleted","effective_at":11}],"has_more":true,"first_id":"audit_1","last_id":null}"#,
                                )
                            } else if path_and_query.starts_with("/v1/organization/users") {
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
                                (
                                    StatusCode::OK,
                                    r#"{"id":"key_1","object":"organization.admin_api_key.deleted","deleted":true}"#,
                                )
                            } else if path_and_query == "/v1/organization/admin_api_keys/key_204" {
                                (StatusCode::NO_CONTENT, "")
                            } else if path_and_query == "/v1/organization/admin_api_keys/key_text" {
                                (
                                    StatusCode::OK,
                                    r#"{"id":"key_text","object":"organization.admin_api_key.deleted","deleted":true}"#,
                                )
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
                            let content_type = if path_and_query.ends_with("/key_text") {
                                "text/plain"
                            } else {
                                "application/json"
                            };
                            let response = Response::builder()
                                .status(status)
                                .header(header::CONTENT_TYPE, content_type)
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
        // Administration/Usage arrays take the bracketed spelling of the
        // official runtime baselines: node's `stringifyQuery` runs qs with
        // `arrayFormat: 'brackets'` and the pin spells the audit filters
        // `project_ids[]`, so `project_ids[]=` — not python's plain repeat
        // spelling `project_ids=`.
        assert!(pairs.contains(&("project_ids[]".to_owned(), "proj_1".to_owned())));
        assert!(pairs.contains(&("project_ids[]".to_owned(), "proj_2".to_owned())));
        assert!(pairs.contains(&("metadata[team]".to_owned(), "sdk".to_owned())));
        // D0145: an explicit null is equivalent to omitting the key; the
        // admin-only Nullable query field (`after`) never sends `after=`.
        assert!(
            !pairs.iter().any(|(name, _)| name == "after"),
            "explicit null must drop the query key entirely, got {pairs:?}"
        );

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

    #[test]
    fn query_encoder_drops_null_and_empty_string_leaf_values() {
        // Same D0145 rule as `transport.rs::append_query`: only the serialized
        // string being empty drops the key, so falsy scalars still encode.
        let query = serde_json::json!({
            "after": null,
            "purpose": "",
            "limit": 0,
            "active": false,
            "emails": ["", null, "user@example.com"],
            "filters": {"team": "", "region": null, "env": "prod"}
        });
        let pairs = encode_query(&query).expect("encode query");
        assert_eq!(
            pairs,
            vec![
                ("limit".to_owned(), "0".to_owned()),
                ("active".to_owned(), "false".to_owned()),
                ("emails[]".to_owned(), "user@example.com".to_owned()),
                ("filters[env]".to_owned(), "prod".to_owned()),
            ]
        );

        // A query whose keys are all dropped must not leave a dangling `?`.
        let empty = encode_query(&serde_json::json!({"after": null, "purpose": ""}))
            .expect("encode all-dropped query");
        assert!(empty.is_empty());
        let mut url = Url::parse("https://api.openai.com/v1/organization/users").expect("URL");
        if !empty.is_empty() {
            let mut serializer = url.query_pairs_mut();
            for (name, value) in empty {
                serializer.append_pair(&name, &value);
            }
        }
        assert_eq!(url.as_str(), "https://api.openai.com/v1/organization/users");
        assert!(url.query().is_none());
    }

    #[test]
    fn audit_effective_at_encodes_as_deep_object_bounds() {
        // The pinned audit route types `effective_at` as an object with exactly
        // gt/gte/lt/lte, so the typed filter must serialize through the admin
        // deep-object path as `effective_at[gt]=…` pairs (5-17).
        let params = AuditLogListParams {
            effective_at: openai_rs_types::Omittable::Value(
                AuditEffectiveAt::default()
                    .with_gt(1_700_000_000)
                    .with_lte(1_800_000_000),
            ),
            project_ids: openai_rs_types::Omittable::Value(vec![
                "proj_1".to_owned(),
                "proj_2".to_owned(),
            ]),
            tenant_only: openai_rs_types::Omittable::Value(true),
            page: AdminListParams {
                limit: openai_rs_types::Omittable::Value(20),
                ..AdminListParams::default()
            },
            ..AuditLogListParams::default()
        };
        let pairs = encode_query(&params).expect("encode audit query");
        assert_eq!(
            pairs,
            vec![
                ("effective_at[gt]".to_owned(), "1700000000".to_owned()),
                ("effective_at[lte]".to_owned(), "1800000000".to_owned()),
                ("project_ids[]".to_owned(), "proj_1".to_owned()),
                ("project_ids[]".to_owned(), "proj_2".to_owned()),
                ("tenant_only".to_owned(), "true".to_owned()),
                ("limit".to_owned(), "20".to_owned()),
            ]
        );

        // Omitted bounds never emit partial `effective_at[…]` keys.
        let omitted = encode_query(&AuditLogListParams::default()).expect("encode default");
        assert!(
            omitted.is_empty(),
            "the default audit query must encode to nothing, got {omitted:?}"
        );
    }

    #[derive(Clone)]
    struct ScriptedAdminResponse {
        status: StatusCode,
        retry_after: Option<HeaderValue>,
        body: &'static str,
    }

    /// Loopback server replaying one scripted response per attempt, mirroring
    /// the platform transport's scripted-response tests.
    async fn serve_scripted_admin_responses(
        script: Vec<ScriptedAdminResponse>,
    ) -> (Url, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted admin server");
        let address = listener.local_addr().expect("scripted admin address");
        let attempts = Arc::new(AtomicUsize::new(0));
        let server_attempts = Arc::clone(&attempts);

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let attempts = Arc::clone(&server_attempts);
                let script = script.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |_request: Request<Incoming>| {
                        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                        let scripted = script
                            .get(attempt.min(script.len().saturating_sub(1)))
                            .expect("scripted admin response");
                        let status = scripted.status;
                        let retry_after = scripted.retry_after.clone();
                        let body = scripted.body;
                        async move {
                            let mut builder = hyper::Response::builder()
                                .status(status)
                                .header(header::CONTENT_TYPE, "application/json");
                            if let Some(retry_after) = retry_after {
                                builder = builder.header(header::RETRY_AFTER, retry_after);
                            }
                            Ok::<_, Infallible>(
                                builder
                                    .body(Full::new(Bytes::from_static(body.as_bytes())))
                                    .expect("build scripted admin response"),
                            )
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let url = Url::parse(&format!("http://{address}/v1/")).expect("scripted admin URL");
        (url, attempts)
    }

    fn loopback_admin_client(base_url: Url, retry_policy: RetryPolicy) -> AdminClient {
        AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .with_retry_policy(retry_policy)
            .build()
            .expect("scripted admin client")
    }

    #[tokio::test]
    async fn admin_get_retries_429_to_success_with_local_backoff() {
        let (base_url, attempts) = serve_scripted_admin_responses(vec![
            ScriptedAdminResponse {
                status: StatusCode::TOO_MANY_REQUESTS,
                retry_after: None,
                body: r#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#,
            },
            ScriptedAdminResponse {
                status: StatusCode::OK,
                retry_after: None,
                body: r#"{"object":"list","data":[],"has_more":false}"#,
            },
        ])
        .await;
        let client = loopback_admin_client(base_url, RetryPolicy::openai_compatible());

        let started = Instant::now();
        let users = client
            .users()
            .list(&AdminListParams::default())
            .await
            .expect("retried admin user list after 429");
        let elapsed = started.elapsed();

        assert!(users.data.is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        // With no `Retry-After`, the first retry waits for local exponential
        // backoff, whose floor is 0.5s * 0.75 = 375ms.
        assert!(
            elapsed >= Duration::from_millis(300),
            "expected local backoff before the retry, waited only {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "429 retry must not be inflated to {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn admin_post_retry_is_gated_by_replayable_mutations() {
        let (base_url, attempts) = serve_scripted_admin_responses(vec![ScriptedAdminResponse {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after: None,
            body: r#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#,
        }])
        .await;

        // A POST mutation is Replayable, so the conservative policy (which
        // disables replayable-mutation retries) must fail on the first
        // attempt without resending the body.
        let conservative = loopback_admin_client(base_url.clone(), RetryPolicy::conservative());
        let error = conservative
            .groups()
            .create(CreateGroupBody {
                name: "engineering".to_owned(),
            })
            .await
            .expect_err("conservative policy must not replay the POST");
        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);

        // The openai_compatible default opts in, exhausting the full retry
        // budget (initial request plus two retries) for the same POST.
        let replayable = loopback_admin_client(base_url, RetryPolicy::openai_compatible());
        let error = replayable
            .groups()
            .create(CreateGroupBody {
                name: "engineering".to_owned(),
            })
            .await
            .expect_err("scripted 429 never succeeds");
        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            4,
            "one initial attempt plus one for the conservative client and two retries"
        );
    }

    #[tokio::test]
    async fn admin_get_does_not_retry_when_the_policy_is_disabled() {
        let (base_url, attempts) = serve_scripted_admin_responses(vec![ScriptedAdminResponse {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after: None,
            body: r#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#,
        }])
        .await;
        let client = loopback_admin_client(base_url, RetryPolicy::disabled());

        let error = client
            .users()
            .list(&AdminListParams::default())
            .await
            .expect_err("disabled policy must not retry even a safe GET");
        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn parseable_retry_after_ms_decides_alone_and_never_falls_back() {
        // 4-31, admin sibling of `transport.rs`: once `retry-after-ms` parses
        // as a float it decides the delay by itself, so a negative value next
        // to a perfectly valid `Retry-After` resolves to local backoff
        // (`Absent`) instead of adopting the coarse header.
        let maximum = RetryPolicy::openai_compatible().max_server_delay;
        let mut headers = http::HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("-500"));
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        assert_eq!(
            server_retry_delay(&headers, maximum),
            ServerDelay::Absent,
            "a negative millisecond value must not fall back to `Retry-After`"
        );

        // Zero parses too, so it also short-circuits into local backoff.
        headers.insert("retry-after-ms", HeaderValue::from_static("0"));
        assert_eq!(server_retry_delay(&headers, maximum), ServerDelay::Absent);

        // Non-finite values parse as floats, so they decide alone as well.
        headers.insert("retry-after-ms", HeaderValue::from_static("nan"));
        assert_eq!(server_retry_delay(&headers, maximum), ServerDelay::TooLong);
        headers.insert("retry-after-ms", HeaderValue::from_static("inf"));
        assert_eq!(server_retry_delay(&headers, maximum), ServerDelay::TooLong);

        // An over-bound millisecond value beside an in-bound `Retry-After`
        // still never adopts the coarse header.
        headers.insert("retry-after-ms", HeaderValue::from_static("130000"));
        assert_eq!(server_retry_delay(&headers, maximum), ServerDelay::TooLong);

        // Only an unparseable millisecond value falls through to the coarse
        // header, matching the platform transport and multipart parsers.
        headers.insert("retry-after-ms", HeaderValue::from_static("soon"));
        assert_eq!(
            server_retry_delay(&headers, maximum),
            ServerDelay::Valid(Duration::from_secs(1))
        );
    }

    #[tokio::test]
    async fn oversized_error_body_is_truncated_into_a_typed_api_error() {
        // 4-25: an error body past the 64KiB limit must still decode into an
        // `ApiError` carrying a truncated preview, mirroring the platform
        // transport instead of collapsing into `BodyTooLarge`.
        let mut payload = r#"{"error":{"message":"validation failed","type":"invalid_request_error"},"padding":""#
            .to_owned();
        payload.push_str(&"a".repeat(DEFAULT_MAX_ERROR_BODY_BYTES + 16 * 1024));
        payload.push_str("\"}");
        let body: &'static str = Box::leak(payload.into_boxed_str());
        let (base_url, _attempts) = serve_scripted_admin_responses(vec![ScriptedAdminResponse {
            status: StatusCode::BAD_REQUEST,
            retry_after: None,
            body,
        }])
        .await;
        let client = loopback_admin_client(base_url, RetryPolicy::openai_compatible());

        let error = client
            .users()
            .list(&AdminListParams::default())
            .await
            .expect_err("400 with an oversized body");
        let Error::Api(api) = &error else {
            panic!("expected a typed API error, got {error:?}");
        };
        assert_eq!(api.status(), StatusCode::BAD_REQUEST);
        assert_eq!(api.request_id(), None);
        let preview = api.body_preview();
        assert!(
            preview.is_truncated(),
            "an oversized error body must be flagged as truncated"
        );
        assert!(preview.as_str().len() <= 8 * 1024);
        assert!(!format!("{error:?}").contains("validation failed"));
    }

    #[tokio::test]
    async fn interrupted_error_body_read_surfaces_response_body_with_status() {
        // 4-25: a connection dying mid-body must surface as `ResponseBody`
        // with the status and request id attached, not as a bare transport
        // error that loses them.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind aborting admin server");
        let address = listener.local_addr().expect("aborting admin address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept aborting admin");
            let mut request = vec![0_u8; 8192];
            let read = stream.read(&mut request).await.expect("read request head");
            assert!(read > 0);
            // Declare a body far longer than the bytes actually sent, then
            // drop the connection so the client's read fails mid-body.
            let response = "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nx-request-id: req_admin_abort\r\ncontent-length: 4096\r\n\r\n{\"error\":{\"message\":\"trunc";
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write partial response");
            drop(stream);
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("aborting admin base");
        let client = loopback_admin_client(base_url, RetryPolicy::openai_compatible());

        let error = client
            .users()
            .list(&AdminListParams::default())
            .await
            .expect_err("interrupted error body read");
        let Error::ResponseBody {
            status, request_id, ..
        } = &error
        else {
            panic!("expected a response-body error, got {error:?}");
        };
        assert_eq!(*status, StatusCode::BAD_REQUEST);
        assert_eq!(request_id.as_deref(), Some("req_admin_abort"));
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

        let deleted = client.api_keys().delete("key_1").await.expect("delete key");
        assert!(deleted.deleted);

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
    async fn audit_logs_loopback_encodes_effective_at_bounds_and_falls_back_to_last_item() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");

        let params = AuditLogListParams {
            effective_at: openai_rs_types::Omittable::Value(
                AuditEffectiveAt::default()
                    .with_gt(1_700_000_000)
                    .with_lte(1_800_000_000),
            ),
            project_ids: openai_rs_types::Omittable::Value(vec!["proj_1".to_owned()]),
            page: AdminListParams {
                limit: openai_rs_types::Omittable::Value(20),
                ..AdminListParams::default()
            },
            ..AuditLogListParams::default()
        };
        let page = client
            .audit_logs()
            .list(&params)
            .await
            .expect("list audit logs");
        assert_eq!(page.data.len(), 2);
        assert!(page.has_more);

        // The scripted page leaves `last_id` null while advertising more
        // results; manual paging (the admin channel has no list_pages stream)
        // recovers the D0147 cursor through the last-item fallback.
        assert_eq!(page.next_after(), None);
        assert_eq!(
            page.next_after_with(page.data.last().map(|log| log.id.as_str())),
            Some("audit_2")
        );

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, Method::GET);
        // The audit filters go out with the pinned `[]` spelling
        // (`project_ids%5B%5D=proj_1` once URL-encoded), matching the
        // OpenAPI parameter name and node's brackets array format.
        assert_eq!(
            captured[0].path_and_query,
            "/v1/organization/audit_logs?effective_at%5Bgt%5D=1700000000&effective_at%5Blte%5D=1800000000&project_ids%5B%5D=proj_1&limit=20"
        );
        assert_eq!(
            captured[0].authorization.as_deref(),
            Some("Bearer admin-test-placeholder-key")
        );
        task.abort();
    }

    #[tokio::test]
    async fn usage_loopback_pins_start_time_bracket_arrays_and_result_decoding() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");

        let params = UsageQueryParams {
            project_ids: openai_rs_types::Omittable::Value(vec![
                "proj_1".to_owned(),
                "proj_2".to_owned(),
            ]),
            group_by: openai_rs_types::Omittable::Value(vec![
                UsageGroupBy::ProjectId,
                UsageGroupBy::Model,
            ]),
            ..UsageQueryParams::new(1_700_000_000)
        };
        let page = client
            .usage()
            .completions(&params)
            .await
            .expect("query completions usage");
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].start_time, 1_700_000_000);
        assert!(matches!(
            page.data[0].results.first(),
            Some(UsageResult::Completions(_))
        ));
        // `has_more=false` closes the cursor even though `next_page` is null.
        assert_eq!(page.next_page(), None);

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, Method::GET);
        // `start_time` is required on every usage route and leads the query;
        // the array filters keep the pinned `[]` spelling like the audit
        // filters (`project_ids%5B%5D=...` once URL-encoded).
        assert_eq!(
            captured[0].path_and_query,
            "/v1/organization/usage/completions?start_time=1700000000&project_ids%5B%5D=proj_1&project_ids%5B%5D=proj_2&group_by%5B%5D=project_id&group_by%5B%5D=model"
        );
        assert_eq!(
            captured[0].authorization.as_deref(),
            Some("Bearer admin-test-placeholder-key")
        );
        task.abort();
    }

    #[tokio::test]
    async fn costs_loopback_pins_one_day_bucket_and_costs_group_by() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");

        let params = UsageCostsQueryParams {
            bucket_width: openai_rs_types::Omittable::Value(UsageCostsBucketWidth::Day),
            group_by: openai_rs_types::Omittable::Value(vec![
                UsageCostsGroupBy::ProjectId,
                UsageCostsGroupBy::LineItem,
            ]),
            ..UsageCostsQueryParams::new(1_700_000_000)
        };
        let page = client
            .usage()
            .costs(&params)
            .await
            .expect("query organization costs");
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].end_time, 1_700_086_400);
        match page.data[0].results.first() {
            Some(UsageResult::Costs(result)) => {
                assert!(matches!(
                    result.amount,
                    openai_rs_types::Omittable::Value(_)
                ));
            }
            _ => panic!("the costs result must route to the Costs variant"),
        }
        assert_eq!(page.next_page(), None);

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, Method::GET);
        // The costs route pins `bucket_width` to `1d` and `group_by` to its
        // own three-value union (`line_item` is costs-only).
        assert_eq!(
            captured[0].path_and_query,
            "/v1/organization/costs?start_time=1700000000&bucket_width=1d&group_by%5B%5D=project_id&group_by%5B%5D=line_item"
        );
        assert_eq!(
            captured[0].authorization.as_deref(),
            Some("Bearer admin-test-placeholder-key")
        );
        task.abort();
    }

    #[tokio::test]
    async fn spend_alerts_loopback_covers_org_and_project_routes() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");
        let alerts = client.spend_alerts();
        // The channel struct keeps a private future-field, so the fixture is
        // built through its public Serde surface.
        let notification_channel =
            serde_json::from_value::<SpendAlertNotificationChannel>(serde_json::json!({
                "type": "email",
                "recipients": ["ops@example.com"]
            }))
            .expect("spend alert notification channel");
        let body = CreateSpendAlertBody {
            threshold_amount: 100,
            currency: SpendCurrency::Usd,
            interval: SpendInterval::Month,
            notification_channel,
        };

        let created = alerts
            .create(body.clone())
            .await
            .expect("create organization spend alert");
        assert_eq!(created.id, "alert/1");
        assert!(matches!(created.object, SpendAlertObject::Organization));

        let listed = alerts.list().await.expect("list organization alerts");
        assert_eq!(listed.data.len(), 1);
        assert_eq!(listed.data[0].threshold_amount, 100);
        assert_eq!(listed.next_after(), None);

        let retrieved = alerts.retrieve("alert/1").await.expect("retrieve alert");
        assert_eq!(retrieved.id, "alert/1");

        let updated = alerts
            .update("alert/1", body.clone())
            .await
            .expect("update organization spend alert");
        assert_eq!(updated.threshold_amount, 100);

        let deleted = alerts
            .delete("alert/1")
            .await
            .expect("delete organization spend alert");
        assert_eq!(deleted.id, "alert/1");
        assert!(deleted.deleted);
        assert!(matches!(
            deleted.object,
            SpendAlertDeletedObject::Organization
        ));

        let project = alerts.project("proj_1");
        let project_created = project
            .create(body)
            .await
            .expect("create project spend alert");
        assert!(matches!(project_created.object, SpendAlertObject::Project));

        let project_listed = project.list().await.expect("list project alerts");
        assert_eq!(project_listed.data.len(), 1);

        let project_deleted = project
            .delete("alert/1")
            .await
            .expect("delete project spend alert");
        assert!(matches!(
            project_deleted.object,
            SpendAlertDeletedObject::Project
        ));

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 8);
        let expected = [
            ("POST", "/v1/organization/spend_alerts"),
            ("GET", "/v1/organization/spend_alerts"),
            ("GET", "/v1/organization/spend_alerts/alert%2F1"),
            ("POST", "/v1/organization/spend_alerts/alert%2F1"),
            ("DELETE", "/v1/organization/spend_alerts/alert%2F1"),
            ("POST", "/v1/organization/projects/proj_1/spend_alerts"),
            ("GET", "/v1/organization/projects/proj_1/spend_alerts"),
            (
                "DELETE",
                "/v1/organization/projects/proj_1/spend_alerts/alert%2F1",
            ),
        ];
        for (index, (method, path)) in expected.iter().enumerate() {
            assert_eq!(captured[index].method.as_str(), *method);
            assert_eq!(captured[index].path_and_query, *path);
            assert_eq!(
                captured[index].authorization.as_deref(),
                Some("Bearer admin-test-placeholder-key")
            );
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&captured[0].body).expect("create alert JSON"),
            serde_json::json!({
                "threshold_amount": 100,
                "currency": "USD",
                "interval": "month",
                "notification_channel": {
                    "type": "email",
                    "recipients": ["ops@example.com"]
                }
            })
        );
        assert!(captured[1].body.is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn spend_limits_loopback_covers_org_and_project_routes() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");
        let limits = client.spend_limits();
        let body = UpdateOrganizationSpendLimitBody {
            threshold_amount: 100,
            currency: SpendLimitCurrency::Usd,
            interval: SpendLimitInterval::Month,
        };
        assert!(body.validate().is_ok());

        let current = limits.get().await.expect("get organization limit");
        assert_eq!(current.threshold_amount, 100);
        assert!(matches!(current.object, SpendLimitObject::Organization));
        assert!(matches!(
            current.enforcement.status,
            SpendLimitEnforcementStatus::Inactive
        ));

        let updated = limits
            .update(body.clone())
            .await
            .expect("update organization limit");
        assert!(matches!(updated.object, SpendLimitObject::Organization));

        let deleted = limits.delete().await.expect("delete organization limit");
        assert!(deleted.deleted);
        assert!(matches!(
            deleted.object,
            SpendLimitDeletedObject::Organization
        ));

        let project = limits.project("proj_1");
        let project_body = UpdateProjectSpendLimitBody {
            threshold_amount: 100,
            currency: SpendLimitCurrency::Usd,
            interval: SpendLimitInterval::Month,
        };
        let project_current = project.get().await.expect("get project limit");
        assert!(matches!(project_current.object, SpendLimitObject::Project));

        project
            .update(project_body)
            .await
            .expect("update project limit");

        let project_deleted = project.delete().await.expect("delete project limit");
        assert!(matches!(
            project_deleted.object,
            SpendLimitDeletedObject::Project
        ));

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 6);
        let expected = [
            ("GET", "/v1/organization/spend_limit"),
            ("POST", "/v1/organization/spend_limit"),
            ("DELETE", "/v1/organization/spend_limit"),
            ("GET", "/v1/organization/projects/proj_1/spend_limit"),
            ("POST", "/v1/organization/projects/proj_1/spend_limit"),
            ("DELETE", "/v1/organization/projects/proj_1/spend_limit"),
        ];
        for (index, (method, path)) in expected.iter().enumerate() {
            assert_eq!(captured[index].method.as_str(), *method);
            assert_eq!(captured[index].path_and_query, *path);
            assert_eq!(
                captured[index].authorization.as_deref(),
                Some("Bearer admin-test-placeholder-key")
            );
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&captured[1].body).expect("update limit JSON"),
            serde_json::json!({"threshold_amount": 100, "currency": "USD", "interval": "month"})
        );
        task.abort();
    }

    #[tokio::test]
    async fn checkpoint_permissions_loopback_list_create_delete_are_admin_only_and_opaque() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");
        let params = ListFineTuningCheckpointPermissionsParams {
            project_id: openai_rs_types::Omittable::Value("proj_1".to_owned()),
            after: openai_rs_types::Omittable::Value("perm_previous".to_owned()),
            limit: openai_rs_types::Omittable::Value(10),
            order: openai_rs_types::Omittable::Value(
                openai_rs_types::fine_tuning::CheckpointPermissionOrder::Descending,
            ),
        };

        let listed = client
            .checkpoint_permissions()
            .list("ft:model/checkpoint", &params)
            .await
            .expect("list checkpoint permissions");
        assert_eq!(listed.data[0].id, "perm_1");
        assert!(!listed.has_more);

        let created = client
            .checkpoint_permissions()
            .create(
                "ft:model/checkpoint",
                CreateFineTuningCheckpointPermissionRequest::new(["proj_1".to_owned()]),
            )
            .await
            .expect("create checkpoint permission");
        assert_eq!(created.data[0].project_id, "proj_1");

        let deleted = client
            .checkpoint_permissions()
            .delete("ft:model/checkpoint", "perm/1")
            .await
            .expect("delete checkpoint permission");
        assert_eq!(deleted.id, "perm/1");
        assert!(deleted.deleted);

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 3);
        assert_eq!(captured[0].method, Method::GET);
        assert_eq!(captured[1].method, Method::POST);
        assert_eq!(captured[2].method, Method::DELETE);
        let permissions_path = "/v1/fine_tuning/checkpoints/ft:model%2Fcheckpoint/permissions";
        assert!(captured[0].path_and_query.starts_with(permissions_path));
        for pair in [
            "after=perm_previous",
            "limit=10",
            "order=descending",
            "project_id=proj_1",
        ] {
            assert!(
                captured[0].path_and_query.contains(pair),
                "missing query pair {pair:?} in {:?}",
                captured[0].path_and_query
            );
        }
        assert_eq!(captured[1].path_and_query, permissions_path);
        assert_eq!(
            captured[2].path_and_query,
            format!("{permissions_path}/perm%2F1")
        );
        for request in captured.iter() {
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer admin-test-placeholder-key")
            );
        }
        assert!(captured[0].body.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&captured[1].body).expect("permission JSON")["project_ids"],
            serde_json::json!(["proj_1"])
        );
        assert!(captured[2].body.is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn data_retention_update_project_posts_pinned_route_and_six_value_domain() {
        let (base_url, captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");

        for value in [
            "organization_default",
            "none",
            "zero_data_retention",
            "modified_abuse_monitoring",
            "enhanced_zero_data_retention",
            "enhanced_modified_abuse_monitoring",
        ] {
            let updated = client
                .data_retention()
                .update_project(
                    "proj_1",
                    UpdateProjectDataRetentionBody {
                        retention_type: DataRetentionType::from_raw(value),
                    },
                )
                .await
                .expect("update project data retention");
            assert_eq!(updated.retention_type.as_str(), "none");
        }

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 6);
        for (index, request) in captured.iter().enumerate() {
            assert_eq!(request.method, Method::POST, "request {index} must be POST");
            assert_eq!(
                request.path_and_query, "/v1/organization/projects/proj_1/data_retention",
                "request {index} must use the pinned project data-retention route"
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer admin-test-placeholder-key")
            );
        }
        for (request, value) in captured.iter().zip([
            "organization_default",
            "none",
            "zero_data_retention",
            "modified_abuse_monitoring",
            "enhanced_zero_data_retention",
            "enhanced_modified_abuse_monitoring",
        ]) {
            assert_eq!(
                serde_json::from_slice::<Value>(&request.body).expect("data-retention JSON"),
                serde_json::json!({ "retention_type": value })
            );
        }
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

    #[tokio::test]
    async fn unexpected_legacy_204_delete_is_not_accepted_as_verified_success() {
        let (base_url, _captured, task) = spawn_server().await;
        let client = AdminClient::builder(key())
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build admin client");
        let error = client
            .api_keys()
            .delete("key_204")
            .await
            .expect_err("204 is not the pinned 200 JSON contract");
        assert_eq!(error.status(), Some(StatusCode::NO_CONTENT));

        let error = client
            .api_keys()
            .delete("key_text")
            .await
            .expect_err("non-JSON success MIME violates the pinned contract");
        assert!(matches!(error, Error::UnexpectedContentType { .. }));
        assert_eq!(error.status(), Some(StatusCode::OK));
        task.abort();
    }

    fn assert_operation<O: AdminOperation>() -> &'static str {
        assert_eq!(O::AUTH, AdminAuthScope::Admin);
        assert!(!O::ID.is_empty());
        assert!(O::ROUTE.starts_with('/'));
        assert_eq!(O::SUCCESS_STATUSES, &[StatusCode::OK]);
        assert_eq!(O::RESPONSE_CONTENT_TYPES, &["application/json"]);
        assert!(!O::REQUEST_TYPE.is_empty());
        assert!(!O::RESPONSE_TYPE.is_empty());
        O::ID
    }

    #[test]
    fn checkpoint_permission_manifest_is_exact_and_has_compiling_admin_markers() {
        assert_eq!(
            ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST,
            &[
                AdminClientOperationContract {
                    operation_id: "listFineTuningCheckpointPermissions",
                    method: "GET",
                    path: "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions",
                    request_mode: "none",
                    response_mode: "json",
                    success_statuses: &[200],
                    response_content_types: &["application/json"],
                    request_type: "()",
                    response_type: "ListFineTuningCheckpointPermissionResponse",
                    request_schema_refs: &[],
                    response_schema_refs: &[
                        "#/components/schemas/ListFineTuningCheckpointPermissionResponse",
                    ],
                },
                AdminClientOperationContract {
                    operation_id: "createFineTuningCheckpointPermission",
                    method: "POST",
                    path: "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions",
                    request_mode: "json",
                    response_mode: "json",
                    success_statuses: &[200],
                    response_content_types: &["application/json"],
                    request_type: "CreateFineTuningCheckpointPermissionRequest",
                    response_type: "ListFineTuningCheckpointPermissionResponse",
                    request_schema_refs: &[
                        "#/components/schemas/CreateFineTuningCheckpointPermissionRequest",
                    ],
                    response_schema_refs: &[
                        "#/components/schemas/ListFineTuningCheckpointPermissionResponse",
                    ],
                },
                AdminClientOperationContract {
                    operation_id: "deleteFineTuningCheckpointPermission",
                    method: "DELETE",
                    path: "/fine_tuning/checkpoints/{fine_tuned_model_checkpoint}/permissions/{permission_id}",
                    request_mode: "none",
                    response_mode: "json",
                    success_statuses: &[200],
                    response_content_types: &["application/json"],
                    request_type: "()",
                    response_type: "DeleteFineTuningCheckpointPermissionResponse",
                    request_schema_refs: &[],
                    response_schema_refs: &[
                        "#/components/schemas/DeleteFineTuningCheckpointPermissionResponse",
                    ],
                },
            ]
        );
        assert_eq!(
            assert_operation::<operations::OpListFineTuningCheckpointPermissions>(),
            "listFineTuningCheckpointPermissions"
        );
        assert_eq!(
            assert_operation::<operations::OpCreateFineTuningCheckpointPermission>(),
            "createFineTuningCheckpointPermission"
        );
        assert_eq!(
            assert_operation::<operations::OpDeleteFineTuningCheckpointPermission>(),
            "deleteFineTuningCheckpointPermission"
        );
    }

    /// Pinned Administration routes from the frozen spec source.
    ///
    /// `include_checkpoint_permissions` adds the three Administration-only
    /// fine-tuning checkpoint-permission operations, which the types-side
    /// manifest deliberately leaves to this channel.
    fn pinned_admin_operations(
        include_checkpoint_permissions: bool,
    ) -> Vec<(String, String, String)> {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/operations.json"))
                .expect("operation manifest JSON");
        let mut prefixes = ["/organization/", "/projects/"].to_vec();
        if include_checkpoint_permissions {
            prefixes.push("/fine_tuning/checkpoints/");
        }
        manifest["client_operations"]
            .as_array()
            .expect("client operation array")
            .iter()
            .filter_map(|operation| {
                let path = operation["path"].as_str()?;
                prefixes
                    .iter()
                    .any(|prefix| path.starts_with(prefix))
                    .then(|| {
                        (
                            operation["operation_id"]
                                .as_str()
                                .expect("pinned operation id")
                                .to_owned(),
                            operation["method"]
                                .as_str()
                                .expect("pinned method")
                                .to_owned(),
                            path.to_owned(),
                        )
                    })
            })
            .collect()
    }

    #[test]
    fn admin_manifest_matches_pinned_operations_json() {
        // 6-15: the binding manifest is checked against the pinned spec source
        // itself (method/path/operation_id, both directions) so the existing
        // self-referencing manifest tests cannot drift from the pin.
        let pinned = pinned_admin_operations(true);
        assert!(!pinned.is_empty(), "pinned admin operations must exist");
        let bound: Vec<(String, String, String)> = ADMIN_CLIENT_OPERATION_MANIFEST
            .iter()
            .chain(ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST)
            .map(|contract| {
                (
                    contract.operation_id.to_owned(),
                    contract.method.to_owned(),
                    contract.path.to_owned(),
                )
            })
            .collect();
        let bound_set: HashSet<&(String, String, String)> = bound.iter().collect();
        let pinned_set: HashSet<&(String, String, String)> = pinned.iter().collect();
        assert_eq!(
            bound.len(),
            bound_set.len(),
            "bound admin operations must be unique"
        );
        for (operation_id, method, path) in &pinned {
            assert!(
                bound_set.contains(&(operation_id.clone(), method.clone(), path.clone())),
                "pinned admin operation {operation_id} ({method} {path}) has no sealed binding"
            );
        }
        for (operation_id, method, path) in &bound {
            assert!(
                pinned_set.contains(&(operation_id.clone(), method.clone(), path.clone())),
                "binding {operation_id} ({method} {path}) is absent from the pinned manifest"
            );
        }
        assert_eq!(pinned.len(), bound.len());
    }

    #[test]
    fn every_manifest_entry_has_a_unique_compiling_operation_marker() {
        assert_eq!(
            ADMIN_OPERATION_MANIFEST.len(),
            pinned_admin_operations(false).len(),
            "the frozen types manifest must cover every pinned Administration \
             operation (checkpoint permissions excluded)"
        );
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

        assert_eq!(
            ADMIN_CLIENT_OPERATION_MANIFEST.len(),
            ADMIN_OPERATION_MANIFEST.len()
        );
        for actual in ADMIN_CLIENT_OPERATION_MANIFEST {
            let expected = ADMIN_OPERATION_MANIFEST
                .iter()
                .find(|expected| expected.operation_id == actual.operation_id)
                .expect("client binding must exist in the frozen types manifest");
            assert_eq!(
                actual.method, expected.method,
                "{} method",
                actual.operation_id
            );
            assert_eq!(actual.path, expected.path, "{} path", actual.operation_id);
            assert_eq!(
                actual.request_mode, expected.request_mode,
                "{} request mode",
                actual.operation_id
            );
            assert_eq!(
                actual.response_mode, expected.response_mode,
                "{} response mode",
                actual.operation_id
            );
            assert_eq!(
                actual.success_statuses, expected.success_statuses,
                "{} statuses",
                actual.operation_id
            );
            assert_eq!(
                actual.response_content_types, expected.response_content_types,
                "{} content types",
                actual.operation_id
            );
            assert_eq!(
                actual.request_type, expected.request_schema,
                "{} request type",
                actual.operation_id
            );
            assert_eq!(
                actual.response_type, expected.response_schema,
                "{} response type",
                actual.operation_id
            );
            assert_eq!(
                actual.request_schema_refs, expected.request_schema_refs,
                "{} request schema refs",
                actual.operation_id
            );
            assert_eq!(
                actual.response_schema_refs, expected.response_schema_refs,
                "{} response schema refs",
                actual.operation_id
            );
        }
    }
}
