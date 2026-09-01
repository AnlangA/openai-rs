use std::collections::BTreeMap;
use std::path::PathBuf;

use openai_rs_types::kernel::{Nullable, Omittable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// Implements `Debug` for a DTO carrying the `extra` flatten escape hatch.
///
/// 6-07: `extra` retains arbitrary server- or caller-supplied properties, so
/// a derived `Debug` prints them verbatim and can leak credential-shaped
/// values into logs. This macro prints the modelled fields listed in the
/// braces, the retained-property count in place of the map, and `<redacted>`
/// for the fields listed in the optional `secret [...]` group — escape-hatch
/// maps such as [`ThreadStartParams::config`], the device login code, and
/// the server-controlled JSON-RPC error payload.
macro_rules! redacted_extra_debug {
    ($name:ident { $($field:ident),* $(,)? }) => {
        redacted_extra_debug!($name secret [] { $($field),* });
    };
    ($name:ident secret [$($secret:ident),* $(,)?] { $($field:ident),* $(,)? }) => {
        impl ::std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    $(.field(stringify!($field), &self.$field))*
                    $(.field(stringify!($secret), &"<redacted>"))*
                    .field("extra", &self.extra.len())
                    .finish()
            }
        }
    };
}

pub(crate) use redacted_extra_debug;

/// Rejects flattened `extra` keys that collide with a typed wire key (7-21).
///
/// `#[serde(flatten)]` merges the extra map over the typed fields of the same
/// object, so a colliding key would silently overwrite a typed value (or
/// manufacture one the typed surface never set). Send paths call this before
/// encoding, reusing the kernel collision check that guards handwritten
/// serializers elsewhere in the workspace.
pub(crate) fn ensure_no_reserved(
    extra: &serde_json::Map<String, Value>,
    method: &'static str,
    reserved: &[&str],
) -> Result<(), crate::Error> {
    openai_rs_types::ExtraFields::try_from_map(extra.clone(), reserved.iter().copied())
        .map(|_| ())
        .map_err(|conflict| crate::Error::ExtraFieldConflict {
            method,
            key: conflict.key().to_owned(),
        })
}

/// W3C Trace Context attached to outbound JSON-RPC requests.
///
/// Wire shape of the pinned `W3cTraceContext`: `traceparent` and
/// `tracestate` are both optional nullable strings, so each field is
/// [`Omittable`]`<`[`Nullable`]`<String>>` and keeps all three wire states.
/// The optional `trace` property of the pinned `JSONRPCRequest` is therefore
/// modelled as `Omittable<Nullable<W3cTraceContext>>` at the injection
/// surface ([`AppServerClient::with_trace_context`](crate::AppServerClient::with_trace_context)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct W3cTraceContext {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub traceparent: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tracestate: Omittable<Nullable<String>>,
}

impl W3cTraceContext {
    /// A context identified by its `traceparent` header value.
    #[must_use]
    pub fn new(traceparent: impl Into<String>) -> Self {
        Self {
            traceparent: Omittable::Value(Nullable::Value(traceparent.into())),
            tracestate: Omittable::Omitted,
        }
    }

    /// Attaches a `tracestate` header value.
    #[must_use]
    pub fn with_tracestate(mut self, tracestate: impl Into<String>) -> Self {
        self.tracestate = Omittable::Value(Nullable::Value(tracestate.into()));
        self
    }
}

/// Metadata sent during the mandatory connection handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub version: String,
}

impl ClientInfo {
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            version: version.into(),
        }
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Capabilities explicitly advertised by the client.
///
/// Mirrors the four optional properties of the pinned
/// `InitializeCapabilities` schema. Each sendable field is [`Omittable`] so a
/// caller decides whether the key is sent at all; capabilities the schema has
/// not modelled yet are retained losslessly in [`InitializeCapabilities::extra`].
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeCapabilities {
    /// Opt into receiving experimental API methods and fields.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub experimental_api: Omittable<bool>,
    /// Allow downstream MCP servers to request OpenAI extended form
    /// elicitations.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub mcp_server_openai_form_elicitation: Omittable<bool>,
    /// Exact notification method names that should be suppressed for this
    /// connection (for example `thread/started`). The pinned schema allows an
    /// explicit `null`, so the value axis is [`Nullable`].
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub opt_out_notification_methods: Omittable<Nullable<Vec<String>>>,
    /// Opt into `attestation/generate` requests for upstream
    /// `x-oai-attestation`.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub request_attestation: Omittable<bool>,
    /// Future capability properties, retained losslessly. Send paths reject a
    /// key that collides with a typed capability (7-21).
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(InitializeCapabilities {
    experimental_api,
    mcp_server_openai_form_elicitation,
    opt_out_notification_methods,
    request_attestation
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub client_info: ClientInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<InitializeCapabilities>,
}

impl InitializeParams {
    #[must_use]
    pub fn new(client_info: ClientInfo) -> Self {
        Self {
            client_info,
            capabilities: None,
        }
    }

    /// Typed wire keys of [`InitializeCapabilities`] that its `extra` map must
    /// not shadow when the params are encoded for `initialize`.
    pub const CAPABILITY_RESERVED_KEYS: &'static [&'static str] = &[
        "experimentalApi",
        "mcpServerOpenaiFormElicitation",
        "optOutNotificationMethods",
        "requestAttestation",
    ];

    /// Ensures no flattened `extra` key shadows a typed key of this request
    /// (7-21). Called by the send path before encoding.
    pub fn validate_extra(&self) -> Result<(), crate::Error> {
        match &self.capabilities {
            Some(capabilities) => ensure_no_reserved(
                &capabilities.extra,
                "initialize",
                Self::CAPABILITY_RESERVED_KEYS,
            ),
            None => Ok(()),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub user_agent: String,
    pub codex_home: PathBuf,
    pub platform_family: String,
    pub platform_os: String,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(InitializeResponse {
    user_agent,
    codex_home,
    platform_family,
    platform_os
});

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoginAppBrand {
    #[default]
    Codex,
    Chatgpt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLoginOptions {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub codex_streamlined_login: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_hosted_login_success_page: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_brand: Option<LoginAppBrand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserLogin {
    pub login_id: String,
    pub auth_url: Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCodeLogin {
    pub login_id: String,
    pub verification_url: Url,
    pub user_code: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginAccountResponse {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub login_id: Option<String>,
    /// The pinned schema types this as a plain string with no `uri`
    /// constraint, so a non-absolute value must not fail the whole login
    /// response at decode time. Consumers resolve it with [`url::Url::parse`]
    /// and surface an explicit error on malformed values.
    #[serde(default)]
    pub auth_url: Option<String>,
    /// Plain string for the same reason as [`LoginAccountResponse::auth_url`].
    #[serde(default)]
    pub verification_url: Option<String>,
    #[serde(default)]
    pub user_code: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

// The device user code authorizes a login; Debug keeps it redacted like the
// direct-side `DeviceCodeLogin` stance.
redacted_extra_debug!(LoginAccountResponse secret [user_code] {
    kind,
    login_id,
    auth_url,
    verification_url
});

openai_rs_types::open_string_enum! {
    /// Outcome of an `account/login/cancel` call.
    ///
    /// The pinned `v2/CancelLoginAccountStatus` enumerates exactly `canceled`
    /// and `notFound`; values added by a newer app-server decode losslessly as
    /// [`CancelLoginStatus::Unknown`].
    pub enum CancelLoginStatus {
        Canceled = "canceled",
        NotFound = "notFound"
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelLoginResponse {
    pub status: CancelLoginStatus,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(CancelLoginResponse { status });

openai_rs_types::open_string_enum! {
    /// ChatGPT plan classification reported by app-server.
    ///
    /// The pinned `v2/PlanType` enumerates the twelve values below; plans
    /// introduced later decode losslessly as [`PlanType::Unknown`].
    pub enum PlanType {
        Free = "free",
        Go = "go",
        Plus = "plus",
        Pro = "pro",
        Prolite = "prolite",
        Team = "team",
        SelfServeBusinessUsageBased = "self_serve_business_usage_based",
        Business = "business",
        EnterpriseCbpUsageBased = "enterprise_cbp_usage_based",
        Enterprise = "enterprise",
        Edu = "edu",
        /// The pinned literal `unknown` plan placeholder, distinct from the
        /// macro-generated [`PlanType::Unknown`] forward-compatibility variant.
        UnknownPlan = "unknown"
    }
}

openai_rs_types::open_string_enum! {
    /// Why an account hit a rate limit.
    ///
    /// The pinned `v2/RateLimitReachedType` enumerates exactly five values;
    /// states added later decode losslessly as [`RateLimitReachedType::Unknown`].
    pub enum RateLimitReachedType {
        RateLimitReached = "rate_limit_reached",
        WorkspaceOwnerCreditsDepleted = "workspace_owner_credits_depleted",
        WorkspaceMemberCreditsDepleted = "workspace_member_credits_depleted",
        WorkspaceOwnerUsageLimitReached = "workspace_owner_usage_limit_reached",
        WorkspaceMemberUsageLimitReached = "workspace_member_usage_limit_reached"
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Open discriminator (`apiKey`, `chatgpt`, `amazonBedrock`, or a future
    /// account type).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan_type: Option<PlanType>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(Account {
    kind,
    email,
    plan_type
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReadResponse {
    pub account: Option<Account>,
    pub requires_openai_auth: bool,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountReadResponse {
    account,
    requires_openai_auth
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    pub used_percent: i32,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(RateLimitWindow {
    used_percent,
    window_duration_mins,
    resets_at
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsSnapshot {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(CreditsSnapshot {
    has_credits,
    unlimited,
    balance
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
    pub credits: Option<CreditsSnapshot>,
    /// Plan classification; unknown plans stay lossless.
    pub plan_type: Option<PlanType>,
    /// Why the limit was reached; unknown states stay lossless.
    pub rate_limit_reached_type: Option<RateLimitReachedType>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(RateLimitSnapshot {
    limit_id,
    limit_name,
    primary,
    secondary,
    credits,
    plan_type,
    rate_limit_reached_type
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsResponse {
    pub rate_limits: RateLimitSnapshot,
    #[serde(default)]
    pub rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitSnapshot>>,
    #[serde(default)]
    pub rate_limit_reset_credits: Option<Value>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountRateLimitsResponse {
    rate_limits,
    rate_limits_by_limit_id,
    rate_limit_reset_credits
});

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageSummary {
    pub lifetime_tokens: Option<i64>,
    pub peak_daily_tokens: Option<i64>,
    pub longest_running_turn_sec: Option<i64>,
    pub current_streak_days: Option<i64>,
    pub longest_streak_days: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountUsageSummary {
    lifetime_tokens,
    peak_daily_tokens,
    longest_running_turn_sec,
    current_streak_days,
    longest_streak_days
});

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageBucket {
    pub start_date: String,
    pub tokens: i64,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(DailyUsageBucket { start_date, tokens });

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageResponse {
    pub summary: AccountUsageSummary,
    pub daily_usage_buckets: Option<Vec<DailyUsageBucket>>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountUsageResponse {
    summary,
    daily_usage_buckets
});

openai_rs_types::open_string_enum! {
    /// String branch of the pinned `v2/AskForApproval`.
    ///
    /// Enumerates `untrusted`, `on-request`, and `never`; policies introduced
    /// later decode losslessly as [`AskForApprovalMode::Unknown`].
    pub enum AskForApprovalMode {
        Untrusted = "untrusted",
        OnRequest = "on-request",
        Never = "never"
    }
}

/// Settings of the granular approval-policy branch.
///
/// Wire shape of the `granular` object inside the pinned
/// `v2/AskForApproval` union. The keys stay snake_case exactly as pinned
/// (`mcp_elicitations`, `rules`, `sandbox_approval` required;
/// `request_permissions`, `skill_approval` optional, defaulting to `false`
/// on the app-server side when omitted). Future sub-keys a later app-server
/// adds inside this known branch stay in [`GranularAskForApproval::extra`]
/// and round-trip losslessly (17-O-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GranularAskForApproval {
    pub mcp_elicitations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_permissions: Option<bool>,
    pub rules: bool,
    pub sandbox_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_approval: Option<bool>,
    /// Future branch properties, retained losslessly (17-O-1).
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl GranularAskForApproval {
    /// Constructs the granular policy from its three pinned required
    /// settings, leaving both optional flags unset.
    #[must_use]
    pub fn new(mcp_elicitations: bool, rules: bool, sandbox_approval: bool) -> Self {
        Self {
            mcp_elicitations,
            request_permissions: None,
            rules,
            sandbox_approval,
            skill_approval: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Explicitly sends `request_permissions`.
    #[must_use]
    pub fn with_request_permissions(mut self, request_permissions: bool) -> Self {
        self.request_permissions = Some(request_permissions);
        self
    }

    /// Explicitly sends `skill_approval`.
    #[must_use]
    pub fn with_skill_approval(mut self, skill_approval: bool) -> Self {
        self.skill_approval = Some(skill_approval);
        self
    }
}

/// Approval policy: either a named string or a granular object.
///
/// Mirrors the two-branch `oneOf` of the pinned `v2/AskForApproval`: the
/// string branch is typed by the open enum [`AskForApprovalMode`] (unknown
/// strings stay lossless), and the object branch serializes as
/// `{"granular": {...}}` with the pinned snake_case keys. A third shape a
/// later app-server introduces stays verbatim in [`AskForApproval::Unknown`]
/// instead of failing the surrounding response decode (D0237 fallback, 13-O-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskForApproval {
    Mode(AskForApprovalMode),
    Granular(GranularAskForApproval),
    /// A policy shape this crate has not modelled; the payload stays verbatim.
    Unknown(Value),
}

impl From<AskForApprovalMode> for AskForApproval {
    fn from(mode: AskForApprovalMode) -> Self {
        Self::Mode(mode)
    }
}

impl From<GranularAskForApproval> for AskForApproval {
    fn from(granular: GranularAskForApproval) -> Self {
        Self::Granular(granular)
    }
}

impl Serialize for AskForApproval {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Mode(mode) => mode.serialize(serializer),
            Self::Granular(granular) => {
                #[derive(Serialize)]
                struct Wrapper<'a> {
                    granular: &'a GranularAskForApproval,
                }
                Wrapper { granular }.serialize(serializer)
            }
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AskForApproval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(_) => AskForApprovalMode::deserialize(value)
                .map(Self::Mode)
                .map_err(serde::de::Error::custom),
            Value::Object(mut object) => {
                let Some(granular) = object.remove("granular") else {
                    // An object without the pinned `granular` key is a future
                    // policy shape, not a decode failure: it stays verbatim in
                    // the Unknown variant (D0237 fallback, 13-O-1).
                    return Ok(Self::Unknown(Value::Object(object)));
                };
                match GranularAskForApproval::deserialize(granular.clone()) {
                    Ok(granular) => Ok(Self::Granular(granular)),
                    // A `granular` body that no longer matches the pinned shape
                    // degrades to the same lossless Unknown instead of failing
                    // the surrounding response decode.
                    Err(_) => {
                        object.insert("granular".to_owned(), granular);
                        Ok(Self::Unknown(Value::Object(object)))
                    }
                }
            }
            other => Err(serde::de::Error::custom(format!(
                "approval policy must be a string or a granular object, got {other}"
            ))),
        }
    }
}

openai_rs_types::open_string_enum! {
    /// Sandbox mode selected for a thread.
    ///
    /// The pinned `v2/SandboxMode` enumerates exactly `read-only`,
    /// `workspace-write`, and `danger-full-access`; modes introduced later
    /// decode losslessly as [`SandboxMode::Unknown`].
    pub enum SandboxMode {
        ReadOnly = "read-only",
        WorkspaceWrite = "workspace-write",
        DangerFullAccess = "danger-full-access"
    }
}

openai_rs_types::open_string_enum! {
    /// Assistant personality selected for a thread or turn.
    ///
    /// The pinned `v2/Personality` enumerates exactly `none`, `friendly`,
    /// and `pragmatic`; personalities introduced later decode losslessly as
    /// [`Personality::Unknown`].
    pub enum Personality {
        None = "none",
        Friendly = "friendly",
        Pragmatic = "pragmatic"
    }
}

openai_rs_types::open_string_enum! {
    /// Where approval requests are routed for review.
    ///
    /// The pinned `v2/ApprovalsReviewer` enumerates exactly `user` (the
    /// interactive user), `auto_review` (a risk-framed reviewing subagent),
    /// and the legacy `guardian_subagent`; reviewers introduced later decode
    /// losslessly as [`ApprovalsReviewer::Unknown`].
    pub enum ApprovalsReviewer {
        User = "user",
        AutoReview = "auto_review",
        GuardianSubagent = "guardian_subagent"
    }
}

openai_rs_types::open_string_enum! {
    /// Analytics classification of how a thread's session started.
    ///
    /// The pinned definition is named `v2/ThreadStartSource` (the property
    /// carrying it is `sessionStartSource`) and enumerates exactly `startup`
    /// and `clear`; sources introduced later decode losslessly as
    /// [`SessionStartSource::Unknown`].
    pub enum SessionStartSource {
        Startup = "startup",
        Clear = "clear"
    }
}

openai_rs_types::open_string_enum! {
    /// Outbound network reachability of an externally sandboxed policy.
    ///
    /// The pinned `v2/NetworkAccess` enumerates exactly `restricted` and
    /// `enabled`; values introduced later decode losslessly as
    /// [`NetworkAccess::Unknown`].
    pub enum NetworkAccess {
        Restricted = "restricted",
        Enabled = "enabled"
    }
}

/// Sandbox policy, typed as the pinned four-branch `v2/SandboxPolicy` tagged
/// union with a lossless escape for branches the pin has not named yet.
///
/// Every branch is discriminated by its camelCase `type` tag. Sub-settings
/// the pin defaults server-side (`readOnly`/`workspaceWrite` default
/// `networkAccess: false`, `externalSandbox` defaults `networkAccess:
/// "restricted"`, `workspaceWrite` defaults `writableRoots: []` and both
/// `exclude*` flags to `false`) are [`Option`] fields left unset to send no
/// key, which lets app-server apply its own defaults; setting them sends the
/// key explicitly.
///
/// Decode follows the D0237 fallback (13-O-1): a missing/non-string tag, an
/// unrecognized tag — a fifth branch a later app-server adds — or a known
/// tag whose sub-settings no longer match the pinned shape stays verbatim in
/// [`SandboxPolicy::Unknown`] instead of failing the surrounding response.
/// The union therefore needs hand-written serde impls; the four branch bodies
/// are buffered through the same `serialize_tagged_branch` /
/// `decode_tagged_branch` helpers the thread-item union uses, and each known
/// branch carries a flatten `extra` map so a pin-legal additive sub-key on a
/// known branch round-trips losslessly (17-O-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxPolicy {
    /// Full access, no sandboxing; carries no pinned sub-settings.
    DangerFullAccess {
        /// Future branch properties, retained losslessly (17-O-1).
        extra: serde_json::Map<String, Value>,
    },
    /// Read-only filesystem view of the host.
    ReadOnly {
        network_access: Option<bool>,
        /// Future branch properties, retained losslessly (17-O-1).
        extra: serde_json::Map<String, Value>,
    },
    /// Sandbox enforcement delegated to an external sandbox implementation.
    ExternalSandbox {
        network_access: Option<NetworkAccess>,
        /// Future branch properties, retained losslessly (17-O-1).
        extra: serde_json::Map<String, Value>,
    },
    /// Writable workspace plus explicit writable roots.
    WorkspaceWrite {
        writable_roots: Option<Vec<PathBuf>>,
        network_access: Option<bool>,
        exclude_slash_tmp: Option<bool>,
        exclude_tmpdir_env_var: Option<bool>,
        /// Future branch properties, retained losslessly (17-O-1).
        extra: serde_json::Map<String, Value>,
    },
    /// A branch this crate has not modelled; the payload stays verbatim.
    Unknown(Value),
}

/// Branch body of the pinned `dangerFullAccess` sandbox policy: the pin names
/// no sub-settings, so the body only carries unpinned future keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DangerFullAccessSandboxPolicyBranch {
    /// Future branch properties, retained losslessly (17-O-1).
    #[serde(default, flatten)]
    extra: serde_json::Map<String, Value>,
}

/// Branch body of the pinned `readOnly` sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadOnlySandboxPolicyBranch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network_access: Option<bool>,
    /// Future branch properties, retained losslessly (17-O-1).
    #[serde(default, flatten)]
    extra: serde_json::Map<String, Value>,
}

/// Branch body of the pinned `externalSandbox` sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalSandboxSandboxPolicyBranch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network_access: Option<NetworkAccess>,
    /// Future branch properties, retained losslessly (17-O-1).
    #[serde(default, flatten)]
    extra: serde_json::Map<String, Value>,
}

/// Branch body of the pinned `workspaceWrite` sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceWriteSandboxPolicyBranch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writable_roots: Option<Vec<PathBuf>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network_access: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude_slash_tmp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude_tmpdir_env_var: Option<bool>,
    /// Future branch properties, retained losslessly (17-O-1).
    #[serde(default, flatten)]
    extra: serde_json::Map<String, Value>,
}

impl Serialize for SandboxPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            // `dangerFullAccess` pins no sub-settings, so an empty extra map
            // makes the branch body an empty object that only collects the tag.
            Self::DangerFullAccess { extra } => serialize_tagged_branch(
                "dangerFullAccess",
                &DangerFullAccessSandboxPolicyBranch {
                    extra: extra.clone(),
                },
                serializer,
            ),
            Self::ReadOnly {
                network_access,
                extra,
            } => serialize_tagged_branch(
                "readOnly",
                &ReadOnlySandboxPolicyBranch {
                    network_access: *network_access,
                    extra: extra.clone(),
                },
                serializer,
            ),
            Self::ExternalSandbox {
                network_access,
                extra,
            } => serialize_tagged_branch(
                "externalSandbox",
                &ExternalSandboxSandboxPolicyBranch {
                    network_access: network_access.clone(),
                    extra: extra.clone(),
                },
                serializer,
            ),
            Self::WorkspaceWrite {
                writable_roots,
                network_access,
                exclude_slash_tmp,
                exclude_tmpdir_env_var,
                extra,
            } => serialize_tagged_branch(
                "workspaceWrite",
                &WorkspaceWriteSandboxPolicyBranch {
                    writable_roots: writable_roots.clone(),
                    network_access: *network_access,
                    exclude_slash_tmp: *exclude_slash_tmp,
                    exclude_tmpdir_env_var: *exclude_tmpdir_env_var,
                    extra: extra.clone(),
                },
                serializer,
            ),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SandboxPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        // A payload without a string `type` tag is not an error here: like an
        // unrecognized tag it stays verbatim in the Unknown variant.
        let tag = value.get("type").and_then(Value::as_str).map(str::to_owned);
        let decoded = match tag.as_deref() {
            Some("dangerFullAccess") => {
                decode_tagged_branch::<DangerFullAccessSandboxPolicyBranch>(value.clone()).map(
                    |branch| Self::DangerFullAccess {
                        extra: branch.extra,
                    },
                )
            }
            Some("readOnly") => decode_tagged_branch::<ReadOnlySandboxPolicyBranch>(value.clone())
                .map(|branch| Self::ReadOnly {
                    network_access: branch.network_access,
                    extra: branch.extra,
                }),
            Some("externalSandbox") => decode_tagged_branch::<ExternalSandboxSandboxPolicyBranch>(
                value.clone(),
            )
            .map(|branch| Self::ExternalSandbox {
                network_access: branch.network_access,
                extra: branch.extra,
            }),
            Some("workspaceWrite") => decode_tagged_branch::<WorkspaceWriteSandboxPolicyBranch>(
                value.clone(),
            )
            .map(|branch| Self::WorkspaceWrite {
                writable_roots: branch.writable_roots,
                network_access: branch.network_access,
                exclude_slash_tmp: branch.exclude_slash_tmp,
                exclude_tmpdir_env_var: branch.exclude_tmpdir_env_var,
                extra: branch.extra,
            }),
            _ => return Ok(Self::Unknown(value)),
        };
        // A known tag whose sub-settings no longer match the pinned shape
        // degrades to the same lossless Unknown instead of failing the
        // surrounding response decode.
        Ok(decoded.unwrap_or_else(|_| Self::Unknown(value)))
    }
}

/// Core, stable subset of `thread/start` parameters.
///
/// Every optional property of the pinned `v2/ThreadStartParams` is modelled,
/// and properties a newer app-server adds are retained losslessly in
/// [`ThreadStartParams::extra`].
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    /// Working directory of the thread. When omitted, app-server inherits the
    /// child process's cwd — and this crate always spawns the child with the
    /// dedicated CODEX_HOME as its cwd — so an absent `cwd` means CODEX_HOME,
    /// never the embedding process's working directory. Set it explicitly to
    /// anchor the thread in a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<AskForApproval>,
    /// Where approval requests raised by this thread and its subsequent turns
    /// are routed for review; defaults to the user on the app-server side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals_reviewer: Option<ApprovalsReviewer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<Personality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_instructions: Option<String>,
    /// Client-supplied analytics source classification. The pinned
    /// `v2/ThreadSource` is a plain string with no enumerated values, so it
    /// stays a free-form string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_source: Option<String>,
    /// Analytics classification of how this session started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_start_source: Option<SessionStartSource>,
    /// Escape hatch for pinned config overrides the typed surface does not
    /// model; serialized verbatim. Values can carry credentials, so `Debug`
    /// keeps them redacted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
    /// Future `thread/start` properties, retained losslessly. Send paths
    /// reject a key that collides with a typed `thread/start` field via
    /// [`ThreadStartParams::validate_extra`] (7-21).
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ThreadStartParams secret [config] {
    model,
    model_provider,
    cwd,
    approval_policy,
    approvals_reviewer,
    sandbox,
    personality,
    service_name,
    service_tier,
    base_instructions,
    developer_instructions,
    thread_source,
    session_start_source,
    ephemeral
});

impl ThreadStartParams {
    /// Typed wire keys that [`ThreadStartParams::extra`] must not shadow when
    /// the params are encoded for `thread/start`.
    pub const RESERVED_KEYS: &'static [&'static str] = &[
        "model",
        "modelProvider",
        "cwd",
        "approvalPolicy",
        "approvalsReviewer",
        "sandbox",
        "personality",
        "serviceName",
        "serviceTier",
        "baseInstructions",
        "developerInstructions",
        "threadSource",
        "sessionStartSource",
        "config",
        "ephemeral",
    ];

    /// Ensures no flattened `extra` key shadows a typed `thread/start` key
    /// (7-21). Called by the send path before encoding.
    pub fn validate_extra(&self) -> Result<(), crate::Error> {
        ensure_no_reserved(&self.extra, "thread/start", Self::RESERVED_KEYS)
    }
}

openai_rs_types::open_string_enum! {
    /// Flag a running thread raises while it waits.
    ///
    /// The pinned `#/definitions/v2/ThreadActiveFlag` enumerates exactly
    /// `waitingOnApproval` and `waitingOnUserInput`; flags introduced later
    /// decode losslessly as [`ThreadActiveFlag::Unknown`].
    pub enum ThreadActiveFlag {
        WaitingOnApproval = "waitingOnApproval",
        WaitingOnUserInput = "waitingOnUserInput"
    }
}

/// Branch body of the pinned `active` thread status.
///
/// Wire shape of `ActiveThreadStatus` in `#/definitions/v2/ThreadStatus`: the
/// `activeFlags` array is required and properties a newer app-server adds are
/// retained losslessly in [`ActiveThreadStatus::extra`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveThreadStatus {
    pub active_flags: Vec<ThreadActiveFlag>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ActiveThreadStatus { active_flags });

/// Runtime status of a thread, typed as the pinned four-branch
/// `#/definitions/v2/ThreadStatus` tagged union with a lossless escape for
/// branches the pin has not named yet.
///
/// Decode follows the D0237 fallback (13-O-2): a missing/non-string tag, an
/// unrecognized tag, or an `active` body that no longer matches the pinned
/// shape stays verbatim in [`ThreadStatus::Unknown`] instead of failing the
/// surrounding response.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadStatus {
    /// The thread exists on disk but has not been loaded into memory.
    NotLoaded,
    /// The thread is loaded and no turn is running.
    Idle,
    /// The thread is loaded but its last state cannot be recovered.
    SystemError,
    /// A turn is running; [`ActiveThreadStatus::active_flags`] says what it
    /// waits on.
    Active(ActiveThreadStatus),
    /// A status branch this crate has not modelled; the payload stays
    /// verbatim.
    Unknown(Value),
}

impl Serialize for ThreadStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::NotLoaded => {
                serialize_tagged_branch("notLoaded", &serde_json::Map::new(), serializer)
            }
            Self::Idle => serialize_tagged_branch("idle", &serde_json::Map::new(), serializer),
            Self::SystemError => {
                serialize_tagged_branch("systemError", &serde_json::Map::new(), serializer)
            }
            Self::Active(active) => serialize_tagged_branch("active", active, serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ThreadStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        // A payload without a string `type` tag is not an error here: like an
        // unrecognized tag it stays verbatim in the Unknown variant.
        let tag = value.get("type").and_then(Value::as_str).map(str::to_owned);
        let decoded = match tag.as_deref() {
            Some("notLoaded") => Ok(Self::NotLoaded),
            Some("idle") => Ok(Self::Idle),
            Some("systemError") => Ok(Self::SystemError),
            Some("active") => {
                decode_tagged_branch::<ActiveThreadStatus>(value.clone()).map(Self::Active)
            }
            _ => return Ok(Self::Unknown(value)),
        };
        // An `active` body that no longer matches the pinned shape degrades to
        // the same lossless Unknown instead of failing the surrounding
        // response decode.
        Ok(decoded.unwrap_or_else(|_| Self::Unknown(value)))
    }
}

openai_rs_types::open_string_enum! {
    /// String branch of the pinned `#/definitions/v2/SessionSource`.
    ///
    /// Enumerates exactly `cli`, `vscode`, `exec`, `appServer`, and `unknown`.
    /// The pin's literal `"unknown"` origin maps to
    /// [`SessionSourceMode::UnknownOrigin`] because the open-enum fallback
    /// variant generated for unseen values is already named `Unknown`;
    /// origins introduced later decode losslessly as
    /// [`SessionSourceMode::Unknown`].
    pub enum SessionSourceMode {
        Cli = "cli",
        Vscode = "vscode",
        Exec = "exec",
        AppServer = "appServer",
        UnknownOrigin = "unknown"
    }
}

/// Branch body of the `thread_spawn` sub-agent source.
///
/// Wire shape of `ThreadSpawnSubAgentSource` in
/// `#/definitions/v2/SubAgentSource`: `depth` and `parent_thread_id` are
/// required while `agent_nickname`/`agent_path`/`agent_role` default to
/// `null` server-side, so they are [`Option`] fields that send no key when
/// unset. The keys stay snake_case exactly as pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ThreadSpawnSubAgentSource {
    pub depth: i32,
    pub parent_thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
}

openai_rs_types::open_string_enum! {
    /// String branch of the pinned `#/definitions/v2/SubAgentSource`.
    ///
    /// Enumerates exactly `review`, `compact`, and `memory_consolidation`;
    /// sources introduced later decode losslessly as
    /// [`SubAgentSourceKind::Unknown`].
    pub enum SubAgentSourceKind {
        Review = "review",
        Compact = "compact",
        MemoryConsolidation = "memory_consolidation"
    }
}

/// Which sub-agent spawned a thread, typed as the pinned three-branch
/// `#/definitions/v2/SubAgentSource` union with a lossless escape for
/// branches the pin has not named yet.
///
/// The string branch is typed by the open enum [`SubAgentSourceKind`], the
/// `thread_spawn` branch carries the pinned spawn metadata, and `other`
/// carries the free-form discriminator. Any other shape — including a
/// multi-key object, which the pin's `additionalProperties: false` forbids —
/// stays verbatim in [`SubAgentSource::Unknown`] (D0237 fallback, 13-O-2).
#[derive(Debug, Clone, PartialEq)]
pub enum SubAgentSource {
    /// A named sub-agent purpose.
    Kind(SubAgentSourceKind),
    /// A sub-agent spawned as its own thread.
    ThreadSpawn(ThreadSpawnSubAgentSource),
    /// An unmodelled purpose name reported verbatim.
    Other(String),
    /// A shape this crate has not modelled; the payload stays verbatim.
    Unknown(Value),
}

impl Serialize for SubAgentSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Kind(kind) => kind.serialize(serializer),
            Self::ThreadSpawn(spawn) => {
                #[derive(Serialize)]
                struct Wrapper<'a> {
                    thread_spawn: &'a ThreadSpawnSubAgentSource,
                }
                Wrapper {
                    thread_spawn: spawn,
                }
                .serialize(serializer)
            }
            Self::Other(other) => {
                #[derive(Serialize)]
                struct Wrapper<'a> {
                    other: &'a str,
                }
                Wrapper { other }.serialize(serializer)
            }
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SubAgentSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match value {
            // The open enum keeps every string, named or not.
            Value::String(_) => Self::Kind(
                SubAgentSourceKind::deserialize(value).map_err(serde::de::Error::custom)?,
            ),
            // The pin gives each object branch `additionalProperties: false`
            // and a single required key, so only a one-key object names a
            // branch; anything else stays verbatim in Unknown.
            Value::Object(object) if object.len() == 1 => {
                let (key, branch) = object
                    .into_iter()
                    .next()
                    .expect("a one-key object yields exactly one entry");
                let mut wrapper = serde_json::Map::new();
                wrapper.insert(key.clone(), branch.clone());
                let wrapper = Value::Object(wrapper);
                match (key.as_str(), branch) {
                    // A known branch body that no longer matches the pinned
                    // shape degrades to the same lossless Unknown (D0237
                    // fallback).
                    ("thread_spawn", branch) => ThreadSpawnSubAgentSource::deserialize(branch)
                        .map(Self::ThreadSpawn)
                        .unwrap_or_else(|_| Self::Unknown(wrapper)),
                    ("other", Value::String(other)) => Self::Other(other),
                    _ => Self::Unknown(wrapper),
                }
            }
            other => Self::Unknown(other),
        })
    }
}

/// Where a thread's session started, typed as the pinned three-branch
/// `#/definitions/v2/SessionSource` union with a lossless escape for branches
/// the pin has not named yet.
///
/// `#/definitions/v2/Thread` requires `source`. The string branch is typed by
/// the open enum [`SessionSourceMode`]; `custom` carries the free-form
/// client-supplied origin and `subAgent` the nested [`SubAgentSource`]. Any
/// other shape — including a multi-key object, which the pin's
/// `additionalProperties: false` forbids — stays verbatim in
/// [`SessionSource::Unknown`] (D0237 fallback, 13-O-2).
#[derive(Debug, Clone, PartialEq)]
pub enum SessionSource {
    /// A named app-server entry point.
    Mode(SessionSourceMode),
    /// A client-supplied free-form origin.
    Custom(String),
    /// A thread spawned as another thread's sub-agent.
    SubAgent(SubAgentSource),
    /// A shape this crate has not modelled; the payload stays verbatim.
    Unknown(Value),
}

impl Serialize for SessionSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Mode(mode) => mode.serialize(serializer),
            Self::Custom(custom) => {
                #[derive(Serialize)]
                struct Wrapper<'a> {
                    custom: &'a str,
                }
                Wrapper { custom }.serialize(serializer)
            }
            Self::SubAgent(sub_agent) => {
                #[derive(Serialize)]
                struct Wrapper<'a> {
                    #[serde(rename = "subAgent")]
                    sub_agent: &'a SubAgentSource,
                }
                Wrapper { sub_agent }.serialize(serializer)
            }
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SessionSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match value {
            // The open enum keeps every string, named or not.
            Value::String(_) => {
                Self::Mode(SessionSourceMode::deserialize(value).map_err(serde::de::Error::custom)?)
            }
            // The pin gives each object branch `additionalProperties: false`
            // and a single required key, so only a one-key object names a
            // branch; anything else stays verbatim in Unknown.
            Value::Object(object) if object.len() == 1 => {
                let (key, branch) = object
                    .into_iter()
                    .next()
                    .expect("a one-key object yields exactly one entry");
                let mut wrapper = serde_json::Map::new();
                wrapper.insert(key.clone(), branch.clone());
                let wrapper = Value::Object(wrapper);
                match (key.as_str(), branch) {
                    ("custom", Value::String(custom)) => Self::Custom(custom),
                    // A known branch body that no longer matches the pinned
                    // shape degrades to the same lossless Unknown (D0237
                    // fallback).
                    ("subAgent", branch) => SubAgentSource::deserialize(branch)
                        .map(Self::SubAgent)
                        .unwrap_or_else(|_| Self::Unknown(wrapper)),
                    _ => Self::Unknown(wrapper),
                }
            }
            other => Self::Unknown(other),
        })
    }
}

openai_rs_types::open_string_enum! {
    /// Analytics classification carried by a thread's `threadSource`.
    ///
    /// `#/definitions/v2/Thread` types `threadSource` as the plain string
    /// `#/definitions/v2/ThreadSource` (no enumerated values), while the pin's
    /// `#/definitions/v2/ThreadSourceKind` enumerates the ten classifications
    /// its `thread/list` `sources` filter accepts. The receive side types the
    /// known ten and keeps every other string verbatim in
    /// [`ThreadSourceKind::Unknown`], which satisfies both definitions.
    pub enum ThreadSourceKind {
        Cli = "cli",
        Vscode = "vscode",
        Exec = "exec",
        AppServer = "appServer",
        SubAgent = "subAgent",
        SubAgentReview = "subAgentReview",
        SubAgentCompact = "subAgentCompact",
        SubAgentThreadSpawn = "subAgentThreadSpawn",
        SubAgentOther = "subAgentOther",
        UnknownKind = "unknown"
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub ephemeral: Option<bool>,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub turns: Option<Vec<Turn>>,
    /// Current runtime status. `#/definitions/v2/Thread` requires `status` as
    /// the pinned `v2/ThreadStatus` union; a branch this crate has not
    /// modelled degrades losslessly to [`ThreadStatus::Unknown`] (13-O-2).
    #[serde(default)]
    pub status: Option<ThreadStatus>,
    /// Origin of the thread. `#/definitions/v2/Thread` requires `source` as
    /// the pinned `v2/SessionSource` union; a branch this crate has not
    /// modelled degrades losslessly to [`SessionSource::Unknown`] (13-O-2).
    #[serde(default)]
    pub source: Option<SessionSource>,
    /// Optional analytics source classification. `#/definitions/v2/Thread`
    /// leaves `threadSource` optional (nullable plain string); unknown
    /// classifications stay verbatim inside [`ThreadSourceKind::Unknown`].
    #[serde(default)]
    pub thread_source: Option<ThreadSourceKind>,
    /// Version of the CLI that created the thread. `#/definitions/v2/Thread`
    /// requires `cliVersion` as a plain string; kept Option-wrapped so a
    /// payload that omits it still decodes (same decode-tolerance style as
    /// the other pinned required keys, D0267).
    #[serde(default)]
    pub cli_version: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(Thread {
    id,
    session_id,
    preview,
    ephemeral,
    model_provider,
    created_at,
    updated_at,
    cwd,
    name,
    turns,
    status,
    source,
    thread_source,
    cli_version
});

/// Response payload of `thread/start`.
///
/// `#/definitions/v2/ThreadStartResponse` requires `thread`, `model`,
/// `modelProvider`, `cwd`, `approvalPolicy`, `approvalsReviewer`, and
/// `sandbox`; like the rest of this DTO the negotiated fields are
/// [`Option`]-wrapped so a payload from an older app-server (or a test fake)
/// that omits one still decodes, with the pin-required keys readable as
/// `Some` (13-O-1).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResponse {
    pub thread: Thread,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub instruction_sources: Vec<PathBuf>,
    /// Approval policy negotiated for the thread, typed as the pinned
    /// `v2/AskForApproval` union; a future policy shape degrades losslessly to
    /// [`AskForApproval::Unknown`] instead of failing the response (13-O-1).
    #[serde(default)]
    pub approval_policy: Option<AskForApproval>,
    /// Reviewer the app-server routes this thread's approval requests to,
    /// typed as the open `v2/ApprovalsReviewer` enum.
    #[serde(default)]
    pub approvals_reviewer: Option<ApprovalsReviewer>,
    /// Sandbox policy negotiated for the thread. The pin marks this legacy
    /// field as "retained for compatibility" and points experimental clients
    /// at `activePermissionProfile` instead; a fifth branch degrades
    /// losslessly to [`SandboxPolicy::Unknown`] (13-O-1).
    #[serde(default)]
    pub sandbox: Option<SandboxPolicy>,
    /// Plain string: the pinned `v2/ReasoningEffort` is a `minLength 1`
    /// string with no enumerated values, and the response key is optional
    /// (nullable).
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ThreadStartResponse {
    thread,
    model,
    model_provider,
    service_tier,
    cwd,
    instruction_sources,
    approval_policy,
    approvals_reviewer,
    sandbox,
    reasoning_effort
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextElement {
    pub byte_range: ByteRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

openai_rs_types::open_string_enum! {
    /// Processing fidelity requested for an image input.
    ///
    /// The pinned `v2/ImageDetail` enumerates exactly `auto`, `low`, `high`,
    /// and `original`; values introduced later decode losslessly as
    /// [`ImageDetail::Unknown`].
    pub enum ImageDetail {
        Auto = "auto",
        Low = "low",
        High = "high",
        Original = "original"
    }
}

/// Typed user input accepted by `turn/start`.
///
/// Exactly the five variants of the pinned `v2/UserInput` schema (`text`,
/// `image`, `localImage`, `skill`, `mention`). Tags outside that set are
/// rejected rather than guessed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UserInput {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        text_elements: Vec<TextElement>,
    },
    Image {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    LocalImage {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    Skill {
        name: String,
        path: PathBuf,
    },
    Mention {
        name: String,
        path: String,
    },
}

impl UserInput {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            text_elements: Vec::new(),
        }
    }
}

openai_rs_types::open_string_enum! {
    /// Requested form of a reasoning summary on `turn/start`.
    ///
    /// The pinned `v2/ReasoningSummary` enumerates `auto`, `concise`,
    /// `detailed`, and the summary-disabling `none`; values introduced later
    /// decode losslessly as [`ReasoningSummary::Unknown`].
    pub enum ReasoningSummary {
        Auto = "auto",
        Concise = "concise",
        Detailed = "detailed",
        None = "none"
    }
}

/// Core, stable subset of `turn/start` parameters.
///
/// Every optional property of the pinned `v2/TurnStartParams` is modelled —
/// including the turn-level `sandboxPolicy`/`approvalPolicy` overrides that
/// apply to this turn and all subsequent turns of the thread — and properties
/// a newer app-server adds are retained losslessly in
/// [`TurnStartParams::extra`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub thread_id: String,
    pub input: Vec<UserInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_user_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Plain string: the pinned `v2/ReasoningEffort` is a `minLength 1`
    /// string with no enumerated values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<ReasoningSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<Personality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// Sandbox policy override for this turn and subsequent turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_policy: Option<SandboxPolicy>,
    /// Approval policy override for this turn and subsequent turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<AskForApproval>,
    /// Where approval requests raised by this turn and subsequent turns are
    /// routed for review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals_reviewer: Option<ApprovalsReviewer>,
    /// Service tier override for this turn and subsequent turns. The pin
    /// types this as a plain nullable string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Future `turn/start` properties, retained losslessly. Send paths reject
    /// a key that collides with a typed `turn/start` field via
    /// [`TurnStartParams::validate_extra`] (7-21).
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(TurnStartParams {
    thread_id,
    input,
    client_user_message_id,
    cwd,
    model,
    effort,
    summary,
    personality,
    output_schema,
    sandbox_policy,
    approval_policy,
    approvals_reviewer,
    service_tier
});

impl TurnStartParams {
    /// Typed wire keys that [`TurnStartParams::extra`] must not shadow when
    /// the params are encoded for `turn/start`.
    pub const RESERVED_KEYS: &'static [&'static str] = &[
        "threadId",
        "input",
        "clientUserMessageId",
        "cwd",
        "model",
        "effort",
        "summary",
        "personality",
        "outputSchema",
        "sandboxPolicy",
        "approvalPolicy",
        "approvalsReviewer",
        "serviceTier",
    ];

    /// Ensures no flattened `extra` key shadows a typed `turn/start` key
    /// (7-21). Called by the send path before encoding.
    pub fn validate_extra(&self) -> Result<(), crate::Error> {
        ensure_no_reserved(&self.extra, "turn/start", Self::RESERVED_KEYS)
    }

    #[must_use]
    pub fn text(thread_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            input: vec![UserInput::text(text)],
            client_user_message_id: None,
            cwd: None,
            model: None,
            effort: None,
            summary: None,
            personality: None,
            output_schema: None,
            sandbox_policy: None,
            approval_policy: None,
            approvals_reviewer: None,
            service_tier: None,
            extra: serde_json::Map::new(),
        }
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of a turn.
    ///
    /// The pinned `v2/TurnStatus` enumerates exactly `completed`,
    /// `interrupted`, `failed`, and `inProgress`; statuses introduced later
    /// decode losslessly as [`TurnStatus::Unknown`].
    pub enum TurnStatus {
        Completed = "completed",
        Interrupted = "interrupted",
        Failed = "failed",
        InProgress = "inProgress"
    }
}

openai_rs_types::open_string_enum! {
    /// How much of a turn's `items` the payload carries.
    ///
    /// The pinned `v2/TurnItemsView` enumerates exactly `notLoaded` (`items`
    /// intentionally empty), `summary` (display summary only), and `full`
    /// (every persisted item); views introduced later decode losslessly as
    /// [`TurnItemsView::Unknown`].
    pub enum TurnItemsView {
        NotLoaded = "notLoaded",
        Summary = "summary",
        Full = "full"
    }
}

impl Default for TurnItemsView {
    /// `#/definitions/v2/Turn` defaults `itemsView` to `full`, so a turn that
    /// omits the key is treated as carrying every persisted item.
    fn default() -> Self {
        Self::Full
    }
}

openai_rs_types::open_string_enum! {
    /// Kind of an active turn that cannot accept same-turn steering.
    ///
    /// The pinned `v2/NonSteerableTurnKind` enumerates exactly `review` and
    /// `compact`; kinds introduced later decode losslessly as
    /// [`NonSteerableTurnKind::Unknown`].
    pub enum NonSteerableTurnKind {
        Review = "review",
        Compact = "compact"
    }
}

openai_rs_types::open_string_enum! {
    /// String branch of the pinned `v2/CodexErrorInfo` union.
    ///
    /// The pinned literal set has exactly the eleven camelCase codes below;
    /// codes introduced later decode losslessly as [`CodexErrorCode::Unknown`].
    pub enum CodexErrorCode {
        ContextWindowExceeded = "contextWindowExceeded",
        SessionBudgetExceeded = "sessionBudgetExceeded",
        UsageLimitExceeded = "usageLimitExceeded",
        ServerOverloaded = "serverOverloaded",
        CyberPolicy = "cyberPolicy",
        InternalServerError = "internalServerError",
        Unauthorized = "unauthorized",
        BadRequest = "badRequest",
        ThreadRollbackFailed = "threadRollbackFailed",
        SandboxError = "sandboxError",
        Other = "other"
    }
}

/// Payload of the four `codexErrorInfo` object variants that forward an
/// upstream HTTP status.
///
/// The pinned schema types `httpStatusCode` as an optional `uint16` that may
/// explicitly be `null`, so an absent key and `null` both decode to [`None`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedHttpStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status_code: Option<u16>,
}

/// Payload of the `activeTurnNotSteerable` variant of [`CodexErrorInfo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTurnNotSteerableDetails {
    pub turn_kind: NonSteerableTurnKind,
}

/// Machine-readable Codex error classification carried by a [`TurnError`].
///
/// Untagged mirror of the pinned `v2/CodexErrorInfo` union: the eleven
/// plain-string codes form the open enum [`CodexErrorCode`] (an unknown code
/// decodes losslessly instead of failing the surrounding error payload), and
/// the five object variants wrap their payload under the pinned camelCase key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum CodexErrorInfo {
    /// One of the eleven pinned error-code strings.
    Code(CodexErrorCode),
    /// The upstream HTTP connection could not be established.
    HttpConnectionFailed {
        http_connection_failed: ForwardedHttpStatus,
    },
    /// Failed to connect to the response SSE stream.
    ResponseStreamConnectionFailed {
        response_stream_connection_failed: ForwardedHttpStatus,
    },
    /// The response SSE stream disconnected in the middle of a turn before
    /// completion.
    ResponseStreamDisconnected {
        response_stream_disconnected: ForwardedHttpStatus,
    },
    /// Reached the retry limit for responses.
    ResponseTooManyFailedAttempts {
        response_too_many_failed_attempts: ForwardedHttpStatus,
    },
    /// `turn/start` or `turn/steer` was submitted while the active turn cannot
    /// accept same-turn steering, for example `/review` or `/compact`.
    ActiveTurnNotSteerable {
        active_turn_not_steerable: ActiveTurnNotSteerableDetails,
    },
}

/// Error payload of a failed turn.
///
/// Wire shape of the pinned `v2/TurnError`: `message` is required, while
/// `additionalDetails` and `codexErrorInfo` are optional nulls; properties
/// added by a newer app-server are retained losslessly in [`TurnError::extra`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_error_info: Option<CodexErrorInfo>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(TurnError {
    message,
    additional_details,
    codex_error_info
});

// --- Thread items (pinned 0.144.5 `v2/ThreadItem`) -------------------------
//
// The eighteen-branch `oneOf` below is discriminated by each branch's `type`
// tag. Open string enums keep values a newer app-server introduces lossless;
// nested closed unions stay pin-faithful because a tag they do not know makes
// the whole item degrade to `ThreadItem::Unknown` instead of failing the
// surrounding `Turn` or `ItemLifecycleNotification` decode.

openai_rs_types::open_string_enum! {
    /// Classifies an assistant message as interim commentary or final answer.
    ///
    /// The pinned `v2/MessagePhase` enumerates exactly `commentary` and
    /// `final_answer`; phases introduced later decode losslessly as
    /// [`MessagePhase::Unknown`]. Providers emit the phase inconsistently, so
    /// an absent key (`None`) means "phase unknown".
    pub enum MessagePhase {
        Commentary = "commentary",
        FinalAnswer = "final_answer"
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of a `commandExecution` thread item.
    ///
    /// The pinned `v2/CommandExecutionStatus` enumerates exactly `inProgress`,
    /// `completed`, `failed`, and `declined`; statuses introduced later decode
    /// losslessly as [`CommandExecutionStatus::Unknown`].
    pub enum CommandExecutionStatus {
        InProgress = "inProgress",
        Completed = "completed",
        Failed = "failed",
        Declined = "declined"
    }
}

openai_rs_types::open_string_enum! {
    /// Who initiated a `commandExecution` thread item.
    ///
    /// The pinned `v2/CommandExecutionSource` enumerates exactly the four
    /// values below; sources introduced later decode losslessly as
    /// [`CommandExecutionSource::Unknown`].
    pub enum CommandExecutionSource {
        Agent = "agent",
        UserShell = "userShell",
        UnifiedExecStartup = "unifiedExecStartup",
        UnifiedExecInteraction = "unifiedExecInteraction"
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of a `fileChange` thread item.
    ///
    /// The pinned `v2/PatchApplyStatus` enumerates exactly `inProgress`,
    /// `completed`, `failed`, and `declined`; statuses introduced later decode
    /// losslessly as [`PatchApplyStatus::Unknown`].
    pub enum PatchApplyStatus {
        InProgress = "inProgress",
        Completed = "completed",
        Failed = "failed",
        Declined = "declined"
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of an `mcpToolCall` thread item.
    ///
    /// The pinned `v2/McpToolCallStatus` enumerates exactly `inProgress`,
    /// `completed`, and `failed`; statuses introduced later decode losslessly
    /// as [`McpToolCallStatus::Unknown`].
    pub enum McpToolCallStatus {
        InProgress = "inProgress",
        Completed = "completed",
        Failed = "failed"
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of a `dynamicToolCall` thread item.
    ///
    /// The pinned `v2/DynamicToolCallStatus` enumerates exactly `inProgress`,
    /// `completed`, and `failed`; statuses introduced later decode losslessly
    /// as [`DynamicToolCallStatus::Unknown`].
    pub enum DynamicToolCallStatus {
        InProgress = "inProgress",
        Completed = "completed",
        Failed = "failed"
    }
}

openai_rs_types::open_string_enum! {
    /// Lifecycle state of a `collabAgentToolCall` thread item.
    ///
    /// The pinned `v2/CollabAgentToolCallStatus` enumerates exactly
    /// `inProgress`, `completed`, and `failed`; statuses introduced later
    /// decode losslessly as [`CollabAgentToolCallStatus::Unknown`].
    pub enum CollabAgentToolCallStatus {
        InProgress = "inProgress",
        Completed = "completed",
        Failed = "failed"
    }
}

openai_rs_types::open_string_enum! {
    /// Last known state of one collab agent.
    ///
    /// The pinned `v2/CollabAgentStatus` enumerates exactly the seven values
    /// below; states introduced later decode losslessly as
    /// [`CollabAgentStatus::Unknown`].
    pub enum CollabAgentStatus {
        PendingInit = "pendingInit",
        Running = "running",
        Interrupted = "interrupted",
        Completed = "completed",
        Errored = "errored",
        Shutdown = "shutdown",
        NotFound = "notFound"
    }
}

openai_rs_types::open_string_enum! {
    /// Name of the collab tool invoked by a `collabAgentToolCall` item.
    ///
    /// The pinned `v2/CollabAgentTool` enumerates exactly the five values
    /// below; tools introduced later decode losslessly as
    /// [`CollabAgentTool::Unknown`].
    pub enum CollabAgentTool {
        SpawnAgent = "spawnAgent",
        SendInput = "sendInput",
        ResumeAgent = "resumeAgent",
        Wait = "wait",
        CloseAgent = "closeAgent"
    }
}

openai_rs_types::open_string_enum! {
    /// Kind of activity a `subAgentActivity` thread item records.
    ///
    /// The pinned `v2/SubAgentActivityKind` enumerates exactly `started`,
    /// `interacted`, and `interrupted`; kinds introduced later decode
    /// losslessly as [`SubAgentActivityKind::Unknown`].
    pub enum SubAgentActivityKind {
        Started = "started",
        Interacted = "interacted",
        Interrupted = "interrupted"
    }
}

/// One rendered fragment of a `hookPrompt` thread item.
///
/// Wire shape of the pinned `v2/HookPromptFragment`: both `hookRunId` and
/// `text` are required strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPromptFragment {
    pub hook_run_id: String,
    pub text: String,
}

/// One memory citation range attached to an `agentMessage` thread item.
///
/// Wire shape of the pinned `v2/MemoryCitationEntry`: `lineStart`/`lineEnd`
/// are required `uint32` values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCitationEntry {
    pub line_start: u32,
    pub line_end: u32,
    pub note: String,
    pub path: String,
}

/// Memory threads an `agentMessage` thread item drew from.
///
/// Wire shape of the pinned `v2/MemoryCitation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCitation {
    pub entries: Vec<MemoryCitationEntry>,
    pub thread_ids: Vec<String>,
}

/// Best-effort parse of one action inside a command line.
///
/// Mirrors the four-branch `oneOf` of the pinned `v2/CommandAction`. The
/// union is closed exactly as pinned: a tag 0.144.5 does not enumerate fails
/// the branch decode and the whole thread item degrades to
/// [`ThreadItem::Unknown`] with its payload intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CommandAction {
    /// Reading one file.
    Read {
        command: String,
        name: String,
        /// Pinned `v2/AbsolutePathBuf`, kept as its lossless string form.
        path: String,
    },
    /// Listing a directory.
    ListFiles {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Searching a directory tree.
    Search {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// Anything the parser did not recognize.
    Unknown { command: String },
}

/// Kind of one change inside a `fileChange` thread item.
///
/// Mirrors the three-branch `oneOf` of the pinned `v2/PatchChangeKind`. The
/// `move_path` property keeps its pinned snake_case wire key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PatchChangeKind {
    /// Newly added file.
    Add,
    /// Deleted file.
    Delete,
    /// Modified file, with its post-move location when it moved.
    Update {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        move_path: Option<String>,
    },
}

/// One file diff inside a `fileChange` thread item.
///
/// Wire shape of the pinned `v2/FileUpdateChange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileUpdateChange {
    pub diff: String,
    pub kind: PatchChangeKind,
    pub path: String,
}

/// Connector application context of an `mcpToolCall` thread item.
///
/// Wire shape of the pinned `v2/McpToolCallAppContext`: only `connectorId`
/// is required; every other property is an optional nullable string, so
/// absent and `null` both decode to `None` and `None` sends no key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCallAppContext {
    pub connector_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

/// Failure detail of an `mcpToolCall` thread item.
///
/// Wire shape of the pinned `v2/McpToolCallError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolCallError {
    pub message: String,
}

/// Result of a completed `mcpToolCall` thread item.
///
/// Wire shape of the pinned `v2/McpToolCallResult`: `content` is a required
/// array of unconstrained JSON values, while `structuredContent` and `_meta`
/// stay verbatim [`Value`]s because the pin declares no inner structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCallResult {
    pub content: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// One output chunk of a `dynamicToolCall` thread item.
///
/// Mirrors the two-branch `oneOf` of the pinned
/// `v2/DynamicToolCallOutputContentItem` (`inputText`/`inputImage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DynamicToolCallOutputContentItem {
    InputText { text: String },
    InputImage { image_url: String },
}

/// Last known state of one target agent of a collab tool call.
///
/// Wire shape of the pinned `v2/CollabAgentState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabAgentState {
    pub status: CollabAgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Web-search action of a `webSearch` thread item.
///
/// Mirrors the four-branch `oneOf` of the pinned `v2/WebSearchAction`
/// (`search`/`openPage`/`findInPage`/`other`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WebSearchAction {
    /// Run one or more search queries.
    Search {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queries: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// Open a page in the browser view.
    OpenPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// Find a pattern inside an open page.
    FindInPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// Any action the pin does not name.
    Other,
}

/// A `userMessage` thread item.
///
/// Wire shape of the pinned `UserMessageThreadItem` branch: `id` and the
/// `content` array of [`UserInput`] are required; `clientId` is an optional
/// nullable string, so absent and `null` both decode to `None` and `None`
/// sends no key.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageThreadItem {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub content: Vec<UserInput>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(UserMessageThreadItem {
    id,
    client_id,
    content
});

/// A `hookPrompt` thread item.
///
/// Wire shape of the pinned `HookPromptThreadItem` branch: `id` and the
/// `fragments` array are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPromptThreadItem {
    pub id: String,
    pub fragments: Vec<HookPromptFragment>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(HookPromptThreadItem { id, fragments });

/// An `agentMessage` thread item.
///
/// Wire shape of the pinned `AgentMessageThreadItem` branch: `id` and `text`
/// are required; `phase` and `memoryCitation` are optional nulls.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageThreadItem {
    pub id: String,
    pub text: String,
    /// Interim commentary versus terminal answer; absent means unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<MessagePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_citation: Option<MemoryCitation>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AgentMessageThreadItem {
    id,
    text,
    phase,
    memory_citation
});

/// A `plan` thread item (experimental).
///
/// Wire shape of the pinned `PlanThreadItem` branch. The completed item is
/// authoritative and may not match the concatenation of `PlanDelta` text.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanThreadItem {
    pub id: String,
    pub text: String,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(PlanThreadItem { id, text });

/// A `reasoning` thread item.
///
/// Wire shape of the pinned `ReasoningThreadItem` branch: only `id` is
/// required; `content` and `summary` default to empty arrays server-side, so
/// an absent key decodes to an empty vector and both arrays always serialize
/// (their pinned default *is* `[]`).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningThreadItem {
    pub id: String,
    #[serde(default)]
    pub content: Vec<String>,
    #[serde(default)]
    pub summary: Vec<String>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ReasoningThreadItem {
    id,
    content,
    summary
});

/// A `commandExecution` thread item.
///
/// Wire shape of the pinned `CommandExecutionThreadItem` branch: `id`,
/// `command`, `commandActions`, `cwd`, and `status` are required;
/// `aggregatedOutput`, `durationMs`, `exitCode`, and `processId` are optional
/// nulls and `source` defaults to `agent` server-side.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionThreadItem {
    pub id: String,
    /// The command to be executed.
    pub command: String,
    /// Best-effort parsing of the actions the command performs.
    pub command_actions: Vec<CommandAction>,
    /// Pinned `v2/LegacyAppPathString`, kept as its lossless string form.
    pub cwd: String,
    pub status: CommandExecutionStatus,
    /// The command's output, aggregated from stdout and stderr.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregated_output: Option<String>,
    /// Duration of the execution in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// The command's exit code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Identifier of the underlying PTY process, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<String>,
    /// Who initiated the command; `None` lets app-server apply its `agent`
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<CommandExecutionSource>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(CommandExecutionThreadItem {
    id,
    command,
    command_actions,
    cwd,
    status,
    aggregated_output,
    duration_ms,
    exit_code,
    process_id,
    source
});

/// A `fileChange` thread item.
///
/// Wire shape of the pinned `FileChangeThreadItem` branch: `id`, `changes`,
/// and `status` are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeThreadItem {
    pub id: String,
    pub changes: Vec<FileUpdateChange>,
    pub status: PatchApplyStatus,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(FileChangeThreadItem {
    id,
    changes,
    status
});

/// An `mcpToolCall` thread item.
///
/// Wire shape of the pinned `McpToolCallThreadItem` branch: `id`, `server`,
/// `tool`, `status`, and the unconstrained `arguments` value are required;
/// every remaining property is optional. `mcpAppResourceUri` is deprecated
/// upstream in favour of `appContext.resourceUri` and stays modelled because
/// 0.144.5 still emits it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCallThreadItem {
    pub id: String,
    pub server: String,
    /// Name of the invoked MCP tool.
    pub tool: String,
    pub status: McpToolCallStatus,
    /// Tool arguments; the pin declares no structure, so the value stays raw.
    pub arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_context: Option<McpToolCallAppContext>,
    /// Duration of the call in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<McpToolCallError>,
    /// Deprecated upstream: prefer `appContext.resourceUri`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_app_resource_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<McpToolCallResult>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(McpToolCallThreadItem {
    id,
    server,
    tool,
    status,
    arguments,
    app_context,
    duration_ms,
    error,
    mcp_app_resource_uri,
    plugin_id,
    result
});

/// A `dynamicToolCall` thread item.
///
/// Wire shape of the pinned `DynamicToolCallThreadItem` branch: `id`, `tool`,
/// `status`, and the unconstrained `arguments` value are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolCallThreadItem {
    pub id: String,
    pub tool: String,
    pub status: DynamicToolCallStatus,
    /// Tool arguments; the pin declares no structure, so the value stays raw.
    pub arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
    /// Duration of the call in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(DynamicToolCallThreadItem {
    id,
    tool,
    status,
    arguments,
    content_items,
    duration_ms,
    namespace,
    success
});

/// A `collabAgentToolCall` thread item.
///
/// Wire shape of the pinned `CollabAgentToolCallThreadItem` branch: `id`,
/// `agentsStates`, `receiverThreadIds`, `senderThreadId`, `status`, and
/// `tool` are required. `reasoningEffort` stays a plain string because the
/// pinned `v2/ReasoningEffort` is a `minLength 1` string with no enumerated
/// values.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabAgentToolCallThreadItem {
    pub id: String,
    /// Last known status of the target agents, keyed by thread id.
    pub agents_states: BTreeMap<String, CollabAgentState>,
    /// Thread ids of the receiving agents; a spawn lists the new agent.
    pub receiver_thread_ids: Vec<String>,
    /// Thread id of the agent issuing the collab request.
    pub sender_thread_id: String,
    pub status: CollabAgentToolCallStatus,
    pub tool: CollabAgentTool,
    /// Model requested for the spawned agent, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Prompt text sent as part of the collab tool call, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Reasoning effort requested for the spawned agent, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(CollabAgentToolCallThreadItem {
    id,
    agents_states,
    receiver_thread_ids,
    sender_thread_id,
    status,
    tool,
    model,
    prompt,
    reasoning_effort
});

/// A `subAgentActivity` thread item.
///
/// Wire shape of the pinned `SubAgentActivityThreadItem` branch: `id`,
/// `agentPath`, `agentThreadId`, and `kind` are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentActivityThreadItem {
    pub id: String,
    pub agent_path: String,
    pub agent_thread_id: String,
    pub kind: SubAgentActivityKind,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(SubAgentActivityThreadItem {
    id,
    agent_path,
    agent_thread_id,
    kind
});

/// A `webSearch` thread item.
///
/// Wire shape of the pinned `WebSearchThreadItem` branch: `id` and `query`
/// are required; `action` is an optional null.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchThreadItem {
    pub id: String,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<WebSearchAction>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(WebSearchThreadItem { id, query, action });

/// An `imageView` thread item.
///
/// Wire shape of the pinned `ImageViewThreadItem` branch: `id` and `path`
/// (pinned `v2/LegacyAppPathString`, kept as its lossless string form) are
/// required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageViewThreadItem {
    pub id: String,
    pub path: String,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ImageViewThreadItem { id, path });

/// A `sleep` thread item.
///
/// Wire shape of the pinned `SleepThreadItem` branch: `id` and the `uint64`
/// `durationMs` are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepThreadItem {
    pub id: String,
    pub duration_ms: u64,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(SleepThreadItem { id, duration_ms });

/// An `imageGeneration` thread item.
///
/// Wire shape of the pinned `ImageGenerationThreadItem` branch: `id`,
/// `result`, and `status` are required. The pin types `status` as a plain
/// string rather than an enum, so it stays a free-form [`String`];
/// `revisedPrompt` is an optional nullable string and `savedPath` an optional
/// nullable `v2/AbsolutePathBuf`, both kept as their lossless string form.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageGenerationThreadItem {
    pub id: String,
    pub result: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ImageGenerationThreadItem {
    id,
    result,
    status,
    revised_prompt,
    saved_path
});

/// An `enteredReviewMode` thread item.
///
/// Wire shape of the pinned `EnteredReviewModeThreadItem` branch: `id` and
/// the review id `review` are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnteredReviewModeThreadItem {
    pub id: String,
    pub review: String,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(EnteredReviewModeThreadItem { id, review });

/// An `exitedReviewMode` thread item.
///
/// Wire shape of the pinned `ExitedReviewModeThreadItem` branch: `id` and
/// the review id `review` are required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitedReviewModeThreadItem {
    pub id: String,
    pub review: String,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ExitedReviewModeThreadItem { id, review });

/// A `contextCompaction` thread item.
///
/// Wire shape of the pinned `ContextCompactionThreadItem` branch: only `id`
/// is required.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionThreadItem {
    pub id: String,
    /// Future branch properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ContextCompactionThreadItem { id });

/// An item inside a turn's thread history, tagged by its `type` property.
///
/// Wire shape of the eighteen-branch `oneOf` of the pinned 0.144.5
/// `v2/ThreadItem`. Every branch models the pinned required and optional
/// properties and keeps everything a newer app-server adds losslessly in its
/// `extra` map. A tag the pin does not enumerate — or a payload that no
/// longer matches its branch shape — decodes to [`ThreadItem::Unknown`]
/// carrying the entire payload verbatim, so one unrecognized item can never
/// fail the surrounding [`Turn`] or [`ItemLifecycleNotification`] decode.
/// Matching the [`Notification`] stance, the enum is `#[non_exhaustive]` and
/// every branch payload is boxed so a `Vec<ThreadItem>` keeps one pointer
/// per item regardless of branch width.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ThreadItem {
    /// Message the user sent (`userMessage`).
    UserMessage(Box<UserMessageThreadItem>),
    /// Prompt text injected by a hook run (`hookPrompt`).
    HookPrompt(Box<HookPromptThreadItem>),
    /// Assistant message text (`agentMessage`).
    AgentMessage(Box<AgentMessageThreadItem>),
    /// Experimental proposed-plan text (`plan`).
    Plan(Box<PlanThreadItem>),
    /// Model reasoning (`reasoning`).
    Reasoning(Box<ReasoningThreadItem>),
    /// A shell command execution (`commandExecution`).
    CommandExecution(Box<CommandExecutionThreadItem>),
    /// A file patch application (`fileChange`).
    FileChange(Box<FileChangeThreadItem>),
    /// A call into an MCP server tool (`mcpToolCall`).
    McpToolCall(Box<McpToolCallThreadItem>),
    /// A call into a client-registered dynamic tool (`dynamicToolCall`).
    DynamicToolCall(Box<DynamicToolCallThreadItem>),
    /// A call into a collab-agent tool (`collabAgentToolCall`).
    CollabAgentToolCall(Box<CollabAgentToolCallThreadItem>),
    /// Lifecycle event of a sub-agent (`subAgentActivity`).
    SubAgentActivity(Box<SubAgentActivityThreadItem>),
    /// A web search (`webSearch`).
    WebSearch(Box<WebSearchThreadItem>),
    /// An image opened in the client's viewer (`imageView`).
    ImageView(Box<ImageViewThreadItem>),
    /// A pause between actions (`sleep`).
    Sleep(Box<SleepThreadItem>),
    /// A generated image (`imageGeneration`).
    ImageGeneration(Box<ImageGenerationThreadItem>),
    /// Review mode was entered (`enteredReviewMode`).
    EnteredReviewMode(Box<EnteredReviewModeThreadItem>),
    /// Review mode was exited (`exitedReviewMode`).
    ExitedReviewMode(Box<ExitedReviewModeThreadItem>),
    /// Context compaction happened (`contextCompaction`).
    ContextCompaction(Box<ContextCompactionThreadItem>),
    /// An item this crate has not modelled; the payload stays verbatim.
    Unknown(Value),
}

/// Serialize one typed branch under its pinned `type` tag.
///
/// Shared by the open tagged unions ([`ThreadItem`], [`ThreadStatus`],
/// [`SandboxPolicy`]). The branch struct is buffered through [`Value`] and the
/// tag is inserted afterwards, mirroring how the decode side separates the tag
/// from the branch body (and keeping the tag out of the branch's `extra` map).
fn serialize_tagged_branch<S, T>(
    tag: &'static str,
    item: &T,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: Serialize,
{
    let mut value = serde_json::to_value(item).map_err(serde::ser::Error::custom)?;
    let Some(object) = value.as_object_mut() else {
        return Err(serde::ser::Error::custom(
            "tagged union branch must serialize to a JSON object",
        ));
    };
    object.insert("type".to_owned(), Value::String(tag.to_owned()));
    value.serialize(serializer)
}

/// Decode one branch body after its `type` tag was matched.
///
/// The tag is removed before decoding so it cannot leak into the branch's
/// `extra` map — the same separation serde applies to internally tagged
/// enums.
fn decode_tagged_branch<T: serde::de::DeserializeOwned>(
    mut value: Value,
) -> Result<T, serde_json::Error> {
    if let Some(object) = value.as_object_mut() {
        object.remove("type");
    }
    serde_json::from_value(value)
}

impl Serialize for ThreadItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::UserMessage(item) => serialize_tagged_branch("userMessage", item, serializer),
            Self::HookPrompt(item) => serialize_tagged_branch("hookPrompt", item, serializer),
            Self::AgentMessage(item) => serialize_tagged_branch("agentMessage", item, serializer),
            Self::Plan(item) => serialize_tagged_branch("plan", item, serializer),
            Self::Reasoning(item) => serialize_tagged_branch("reasoning", item, serializer),
            Self::CommandExecution(item) => {
                serialize_tagged_branch("commandExecution", item, serializer)
            }
            Self::FileChange(item) => serialize_tagged_branch("fileChange", item, serializer),
            Self::McpToolCall(item) => serialize_tagged_branch("mcpToolCall", item, serializer),
            Self::DynamicToolCall(item) => {
                serialize_tagged_branch("dynamicToolCall", item, serializer)
            }
            Self::CollabAgentToolCall(item) => {
                serialize_tagged_branch("collabAgentToolCall", item, serializer)
            }
            Self::SubAgentActivity(item) => {
                serialize_tagged_branch("subAgentActivity", item, serializer)
            }
            Self::WebSearch(item) => serialize_tagged_branch("webSearch", item, serializer),
            Self::ImageView(item) => serialize_tagged_branch("imageView", item, serializer),
            Self::Sleep(item) => serialize_tagged_branch("sleep", item, serializer),
            Self::ImageGeneration(item) => {
                serialize_tagged_branch("imageGeneration", item, serializer)
            }
            Self::EnteredReviewMode(item) => {
                serialize_tagged_branch("enteredReviewMode", item, serializer)
            }
            Self::ExitedReviewMode(item) => {
                serialize_tagged_branch("exitedReviewMode", item, serializer)
            }
            Self::ContextCompaction(item) => {
                serialize_tagged_branch("contextCompaction", item, serializer)
            }
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ThreadItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        // A payload without a string `type` tag (missing key, non-string
        // value, or not an object at all) is not an error: it stays verbatim
        // in the Unknown variant, exactly like an unrecognized tag.
        let tag = value.get("type").and_then(Value::as_str).map(str::to_owned);
        let decoded = match tag.as_deref() {
            Some("userMessage") => decode_tagged_branch::<UserMessageThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::UserMessage),
            Some("hookPrompt") => decode_tagged_branch::<HookPromptThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::HookPrompt),
            Some("agentMessage") => decode_tagged_branch::<AgentMessageThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::AgentMessage),
            Some("plan") => decode_tagged_branch::<PlanThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::Plan),
            Some("reasoning") => decode_tagged_branch::<ReasoningThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::Reasoning),
            Some("commandExecution") => {
                decode_tagged_branch::<CommandExecutionThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::CommandExecution)
            }
            Some("fileChange") => decode_tagged_branch::<FileChangeThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::FileChange),
            Some("mcpToolCall") => decode_tagged_branch::<McpToolCallThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::McpToolCall),
            Some("dynamicToolCall") => {
                decode_tagged_branch::<DynamicToolCallThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::DynamicToolCall)
            }
            Some("collabAgentToolCall") => {
                decode_tagged_branch::<CollabAgentToolCallThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::CollabAgentToolCall)
            }
            Some("subAgentActivity") => {
                decode_tagged_branch::<SubAgentActivityThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::SubAgentActivity)
            }
            Some("webSearch") => decode_tagged_branch::<WebSearchThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::WebSearch),
            Some("imageView") => decode_tagged_branch::<ImageViewThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::ImageView),
            Some("sleep") => decode_tagged_branch::<SleepThreadItem>(value.clone())
                .map(Box::new)
                .map(Self::Sleep),
            Some("imageGeneration") => {
                decode_tagged_branch::<ImageGenerationThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::ImageGeneration)
            }
            Some("enteredReviewMode") => {
                decode_tagged_branch::<EnteredReviewModeThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::EnteredReviewMode)
            }
            Some("exitedReviewMode") => {
                decode_tagged_branch::<ExitedReviewModeThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::ExitedReviewMode)
            }
            Some("contextCompaction") => {
                decode_tagged_branch::<ContextCompactionThreadItem>(value.clone())
                    .map(Box::new)
                    .map(Self::ContextCompaction)
            }
            _ => return Ok(Self::Unknown(value)),
        };
        // A known tag whose payload no longer matches the pinned branch
        // shape — a renamed required field, a new nested-union tag — degrades
        // to the same lossless Unknown instead of failing the surrounding
        // notification decode.
        Ok(decoded.unwrap_or_else(|_| Self::Unknown(value)))
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    /// Thread items currently included in this turn payload, typed as the
    /// pinned `v2/ThreadItem` union; unrecognized items stay verbatim as
    /// [`ThreadItem::Unknown`] instead of failing the turn decode (7-05).
    #[serde(default)]
    pub items: Vec<ThreadItem>,
    pub status: TurnStatus,
    /// How much of [`Turn::items`] this payload carries. `#/definitions/v2/Turn`
    /// leaves `itemsView` optional with a pinned default of `full`, so `None`
    /// means the default — see [`Turn::items_view`] (13-O-3).
    #[serde(default)]
    pub items_view: Option<TurnItemsView>,
    /// Populated when [`Turn::status`] is `failed`; typed as the pinned
    /// `v2/TurnError` payload with unknown properties retained losslessly.
    #[serde(default)]
    pub error: Option<TurnError>,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Turn {
    /// Effective items view, applying the pinned `full` default when the
    /// app-server omits `itemsView` (`#/definitions/v2/Turn`, 13-O-3).
    #[must_use]
    pub fn items_view(&self) -> TurnItemsView {
        self.items_view.clone().unwrap_or_default()
    }
}

redacted_extra_debug!(Turn {
    id,
    items,
    status,
    items_view,
    error,
    started_at,
    completed_at,
    duration_ms
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResponse {
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(TurnStartResponse { turn });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyResponse {
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(EmptyResponse {});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginCompletedNotification {
    pub login_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountLoginCompletedNotification {
    login_id,
    success,
    error
});

openai_rs_types::open_string_enum! {
    /// How the app-server authenticated the current account.
    ///
    /// The pinned `v2/AuthMode` enumerates seven values (`apikey`, `chatgpt`,
    /// `chatgptAuthTokens`, `headers`, `agentIdentity`, `personalAccessToken`,
    /// `bedrockApiKey`); modes introduced later decode losslessly as
    /// [`AuthMode::Unknown`].
    pub enum AuthMode {
        ApiKey = "apikey",
        Chatgpt = "chatgpt",
        ChatgptAuthTokens = "chatgptAuthTokens",
        Headers = "headers",
        AgentIdentity = "agentIdentity",
        PersonalAccessToken = "personalAccessToken",
        BedrockApiKey = "bedrockApiKey"
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdatedNotification {
    pub auth_mode: Option<AuthMode>,
    pub plan_type: Option<PlanType>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountUpdatedNotification {
    auth_mode,
    plan_type
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsUpdatedNotification {
    pub rate_limits: RateLimitSnapshot,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AccountRateLimitsUpdatedNotification { rate_limits });

/// Turn failure broadcast on the dedicated `error` notification channel.
///
/// Wire shape of the pinned `v2/ErrorNotification`: `threadId`, `turnId`,
/// `willRetry`, and the typed [`TurnError`] are all required; envelope
/// properties added by a newer app-server stay lossless in
/// [`ErrorNotification::extra`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorNotification {
    pub thread_id: String,
    pub turn_id: String,
    /// Whether the app-server will retry the failed turn on its own.
    pub will_retry: bool,
    pub error: TurnError,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ErrorNotification {
    thread_id,
    turn_id,
    will_retry,
    error
});

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartedNotification {
    pub thread: Thread,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ThreadStartedNotification { thread });

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartedNotification {
    pub thread_id: String,
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(TurnStartedNotification { thread_id, turn });

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletedNotification {
    pub thread_id: String,
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(TurnCompletedNotification { thread_id, turn });

/// Item lifecycle broadcast on `item/started` and `item/completed`.
///
/// Wire shape of the pinned `v2/ItemStartedNotification` and
/// `v2/ItemCompletedNotification`: `threadId`, `turnId`, and the
/// [`ThreadItem`] are required while `startedAtMs`/`completedAtMs` are
/// optional. An item this crate has not modelled decodes losslessly as
/// [`ThreadItem::Unknown`] instead of failing the notification.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLifecycleNotification {
    pub thread_id: String,
    pub turn_id: String,
    pub item: ThreadItem,
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(ItemLifecycleNotification {
    thread_id,
    turn_id,
    item,
    started_at_ms,
    completed_at_ms
});

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageDeltaNotification {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub delta: String,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

redacted_extra_debug!(AgentMessageDeltaNotification {
    thread_id,
    turn_id,
    item_id,
    delta
});

/// Full, lossless envelope for a notification not understood by this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct RawNotification {
    pub method: String,
    pub params: Option<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Notification {
    AccountLoginCompleted(Box<AccountLoginCompletedNotification>),
    AccountUpdated(Box<AccountUpdatedNotification>),
    AccountRateLimitsUpdated(Box<AccountRateLimitsUpdatedNotification>),
    Error(Box<ErrorNotification>),
    ThreadStarted(Box<ThreadStartedNotification>),
    TurnStarted(Box<TurnStartedNotification>),
    TurnCompleted(Box<TurnCompletedNotification>),
    ItemStarted(Box<ItemLifecycleNotification>),
    ItemCompleted(Box<ItemLifecycleNotification>),
    AgentMessageDelta(Box<AgentMessageDeltaNotification>),
    Unknown(Box<RawNotification>),
}

#[cfg(any(feature = "app-server", test))]
pub(crate) fn decode_notification(
    method: String,
    params: Option<Value>,
    raw: Value,
) -> Notification {
    fn typed<T: serde::de::DeserializeOwned>(params: &Option<Value>) -> Option<T> {
        serde_json::from_value(params.clone().unwrap_or(Value::Null)).ok()
    }

    let known = matches!(
        method.as_str(),
        "account/login/completed"
            | "account/updated"
            | "account/rateLimits/updated"
            | "error"
            | "thread/started"
            | "turn/started"
            | "turn/completed"
            | "item/started"
            | "item/completed"
            | "item/agentMessage/delta"
    );
    let decoded = match method.as_str() {
        "account/login/completed" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountLoginCompleted),
        "account/updated" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountUpdated),
        "account/rateLimits/updated" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountRateLimitsUpdated),
        "error" => typed(&params).map(Box::new).map(Notification::Error),
        "thread/started" => typed(&params)
            .map(Box::new)
            .map(Notification::ThreadStarted),
        "turn/started" => typed(&params).map(Box::new).map(Notification::TurnStarted),
        "turn/completed" => typed(&params)
            .map(Box::new)
            .map(Notification::TurnCompleted),
        "item/started" => typed(&params).map(Box::new).map(Notification::ItemStarted),
        "item/completed" => typed(&params)
            .map(Box::new)
            .map(Notification::ItemCompleted),
        "item/agentMessage/delta" => typed(&params)
            .map(Box::new)
            .map(Notification::AgentMessageDelta),
        _ => None,
    };
    decoded.unwrap_or_else(|| {
        if known {
            tracing::warn!(
                rpc.method = method.as_str(),
                "typed decode failed for known app-server notification"
            );
        }
        Notification::Unknown(Box::new(RawNotification {
            method,
            params,
            raw,
        }))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        AccountLoginCompletedNotification, AccountUpdatedNotification, ActiveThreadStatus,
        ActiveTurnNotSteerableDetails, AgentMessageDeltaNotification, AgentMessageThreadItem,
        ApprovalsReviewer, AskForApproval, AskForApprovalMode, AuthMode, ByteRange,
        CancelLoginResponse, CancelLoginStatus, ClientInfo, CodexErrorCode, CodexErrorInfo,
        CollabAgentState, CollabAgentStatus, CollabAgentTool, CollabAgentToolCallStatus,
        CollabAgentToolCallThreadItem, CommandAction, CommandExecutionSource,
        CommandExecutionStatus, CommandExecutionThreadItem, ContextCompactionThreadItem,
        DynamicToolCallOutputContentItem, DynamicToolCallStatus, DynamicToolCallThreadItem,
        EnteredReviewModeThreadItem, ErrorNotification, ExitedReviewModeThreadItem,
        FileChangeThreadItem, FileUpdateChange, ForwardedHttpStatus, GranularAskForApproval,
        HookPromptFragment, HookPromptThreadItem, ImageDetail, ImageGenerationThreadItem,
        ImageViewThreadItem, InitializeCapabilities, InitializeParams, ItemLifecycleNotification,
        LoginAccountResponse, McpToolCallAppContext, McpToolCallResult, McpToolCallStatus,
        McpToolCallThreadItem, MemoryCitation, MemoryCitationEntry, MessagePhase, NetworkAccess,
        NonSteerableTurnKind, Notification, Nullable, Omittable, PatchApplyStatus, PatchChangeKind,
        Personality, PlanThreadItem, PlanType, RateLimitReachedType, RateLimitSnapshot,
        ReasoningSummary, ReasoningThreadItem, SandboxMode, SandboxPolicy, SessionSource,
        SessionSourceMode, SessionStartSource, SleepThreadItem, SubAgentActivityKind,
        SubAgentActivityThreadItem, SubAgentSource, SubAgentSourceKind, TextElement, Thread,
        ThreadActiveFlag, ThreadItem, ThreadSourceKind, ThreadSpawnSubAgentSource,
        ThreadStartParams, ThreadStartResponse, ThreadStartedNotification, ThreadStatus, Turn,
        TurnError, TurnItemsView, TurnStartParams, TurnStatus, UserInput, UserMessageThreadItem,
        W3cTraceContext, WebSearchAction, WebSearchThreadItem, decode_notification,
    };

    #[test]
    fn text_input_serializes_without_handwritten_json() -> Result<(), serde_json::Error> {
        let params = TurnStartParams::text("thr_123", "hello");
        let encoded = serde_json::to_value(params)?;
        assert_eq!(
            encoded,
            json!({
                "threadId": "thr_123",
                "input": [{"type": "text", "text": "hello"}]
            })
        );
        Ok(())
    }

    /// 7-21: a flattened `extra` key that would shadow a typed wire key is
    /// rejected before encoding instead of silently overwriting the typed
    /// value, while future keys stay lossless.
    #[test]
    fn send_params_extra_collisions_are_rejected() {
        let mut thread_params = ThreadStartParams::default();
        thread_params
            .extra
            .insert("modelProvider".to_owned(), json!("shadowed"));
        match thread_params.validate_extra() {
            Err(crate::Error::ExtraFieldConflict { method, key }) => {
                assert_eq!((method, key.as_str()), ("thread/start", "modelProvider"));
            }
            other => panic!("expected ExtraFieldConflict, got {other:?}"),
        }

        let mut turn_params = TurnStartParams::text("thr_1", "hello");
        turn_params
            .extra
            .insert("outputSchema".to_owned(), json!({"type": "object"}));
        match turn_params.validate_extra() {
            Err(crate::Error::ExtraFieldConflict { method, key }) => {
                assert_eq!((method, key.as_str()), ("turn/start", "outputSchema"));
            }
            other => panic!("expected ExtraFieldConflict, got {other:?}"),
        }

        // Future keys are retained losslessly and pass the check.
        let mut future = TurnStartParams::text("thr_1", "hello");
        future
            .extra
            .insert("futureTurnOption".to_owned(), json!(true));
        future.validate_extra().expect("future key is retained");
        let encoded = serde_json::to_value(&future).expect("serialize");
        assert_eq!(encoded["futureTurnOption"], json!(true));

        // `initialize` has no top-level extra; the nested capabilities map is
        // the checked surface.
        let bare = InitializeParams::new(ClientInfo::new("test", "0.0.0"));
        bare.validate_extra()
            .expect("no capabilities is trivially valid");

        let mut capabilities = InitializeCapabilities::default();
        capabilities
            .extra
            .insert("experimentalApi".to_owned(), json!(true));
        let nested = InitializeParams {
            capabilities: Some(capabilities),
            ..bare
        };
        match nested.validate_extra() {
            Err(crate::Error::ExtraFieldConflict { method, key }) => {
                assert_eq!((method, key.as_str()), ("initialize", "experimentalApi"));
            }
            other => panic!("expected ExtraFieldConflict, got {other:?}"),
        }
    }

    /// 7-21: the hand-maintained reserved-key lists must cover exactly the
    /// typed wire keys each params object serializes, so a newly modelled
    /// field cannot silently fall out of the collision check.
    #[test]
    fn reserved_key_lists_match_the_serialized_typed_fields() -> Result<(), serde_json::Error> {
        fn sorted_keys(encoded: &serde_json::Value) -> Vec<String> {
            let mut keys: Vec<String> = encoded
                .as_object()
                .expect("params serialize to an object")
                .keys()
                .cloned()
                .collect();
            keys.sort();
            keys
        }

        let thread = ThreadStartParams {
            model: Some("gpt-5-codex".to_owned()),
            model_provider: Some("openai".to_owned()),
            cwd: Some(PathBuf::from("/tmp")),
            approval_policy: Some(AskForApproval::Mode(AskForApprovalMode::OnRequest)),
            approvals_reviewer: Some(ApprovalsReviewer::User),
            sandbox: Some(SandboxMode::WorkspaceWrite),
            personality: Some(Personality::Friendly),
            service_name: Some("svc".to_owned()),
            service_tier: Some("flex".to_owned()),
            base_instructions: Some("base".to_owned()),
            developer_instructions: Some("dev".to_owned()),
            thread_source: Some("vscode".to_owned()),
            session_start_source: Some(SessionStartSource::Startup),
            config: Some(serde_json::Map::new()),
            ephemeral: Some(true),
            extra: serde_json::Map::new(),
        };
        let mut thread_reserved = ThreadStartParams::RESERVED_KEYS.to_vec();
        thread_reserved.sort();
        assert_eq!(
            sorted_keys(&serde_json::to_value(&thread)?),
            thread_reserved
        );

        let turn = TurnStartParams {
            thread_id: "thr_1".to_owned(),
            input: vec![UserInput::text("hello")],
            client_user_message_id: Some("msg_1".to_owned()),
            cwd: Some(PathBuf::from("/tmp")),
            model: Some("gpt-5-codex".to_owned()),
            effort: Some("medium".to_owned()),
            summary: Some(ReasoningSummary::Auto),
            personality: Some(Personality::Friendly),
            output_schema: Some(json!({"type": "object"})),
            sandbox_policy: Some(SandboxPolicy::DangerFullAccess {
                extra: serde_json::Map::new(),
            }),
            approval_policy: Some(AskForApproval::Mode(AskForApprovalMode::Never)),
            approvals_reviewer: Some(ApprovalsReviewer::User),
            service_tier: Some("flex".to_owned()),
            extra: serde_json::Map::new(),
        };
        let mut turn_reserved = TurnStartParams::RESERVED_KEYS.to_vec();
        turn_reserved.sort();
        assert_eq!(sorted_keys(&serde_json::to_value(&turn)?), turn_reserved);

        let capabilities = InitializeCapabilities {
            experimental_api: Omittable::Value(true),
            mcp_server_openai_form_elicitation: Omittable::Value(true),
            opt_out_notification_methods: Omittable::Value(Nullable::Value(vec![
                "thread/started".to_owned(),
            ])),
            request_attestation: Omittable::Value(true),
            extra: serde_json::Map::new(),
        };
        let mut capability_reserved = InitializeParams::CAPABILITY_RESERVED_KEYS.to_vec();
        capability_reserved.sort();
        assert_eq!(
            sorted_keys(&serde_json::to_value(&capabilities)?),
            capability_reserved
        );
        Ok(())
    }

    /// The pinned `v2/UserInput` schema names this property `text_elements`
    /// even though every neighbouring property of the Text variant is
    /// camelCase. The enum-level `rename_all` only renames variant tags, so
    /// the field keeps its snake_case Rust name; this test locks that wire key
    /// so a future `rename_all_fields`/variant-level `rename_all` cannot break
    /// it silently.
    #[test]
    fn text_input_elements_keep_the_pinned_snake_case_wire_key() -> Result<(), serde_json::Error> {
        let input = UserInput::Text {
            text: "see @file".to_owned(),
            text_elements: vec![TextElement {
                byte_range: ByteRange { start: 4, end: 10 },
                placeholder: Some("@file".to_owned()),
            }],
        };
        let encoded = serde_json::to_value(&input)?;
        assert_eq!(
            encoded,
            json!({
                "type": "text",
                "text": "see @file",
                "text_elements": [{
                    "byteRange": {"start": 4, "end": 10},
                    "placeholder": "@file",
                }]
            })
        );
        assert!(encoded.get("text_elements").is_some());
        assert!(encoded.get("textElements").is_none());

        let decoded: UserInput = serde_json::from_value(encoded)?;
        assert_eq!(decoded, input);
        Ok(())
    }

    /// `authUrl`/`verificationUrl` are pinned as plain strings without a `uri`
    /// constraint; decoding must accept values that are not absolute URLs and
    /// leave the parse decision to the consuming login methods.
    #[test]
    fn login_account_response_accepts_non_absolute_urls() -> Result<(), serde_json::Error> {
        let response: LoginAccountResponse = serde_json::from_value(json!({
            "type": "chatgpt",
            "loginId": "login-1",
            "authUrl": "chatgpt.com/auth",
            "futureField": 7,
        }))?;
        assert_eq!(response.login_id.as_deref(), Some("login-1"));
        assert_eq!(response.auth_url.as_deref(), Some("chatgpt.com/auth"));
        assert_eq!(response.verification_url, None);
        Ok(())
    }

    #[test]
    fn user_input_accepts_exactly_the_five_pinned_variants() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_value(UserInput::text("hello"))?,
            json!({"type": "text", "text": "hello"})
        );
        assert!(matches!(
            serde_json::from_value::<UserInput>(json!({
                "type": "image",
                "url": "https://example.test/a.png",
                "detail": "high"
            }))?,
            UserInput::Image { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<UserInput>(json!({
                "type": "localImage",
                "path": "/tmp/a.png"
            }))?,
            UserInput::LocalImage { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<UserInput>(json!({
                "type": "skill",
                "name": "deploy",
                "path": "/skills/deploy"
            }))?,
            UserInput::Skill { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<UserInput>(json!({
                "type": "mention",
                "name": "file",
                "path": "src/main.rs"
            }))?,
            UserInput::Mention { .. }
        ));

        // `audio` and `localAudio` are absent from the pinned
        // `#/definitions/v2/UserInput/oneOf`; decoding them must fail instead
        // of producing a payload 0.144.5 cannot deserialize.
        assert!(
            serde_json::from_value::<UserInput>(json!({
                "type": "audio",
                "url": "data:audio/wav;base64,AA=="
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<UserInput>(json!({
                "type": "localAudio",
                "path": "/tmp/a.wav"
            }))
            .is_err()
        );
        Ok(())
    }

    /// 7-05: every branch of the pinned 0.144.5 `v2/ThreadItem` eighteen-way
    /// `oneOf` decodes to its typed model and serializes back to the exact
    /// official wire form (including keys retained through `extra`).
    #[test]
    fn thread_item_decodes_and_round_trips_every_pinned_branch() -> Result<(), serde_json::Error> {
        let extra =
            |key: &str, value: serde_json::Value| [(key.to_owned(), value)].into_iter().collect();
        let cases: Vec<(serde_json::Value, ThreadItem)> = vec![
            (
                json!({
                    "type": "userMessage",
                    "id": "item_user",
                    "clientId": "client-9",
                    "content": [{"type": "text", "text": "hello"}],
                    "futureUserField": true
                }),
                ThreadItem::UserMessage(Box::new(UserMessageThreadItem {
                    id: "item_user".to_owned(),
                    client_id: Some("client-9".to_owned()),
                    content: vec![UserInput::text("hello")],
                    extra: extra("futureUserField", json!(true)),
                })),
            ),
            (
                json!({
                    "type": "hookPrompt",
                    "id": "item_hook",
                    "fragments": [{"hookRunId": "run_1", "text": "review prompt"}]
                }),
                ThreadItem::HookPrompt(Box::new(HookPromptThreadItem {
                    id: "item_hook".to_owned(),
                    fragments: vec![HookPromptFragment {
                        hook_run_id: "run_1".to_owned(),
                        text: "review prompt".to_owned(),
                    }],
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "agentMessage",
                    "id": "item_agent",
                    "text": "the answer",
                    "phase": "final_answer",
                    "memoryCitation": {
                        "entries": [{
                            "lineStart": 3,
                            "lineEnd": 9,
                            "note": "source",
                            "path": "/tmp/a.md"
                        }],
                        "threadIds": ["thr_1", "thr_2"]
                    }
                }),
                ThreadItem::AgentMessage(Box::new(AgentMessageThreadItem {
                    id: "item_agent".to_owned(),
                    text: "the answer".to_owned(),
                    phase: Some(MessagePhase::FinalAnswer),
                    memory_citation: Some(MemoryCitation {
                        entries: vec![MemoryCitationEntry {
                            line_start: 3,
                            line_end: 9,
                            note: "source".to_owned(),
                            path: "/tmp/a.md".to_owned(),
                        }],
                        thread_ids: vec!["thr_1".to_owned(), "thr_2".to_owned()],
                    }),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "plan", "id": "item_plan", "text": "1. read\n2. write"}),
                ThreadItem::Plan(Box::new(PlanThreadItem {
                    id: "item_plan".to_owned(),
                    text: "1. read\n2. write".to_owned(),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "reasoning", "id": "item_reason", "content": ["step one"],
                       "summary": ["because"]}),
                ThreadItem::Reasoning(Box::new(ReasoningThreadItem {
                    id: "item_reason".to_owned(),
                    content: vec!["step one".to_owned()],
                    summary: vec!["because".to_owned()],
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "commandExecution",
                    "id": "item_cmd",
                    "command": "cat notes.md | wc -l",
                    "commandActions": [
                        {"type": "read", "command": "cat", "name": "notes.md",
                         "path": "/tmp/notes.md"},
                        {"type": "listFiles", "command": "ls"}
                    ],
                    "cwd": "/tmp",
                    "status": "completed",
                    "aggregatedOutput": "42 /tmp/notes.md",
                    "durationMs": 1200,
                    "exitCode": 0,
                    "source": "agent"
                }),
                ThreadItem::CommandExecution(Box::new(CommandExecutionThreadItem {
                    id: "item_cmd".to_owned(),
                    command: "cat notes.md | wc -l".to_owned(),
                    command_actions: vec![
                        CommandAction::Read {
                            command: "cat".to_owned(),
                            name: "notes.md".to_owned(),
                            path: "/tmp/notes.md".to_owned(),
                        },
                        CommandAction::ListFiles {
                            command: "ls".to_owned(),
                            path: None,
                        },
                    ],
                    cwd: "/tmp".to_owned(),
                    status: CommandExecutionStatus::Completed,
                    aggregated_output: Some("42 /tmp/notes.md".to_owned()),
                    duration_ms: Some(1200),
                    exit_code: Some(0),
                    process_id: None,
                    source: Some(CommandExecutionSource::Agent),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "fileChange",
                    "id": "item_diff",
                    "status": "completed",
                    "changes": [
                        {"diff": "@@ -1 +1 @@", "kind": {"type": "add"}, "path": "/tmp/new.rs"},
                        {"diff": "@@ -3 +3 @@", "kind": {"type": "update", "move_path": "/tmp/r.rs"},
                         "path": "/tmp/old.rs"}
                    ]
                }),
                ThreadItem::FileChange(Box::new(FileChangeThreadItem {
                    id: "item_diff".to_owned(),
                    changes: vec![
                        FileUpdateChange {
                            diff: "@@ -1 +1 @@".to_owned(),
                            kind: PatchChangeKind::Add,
                            path: "/tmp/new.rs".to_owned(),
                        },
                        FileUpdateChange {
                            diff: "@@ -3 +3 @@".to_owned(),
                            kind: PatchChangeKind::Update {
                                move_path: Some("/tmp/r.rs".to_owned()),
                            },
                            path: "/tmp/old.rs".to_owned(),
                        },
                    ],
                    status: PatchApplyStatus::Completed,
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "mcpToolCall",
                    "id": "item_mcp",
                    "server": "github",
                    "tool": "create_issue",
                    "status": "completed",
                    "arguments": {"title": "fix"},
                    "appContext": {"connectorId": "conn_1", "appName": "GitHub"},
                    "durationMs": 900,
                    "result": {
                        "content": [{"type": "text", "text": "issue #7"}],
                        "structuredContent": {"n": 1},
                        "_meta": {"trace": "t"}
                    }
                }),
                ThreadItem::McpToolCall(Box::new(McpToolCallThreadItem {
                    id: "item_mcp".to_owned(),
                    server: "github".to_owned(),
                    tool: "create_issue".to_owned(),
                    status: McpToolCallStatus::Completed,
                    arguments: json!({"title": "fix"}),
                    app_context: Some(McpToolCallAppContext {
                        connector_id: "conn_1".to_owned(),
                        action_name: None,
                        app_name: Some("GitHub".to_owned()),
                        link_id: None,
                        resource_uri: None,
                        template_id: None,
                    }),
                    duration_ms: Some(900),
                    error: None,
                    mcp_app_resource_uri: None,
                    plugin_id: None,
                    result: Some(McpToolCallResult {
                        content: vec![json!({"type": "text", "text": "issue #7"})],
                        structured_content: Some(json!({"n": 1})),
                        meta: Some(json!({"trace": "t"})),
                    }),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "dynamicToolCall",
                    "id": "item_dyn",
                    "tool": "render_chart",
                    "status": "completed",
                    "arguments": {"kind": "bar"},
                    "namespace": "viz",
                    "success": true,
                    "durationMs": 42,
                    "contentItems": [
                        {"type": "inputText", "text": "chart rendered"},
                        {"type": "inputImage", "imageUrl": "https://example.test/c.png"}
                    ]
                }),
                ThreadItem::DynamicToolCall(Box::new(DynamicToolCallThreadItem {
                    id: "item_dyn".to_owned(),
                    tool: "render_chart".to_owned(),
                    status: DynamicToolCallStatus::Completed,
                    arguments: json!({"kind": "bar"}),
                    content_items: Some(vec![
                        DynamicToolCallOutputContentItem::InputText {
                            text: "chart rendered".to_owned(),
                        },
                        DynamicToolCallOutputContentItem::InputImage {
                            image_url: "https://example.test/c.png".to_owned(),
                        },
                    ]),
                    duration_ms: Some(42),
                    namespace: Some("viz".to_owned()),
                    success: Some(true),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "collabAgentToolCall",
                    "id": "item_collab",
                    "senderThreadId": "thr_main",
                    "receiverThreadIds": ["thr_spawn"],
                    "tool": "spawnAgent",
                    "status": "completed",
                    "agentsStates": {"thr_spawn": {"status": "completed", "message": "done"}},
                    "model": "gpt-5-codex",
                    "prompt": "investigate",
                    "reasoningEffort": "high"
                }),
                ThreadItem::CollabAgentToolCall(Box::new(CollabAgentToolCallThreadItem {
                    id: "item_collab".to_owned(),
                    agents_states: [(
                        "thr_spawn".to_owned(),
                        CollabAgentState {
                            status: CollabAgentStatus::Completed,
                            message: Some("done".to_owned()),
                        },
                    )]
                    .into_iter()
                    .collect(),
                    receiver_thread_ids: vec!["thr_spawn".to_owned()],
                    sender_thread_id: "thr_main".to_owned(),
                    status: CollabAgentToolCallStatus::Completed,
                    tool: CollabAgentTool::SpawnAgent,
                    model: Some("gpt-5-codex".to_owned()),
                    prompt: Some("investigate".to_owned()),
                    reasoning_effort: Some("high".to_owned()),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "subAgentActivity",
                    "id": "item_sub",
                    "agentPath": "/thr_main>thr_spawn",
                    "agentThreadId": "thr_spawn",
                    "kind": "started"
                }),
                ThreadItem::SubAgentActivity(Box::new(SubAgentActivityThreadItem {
                    id: "item_sub".to_owned(),
                    agent_path: "/thr_main>thr_spawn".to_owned(),
                    agent_thread_id: "thr_spawn".to_owned(),
                    kind: SubAgentActivityKind::Started,
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "webSearch",
                    "id": "item_web",
                    "query": "rust serde",
                    "action": {"type": "search", "queries": ["rust serde", "serde json"]}
                }),
                ThreadItem::WebSearch(Box::new(WebSearchThreadItem {
                    id: "item_web".to_owned(),
                    query: "rust serde".to_owned(),
                    action: Some(WebSearchAction::Search {
                        queries: Some(vec!["rust serde".to_owned(), "serde json".to_owned()]),
                        query: None,
                    }),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "imageView", "id": "item_img", "path": "/tmp/shot.png"}),
                ThreadItem::ImageView(Box::new(ImageViewThreadItem {
                    id: "item_img".to_owned(),
                    path: "/tmp/shot.png".to_owned(),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "sleep", "id": "item_sleep", "durationMs": 1500}),
                ThreadItem::Sleep(Box::new(SleepThreadItem {
                    id: "item_sleep".to_owned(),
                    duration_ms: 1500,
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({
                    "type": "imageGeneration",
                    "id": "item_gen",
                    "status": "completed",
                    "result": "/home/codex/sessions/img.png",
                    "revisedPrompt": "a cat",
                    "savedPath": "/home/codex/sessions/img.png"
                }),
                ThreadItem::ImageGeneration(Box::new(ImageGenerationThreadItem {
                    id: "item_gen".to_owned(),
                    result: "/home/codex/sessions/img.png".to_owned(),
                    status: "completed".to_owned(),
                    revised_prompt: Some("a cat".to_owned()),
                    saved_path: Some("/home/codex/sessions/img.png".to_owned()),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "enteredReviewMode", "id": "item_rev_in", "review": "rev_1"}),
                ThreadItem::EnteredReviewMode(Box::new(EnteredReviewModeThreadItem {
                    id: "item_rev_in".to_owned(),
                    review: "rev_1".to_owned(),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "exitedReviewMode", "id": "item_rev_out", "review": "rev_1"}),
                ThreadItem::ExitedReviewMode(Box::new(ExitedReviewModeThreadItem {
                    id: "item_rev_out".to_owned(),
                    review: "rev_1".to_owned(),
                    extra: serde_json::Map::new(),
                })),
            ),
            (
                json!({"type": "contextCompaction", "id": "item_compact"}),
                ThreadItem::ContextCompaction(Box::new(ContextCompactionThreadItem {
                    id: "item_compact".to_owned(),
                    extra: serde_json::Map::new(),
                })),
            ),
        ];
        assert_eq!(
            cases.len(),
            18,
            "the pin enumerates exactly eighteen branches"
        );
        for (wire, expected) in cases {
            let decoded: ThreadItem = serde_json::from_value(wire.clone())?;
            assert_eq!(
                decoded, expected,
                "wire {wire} did not decode to its typed branch model"
            );
            assert_eq!(
                serde_json::to_value(&decoded)?,
                wire,
                "typed branch model did not serialize back to the official form {wire}"
            );
        }
        Ok(())
    }

    /// 7-05: a tag the pin does not enumerate, a payload without a usable
    /// `type` tag, and a known tag whose body no longer matches the pinned
    /// branch shape all degrade to `ThreadItem::Unknown` with the payload
    /// verbatim — never an error that would fail the surrounding decode.
    #[test]
    fn thread_item_unknown_tags_and_malformed_branches_stay_lossless()
    -> Result<(), serde_json::Error> {
        let payloads = [
            json!({"type": "futureThing", "id": "i_1", "blob": {"deep": [1, 2]}}),
            json!({"id": "i_2"}),
            json!({"type": 7, "id": "i_3"}),
            json!("not an object"),
            json!({"type": "agentMessage", "id": "i_4"}),
            json!({"type": "sleep", "id": "i_5", "durationMs": "soon"}),
            json!({"type": "userMessage", "id": "i_6", "content": [
                {"type": "futureInput", "text": "x"}
            ]}),
        ];
        for payload in payloads {
            let decoded: ThreadItem = serde_json::from_value(payload.clone())?;
            match &decoded {
                ThreadItem::Unknown(value) => assert_eq!(*value, payload),
                other => panic!("payload {payload} decoded to a typed branch {other:?}"),
            }
            assert_eq!(
                serde_json::to_value(&decoded)?,
                payload,
                "Unknown must re-serialize the payload byte-for-byte in JSON terms"
            );
        }
        Ok(())
    }

    /// 7-05: the item-carried open string enums keep values a newer
    /// app-server introduces lossless, so an unknown status never degrades
    /// the whole item to `Unknown`.
    #[test]
    fn thread_item_status_enums_decode_known_and_unknown_values() {
        for (wire, expected) in [
            ("commentary", MessagePhase::Commentary),
            ("final_answer", MessagePhase::FinalAnswer),
        ] {
            assert_eq!(MessagePhase::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("inProgress", CommandExecutionStatus::InProgress),
            ("completed", CommandExecutionStatus::Completed),
            ("failed", CommandExecutionStatus::Failed),
            ("declined", CommandExecutionStatus::Declined),
        ] {
            assert_eq!(CommandExecutionStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("agent", CommandExecutionSource::Agent),
            ("userShell", CommandExecutionSource::UserShell),
            (
                "unifiedExecStartup",
                CommandExecutionSource::UnifiedExecStartup,
            ),
            (
                "unifiedExecInteraction",
                CommandExecutionSource::UnifiedExecInteraction,
            ),
        ] {
            assert_eq!(CommandExecutionSource::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("inProgress", PatchApplyStatus::InProgress),
            ("completed", PatchApplyStatus::Completed),
            ("failed", PatchApplyStatus::Failed),
            ("declined", PatchApplyStatus::Declined),
        ] {
            assert_eq!(PatchApplyStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("inProgress", McpToolCallStatus::InProgress),
            ("completed", McpToolCallStatus::Completed),
            ("failed", McpToolCallStatus::Failed),
        ] {
            assert_eq!(McpToolCallStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("inProgress", DynamicToolCallStatus::InProgress),
            ("completed", DynamicToolCallStatus::Completed),
            ("failed", DynamicToolCallStatus::Failed),
        ] {
            assert_eq!(DynamicToolCallStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("inProgress", CollabAgentToolCallStatus::InProgress),
            ("completed", CollabAgentToolCallStatus::Completed),
            ("failed", CollabAgentToolCallStatus::Failed),
        ] {
            assert_eq!(CollabAgentToolCallStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("pendingInit", CollabAgentStatus::PendingInit),
            ("running", CollabAgentStatus::Running),
            ("interrupted", CollabAgentStatus::Interrupted),
            ("completed", CollabAgentStatus::Completed),
            ("errored", CollabAgentStatus::Errored),
            ("shutdown", CollabAgentStatus::Shutdown),
            ("notFound", CollabAgentStatus::NotFound),
        ] {
            assert_eq!(CollabAgentStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("spawnAgent", CollabAgentTool::SpawnAgent),
            ("sendInput", CollabAgentTool::SendInput),
            ("resumeAgent", CollabAgentTool::ResumeAgent),
            ("wait", CollabAgentTool::Wait),
            ("closeAgent", CollabAgentTool::CloseAgent),
        ] {
            assert_eq!(CollabAgentTool::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("started", SubAgentActivityKind::Started),
            ("interacted", SubAgentActivityKind::Interacted),
            ("interrupted", SubAgentActivityKind::Interrupted),
        ] {
            assert_eq!(SubAgentActivityKind::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        // An unknown status stays a typed item carrying the raw value.
        let decoded: ThreadItem = serde_json::from_value(json!({
            "type": "commandExecution",
            "id": "item_cmd",
            "command": "ls",
            "commandActions": [],
            "cwd": "/tmp",
            "status": "queued"
        }))
        .unwrap_or_else(|error| panic!("unknown status must not fail the item: {error}"));
        let ThreadItem::CommandExecution(item) = &decoded else {
            panic!("unexpected branch {decoded:?}");
        };
        assert!(!item.status.is_known());
        assert_eq!(item.status.unknown_value(), Some("queued"));
        assert_eq!(
            serde_json::to_value(&decoded).unwrap_or_else(|error| panic!("{error}"))["status"],
            json!("queued")
        );
    }

    /// 7-05: `Turn.items` and the `item/started`/`item/completed`
    /// notifications carry the typed [`ThreadItem`]; mixed known and unknown
    /// items stay lossless through both surfaces.
    #[test]
    fn turn_and_item_lifecycle_carry_the_typed_thread_item() -> Result<(), serde_json::Error> {
        let turn: Turn = serde_json::from_value(json!({
            "id": "turn_456",
            "items": [
                {"type": "agentMessage", "id": "item_1", "text": "working on it"},
                {"type": "futureThing", "id": "item_2", "kept": [1, 2]}
            ],
            "status": "inProgress"
        }))?;
        assert_eq!(turn.items.len(), 2);
        assert_eq!(
            turn.items[0],
            ThreadItem::AgentMessage(Box::new(AgentMessageThreadItem {
                id: "item_1".to_owned(),
                text: "working on it".to_owned(),
                phase: None,
                memory_citation: None,
                extra: serde_json::Map::new(),
            }))
        );
        assert_eq!(
            turn.items[1],
            ThreadItem::Unknown(json!({"type": "futureThing", "id": "item_2", "kept": [1, 2]}))
        );
        let encoded = serde_json::to_value(&turn)?;
        assert_eq!(encoded["items"][0]["text"], json!("working on it"));
        assert_eq!(encoded["items"][1]["type"], json!("futureThing"));

        let params = json!({
            "threadId": "thr_123",
            "turnId": "turn_456",
            "item": {
                "type": "commandExecution",
                "id": "item_cmd",
                "command": "cargo test",
                "commandActions": [{"type": "unknown", "command": "cargo test"}],
                "cwd": "/src",
                "status": "inProgress"
            },
            "startedAtMs": 1730947200000_i64,
            // The DTO keeps the pinned null-carrying optional keys on the
            // wire, so the round-trip emits this key explicitly.
            "completedAtMs": null
        });
        let notification = decode_notification(
            "item/started".to_owned(),
            Some(params.clone()),
            json!({"method": "item/started"}),
        );
        let Notification::ItemStarted(started) = notification else {
            panic!("expected an item lifecycle notification, got {notification:?}");
        };
        assert_eq!(
            *started,
            ItemLifecycleNotification {
                thread_id: "thr_123".to_owned(),
                turn_id: "turn_456".to_owned(),
                item: ThreadItem::CommandExecution(Box::new(CommandExecutionThreadItem {
                    id: "item_cmd".to_owned(),
                    command: "cargo test".to_owned(),
                    command_actions: vec![CommandAction::Unknown {
                        command: "cargo test".to_owned(),
                    }],
                    cwd: "/src".to_owned(),
                    status: CommandExecutionStatus::InProgress,
                    aggregated_output: None,
                    duration_ms: None,
                    exit_code: None,
                    process_id: None,
                    source: None,
                    extra: serde_json::Map::new(),
                })),
                started_at_ms: Some(1730947200000),
                completed_at_ms: None,
                extra: serde_json::Map::new(),
            }
        );
        assert_eq!(serde_json::to_value(&*started)?, params);
        Ok(())
    }

    #[test]
    fn initialize_capabilities_serialize_exactly_the_four_pinned_properties()
    -> Result<(), serde_json::Error> {
        let capabilities = InitializeCapabilities {
            experimental_api: Omittable::Value(true),
            mcp_server_openai_form_elicitation: Omittable::Value(true),
            opt_out_notification_methods: Omittable::Value(Nullable::Value(vec![
                "thread/started".to_owned(),
            ])),
            request_attestation: Omittable::Value(false),
            extra: serde_json::Map::new(),
        };
        assert_eq!(
            serde_json::to_value(&capabilities)?,
            json!({
                "experimentalApi": true,
                "mcpServerOpenaiFormElicitation": true,
                "optOutNotificationMethods": ["thread/started"],
                "requestAttestation": false,
            })
        );

        // An all-omitted capabilities object sends no keys at all; there is no
        // invented `extensions` escape hatch anymore.
        assert_eq!(
            serde_json::to_value(InitializeCapabilities::default())?,
            json!({})
        );
        Ok(())
    }

    #[test]
    fn initialize_params_serialize_the_pinned_handshake_shape() -> Result<(), serde_json::Error> {
        let params = InitializeParams::new(ClientInfo::new("test", "0.0.0"));
        assert_eq!(
            serde_json::to_value(&params)?,
            json!({"clientInfo": {"name": "test", "version": "0.0.0"}})
        );

        let negotiated = InitializeParams {
            capabilities: Some(InitializeCapabilities {
                experimental_api: true.into(),
                ..InitializeCapabilities::default()
            }),
            ..params
        };
        assert_eq!(
            serde_json::to_value(&negotiated)?,
            json!({
                "clientInfo": {"name": "test", "version": "0.0.0"},
                "capabilities": {"experimentalApi": true},
            })
        );
        Ok(())
    }

    #[test]
    fn initialize_capabilities_keep_future_properties_and_null_losslessly()
    -> Result<(), serde_json::Error> {
        let capabilities: InitializeCapabilities = serde_json::from_value(json!({
            "experimentalApi": true,
            "optOutNotificationMethods": null,
            "futureCapability": {"nested": [1, 2]},
        }))?;
        assert_eq!(capabilities.experimental_api, Omittable::Value(true));
        assert_eq!(
            capabilities.opt_out_notification_methods,
            Omittable::Value(Nullable::Null)
        );
        assert_eq!(
            serde_json::to_value(&capabilities)?,
            json!({
                "experimentalApi": true,
                "optOutNotificationMethods": null,
                "futureCapability": {"nested": [1, 2]},
            })
        );
        Ok(())
    }

    #[test]
    fn unknown_notification_keeps_entire_envelope() {
        let raw = json!({
            "method": "future/event",
            "params": {"new": [1, 2, 3]},
            "futureEnvelopeField": true
        });
        let notification = decode_notification(
            "future/event".to_owned(),
            raw.get("params").cloned(),
            raw.clone(),
        );
        match notification {
            Notification::Unknown(unknown) => assert_eq!(unknown.raw, raw),
            other => panic!("expected unknown notification, got {other:?}"),
        }
    }

    #[test]
    fn known_notification_decode_failure_emits_warn() {
        let subscriber = WarnCapture::default();
        let _guard = tracing::subscriber::set_default(subscriber.clone());
        let notification = decode_notification(
            "turn/completed".to_owned(),
            Some(json!({"not":"a turn"})),
            json!({"method":"turn/completed"}),
        );
        assert!(matches!(notification, Notification::Unknown(_)));
        let events = subscriber.messages();
        assert!(events.iter().any(|message| {
            message.contains("typed decode failed for known app-server notification")
        }));
        assert!(
            events
                .iter()
                .any(|message| message.contains("turn/completed"))
        );
        assert!(
            !events
                .iter()
                .any(|message| message.contains("a turn") || message.contains("\"not\""))
        );
    }

    /// 8-12: the `account/login/completed` branch decodes the success shape
    /// through `decode_notification` and re-encodes the pinned envelope
    /// losslessly (future keys included).
    #[test]
    fn account_login_completed_notification_decodes_and_round_trips()
    -> Result<(), serde_json::Error> {
        let params = json!({
            "loginId": "login-browser",
            "success": true,
            "error": null,
            "futureLoginField": {"kept": [1, 2]}
        });
        let notification = decode_notification(
            "account/login/completed".to_owned(),
            Some(params.clone()),
            json!({"method": "account/login/completed"}),
        );
        let Notification::AccountLoginCompleted(completed) = notification else {
            panic!("expected a login-completed notification, got {notification:?}");
        };
        assert_eq!(
            *completed,
            AccountLoginCompletedNotification {
                login_id: Some("login-browser".to_owned()),
                success: true,
                error: None,
                extra: json!({"futureLoginField": {"kept": [1, 2]}})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            }
        );
        assert_eq!(serde_json::to_value(&*completed)?, params);
        Ok(())
    }

    /// 8-12: the `account/rateLimits/updated` branch decodes a typed
    /// [`RateLimitSnapshot`] and re-encodes the pinned envelope losslessly.
    #[test]
    fn account_rate_limits_updated_notification_decodes_and_round_trips()
    -> Result<(), serde_json::Error> {
        let params = json!({
            "rateLimits": {
                "limitId": "codex",
                "limitName": null,
                "primary": {
                    "usedPercent": 25,
                    "windowDurationMins": 15,
                    "resetsAt": 1730947200
                },
                "secondary": null,
                "credits": null,
                "planType": "future_plan",
                "rateLimitReachedType": "future_state"
            }
        });
        let notification = decode_notification(
            "account/rateLimits/updated".to_owned(),
            Some(params.clone()),
            json!({"method": "account/rateLimits/updated"}),
        );
        let Notification::AccountRateLimitsUpdated(updated) = notification else {
            panic!("expected a rate-limits notification, got {notification:?}");
        };
        assert_eq!(updated.rate_limits.limit_id.as_deref(), Some("codex"));
        assert_eq!(
            updated
                .rate_limits
                .primary
                .as_ref()
                .map(|window| window.used_percent),
            Some(25)
        );
        assert_eq!(
            updated.rate_limits.plan_type,
            Some(PlanType::from_raw("future_plan"))
        );
        assert_eq!(serde_json::to_value(&*updated)?, params);
        Ok(())
    }

    /// 8-12: the `thread/started` branch decodes the typed [`Thread`] and
    /// re-encodes the pinned envelope losslessly.
    #[test]
    fn thread_started_notification_decodes_and_round_trips() -> Result<(), serde_json::Error> {
        // The DTO keeps the pinned null-carrying optional keys on the wire, so
        // the round-trip emits them explicitly. `status`/`source`/
        // `threadSource` ride along typed (13-O-2).
        let params = json!({
            "thread": {
                "id": "thr_123",
                "sessionId": "thr_123",
                "preview": null,
                "ephemeral": null,
                "modelProvider": null,
                "createdAt": null,
                "updatedAt": null,
                "cwd": null,
                "name": null,
                "turns": null,
                "status": {"type": "active", "activeFlags": ["waitingOnApproval", "waitingOnUserInput"]},
                "source": "cli",
                "threadSource": "subAgentThreadSpawn",
                "cliVersion": "0.42.0",
                "futureThreadField": true
            }
        });
        let notification = decode_notification(
            "thread/started".to_owned(),
            Some(params.clone()),
            json!({"method": "thread/started"}),
        );
        let Notification::ThreadStarted(started) = notification else {
            panic!("expected a thread-started notification, got {notification:?}");
        };
        assert_eq!(
            *started,
            ThreadStartedNotification {
                thread: Thread {
                    id: "thr_123".to_owned(),
                    session_id: Some("thr_123".to_owned()),
                    preview: None,
                    ephemeral: None,
                    model_provider: None,
                    created_at: None,
                    updated_at: None,
                    cwd: None,
                    name: None,
                    turns: None,
                    status: Some(ThreadStatus::Active(ActiveThreadStatus {
                        active_flags: vec![
                            ThreadActiveFlag::WaitingOnApproval,
                            ThreadActiveFlag::WaitingOnUserInput
                        ],
                        extra: serde_json::Map::new(),
                    })),
                    source: Some(SessionSource::Mode(SessionSourceMode::Cli)),
                    thread_source: Some(ThreadSourceKind::SubAgentThreadSpawn),
                    cli_version: Some("0.42.0".to_owned()),
                    extra: json!({"futureThreadField": true})
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                },
                extra: serde_json::Map::new(),
            }
        );
        assert_eq!(serde_json::to_value(&*started)?, params);
        Ok(())
    }

    /// 13-O-1: a `thread/start` response exposes the negotiated approval
    /// policy, approval reviewer, sandbox policy, and reasoning effort under
    /// their pinned wire keys, and re-encodes the payload losslessly.
    #[test]
    fn thread_start_response_decodes_the_negotiated_approval_and_sandbox_fields()
    -> Result<(), serde_json::Error> {
        let params = json!({
            "thread": {
                "id": "thr_123",
                "sessionId": null,
                "preview": null,
                "ephemeral": null,
                "modelProvider": null,
                "createdAt": null,
                "updatedAt": null,
                "cwd": null,
                "name": null,
                "turns": null,
                "status": null,
                "source": null,
                "threadSource": null,
                "cliVersion": null
            },
            "model": "gpt-5-codex",
            "modelProvider": "openai",
            "cwd": "/tmp",
            "instructionSources": [],
            "serviceTier": "flex",
            "approvalPolicy": {
                "granular": {
                    "mcp_elicitations": false,
                    "rules": true,
                    "sandbox_approval": true
                }
            },
            "approvalsReviewer": "auto_review",
            "sandbox": {
                "type": "workspaceWrite",
                "writableRoots": ["/w"],
                "networkAccess": false
            },
            "reasoningEffort": "medium"
        });
        let response: ThreadStartResponse = serde_json::from_value(params.clone())?;
        assert_eq!(
            response.approval_policy,
            Some(AskForApproval::Granular(GranularAskForApproval::new(
                false, true, true
            )))
        );
        assert_eq!(
            response.approvals_reviewer,
            Some(ApprovalsReviewer::AutoReview)
        );
        assert_eq!(
            response.sandbox,
            Some(SandboxPolicy::WorkspaceWrite {
                writable_roots: Some(vec![PathBuf::from("/w")]),
                network_access: Some(false),
                exclude_slash_tmp: None,
                exclude_tmpdir_env_var: None,
                extra: serde_json::Map::new(),
            })
        );
        assert_eq!(response.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(serde_json::to_value(&response)?, params);
        Ok(())
    }

    /// 13-O-1: a sandbox branch or approval-policy shape the pin has not
    /// named degrades to the lossless Unknown variants instead of failing the
    /// `thread/start` response decode.
    #[test]
    fn thread_start_response_degrades_unknown_approval_and_sandbox_shapes()
    -> Result<(), serde_json::Error> {
        let params = json!({
            "thread": {"id": "thr_123"},
            "model": "gpt-5-codex",
            "modelProvider": "openai",
            "cwd": "/tmp",
            "approvalPolicy": {"policy": {"mode": "future"}},
            "approvalsReviewer": "future_reviewer",
            "sandbox": {"type": "gpuSandbox", "isolation": "m1"}
        });
        let response: ThreadStartResponse = serde_json::from_value(params.clone())?;
        assert_eq!(
            response.approval_policy,
            Some(AskForApproval::Unknown(
                json!({"policy": {"mode": "future"}})
            ))
        );
        assert_eq!(
            response.approvals_reviewer,
            Some(ApprovalsReviewer::Unknown("future_reviewer".into()))
        );
        assert_eq!(
            response.sandbox,
            Some(SandboxPolicy::Unknown(
                json!({"type": "gpuSandbox", "isolation": "m1"})
            ))
        );
        // The unknown payloads re-encode byte-for-byte, so a client that
        // echoes the negotiated posture back stays conforming.
        let encoded = serde_json::to_value(&response)?;
        assert_eq!(
            encoded["approvalPolicy"],
            json!({"policy": {"mode": "future"}})
        );
        assert_eq!(encoded["approvalsReviewer"], json!("future_reviewer"));
        assert_eq!(
            encoded["sandbox"],
            json!({"type": "gpuSandbox", "isolation": "m1"})
        );
        Ok(())
    }

    /// 13-O-2: [`ThreadStatus`] types every branch of the pinned
    /// `v2/ThreadStatus` union, keeps the `activeFlags` array (including
    /// flags this crate has not named and future branch properties), and
    /// degrades unknown branches losslessly.
    #[test]
    fn thread_status_types_every_pinned_branch_and_stays_lossless() -> Result<(), serde_json::Error>
    {
        let cases = [
            (json!({"type": "notLoaded"}), ThreadStatus::NotLoaded),
            (json!({"type": "idle"}), ThreadStatus::Idle),
            (json!({"type": "systemError"}), ThreadStatus::SystemError),
            (
                json!({"type": "active", "activeFlags": ["waitingOnApproval"]}),
                ThreadStatus::Active(ActiveThreadStatus {
                    active_flags: vec![ThreadActiveFlag::WaitingOnApproval],
                    extra: serde_json::Map::new(),
                }),
            ),
            (
                json!({
                    "type": "active",
                    "activeFlags": ["waitingOnUserInput", "futureFlag"],
                    "futureActiveField": true
                }),
                ThreadStatus::Active(ActiveThreadStatus {
                    active_flags: vec![
                        ThreadActiveFlag::WaitingOnUserInput,
                        ThreadActiveFlag::Unknown("futureFlag".into()),
                    ],
                    extra: json!({"futureActiveField": true})
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                }),
            ),
        ];
        for (wire, expected) in cases {
            assert_eq!(
                serde_json::from_value::<ThreadStatus>(wire.clone())?,
                expected
            );
            assert_eq!(serde_json::to_value(&expected)?, wire);
        }

        // A status branch the pin has not named, and a payload without a
        // usable `type` tag, both stay verbatim instead of failing the
        // surrounding response.
        for unmodelled in [
            json!({"type": "waitingOnModel", "detail": "soon"}),
            json!("idle"),
            json!({"activeFlags": []}),
            json!({"type": 7}),
        ] {
            let decoded = serde_json::from_value::<ThreadStatus>(unmodelled.clone())
                .expect("an unmodelled thread status decodes losslessly");
            assert_eq!(decoded, ThreadStatus::Unknown(unmodelled.clone()));
            assert_eq!(serde_json::to_value(&decoded)?, unmodelled);
        }
        Ok(())
    }

    /// 13-O-2: [`SessionSource`] types all three pinned branches — the string
    /// enum, `custom`, and the nested `subAgent` union — and keeps unknown
    /// shapes lossless, including objects the pin's
    /// `additionalProperties: false` forbids.
    #[test]
    fn thread_source_types_every_pinned_branch_and_stays_lossless() -> Result<(), serde_json::Error>
    {
        let cases = [
            (json!("cli"), SessionSource::Mode(SessionSourceMode::Cli)),
            (
                json!("vscode"),
                SessionSource::Mode(SessionSourceMode::Vscode),
            ),
            (json!("exec"), SessionSource::Mode(SessionSourceMode::Exec)),
            (
                json!("appServer"),
                SessionSource::Mode(SessionSourceMode::AppServer),
            ),
            (
                json!("unknown"),
                SessionSource::Mode(SessionSourceMode::UnknownOrigin),
            ),
            (
                json!("future-origin"),
                SessionSource::Mode(SessionSourceMode::Unknown("future-origin".into())),
            ),
            (
                json!({"custom": "neovim"}),
                SessionSource::Custom("neovim".to_owned()),
            ),
            (
                json!({"subAgent": "review"}),
                SessionSource::SubAgent(SubAgentSource::Kind(SubAgentSourceKind::Review)),
            ),
            (
                json!({"subAgent": "memory_consolidation"}),
                SessionSource::SubAgent(SubAgentSource::Kind(
                    SubAgentSourceKind::MemoryConsolidation,
                )),
            ),
            (
                json!({"subAgent": {"other": "custom agent"}}),
                SessionSource::SubAgent(SubAgentSource::Other("custom agent".to_owned())),
            ),
            (
                json!({"subAgent": {"thread_spawn": {
                    "depth": 2,
                    "parent_thread_id": "thr_parent",
                    "agent_nickname": "scout",
                    "agent_path": "/agents/scout.toml",
                    "agent_role": "explorer"
                }}}),
                SessionSource::SubAgent(SubAgentSource::ThreadSpawn(ThreadSpawnSubAgentSource {
                    depth: 2,
                    parent_thread_id: "thr_parent".to_owned(),
                    agent_nickname: Some("scout".to_owned()),
                    agent_path: Some("/agents/scout.toml".to_owned()),
                    agent_role: Some("explorer".to_owned()),
                })),
            ),
        ];
        for (wire, expected) in cases {
            assert_eq!(
                serde_json::from_value::<SessionSource>(wire.clone())?,
                expected
            );
            assert_eq!(serde_json::to_value(&expected)?, wire);
        }

        // An unnamed single-key branch, a multi-key object the pin forbids,
        // and a non-object payload all stay verbatim in Unknown.
        for unmodelled in [
            json!({"futureBranch": 1}),
            json!({"custom": "neovim", "subAgent": "cli"}),
            json!(7),
        ] {
            let decoded = serde_json::from_value::<SessionSource>(unmodelled.clone())
                .expect("an unmodelled session source decodes losslessly");
            assert_eq!(decoded, SessionSource::Unknown(unmodelled.clone()));
            assert_eq!(serde_json::to_value(&decoded)?, unmodelled);
        }

        // A `thread_spawn` body that no longer matches the pinned shape
        // degrades the same way instead of failing the thread decode.
        let malformed = json!({"subAgent": {"thread_spawn": {"depth": "two"}}});
        let decoded = serde_json::from_value::<SessionSource>(malformed.clone())
            .expect("a malformed thread_spawn branch degrades losslessly");
        assert_eq!(
            decoded,
            SessionSource::SubAgent(SubAgentSource::Unknown(
                json!({"thread_spawn": {"depth": "two"}})
            ))
        );
        assert_eq!(serde_json::to_value(&decoded)?, malformed);
        Ok(())
    }

    /// 13-O-2: `threadSource` types the ten classifications the pin's
    /// `v2/ThreadSourceKind` enumerates and keeps any other string verbatim.
    #[test]
    fn thread_source_kind_types_the_pinned_classifications() {
        for (wire, expected) in [
            ("cli", ThreadSourceKind::Cli),
            ("vscode", ThreadSourceKind::Vscode),
            ("exec", ThreadSourceKind::Exec),
            ("appServer", ThreadSourceKind::AppServer),
            ("subAgent", ThreadSourceKind::SubAgent),
            ("subAgentReview", ThreadSourceKind::SubAgentReview),
            ("subAgentCompact", ThreadSourceKind::SubAgentCompact),
            ("subAgentThreadSpawn", ThreadSourceKind::SubAgentThreadSpawn),
            ("subAgentOther", ThreadSourceKind::SubAgentOther),
            ("unknown", ThreadSourceKind::UnknownKind),
        ] {
            assert_eq!(ThreadSourceKind::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        let future = ThreadSourceKind::from_raw("futureKind");
        assert_eq!(future.as_str(), "futureKind");
        assert_eq!(future.unknown_value(), Some("futureKind"));
    }

    /// 13-O-3: `itemsView` decodes each pinned value, keeps unknown values
    /// lossless, and treats an absent key as the pinned `full` default.
    #[test]
    fn turn_items_view_decodes_known_values_and_defaults_to_full() -> Result<(), serde_json::Error>
    {
        for (wire, expected) in [
            ("notLoaded", TurnItemsView::NotLoaded),
            ("summary", TurnItemsView::Summary),
            ("full", TurnItemsView::Full),
        ] {
            assert_eq!(TurnItemsView::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        assert_eq!(TurnItemsView::from_raw("futureView").as_str(), "futureView");

        let summarized: Turn = serde_json::from_value(json!({
            "id": "turn_1",
            "items": [],
            "status": "inProgress",
            "itemsView": "summary"
        }))?;
        assert_eq!(summarized.items_view, Some(TurnItemsView::Summary));
        assert_eq!(
            serde_json::to_value(&summarized)?["itemsView"],
            json!("summary")
        );

        // Absent stays default-tolerant: the key is not required and reads
        // back as the pinned `full` default.
        let bare: Turn =
            serde_json::from_value(json!({"id": "turn_2", "items": [], "status": "completed"}))?;
        assert_eq!(bare.items_view, None);
        assert_eq!(bare.items_view(), TurnItemsView::Full);
        Ok(())
    }

    /// 8-12: the `turn/started` branch decodes the typed [`Turn`] and
    /// re-encodes the pinned envelope losslessly.
    #[test]
    fn turn_started_notification_decodes_and_round_trips() -> Result<(), serde_json::Error> {
        // The DTO keeps the pinned null-carrying optional keys on the wire, so
        // the round-trip emits them explicitly; `itemsView` rides along typed
        // (13-O-3).
        let params = json!({
            "threadId": "thr_123",
            "turn": {
                "id": "turn_456",
                "items": [],
                "status": "inProgress",
                "itemsView": "summary",
                "error": null,
                "startedAt": null,
                "completedAt": null,
                "durationMs": null,
                "futureTurnField": 7
            }
        });
        let notification = decode_notification(
            "turn/started".to_owned(),
            Some(params.clone()),
            json!({"method": "turn/started"}),
        );
        let Notification::TurnStarted(started) = notification else {
            panic!("expected a turn-started notification, got {notification:?}");
        };
        assert_eq!(started.thread_id, "thr_123");
        assert_eq!(started.turn.id, "turn_456");
        assert_eq!(started.turn.status, TurnStatus::InProgress);
        assert_eq!(started.turn.items_view, Some(TurnItemsView::Summary));
        assert_eq!(started.turn.items_view(), TurnItemsView::Summary);
        assert_eq!(started.turn.extra["futureTurnField"], json!(7));
        assert_eq!(serde_json::to_value(&*started)?, params);
        Ok(())
    }

    /// 8-12: `item/agentMessage/delta` is the only channel for incremental
    /// agent text — decode through `decode_notification` and re-encode the
    /// pinned envelope losslessly.
    #[test]
    fn agent_message_delta_notification_decodes_and_round_trips() -> Result<(), serde_json::Error> {
        let params = json!({
            "threadId": "thr_123",
            "turnId": "turn_456",
            "itemId": "item_1",
            "delta": " incre",
            "futureDeltaField": true
        });
        let notification = decode_notification(
            "item/agentMessage/delta".to_owned(),
            Some(params.clone()),
            json!({"method": "item/agentMessage/delta"}),
        );
        let Notification::AgentMessageDelta(delta) = notification else {
            panic!("expected an agent-message delta, got {notification:?}");
        };
        assert_eq!(
            *delta,
            AgentMessageDeltaNotification {
                thread_id: "thr_123".to_owned(),
                turn_id: "turn_456".to_owned(),
                item_id: "item_1".to_owned(),
                delta: " incre".to_owned(),
                extra: json!({"futureDeltaField": true})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            }
        );
        assert_eq!(serde_json::to_value(&*delta)?, params);
        Ok(())
    }

    #[derive(Clone, Default)]
    struct WarnCapture {
        messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl WarnCapture {
        fn messages(&self) -> Vec<String> {
            self.messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl tracing::Subscriber for WarnCapture {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::TRACE)
        }

        fn new_span(&self, _attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = MessageVisitor(String::new());
            event.record(&mut visitor);
            self.messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(visitor.0);
        }

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    struct MessageVisitor(String);

    impl tracing::field::Visit for MessageVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(field.name());
            self.0.push('=');
            self.0.push_str(&format!("{value:?}"));
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(field.name());
            self.0.push('=');
            self.0.push_str(value);
        }
    }

    /// 3-02: `thread/start` enum parameters cover the pinned wire domains and
    /// unknown values round-trip losslessly.
    #[test]
    fn thread_start_params_serialize_typed_enum_domains() -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            ("untrusted", AskForApprovalMode::Untrusted),
            ("on-request", AskForApprovalMode::OnRequest),
            ("never", AskForApprovalMode::Never),
        ] {
            assert_eq!(AskForApprovalMode::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("read-only", SandboxMode::ReadOnly),
            ("workspace-write", SandboxMode::WorkspaceWrite),
            ("danger-full-access", SandboxMode::DangerFullAccess),
        ] {
            assert_eq!(SandboxMode::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("none", Personality::None),
            ("friendly", Personality::Friendly),
            ("pragmatic", Personality::Pragmatic),
        ] {
            assert_eq!(Personality::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let params = ThreadStartParams {
            approval_policy: Some(AskForApproval::Mode(AskForApprovalMode::OnRequest)),
            sandbox: Some(SandboxMode::WorkspaceWrite),
            personality: Some(Personality::Pragmatic),
            ..ThreadStartParams::default()
        };
        assert_eq!(
            serde_json::to_value(&params)?,
            json!({
                "approvalPolicy": "on-request",
                "sandbox": "workspace-write",
                "personality": "pragmatic"
            })
        );

        let future = ThreadStartParams {
            sandbox: Some(SandboxMode::from_raw("future-sandbox")),
            personality: Some(Personality::from_raw("future-persona")),
            approval_policy: Some(AskForApproval::Mode(AskForApprovalMode::from_raw(
                "future-policy",
            ))),
            ..ThreadStartParams::default()
        };
        let encoded = serde_json::to_value(&future)?;
        assert_eq!(encoded["sandbox"], json!("future-sandbox"));
        assert_eq!(encoded["personality"], json!("future-persona"));
        assert_eq!(encoded["approvalPolicy"], json!("future-policy"));
        assert_eq!(
            serde_json::from_value::<ThreadStartParams>(encoded)?,
            future
        );
        Ok(())
    }

    /// 3-02: the granular object branch of `v2/AskForApproval` keeps the
    /// pinned `{"granular": {...}}` wrapper with snake_case settings keys.
    #[test]
    fn approval_policy_granular_branch_matches_the_pinned_wire_shape()
    -> Result<(), serde_json::Error> {
        let granular = GranularAskForApproval::new(true, false, true)
            .with_request_permissions(true)
            .with_skill_approval(false);
        let policy = AskForApproval::Granular(granular);
        let encoded = serde_json::to_value(&policy)?;
        assert_eq!(
            encoded,
            json!({
                "granular": {
                    "mcp_elicitations": true,
                    "rules": false,
                    "sandbox_approval": true,
                    "request_permissions": true,
                    "skill_approval": false
                }
            })
        );
        assert_eq!(serde_json::from_value::<AskForApproval>(encoded)?, policy);

        // Optional flags are omitted entirely when unset; the pin defaults
        // both to `false` on the app-server side.
        let minimal = AskForApproval::Granular(GranularAskForApproval::new(false, true, false));
        let encoded = serde_json::to_value(&minimal)?;
        assert_eq!(
            encoded,
            json!({
                "granular": {
                    "mcp_elicitations": false,
                    "rules": true,
                    "sandbox_approval": false
                }
            })
        );
        assert_eq!(serde_json::from_value::<AskForApproval>(encoded)?, minimal);

        // The granular policy nests inside `thread/start` exactly like the
        // string branch.
        let params = ThreadStartParams {
            approval_policy: Some(policy),
            ..ThreadStartParams::default()
        };
        let encoded = serde_json::to_value(&params)?;
        assert_eq!(
            encoded["approvalPolicy"]["granular"]["sandbox_approval"],
            json!(true)
        );
        assert_eq!(
            serde_json::from_value::<ThreadStartParams>(encoded)?,
            params
        );

        // 13-O-1: an object without the pinned `granular` key is a future
        // policy shape, so it stays verbatim in the Unknown variant instead
        // of failing the surrounding response decode.
        let future = json!({"policy": {"mode": "future"}});
        let decoded = serde_json::from_value::<AskForApproval>(future.clone())
            .expect("an unmodelled approval policy decodes losslessly");
        assert_eq!(decoded, AskForApproval::Unknown(future.clone()));
        assert_eq!(serde_json::to_value(&decoded)?, future);
        Ok(())
    }

    /// 3-02: `turn/start` enum parameters cover the pinned wire domains;
    /// `effort` stays a plain string because the pinned `v2/ReasoningEffort`
    /// enumerates no values.
    #[test]
    fn turn_start_params_serialize_typed_enum_domains() -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            ("auto", ReasoningSummary::Auto),
            ("concise", ReasoningSummary::Concise),
            ("detailed", ReasoningSummary::Detailed),
            ("none", ReasoningSummary::None),
        ] {
            assert_eq!(ReasoningSummary::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let params = TurnStartParams {
            summary: Some(ReasoningSummary::Concise),
            personality: Some(Personality::None),
            effort: Some("minimal".to_owned()),
            ..TurnStartParams::text("thr_123", "hello")
        };
        let encoded = serde_json::to_value(&params)?;
        assert_eq!(encoded["summary"], json!("concise"));
        assert_eq!(encoded["personality"], json!("none"));
        assert_eq!(encoded["effort"], json!("minimal"));

        let future = TurnStartParams {
            summary: Some(ReasoningSummary::from_raw("future-summary")),
            personality: Some(Personality::from_raw("future-persona")),
            ..TurnStartParams::text("thr_123", "hello")
        };
        let encoded = serde_json::to_value(&future)?;
        assert_eq!(encoded["summary"], json!("future-summary"));
        assert_eq!(encoded["personality"], json!("future-persona"));
        assert_eq!(serde_json::from_value::<TurnStartParams>(encoded)?, future);
        Ok(())
    }

    /// 3-22: `image`/`localImage` `detail` covers the four pinned values and
    /// keeps unknown fidelity strings losslessly.
    #[test]
    fn image_detail_covers_the_pinned_domain_and_keeps_unknowns() -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            ("auto", ImageDetail::Auto),
            ("low", ImageDetail::Low),
            ("high", ImageDetail::High),
            ("original", ImageDetail::Original),
        ] {
            assert_eq!(ImageDetail::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let image = UserInput::Image {
            url: "https://example.test/a.png".to_owned(),
            detail: Some(ImageDetail::from_raw("ultra")),
        };
        let encoded = serde_json::to_value(&image)?;
        assert_eq!(encoded["detail"], json!("ultra"));
        assert_eq!(serde_json::from_value::<UserInput>(encoded)?, image);

        let local = UserInput::LocalImage {
            path: PathBuf::from("/tmp/a.png"),
            detail: Some(ImageDetail::Original),
        };
        let encoded = serde_json::to_value(&local)?;
        assert_eq!(
            encoded,
            json!({"type": "localImage", "path": "/tmp/a.png", "detail": "original"})
        );
        assert_eq!(serde_json::from_value::<UserInput>(encoded)?, local);
        Ok(())
    }

    /// 3-23: receive-side closed enums decode their pinned domains and keep
    /// unknown values from a newer app-server losslessly.
    #[test]
    fn receive_side_enums_decode_known_and_unknown_values() -> Result<(), serde_json::Error> {
        let turn: Turn = serde_json::from_value(json!({
            "id": "turn_1", "items": [], "status": "inProgress"
        }))?;
        assert_eq!(turn.status, TurnStatus::InProgress);
        for (wire, expected) in [
            ("completed", TurnStatus::Completed),
            ("interrupted", TurnStatus::Interrupted),
            ("failed", TurnStatus::Failed),
            ("inProgress", TurnStatus::InProgress),
        ] {
            assert_eq!(TurnStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        let turn: Turn =
            serde_json::from_value(json!({"id": "turn_1", "items": [], "status": "queued"}))?;
        assert!(!turn.status.is_known());
        assert_eq!(turn.status.unknown_value(), Some("queued"));
        let encoded = serde_json::to_value(&turn)?;
        assert_eq!(encoded["status"], json!("queued"));

        for (wire, expected) in [
            ("canceled", CancelLoginStatus::Canceled),
            ("notFound", CancelLoginStatus::NotFound),
        ] {
            assert_eq!(CancelLoginStatus::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        let canceled: CancelLoginResponse = serde_json::from_value(json!({"status": "notFound"}))?;
        assert_eq!(canceled.status, CancelLoginStatus::NotFound);

        for (wire, expected) in [
            ("free", PlanType::Free),
            ("go", PlanType::Go),
            ("plus", PlanType::Plus),
            ("pro", PlanType::Pro),
            ("prolite", PlanType::Prolite),
            ("team", PlanType::Team),
            (
                "self_serve_business_usage_based",
                PlanType::SelfServeBusinessUsageBased,
            ),
            ("business", PlanType::Business),
            (
                "enterprise_cbp_usage_based",
                PlanType::EnterpriseCbpUsageBased,
            ),
            ("enterprise", PlanType::Enterprise),
            ("edu", PlanType::Edu),
            ("unknown", PlanType::UnknownPlan),
        ] {
            assert_eq!(PlanType::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        for (wire, expected) in [
            ("rate_limit_reached", RateLimitReachedType::RateLimitReached),
            (
                "workspace_owner_credits_depleted",
                RateLimitReachedType::WorkspaceOwnerCreditsDepleted,
            ),
            (
                "workspace_member_credits_depleted",
                RateLimitReachedType::WorkspaceMemberCreditsDepleted,
            ),
            (
                "workspace_owner_usage_limit_reached",
                RateLimitReachedType::WorkspaceOwnerUsageLimitReached,
            ),
            (
                "workspace_member_usage_limit_reached",
                RateLimitReachedType::WorkspaceMemberUsageLimitReached,
            ),
        ] {
            assert_eq!(RateLimitReachedType::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let snapshot: RateLimitSnapshot = serde_json::from_value(json!({
            "planType": "future_plan",
            "rateLimitReachedType": "future_state"
        }))?;
        assert_eq!(snapshot.plan_type, Some(PlanType::from_raw("future_plan")));
        assert_eq!(
            snapshot.rate_limit_reached_type,
            Some(RateLimitReachedType::from_raw("future_state"))
        );
        let encoded = serde_json::to_value(&snapshot)?;
        assert_eq!(encoded["planType"], json!("future_plan"));
        assert_eq!(encoded["rateLimitReachedType"], json!("future_state"));

        for (wire, expected) in [
            ("apikey", AuthMode::ApiKey),
            ("chatgpt", AuthMode::Chatgpt),
            ("chatgptAuthTokens", AuthMode::ChatgptAuthTokens),
            ("headers", AuthMode::Headers),
            ("agentIdentity", AuthMode::AgentIdentity),
            ("personalAccessToken", AuthMode::PersonalAccessToken),
            ("bedrockApiKey", AuthMode::BedrockApiKey),
        ] {
            assert_eq!(AuthMode::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        let updated: AccountUpdatedNotification =
            serde_json::from_value(json!({"authMode": "chatgptAuthTokens", "planType": "edu"}))?;
        assert_eq!(updated.auth_mode, Some(AuthMode::ChatgptAuthTokens));
        assert_eq!(updated.plan_type, Some(PlanType::Edu));

        let updated: AccountUpdatedNotification =
            serde_json::from_value(json!({"authMode": "futureMode"}))?;
        assert_eq!(
            updated.auth_mode.as_ref().map(|mode| mode.as_str()),
            Some("futureMode")
        );
        let encoded = serde_json::to_value(&updated)?;
        assert_eq!(encoded["authMode"], json!("futureMode"));
        Ok(())
    }

    /// 4-37: the string branch of `v2/CodexErrorInfo` covers the eleven pinned
    /// codes and keeps codes from a newer app-server losslessly.
    #[test]
    fn codex_error_info_string_branch_covers_the_pinned_domain_and_keeps_unknowns()
    -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            (
                "contextWindowExceeded",
                CodexErrorCode::ContextWindowExceeded,
            ),
            (
                "sessionBudgetExceeded",
                CodexErrorCode::SessionBudgetExceeded,
            ),
            ("usageLimitExceeded", CodexErrorCode::UsageLimitExceeded),
            ("serverOverloaded", CodexErrorCode::ServerOverloaded),
            ("cyberPolicy", CodexErrorCode::CyberPolicy),
            ("internalServerError", CodexErrorCode::InternalServerError),
            ("unauthorized", CodexErrorCode::Unauthorized),
            ("badRequest", CodexErrorCode::BadRequest),
            ("threadRollbackFailed", CodexErrorCode::ThreadRollbackFailed),
            ("sandboxError", CodexErrorCode::SandboxError),
            ("other", CodexErrorCode::Other),
        ] {
            assert_eq!(CodexErrorCode::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
            assert_eq!(
                serde_json::from_value::<CodexErrorInfo>(json!(wire))?,
                CodexErrorInfo::Code(expected)
            );
        }

        let unknown: CodexErrorInfo = serde_json::from_value(json!("futureCode"))?;
        assert_eq!(
            unknown,
            CodexErrorInfo::Code(CodexErrorCode::from_raw("futureCode"))
        );
        assert_eq!(serde_json::to_value(&unknown)?, json!("futureCode"));
        Ok(())
    }

    /// 4-37: the five object variants of `v2/CodexErrorInfo` keep the pinned
    /// single-key wrapper shape and an optional nullable `httpStatusCode`.
    #[test]
    fn codex_error_info_object_variants_match_the_pinned_wire_shape()
    -> Result<(), serde_json::Error> {
        let forwarded = |code: Option<u16>| ForwardedHttpStatus {
            http_status_code: code,
        };
        let cases = [
            (
                json!({"httpConnectionFailed": {"httpStatusCode": 503}}),
                CodexErrorInfo::HttpConnectionFailed {
                    http_connection_failed: forwarded(Some(503)),
                },
            ),
            (
                json!({"responseStreamDisconnected": {}}),
                CodexErrorInfo::ResponseStreamDisconnected {
                    response_stream_disconnected: forwarded(None),
                },
            ),
            (
                json!({"responseTooManyFailedAttempts": {"httpStatusCode": 429}}),
                CodexErrorInfo::ResponseTooManyFailedAttempts {
                    response_too_many_failed_attempts: forwarded(Some(429)),
                },
            ),
            (
                json!({"activeTurnNotSteerable": {"turnKind": "review"}}),
                CodexErrorInfo::ActiveTurnNotSteerable {
                    active_turn_not_steerable: ActiveTurnNotSteerableDetails {
                        turn_kind: NonSteerableTurnKind::Review,
                    },
                },
            ),
            (
                json!({"activeTurnNotSteerable": {"turnKind": "compact"}}),
                CodexErrorInfo::ActiveTurnNotSteerable {
                    active_turn_not_steerable: ActiveTurnNotSteerableDetails {
                        turn_kind: NonSteerableTurnKind::Compact,
                    },
                },
            ),
            (
                json!({"activeTurnNotSteerable": {"turnKind": "futureKind"}}),
                CodexErrorInfo::ActiveTurnNotSteerable {
                    active_turn_not_steerable: ActiveTurnNotSteerableDetails {
                        turn_kind: NonSteerableTurnKind::from_raw("futureKind"),
                    },
                },
            ),
        ];
        for (wire, expected) in cases {
            assert_eq!(
                serde_json::from_value::<CodexErrorInfo>(wire.clone())?,
                expected
            );
            assert_eq!(serde_json::to_value(&expected)?, wire);
        }

        // An explicit `httpStatusCode: null` and an absent key both mean "no
        // status was forwarded", so both decode to `None` and encode without
        // the key - the same conflation the crate applies to `Account.email`.
        let explicit_null: CodexErrorInfo = serde_json::from_value(
            json!({"responseStreamConnectionFailed": {"httpStatusCode": null}}),
        )?;
        assert_eq!(
            explicit_null,
            CodexErrorInfo::ResponseStreamConnectionFailed {
                response_stream_connection_failed: ForwardedHttpStatus {
                    http_status_code: None,
                },
            }
        );
        assert_eq!(
            serde_json::to_value(&explicit_null)?,
            json!({"responseStreamConnectionFailed": {}})
        );
        Ok(())
    }

    /// 4-37: the dedicated `error` notification decodes through
    /// `decode_notification` and serializes back to the pinned envelope.
    #[test]
    fn error_notification_decodes_and_serializes_the_pinned_shape() -> Result<(), serde_json::Error>
    {
        let raw = json!({
            "method": "error",
            "params": {
                "threadId": "thr_123",
                "turnId": "turn_456",
                "willRetry": true,
                "error": {
                    "message": "turn failed",
                    "additionalDetails": "provider unavailable",
                    "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": 502}},
                    "futureErrorField": 7
                },
                "futureField": true
            }
        });
        let notification =
            decode_notification("error".to_owned(), raw.get("params").cloned(), raw.clone());
        let Notification::Error(notification) = notification else {
            panic!("expected an error notification");
        };
        let expected = ErrorNotification {
            thread_id: "thr_123".to_owned(),
            turn_id: "turn_456".to_owned(),
            will_retry: true,
            error: TurnError {
                message: "turn failed".to_owned(),
                additional_details: Some("provider unavailable".to_owned()),
                codex_error_info: Some(CodexErrorInfo::ResponseStreamDisconnected {
                    response_stream_disconnected: ForwardedHttpStatus {
                        http_status_code: Some(502),
                    },
                }),
                extra: [("futureErrorField".to_owned(), json!(7))]
                    .into_iter()
                    .collect(),
            },
            extra: [("futureField".to_owned(), json!(true))]
                .into_iter()
                .collect(),
        };
        assert_eq!(*notification, expected);
        assert_eq!(serde_json::to_value(&*notification)?, raw["params"].clone());
        Ok(())
    }

    /// 4-37: `Turn.error` is the typed `v2/TurnError` payload; a failed turn
    /// round-trips it losslessly and a non-failed turn omits the key.
    #[test]
    fn failed_turn_error_is_typed_and_lossless() -> Result<(), serde_json::Error> {
        let failed: Turn = serde_json::from_value(json!({
            "id": "turn_456",
            "items": [],
            "status": "failed",
            "error": {
                "message": "boom",
                "codexErrorInfo": "usageLimitExceeded"
            }
        }))?;
        let Some(error) = failed.error.as_ref() else {
            panic!("missing typed turn error");
        };
        assert_eq!(error.message, "boom");
        assert_eq!(error.additional_details, None);
        assert_eq!(
            error.codex_error_info,
            Some(CodexErrorInfo::Code(CodexErrorCode::UsageLimitExceeded))
        );
        let encoded = serde_json::to_value(&failed)?;
        assert_eq!(
            encoded["error"]["codexErrorInfo"],
            json!("usageLimitExceeded")
        );

        let clean: Turn =
            serde_json::from_value(json!({"id": "turn_1", "items": [], "status": "completed"}))?;
        assert_eq!(clean.error, None);
        // An absent `itemsView` decodes as `None` and reads back as the pinned
        // `full` default (13-O-3).
        assert_eq!(clean.items_view, None);
        assert_eq!(clean.items_view(), TurnItemsView::Full);
        // `Turn` keeps the pinned null-carrying optional keys on the wire.
        assert_eq!(
            serde_json::to_value(&clean)?,
            json!({
                "id": "turn_1",
                "items": [],
                "status": "completed",
                "itemsView": null,
                "error": null,
                "startedAt": null,
                "completedAt": null,
                "durationMs": null
            })
        );
        Ok(())
    }

    /// 6-02: `thread/start` carries the five previously missing pinned
    /// optional properties under their exact camelCase wire keys, and future
    /// properties stay lossless through the new `extra` escape hatch.
    #[test]
    fn thread_start_params_serialize_the_previously_missing_pinned_fields()
    -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            ("user", ApprovalsReviewer::User),
            ("auto_review", ApprovalsReviewer::AutoReview),
            ("guardian_subagent", ApprovalsReviewer::GuardianSubagent),
        ] {
            assert_eq!(ApprovalsReviewer::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }
        for (wire, expected) in [
            ("startup", SessionStartSource::Startup),
            ("clear", SessionStartSource::Clear),
        ] {
            assert_eq!(SessionStartSource::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let params = ThreadStartParams {
            approvals_reviewer: Some(ApprovalsReviewer::AutoReview),
            thread_source: Some("cli".to_owned()),
            session_start_source: Some(SessionStartSource::Startup),
            service_tier: Some("flex".to_owned()),
            config: Some(
                [("model_reasoning_summary".to_owned(), json!("detailed"))]
                    .into_iter()
                    .collect(),
            ),
            extra: [("futureField".to_owned(), json!(7))].into_iter().collect(),
            ..ThreadStartParams::default()
        };
        let encoded = serde_json::to_value(&params)?;
        assert_eq!(encoded["approvalsReviewer"], json!("auto_review"));
        assert_eq!(encoded["threadSource"], json!("cli"));
        assert_eq!(encoded["sessionStartSource"], json!("startup"));
        assert_eq!(encoded["serviceTier"], json!("flex"));
        assert_eq!(
            encoded["config"]["model_reasoning_summary"],
            json!("detailed")
        );
        assert_eq!(encoded["futureField"], json!(7));
        assert_eq!(
            serde_json::from_value::<ThreadStartParams>(encoded)?,
            params
        );

        // The reviewer and start-source enums stay open: values a newer
        // app-server introduced round-trip losslessly.
        let future = ThreadStartParams {
            approvals_reviewer: Some(ApprovalsReviewer::from_raw("futureReviewer")),
            session_start_source: Some(SessionStartSource::from_raw("futureSource")),
            ..ThreadStartParams::default()
        };
        let encoded = serde_json::to_value(&future)?;
        assert_eq!(encoded["approvalsReviewer"], json!("futureReviewer"));
        assert_eq!(encoded["sessionStartSource"], json!("futureSource"));
        assert_eq!(
            serde_json::from_value::<ThreadStartParams>(encoded)?,
            future
        );
        Ok(())
    }

    /// 6-02: `turn/start` carries the turn-level `sandboxPolicy`,
    /// `approvalPolicy`, `approvalsReviewer`, and `serviceTier` overrides
    /// under their exact camelCase wire keys.
    #[test]
    fn turn_start_params_serialize_the_turn_level_overrides() -> Result<(), serde_json::Error> {
        let params = TurnStartParams {
            sandbox_policy: Some(SandboxPolicy::WorkspaceWrite {
                writable_roots: Some(vec![PathBuf::from("/workspace")]),
                network_access: Some(true),
                exclude_slash_tmp: Some(true),
                exclude_tmpdir_env_var: Some(false),
                extra: serde_json::Map::new(),
            }),
            approval_policy: Some(AskForApproval::Granular(GranularAskForApproval::new(
                false, true, true,
            ))),
            approvals_reviewer: Some(ApprovalsReviewer::GuardianSubagent),
            service_tier: Some("priority".to_owned()),
            extra: [("futureTurnField".to_owned(), json!("kept"))]
                .into_iter()
                .collect(),
            ..TurnStartParams::text("thr_123", "hello")
        };
        let encoded = serde_json::to_value(&params)?;
        assert_eq!(
            encoded["sandboxPolicy"],
            json!({
                "type": "workspaceWrite",
                "writableRoots": ["/workspace"],
                "networkAccess": true,
                "excludeSlashTmp": true,
                "excludeTmpdirEnvVar": false
            })
        );
        assert_eq!(encoded["approvalPolicy"]["granular"]["rules"], json!(true));
        assert_eq!(encoded["approvalsReviewer"], json!("guardian_subagent"));
        assert_eq!(encoded["serviceTier"], json!("priority"));
        assert_eq!(encoded["futureTurnField"], json!("kept"));
        assert_eq!(serde_json::from_value::<TurnStartParams>(encoded)?, params);
        Ok(())
    }

    /// 6-02: the turn-level `v2/SandboxPolicy` tagged union keeps all four
    /// pinned branch shapes, and the sub-settings the pin defaults
    /// server-side stay absent when unset.
    #[test]
    fn sandbox_policy_matches_the_pinned_tagged_union() -> Result<(), serde_json::Error> {
        for (wire, expected) in [
            ("restricted", NetworkAccess::Restricted),
            ("enabled", NetworkAccess::Enabled),
        ] {
            assert_eq!(NetworkAccess::from_raw(wire), expected);
            assert_eq!(expected.as_str(), wire);
        }

        let cases = [
            (
                json!({"type": "dangerFullAccess"}),
                SandboxPolicy::DangerFullAccess {
                    extra: serde_json::Map::new(),
                },
            ),
            (
                json!({"type": "readOnly"}),
                SandboxPolicy::ReadOnly {
                    network_access: None,
                    extra: serde_json::Map::new(),
                },
            ),
            (
                json!({"type": "readOnly", "networkAccess": true}),
                SandboxPolicy::ReadOnly {
                    network_access: Some(true),
                    extra: serde_json::Map::new(),
                },
            ),
            (
                json!({"type": "externalSandbox"}),
                SandboxPolicy::ExternalSandbox {
                    network_access: None,
                    extra: serde_json::Map::new(),
                },
            ),
            (
                json!({"type": "externalSandbox", "networkAccess": "enabled"}),
                SandboxPolicy::ExternalSandbox {
                    network_access: Some(NetworkAccess::Enabled),
                    extra: serde_json::Map::new(),
                },
            ),
            (
                json!({
                    "type": "workspaceWrite",
                    "writableRoots": ["/w"],
                    "networkAccess": false,
                    "excludeSlashTmp": true,
                    "excludeTmpdirEnvVar": true
                }),
                SandboxPolicy::WorkspaceWrite {
                    writable_roots: Some(vec![PathBuf::from("/w")]),
                    network_access: Some(false),
                    exclude_slash_tmp: Some(true),
                    exclude_tmpdir_env_var: Some(true),
                    extra: serde_json::Map::new(),
                },
            ),
        ];
        for (wire, expected) in cases {
            assert_eq!(
                serde_json::from_value::<SandboxPolicy>(wire.clone())?,
                expected
            );
            assert_eq!(serde_json::to_value(&expected)?, wire);
        }

        // 13-O-1: an unknown branch tag — a fifth branch a later app-server
        // adds — and a payload without a usable `type` tag stay verbatim in
        // the Unknown variant instead of failing the surrounding response
        // decode, and re-encode byte-for-byte.
        for unmodelled in [
            json!({"type": "futureMode", "futureSetting": true}),
            json!("readOnly"),
        ] {
            let decoded = serde_json::from_value::<SandboxPolicy>(unmodelled.clone())
                .expect("an unmodelled sandbox policy decodes losslessly");
            assert_eq!(
                decoded,
                SandboxPolicy::Unknown(unmodelled.clone()),
                "the payload must stay verbatim"
            );
            assert_eq!(serde_json::to_value(&decoded)?, unmodelled);
        }

        // A known tag whose sub-settings no longer match the pinned shape
        // degrades to the same lossless Unknown.
        let malformed = json!({"type": "readOnly", "networkAccess": "fast"});
        let decoded =
            serde_json::from_value::<SandboxPolicy>(malformed.clone()).expect("malformed degrades");
        assert_eq!(decoded, SandboxPolicy::Unknown(malformed));

        // Nested inside `turn/start` the policy sits under its pinned key.
        let params = TurnStartParams {
            sandbox_policy: Some(SandboxPolicy::ReadOnly {
                network_access: Some(false),
                extra: serde_json::Map::new(),
            }),
            ..TurnStartParams::text("thr_123", "hello")
        };
        let encoded = serde_json::to_value(&params)?;
        assert_eq!(
            encoded["sandboxPolicy"],
            json!({"type": "readOnly", "networkAccess": false})
        );
        assert_eq!(serde_json::from_value::<TurnStartParams>(encoded)?, params);
        Ok(())
    }

    /// 17-O-1: a pin-legal additive sub-key on a known branch (and inside the
    /// known `granular` approval-policy object) decodes into the branch's
    /// flatten `extra` map and re-encodes byte-equal, instead of being
    /// silently dropped.
    #[test]
    fn sandbox_policy_known_branches_retain_future_sub_keys() -> Result<(), serde_json::Error> {
        let wire = json!({
            "type": "workspaceWrite",
            "writableRoots": ["/w"],
            "networkAccess": false,
            "futureFlag": true
        });
        let decoded = serde_json::from_value::<SandboxPolicy>(wire.clone())?;
        let SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            extra,
            ..
        } = &decoded
        else {
            panic!("expected a workspaceWrite branch, got {decoded:?}");
        };
        assert_eq!(writable_roots, &Some(vec![PathBuf::from("/w")]));
        assert_eq!(network_access, &Some(false));
        assert_eq!(
            serde_json::Value::Object(extra.clone()),
            json!({"futureFlag": true}),
            "the future sub-key must be retained"
        );
        // Re-encode is byte-equal: the retained key rides along, and an empty
        // extra map on constructed branches emits nothing.
        assert_eq!(serde_json::to_value(&decoded)?, wire);

        // A constructed branch with an empty extra map stays byte-identical.
        let constructed = SandboxPolicy::WorkspaceWrite {
            writable_roots: Some(vec![PathBuf::from("/w")]),
            network_access: Some(false),
            exclude_slash_tmp: None,
            exclude_tmpdir_env_var: None,
            extra: serde_json::Map::new(),
        };
        assert_eq!(
            serde_json::to_value(&constructed)?,
            json!({"type": "workspaceWrite", "writableRoots": ["/w"], "networkAccess": false})
        );

        // The known `granular` approval-policy branch keeps additive sub-keys
        // the same way, and `dangerFullAccess` (no pinned sub-settings)
        // retains future keys too.
        let granular_wire = json!({
            "approvalPolicy": {
                "granular": {
                    "mcp_elicitations": false,
                    "rules": true,
                    "sandbox_approval": true,
                    "futureGranularKey": 7
                }
            },
            "sandboxPolicy": {"type": "dangerFullAccess", "futureDangerKey": "kept"}
        });
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Carrier {
            #[serde(rename = "approvalPolicy")]
            approval_policy: AskForApproval,
            #[serde(rename = "sandboxPolicy")]
            sandbox_policy: SandboxPolicy,
        }
        let carrier: Carrier = serde_json::from_value(granular_wire.clone())?;
        let AskForApproval::Granular(granular) = &carrier.approval_policy else {
            panic!("expected a granular approval policy");
        };
        assert_eq!(
            serde_json::Value::Object(granular.extra.clone()),
            json!({"futureGranularKey": 7})
        );
        let SandboxPolicy::DangerFullAccess { extra } = &carrier.sandbox_policy else {
            panic!("expected a dangerFullAccess branch");
        };
        assert_eq!(
            serde_json::Value::Object(extra.clone()),
            json!({"futureDangerKey": "kept"})
        );
        assert_eq!(serde_json::to_value(&carrier)?, granular_wire);
        Ok(())
    }

    /// 6-07: DTOs carrying the `extra` flatten escape hatch keep retained
    /// properties out of `Debug` output; the config escape hatch and the
    /// device user code are redacted the same way.
    #[test]
    fn extra_carriers_debug_never_leaks_retained_values() -> Result<(), serde_json::Error> {
        let thread: Thread = serde_json::from_value(json!({
            "id": "thr_123",
            "openaiApiKey": "sk-secret-value",
            "authorization": "Bearer secret-token"
        }))?;
        let rendered = format!("{thread:?}");
        assert!(
            rendered.contains("Thread {"),
            "unexpected output: {rendered}"
        );
        assert!(
            rendered.contains("extra: 2"),
            "unexpected output: {rendered}"
        );
        assert!(!rendered.contains("sk-secret-value"));
        assert!(!rendered.contains("secret-token"));
        assert!(!rendered.contains("openaiApiKey"));

        let turn_params = TurnStartParams {
            extra: [("futureToken".to_owned(), json!("sk-turn-secret"))]
                .into_iter()
                .collect(),
            ..TurnStartParams::text("thr_123", "hello")
        };
        let rendered = format!("{turn_params:?}");
        assert!(
            !rendered.contains("sk-turn-secret"),
            "unexpected output: {rendered}"
        );
        assert!(
            rendered.contains("extra: 1"),
            "unexpected output: {rendered}"
        );

        let thread_params = ThreadStartParams {
            config: Some(
                [("apiKey".to_owned(), json!("sk-config-secret"))]
                    .into_iter()
                    .collect(),
            ),
            ..ThreadStartParams::default()
        };
        let rendered = format!("{thread_params:?}");
        assert!(
            !rendered.contains("sk-config-secret"),
            "unexpected output: {rendered}"
        );
        assert!(
            rendered.contains("config: \"<redacted>\""),
            "unexpected output: {rendered}"
        );

        let login: LoginAccountResponse = serde_json::from_value(json!({
            "type": "chatgptDeviceCode",
            "userCode": "ABCD-1234"
        }))?;
        let rendered = format!("{login:?}");
        assert!(
            !rendered.contains("ABCD-1234"),
            "unexpected output: {rendered}"
        );
        Ok(())
    }

    /// 6-13: the dedicated `error` channel belongs to the known-notification
    /// warn list, so a typed decode failure is logged instead of being
    /// silently degraded to `Unknown`.
    #[test]
    fn error_notification_decode_failure_emits_warn() {
        let subscriber = WarnCapture::default();
        let _guard = tracing::subscriber::set_default(subscriber.clone());
        let notification = decode_notification(
            "error".to_owned(),
            Some(json!({"threadId": "thr_123"})),
            json!({"method": "error"}),
        );
        assert!(matches!(notification, Notification::Unknown(_)));
        let events = subscriber.messages();
        assert!(events.iter().any(|message| {
            message.contains("typed decode failed for known app-server notification")
        }));
        assert!(
            events
                .iter()
                .any(|message| message.contains("rpc.method=error")),
            "warn events: {events:?}"
        );
        assert!(!events.iter().any(|message| message.contains("thr_123")));
    }

    /// Trace injection support: `W3cTraceContext` keeps the pinned optional
    /// nullable string pair in every wire state.
    #[test]
    fn w3c_trace_context_serializes_the_pinned_wire_states() -> Result<(), serde_json::Error> {
        let context =
            W3cTraceContext::new("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
                .with_tracestate("congo=4");
        assert_eq!(
            serde_json::to_value(&context)?,
            json!({
                "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                "tracestate": "congo=4"
            })
        );

        // An unset `tracestate` stays off the wire entirely.
        let parent_only = W3cTraceContext::new("00-abc");
        assert_eq!(
            serde_json::to_value(&parent_only)?,
            json!({"traceparent": "00-abc"})
        );

        // An explicit null keeps the key present, the same three-state
        // posture `RpcError.data` uses.
        let explicit_null = W3cTraceContext {
            traceparent: Omittable::Value(Nullable::Null),
            tracestate: Omittable::Omitted,
        };
        assert_eq!(
            serde_json::to_value(&explicit_null)?,
            json!({"traceparent": null})
        );
        assert_eq!(
            serde_json::from_value::<W3cTraceContext>(json!({
                "traceparent": null,
                "tracestate": "rojo=1"
            }))?,
            W3cTraceContext {
                traceparent: Omittable::Value(Nullable::Null),
                tracestate: Omittable::Value(Nullable::Value("rojo=1".to_owned())),
            }
        );
        Ok(())
    }
}
