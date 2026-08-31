use std::collections::BTreeMap;
use std::path::PathBuf;

use openai_rs_types::kernel::{Nullable, Omittable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Future capability properties, retained losslessly.
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub user_agent: String,
    pub codex_home: PathBuf,
    pub platform_family: String,
    pub platform_os: String,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelLoginResponse {
    pub status: CancelLoginStatus,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReadResponse {
    pub account: Option<Account>,
    pub requires_openai_auth: bool,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    pub used_percent: i32,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsSnapshot {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageBucket {
    pub start_date: String,
    pub tokens: i64,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageResponse {
    pub summary: AccountUsageSummary,
    pub daily_usage_buckets: Option<Vec<DailyUsageBucket>>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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
/// on the app-server side when omitted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GranularAskForApproval {
    pub mcp_elicitations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_permissions: Option<bool>,
    pub rules: bool,
    pub sandbox_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_approval: Option<bool>,
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
/// `{"granular": {...}}` with the pinned snake_case keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskForApproval {
    Mode(AskForApprovalMode),
    Granular(GranularAskForApproval),
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
                let granular = object.remove("granular").ok_or_else(|| {
                    serde::de::Error::custom(
                        "granular approval policy object requires a `granular` key",
                    )
                })?;
                GranularAskForApproval::deserialize(granular)
                    .map(Self::Granular)
                    .map_err(serde::de::Error::custom)
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

/// Core, stable subset of `thread/start` parameters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<AskForApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<Personality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

impl TurnStartParams {
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    #[serde(default)]
    pub items: Vec<Value>,
    pub status: TurnStatus,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResponse {
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyResponse {
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginCompletedNotification {
    pub login_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdatedNotification {
    pub auth_mode: Option<AuthMode>,
    pub plan_type: Option<PlanType>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsUpdatedNotification {
    pub rate_limits: RateLimitSnapshot,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Turn failure broadcast on the dedicated `error` notification channel.
///
/// Wire shape of the pinned `v2/ErrorNotification`: `threadId`, `turnId`,
/// `willRetry`, and the typed [`TurnError`] are all required; envelope
/// properties added by a newer app-server stay lossless in
/// [`ErrorNotification::extra`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartedNotification {
    pub thread: Thread,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartedNotification {
    pub thread_id: String,
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletedNotification {
    pub thread_id: String,
    pub turn: Turn,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLifecycleNotification {
    pub thread_id: String,
    pub turn_id: String,
    pub item: Value,
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageDeltaNotification {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub delta: String,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

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
        AccountUpdatedNotification, ActiveTurnNotSteerableDetails, AskForApproval,
        AskForApprovalMode, AuthMode, ByteRange, CancelLoginResponse, CancelLoginStatus,
        ClientInfo, CodexErrorCode, CodexErrorInfo, ErrorNotification, ForwardedHttpStatus,
        GranularAskForApproval, ImageDetail, InitializeCapabilities, InitializeParams,
        LoginAccountResponse, NonSteerableTurnKind, Notification, Nullable, Omittable, Personality,
        PlanType, RateLimitReachedType, RateLimitSnapshot, ReasoningSummary, SandboxMode,
        TextElement, ThreadStartParams, Turn, TurnError, TurnStartParams, TurnStatus, UserInput,
        decode_notification,
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

        // The object branch is pinned to a single required `granular` key.
        assert!(
            serde_json::from_value::<AskForApproval>(json!({"other": true})).is_err(),
            "object without a `granular` key must not decode"
        );
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
        // `Turn` keeps the pinned null-carrying optional keys on the wire.
        assert_eq!(
            serde_json::to_value(&clean)?,
            json!({
                "id": "turn_1",
                "items": [],
                "status": "completed",
                "error": null,
                "startedAt": null,
                "completedAt": null,
                "durationMs": null
            })
        );
        Ok(())
    }
}
