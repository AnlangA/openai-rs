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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelLoginResponse {
    /// Open string so newly added server statuses remain lossless.
    pub status: String,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Open discriminator (`apiKey`, `chatgpt`, or a future account type).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan_type: Option<String>,
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
    /// Open string; app-server may add new plan names.
    pub plan_type: Option<String>,
    /// Open string; app-server may add new reached states.
    pub rate_limit_reached_type: Option<String>,
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
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
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
        detail: Option<String>,
    },
    LocalImage {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    #[serde(default)]
    pub items: Vec<Value>,
    /// Open status string (`inProgress`, `completed`, `interrupted`, `failed`, …).
    pub status: String,
    #[serde(default)]
    pub error: Option<Value>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdatedNotification {
    pub auth_mode: Option<String>,
    pub plan_type: Option<String>,
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

    match method.as_str() {
        "account/login/completed" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountLoginCompleted),
        "account/updated" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountUpdated),
        "account/rateLimits/updated" => typed(&params)
            .map(Box::new)
            .map(Notification::AccountRateLimitsUpdated),
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
    }
    .unwrap_or(Notification::Unknown(Box::new(RawNotification {
        method,
        params,
        raw,
    })))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ByteRange, ClientInfo, InitializeCapabilities, InitializeParams, LoginAccountResponse,
        Notification, Nullable, Omittable, TextElement, TurnStartParams, UserInput,
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
}
