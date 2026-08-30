//! Beta ChatKit session, thread, and thread-item wire types.
//!
//! This surface requires the `OpenAI-Beta: chatkit_beta=v1` request header and
//! access to the hosted ChatKit workflow API. Hosted Agent Builder workflows
//! are a transition-path integration; callers should consult the current
//! ChatKit access and migration documentation before depending on it.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};
use thiserror::Error;

use crate::{
    ExtraFields, JsonText, ModelId, Nullable, Omittable, WireSecret, responses::UnknownTaggedObject,
};

crate::opaque_string_id! {
    /// Opaque ChatKit session identifier.
    pub struct ChatKitSessionId;
}

crate::opaque_string_id! {
    /// Opaque ChatKit thread identifier.
    pub struct ChatKitThreadId;
}

crate::opaque_string_id! {
    /// Opaque ChatKit thread-item identifier.
    pub struct ChatKitThreadItemId;
}

crate::open_string_enum! {
    /// ChatKit collection ordering.
    pub enum ChatKitListOrder {
        Ascending = "asc",
        Descending = "desc",
    }
}

crate::open_string_enum! {
    /// ChatKit session lifecycle state.
    pub enum ChatKitSessionStatus {
        Active = "active",
        Expired = "expired",
        Cancelled = "cancelled",
    }
}

crate::open_string_enum! {
    /// Attachment kind surfaced on a user message.
    pub enum ChatKitAttachmentType {
        Image = "image",
        File = "file",
    }
}

crate::open_string_enum! {
    /// Client-side tool call lifecycle state.
    pub enum ChatKitClientToolCallStatus {
        InProgress = "in_progress",
        Completed = "completed",
    }
}

crate::open_string_enum! {
    /// Task subtype rendered by ChatKit.
    pub enum ChatKitTaskType {
        Custom = "custom",
        Thought = "thought",
    }
}

crate::open_string_enum! {
    /// Object discriminator for a ChatKit session.
    pub enum ChatKitSessionObject {
        Session = "chatkit.session",
    }
}

crate::open_string_enum! {
    /// Object discriminator for a ChatKit thread.
    pub enum ChatKitThreadObject {
        Thread = "chatkit.thread",
    }
}

crate::open_string_enum! {
    /// Object discriminator returned after deleting a ChatKit thread.
    pub enum DeletedChatKitThreadObject {
        Deleted = "chatkit.thread.deleted",
    }
}

crate::open_string_enum! {
    /// Object discriminator shared by ChatKit thread items.
    pub enum ChatKitThreadItemObject {
        Item = "chatkit.thread_item",
    }
}

crate::open_string_enum! {
    /// Object discriminator for ChatKit list envelopes.
    pub enum ChatKitListObject {
        List = "list",
    }
}

crate::open_string_enum! {
    /// Anchor used to expire a ChatKit session.
    pub enum ChatKitExpirationAnchor {
        CreatedAt = "created_at",
    }
}

/// Validation error for a Beta ChatKit request value.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatKitValidationError {
    /// A user identifier is empty.
    #[error("ChatKit user identifier must not be empty")]
    InvalidUser,
    /// A thread-list user filter exceeds its endpoint limit.
    #[error("ChatKit thread user filter must contain 1..=512 characters")]
    InvalidUserFilter,
    /// A list page size is outside `0..=100`.
    #[error("ChatKit list limit must be between 0 and 100, got {value}")]
    InvalidListLimit {
        /// Rejected value.
        value: u16,
    },
    /// Session expiration is outside `1..=600` seconds.
    #[error("ChatKit session expiration must be between 1 and 600 seconds, got {seconds}")]
    InvalidExpiration {
        /// Rejected value.
        seconds: u16,
    },
    /// A positive configuration value was zero.
    #[error("ChatKit configuration value must be positive")]
    ZeroLimit,
    /// Upload size is outside `1..=512` MB.
    #[error("ChatKit max_file_size must be between 1 and 512 MB, got {megabytes}")]
    InvalidFileSize {
        /// Rejected value.
        megabytes: u16,
    },
    /// Workflow state contains more than 64 variables.
    #[error("ChatKit workflow state contains {actual} variables; maximum is 64")]
    TooManyStateVariables {
        /// Observed count.
        actual: usize,
    },
    /// A workflow-state key exceeds 64 characters.
    #[error("ChatKit workflow-state key exceeds 64 characters")]
    StateKeyTooLong,
    /// A workflow-state string exceeds 10 MiB in Unicode scalar values.
    #[error("ChatKit workflow-state string exceeds 10485760 characters")]
    StateStringTooLong,
    /// A JSON number could not represent a non-finite float.
    #[error("ChatKit workflow-state number must be finite")]
    NonFiniteNumber,
}

/// Validated user identifier used to scope ChatKit sessions and threads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatKitUserId(Box<str>);

impl ChatKitUserId {
    /// Validates the non-empty user scope required during session creation.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ChatKitValidationError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ChatKitValidationError::InvalidUser);
        }
        Ok(Self(value))
    }

    /// Borrows the wire value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated `user` query filter for `GET /chatkit/threads`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatKitUserFilter(Box<str>);

impl ChatKitUserFilter {
    /// Applies the endpoint's `1..=512` character constraint.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ChatKitValidationError> {
        let value = value.into();
        let len = value.chars().count();
        if len == 0 || len > 512 {
            return Err(ChatKitValidationError::InvalidUserFilter);
        }
        Ok(Self(value))
    }

    /// Borrows the filter value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ChatKitUserFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Box::<str>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ChatKitUserId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Box::<str>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Validated page size accepted by ChatKit list endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatKitListLimit(u8);

impl ChatKitListLimit {
    /// Creates a page size in `0..=100`.
    pub fn new(value: u16) -> Result<Self, ChatKitValidationError> {
        u8::try_from(value)
            .ok()
            .filter(|value| *value <= 100)
            .map(Self)
            .ok_or(ChatKitValidationError::InvalidListLimit { value })
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ChatKitListLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Positive integer used by rate, file-count, and history limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatKitPositiveLimit(u64);

impl ChatKitPositiveLimit {
    /// Rejects zero.
    pub fn new(value: u64) -> Result<Self, ChatKitValidationError> {
        if value == 0 {
            Err(ChatKitValidationError::ZeroLimit)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ChatKitPositiveLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Per-file upload limit in megabytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChatKitFileSizeMb(u16);

impl ChatKitFileSizeMb {
    /// Creates a size in `1..=512` MB.
    pub fn new(value: u16) -> Result<Self, ChatKitValidationError> {
        if (1..=512).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ChatKitValidationError::InvalidFileSize { megabytes: value })
        }
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ChatKitFileSizeMb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Primitive value accepted in workflow state variables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ChatKitStateValue {
    /// Text value.
    String(String),
    /// Signed integer value.
    Integer(i64),
    /// Boolean value.
    Boolean(bool),
    /// Other finite JSON number.
    Number(Number),
}

impl ChatKitStateValue {
    /// Creates a bounded string state value.
    pub fn string(value: impl Into<String>) -> Result<Self, ChatKitValidationError> {
        let value = value.into();
        if value.chars().count() > 10_485_760 {
            return Err(ChatKitValidationError::StateStringTooLong);
        }
        Ok(Self::String(value))
    }

    /// Creates a finite floating-point state value.
    pub fn number(value: f64) -> Result<Self, ChatKitValidationError> {
        Number::from_f64(value)
            .map(Self::Number)
            .ok_or(ChatKitValidationError::NonFiniteNumber)
    }
}

/// Validated workflow state-variable map.
#[derive(Clone, Default, PartialEq)]
pub struct ChatKitStateVariables(BTreeMap<String, ChatKitStateValue>);

impl ChatKitStateVariables {
    /// Creates an empty map.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Inserts one validated variable.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: ChatKitStateValue,
    ) -> Result<Option<ChatKitStateValue>, ChatKitValidationError> {
        let key = key.into();
        validate_state_entry(&key, &value)?;
        if !self.0.contains_key(&key) && self.0.len() == 64 {
            return Err(ChatKitValidationError::TooManyStateVariables { actual: 65 });
        }
        Ok(self.0.insert(key, value))
    }

    /// Iterates in stable key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &ChatKitStateValue)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }
}

impl fmt::Debug for ChatKitStateVariables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChatKitStateVariables")
            .field("property_count", &self.0.len())
            .finish()
    }
}

impl Serialize for ChatKitStateVariables {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChatKitStateVariables {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, ChatKitStateValue>::deserialize(deserializer)?;
        if values.len() > 64 {
            return Err(serde::de::Error::custom(
                ChatKitValidationError::TooManyStateVariables {
                    actual: values.len(),
                },
            ));
        }
        for (key, value) in &values {
            validate_state_entry(key, value).map_err(serde::de::Error::custom)?;
        }
        Ok(Self(values))
    }
}

fn validate_state_entry(
    key: &str,
    value: &ChatKitStateValue,
) -> Result<(), ChatKitValidationError> {
    if key.chars().count() > 64 {
        return Err(ChatKitValidationError::StateKeyTooLong);
    }
    if let ChatKitStateValue::String(value) = value
        && value.chars().count() > 10_485_760
    {
        return Err(ChatKitValidationError::StateStringTooLong);
    }
    Ok(())
}

/// Optional tracing overrides on a workflow request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitWorkflowTracingRequest {
    /// Whether tracing is enabled.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub enabled: Omittable<bool>,
}

/// Workflow reference and invocation overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitWorkflowRequest {
    /// Workflow identifier.
    pub id: String,
    /// Optional deployed version.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub version: Omittable<String>,
    /// Optional primitive state-variable map.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub state_variables: Omittable<ChatKitStateVariables>,
    /// Optional tracing override.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tracing: Omittable<ChatKitWorkflowTracingRequest>,
}

impl ChatKitWorkflowRequest {
    /// Creates a minimal latest-version workflow reference.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: Omittable::Omitted,
            state_variables: Omittable::Omitted,
            tracing: Omittable::Omitted,
        }
    }

    /// Selects a workflow version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Omittable::Value(version.into());
        self
    }

    /// Attaches validated workflow state.
    #[must_use]
    pub fn with_state_variables(mut self, state: ChatKitStateVariables) -> Self {
        self.state_variables = Omittable::Value(state);
        self
    }
}

/// Optional session-expiration override.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatKitExpiresAfterRequest {
    /// Fixed creation-time anchor.
    pub anchor: ChatKitExpirationAnchor,
    /// Seconds after creation.
    pub seconds: u16,
}

impl ChatKitExpiresAfterRequest {
    /// Creates an expiration in `1..=600` seconds.
    pub fn new(seconds: u16) -> Result<Self, ChatKitValidationError> {
        if !(1..=600).contains(&seconds) {
            return Err(ChatKitValidationError::InvalidExpiration { seconds });
        }
        Ok(Self {
            anchor: ChatKitExpirationAnchor::CreatedAt,
            seconds,
        })
    }
}

impl<'de> Deserialize<'de> for ChatKitExpiresAfterRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            anchor: ChatKitExpirationAnchor,
            seconds: u16,
        }
        let wire = Wire::deserialize(deserializer)?;
        let mut value = Self::new(wire.seconds).map_err(serde::de::Error::custom)?;
        value.anchor = wire.anchor;
        Ok(value)
    }
}

/// Optional session rate-limit override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitRateLimitsRequest {
    /// Requests allowed in one minute.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_requests_per_1_minute: Omittable<ChatKitPositiveLimit>,
}

/// Optional automatic-thread-titling override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitAutomaticThreadTitlingRequest {
    /// Whether title generation is enabled.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub enabled: Omittable<bool>,
}

/// Optional file-upload behavior for a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitFileUploadRequest {
    /// Whether uploads are enabled.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub enabled: Omittable<bool>,
    /// Maximum size in megabytes.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_file_size: Omittable<ChatKitFileSizeMb>,
    /// Maximum file count.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub max_files: Omittable<ChatKitPositiveLimit>,
}

/// Optional history behavior for a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitHistoryRequest {
    /// Whether prior threads are visible.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub enabled: Omittable<bool>,
    /// Number of recent threads visible when bounded.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub recent_threads: Omittable<ChatKitPositiveLimit>,
}

/// Optional ChatKit runtime feature overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitConfigurationRequest {
    /// Automatic titling configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub automatic_thread_titling: Omittable<ChatKitAutomaticThreadTitlingRequest>,
    /// File-upload configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_upload: Omittable<ChatKitFileUploadRequest>,
    /// History configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub history: Omittable<ChatKitHistoryRequest>,
}

/// JSON body for provisioning a Beta ChatKit session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChatKitSessionRequest {
    /// Hosted workflow reference.
    pub workflow: ChatKitWorkflowRequest,
    /// End-user scope. This must be unique per end user.
    pub user: ChatKitUserId,
    /// Optional expiration override.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub expires_after: Omittable<ChatKitExpiresAfterRequest>,
    /// Optional rate-limit override.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub rate_limits: Omittable<ChatKitRateLimitsRequest>,
    /// Optional runtime configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub chatkit_configuration: Omittable<ChatKitConfigurationRequest>,
}

impl CreateChatKitSessionRequest {
    /// Creates a minimal session request.
    pub fn new(
        workflow: ChatKitWorkflowRequest,
        user: impl Into<Box<str>>,
    ) -> Result<Self, ChatKitValidationError> {
        Ok(Self {
            workflow,
            user: ChatKitUserId::new(user)?,
            expires_after: Omittable::Omitted,
            rate_limits: Omittable::Omitted,
            chatkit_configuration: Omittable::Omitted,
        })
    }

    /// Sets an expiration override.
    #[must_use]
    pub fn with_expiration(mut self, expiration: ChatKitExpiresAfterRequest) -> Self {
        self.expires_after = Omittable::Value(expiration);
        self
    }

    /// Sets runtime configuration.
    #[must_use]
    pub fn with_configuration(mut self, configuration: ChatKitConfigurationRequest) -> Self {
        self.chatkit_configuration = Omittable::Value(configuration);
        self
    }
}

/// Resolved workflow tracing settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitWorkflowTracing {
    /// Whether tracing is enabled.
    pub enabled: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved workflow metadata returned for a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitWorkflow {
    /// Workflow identifier.
    pub id: String,
    /// Required-nullable workflow version.
    pub version: Nullable<String>,
    /// Required-nullable state variables.
    pub state_variables: Nullable<BTreeMap<String, ChatKitStateValue>>,
    /// Resolved tracing settings.
    pub tracing: ChatKitWorkflowTracing,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved rate-limit settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitRateLimits {
    /// Requests allowed in one minute.
    pub max_requests_per_1_minute: u64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved automatic-thread-titling settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitAutomaticThreadTitling {
    /// Whether automatic title generation is enabled.
    pub enabled: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved file-upload settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitFileUpload {
    /// Whether uploads are enabled.
    pub enabled: bool,
    /// Required-nullable size limit.
    pub max_file_size: Nullable<u64>,
    /// Required-nullable count limit.
    pub max_files: Nullable<u64>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved history settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitHistory {
    /// Whether history is enabled.
    pub enabled: bool,
    /// Required-nullable number of recent threads.
    pub recent_threads: Nullable<u64>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Resolved ChatKit session configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitConfiguration {
    /// Automatic titling settings.
    pub automatic_thread_titling: ChatKitAutomaticThreadTitling,
    /// File-upload settings.
    pub file_upload: ChatKitFileUpload,
    /// History settings.
    pub history: ChatKitHistory,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// A provisioned Beta ChatKit session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitSession {
    /// Session identifier.
    pub id: ChatKitSessionId,
    /// Session object discriminator.
    pub object: ChatKitSessionObject,
    /// Expiration timestamp in Unix seconds.
    pub expires_at: i64,
    /// Ephemeral secret handed to the ChatKit frontend.
    pub client_secret: WireSecret,
    /// Resolved workflow metadata.
    pub workflow: ChatKitWorkflow,
    /// End-user scope.
    pub user: String,
    /// Resolved rate limits.
    pub rate_limits: ChatKitRateLimits,
    /// Convenience copy of the per-minute limit.
    pub max_requests_per_1_minute: u64,
    /// Lifecycle status.
    pub status: ChatKitSessionStatus,
    /// Resolved runtime feature configuration.
    pub chatkit_configuration: ChatKitConfiguration,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatKitSession {
    /// Future response properties.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

fn tagged_type<'a>(value: &'a Value, context: &'static str) -> Result<&'a str, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{context} must be a JSON object"))?
        .get("type")
        .ok_or_else(|| format!("{context} is missing string field `type`"))?
        .as_str()
        .ok_or_else(|| format!("{context} field `type` must be a string"))
}

literal_tag!(ActiveStatusTag, Active, "active");
literal_tag!(LockedStatusTag, Locked, "locked");
literal_tag!(ClosedStatusTag, Closed, "closed");

/// Active ChatKit thread status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitActiveThreadStatus {
    #[serde(rename = "type")]
    kind: ActiveStatusTag,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Locked ChatKit thread status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitLockedThreadStatus {
    #[serde(rename = "type")]
    kind: LockedStatusTag,
    /// Required-nullable lock reason.
    pub reason: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Closed ChatKit thread status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitClosedThreadStatus {
    #[serde(rename = "type")]
    kind: ClosedStatusTag,
    /// Required-nullable close reason.
    pub reason: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Discriminator-aware ChatKit thread status.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatKitThreadStatus {
    /// Thread accepts input.
    Active(ChatKitActiveThreadStatus),
    /// Thread is temporarily locked.
    Locked(ChatKitLockedThreadStatus),
    /// Thread is closed.
    Closed(ChatKitClosedThreadStatus),
    /// Future status retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ChatKitThreadStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Active(value) => value.serialize(serializer),
            Self::Locked(value) => value.serialize(serializer),
            Self::Closed(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatKitThreadStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "ChatKit thread status").map_err(serde::de::Error::custom)? {
            "active" => serde_json::from_value(value)
                .map(Self::Active)
                .map_err(serde::de::Error::custom),
            "locked" => serde_json::from_value(value)
                .map(Self::Locked)
                .map_err(serde::de::Error::custom),
            "closed" => serde_json::from_value(value)
                .map(Self::Closed)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// ChatKit thread resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitThread {
    /// Thread identifier.
    pub id: ChatKitThreadId,
    /// Object discriminator.
    pub object: ChatKitThreadObject,
    /// Creation timestamp in Unix seconds.
    pub created_at: i64,
    /// Required-nullable generated title.
    pub title: Nullable<String>,
    /// Current thread status.
    pub status: ChatKitThreadStatus,
    /// Owning end user.
    pub user: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatKitThread {
    /// Future response properties.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation returned after deleting a ChatKit thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeletedChatKitThread {
    /// Deleted thread identifier.
    pub id: ChatKitThreadId,
    /// Object discriminator.
    pub object: DeletedChatKitThreadObject,
    /// Whether deletion completed.
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Query parameters for listing ChatKit threads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitThreadListParams {
    /// Page size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<ChatKitListLimit>,
    /// Creation-time order.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ChatKitListOrder>,
    /// Forward cursor.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<ChatKitThreadId>,
    /// Backward cursor.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub before: Omittable<ChatKitThreadId>,
    /// Optional end-user filter.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<ChatKitUserFilter>,
}

impl ChatKitThreadListParams {
    /// Creates an unfiltered request using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by one validated user identifier.
    pub fn with_user(mut self, user: impl Into<Box<str>>) -> Result<Self, ChatKitValidationError> {
        self.user = Omittable::Value(ChatKitUserFilter::new(user)?);
        Ok(self)
    }

    /// Sets a forward cursor.
    #[must_use]
    pub fn after(mut self, cursor: impl Into<ChatKitThreadId>) -> Self {
        self.after = Omittable::Value(cursor.into());
        self
    }
}

/// Query parameters for listing one thread's items.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatKitThreadItemListParams {
    /// Page size.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<ChatKitListLimit>,
    /// Creation-time order.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<ChatKitListOrder>,
    /// Forward item cursor.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<ChatKitThreadItemId>,
    /// Backward item cursor.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub before: Omittable<ChatKitThreadItemId>,
}

impl ChatKitThreadItemListParams {
    /// Creates a request using service defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a forward cursor.
    #[must_use]
    pub fn after(mut self, cursor: impl Into<ChatKitThreadItemId>) -> Self {
        self.after = Omittable::Value(cursor.into());
        self
    }
}

/// Page of ChatKit threads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitThreadList {
    /// List discriminator.
    pub object: ChatKitListObject,
    /// Threads in this page.
    pub data: Vec<ChatKitThread>,
    /// Required-nullable first cursor.
    pub first_id: Nullable<ChatKitThreadId>,
    /// Required-nullable last cursor.
    pub last_id: Nullable<ChatKitThreadId>,
    /// Whether another page is available.
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatKitThreadList {
    /// Returns a forward cursor only when another page is advertised.
    #[must_use]
    pub fn next_after(&self) -> Option<&ChatKitThreadId> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }
}

/// Attachment metadata on a user message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitAttachment {
    /// Attachment kind.
    #[serde(rename = "type")]
    pub kind: ChatKitAttachmentType,
    /// Attachment identifier.
    pub id: String,
    /// Original display name.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// Required-nullable preview URL.
    pub preview_url: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Tool selection recorded in user-message inference options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitToolChoice {
    /// Requested tool identifier.
    pub id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Required inference choices recorded on a user message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitInferenceOptions {
    /// Required-nullable tool choice.
    pub tool_choice: Nullable<ChatKitToolChoice>,
    /// Required-nullable model override.
    pub model: Nullable<ModelId>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

literal_tag!(InputTextTag, InputText, "input_text");
literal_tag!(QuotedTextTag, QuotedText, "quoted_text");

/// Plain user text content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitUserInputText {
    #[serde(rename = "type")]
    kind: InputTextTag,
    /// Text supplied by the user.
    pub text: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// User-quoted text content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitUserQuotedText {
    #[serde(rename = "type")]
    kind: QuotedTextTag,
    /// Quoted text.
    pub text: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Discriminator-aware user-message content block.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatKitUserMessageContent {
    /// Plain input text.
    InputText(ChatKitUserInputText),
    /// Quoted text.
    QuotedText(ChatKitUserQuotedText),
    /// Future content block retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ChatKitUserMessageContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::InputText(value) => value.serialize(serializer),
            Self::QuotedText(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatKitUserMessageContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "ChatKit user content").map_err(serde::de::Error::custom)? {
            "input_text" => serde_json::from_value(value)
                .map(Self::InputText)
                .map_err(serde::de::Error::custom),
            "quoted_text" => serde_json::from_value(value)
                .map(Self::QuotedText)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

literal_tag!(FileSourceTag, File, "file");
literal_tag!(UrlSourceTag, Url, "url");

/// File source referenced by an assistant annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitFileAnnotationSource {
    #[serde(rename = "type")]
    kind: FileSourceTag,
    /// Referenced filename.
    pub filename: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// URL source referenced by an assistant annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitUrlAnnotationSource {
    #[serde(rename = "type")]
    kind: UrlSourceTag,
    /// Referenced URL.
    pub url: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// File annotation attached to assistant text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitFileAnnotation {
    #[serde(rename = "type")]
    kind: FileSourceTag,
    /// Referenced file source.
    pub source: ChatKitFileAnnotationSource,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// URL annotation attached to assistant text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitUrlAnnotation {
    #[serde(rename = "type")]
    kind: UrlSourceTag,
    /// Referenced URL source.
    pub source: ChatKitUrlAnnotationSource,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Annotation attached to assistant output text.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatKitAnnotation {
    /// File citation.
    File(ChatKitFileAnnotation),
    /// URL citation.
    Url(ChatKitUrlAnnotation),
    /// Future annotation retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ChatKitAnnotation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::File(value) => value.serialize(serializer),
            Self::Url(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatKitAnnotation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "ChatKit annotation").map_err(serde::de::Error::custom)? {
            "file" => serde_json::from_value(value)
                .map(Self::File)
                .map_err(serde::de::Error::custom),
            "url" => serde_json::from_value(value)
                .map(Self::Url)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

literal_tag!(OutputTextTag, OutputText, "output_text");

/// Assistant output text with citations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitResponseOutputText {
    #[serde(rename = "type")]
    kind: OutputTextTag,
    /// Assistant-generated text.
    pub text: String,
    /// Ordered annotations.
    pub annotations: Vec<ChatKitAnnotation>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

literal_tag!(UserMessageTag, UserMessage, "chatkit.user_message");
literal_tag!(
    AssistantMessageTag,
    AssistantMessage,
    "chatkit.assistant_message"
);
literal_tag!(WidgetTag, Widget, "chatkit.widget");
literal_tag!(
    ClientToolCallTag,
    ClientToolCall,
    "chatkit.client_tool_call"
);
literal_tag!(TaskTag, Task, "chatkit.task");
literal_tag!(TaskGroupTag, TaskGroup, "chatkit.task_group");

/// User-authored ChatKit thread item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitUserMessageItem {
    /// Item identifier.
    pub id: ChatKitThreadItemId,
    /// Object discriminator.
    pub object: ChatKitThreadItemObject,
    /// Creation timestamp.
    pub created_at: i64,
    /// Parent thread identifier.
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: UserMessageTag,
    /// Ordered user content.
    pub content: Vec<ChatKitUserMessageContent>,
    /// Ordered attachments.
    pub attachments: Vec<ChatKitAttachment>,
    /// Required-nullable inference overrides.
    pub inference_options: Nullable<ChatKitInferenceOptions>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Assistant-authored ChatKit thread item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitAssistantMessageItem {
    pub id: ChatKitThreadItemId,
    pub object: ChatKitThreadItemObject,
    pub created_at: i64,
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: AssistantMessageTag,
    /// Ordered assistant text segments.
    pub content: Vec<ChatKitResponseOutputText>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Widget-rendering ChatKit thread item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitWidgetItem {
    pub id: ChatKitThreadItemId,
    pub object: ChatKitThreadItemObject,
    pub created_at: i64,
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: WidgetTag,
    /// Serialized widget payload.
    pub widget: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Client-side tool call recorded in a ChatKit thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitClientToolCallItem {
    pub id: ChatKitThreadItemId,
    pub object: ChatKitThreadItemObject,
    pub created_at: i64,
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: ClientToolCallTag,
    /// Execution status.
    pub status: ChatKitClientToolCallStatus,
    /// Tool-call identifier.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// JSON text arguments retained lazily.
    pub arguments: JsonText,
    /// Required-nullable JSON text output.
    pub output: Nullable<JsonText>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// One standalone workflow task item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitTaskItem {
    pub id: ChatKitThreadItemId,
    pub object: ChatKitThreadItemObject,
    pub created_at: i64,
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: TaskTag,
    /// Task subtype.
    pub task_type: ChatKitTaskType,
    /// Required-nullable heading.
    pub heading: Nullable<String>,
    /// Required-nullable summary.
    pub summary: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// One task inside a task-group item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitTaskGroupTask {
    /// Task subtype.
    #[serde(rename = "type")]
    pub task_type: ChatKitTaskType,
    /// Required-nullable heading.
    pub heading: Nullable<String>,
    /// Required-nullable summary.
    pub summary: Nullable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Group of workflow tasks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatKitTaskGroupItem {
    pub id: ChatKitThreadItemId,
    pub object: ChatKitThreadItemObject,
    pub created_at: i64,
    pub thread_id: ChatKitThreadId,
    #[serde(rename = "type")]
    kind: TaskGroupTag,
    /// Tasks included in the group.
    pub tasks: Vec<ChatKitTaskGroupTask>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Complete ChatKit thread-item union.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ChatKitThreadItem {
    /// User-authored message.
    UserMessage(ChatKitUserMessageItem),
    /// Assistant-authored message.
    AssistantMessage(ChatKitAssistantMessageItem),
    /// Widget payload.
    Widget(ChatKitWidgetItem),
    /// Client-side tool call.
    ClientToolCall(ChatKitClientToolCallItem),
    /// Standalone task.
    Task(ChatKitTaskItem),
    /// Task group.
    TaskGroup(ChatKitTaskGroupItem),
    /// Future thread-item type retained without loss.
    Unknown(UnknownTaggedObject),
}

impl Serialize for ChatKitThreadItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::UserMessage(value) => value.serialize(serializer),
            Self::AssistantMessage(value) => value.serialize(serializer),
            Self::Widget(value) => value.serialize(serializer),
            Self::ClientToolCall(value) => value.serialize(serializer),
            Self::Task(value) => value.serialize(serializer),
            Self::TaskGroup(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ChatKitThreadItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match tagged_type(&value, "ChatKit thread item").map_err(serde::de::Error::custom)? {
            "chatkit.user_message" => serde_json::from_value(value)
                .map(Self::UserMessage)
                .map_err(serde::de::Error::custom),
            "chatkit.assistant_message" => serde_json::from_value(value)
                .map(Self::AssistantMessage)
                .map_err(serde::de::Error::custom),
            "chatkit.widget" => serde_json::from_value(value)
                .map(Self::Widget)
                .map_err(serde::de::Error::custom),
            "chatkit.client_tool_call" => serde_json::from_value(value)
                .map(Self::ClientToolCall)
                .map_err(serde::de::Error::custom),
            "chatkit.task" => serde_json::from_value(value)
                .map(Self::Task)
                .map_err(serde::de::Error::custom),
            "chatkit.task_group" => serde_json::from_value(value)
                .map(Self::TaskGroup)
                .map_err(serde::de::Error::custom),
            _ => UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Page of ChatKit thread items.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatKitThreadItemList {
    /// List discriminator.
    pub object: ChatKitListObject,
    /// Items in this page.
    pub data: Vec<ChatKitThreadItem>,
    /// Required-nullable first cursor.
    pub first_id: Nullable<ChatKitThreadItemId>,
    /// Required-nullable last cursor.
    pub last_id: Nullable<ChatKitThreadItemId>,
    /// Whether another page is available.
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ChatKitThreadItemList {
    /// Returns a forward cursor only when another page is advertised.
    #[must_use]
    pub fn next_after(&self) -> Option<&ChatKitThreadItemId> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Nullable::Value(value) => Some(value),
            Nullable::Null => None,
        }
    }
}

macro_rules! extra_accessor {
    ($($type:ty),+ $(,)?) => {
        $(
            impl $type {
                /// Response properties not known by this crate version.
                #[must_use]
                pub const fn extra(&self) -> &ExtraFields {
                    &self.extra
                }
            }
        )+
    };
}

extra_accessor!(
    ChatKitWorkflowTracing,
    ChatKitWorkflow,
    ChatKitRateLimits,
    ChatKitAutomaticThreadTitling,
    ChatKitFileUpload,
    ChatKitHistory,
    ChatKitConfiguration,
    ChatKitActiveThreadStatus,
    ChatKitLockedThreadStatus,
    ChatKitClosedThreadStatus,
    DeletedChatKitThread,
    ChatKitThreadList,
    ChatKitAttachment,
    ChatKitToolChoice,
    ChatKitInferenceOptions,
    ChatKitUserInputText,
    ChatKitUserQuotedText,
    ChatKitFileAnnotationSource,
    ChatKitUrlAnnotationSource,
    ChatKitFileAnnotation,
    ChatKitUrlAnnotation,
    ChatKitResponseOutputText,
    ChatKitUserMessageItem,
    ChatKitAssistantMessageItem,
    ChatKitWidgetItem,
    ChatKitClientToolCallItem,
    ChatKitTaskItem,
    ChatKitTaskGroupTask,
    ChatKitTaskGroupItem,
    ChatKitThreadItemList,
);

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(CreateChatKitSessionRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatKitSession: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatKitThread: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatKitThreadItem: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatKitThreadList: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ChatKitThreadItemList: Serialize, DeserializeOwned, Send, Sync);

    fn session_json() -> Value {
        json!({
            "id": "cksess_123",
            "object": "chatkit.session",
            "expires_at": 1712349876,
            "client_secret": "ek_private_value",
            "workflow": {
                "id": "workflow_alpha",
                "version": null,
                "state_variables": null,
                "tracing": {"enabled": true}
            },
            "user": "user_789",
            "rate_limits": {"max_requests_per_1_minute": 60},
            "max_requests_per_1_minute": 60,
            "status": "active",
            "chatkit_configuration": {
                "automatic_thread_titling": {"enabled": true},
                "file_upload": {"enabled": true, "max_file_size": 16, "max_files": 20},
                "history": {"enabled": true, "recent_threads": null}
            }
        })
    }

    #[test]
    fn create_session_needs_no_handwritten_json_and_validates_bounds() {
        let mut state = ChatKitStateVariables::new();
        state
            .insert(
                "tenant",
                ChatKitStateValue::string("blue").expect("state string"),
            )
            .expect("state variable");
        let workflow = ChatKitWorkflowRequest::new("workflow_alpha")
            .with_version("2026-01-01")
            .with_state_variables(state);
        let configuration = ChatKitConfigurationRequest {
            automatic_thread_titling: Omittable::Value(ChatKitAutomaticThreadTitlingRequest {
                enabled: Omittable::Value(false),
            }),
            file_upload: Omittable::Value(ChatKitFileUploadRequest {
                enabled: Omittable::Value(true),
                max_file_size: Omittable::Value(ChatKitFileSizeMb::new(32).expect("file size")),
                max_files: Omittable::Value(ChatKitPositiveLimit::new(4).expect("file count")),
            }),
            history: Omittable::Omitted,
        };
        let request = CreateChatKitSessionRequest::new(workflow, "user_789")
            .expect("session request")
            .with_expiration(ChatKitExpiresAfterRequest::new(600).expect("expiration"))
            .with_configuration(configuration);
        let value = serde_json::to_value(request).expect("serialize request");
        assert_eq!(value["workflow"]["state_variables"]["tenant"], "blue");
        assert_eq!(value["expires_after"]["seconds"], 600);
        assert!(value.get("rate_limits").is_none());
        assert!(ChatKitExpiresAfterRequest::new(0).is_err());
        assert!(ChatKitFileSizeMb::new(513).is_err());
        assert!(CreateChatKitSessionRequest::new(ChatKitWorkflowRequest::new("wf"), "").is_err());
        assert!(ChatKitUserId::new("x".repeat(513)).is_ok());
        assert!(ChatKitUserFilter::new("x".repeat(513)).is_err());
    }

    #[test]
    fn session_secret_is_redacted_and_required_nulls_roundtrip() {
        let value = session_json();
        let session: ChatKitSession = serde_json::from_value(value.clone()).expect("session");
        let debug = format!("{session:?}");
        assert!(!debug.contains("ek_private_value"));
        assert!(matches!(session.workflow.version, Nullable::Null));
        assert!(matches!(session.workflow.state_variables, Nullable::Null));
        assert_eq!(serde_json::to_value(session).expect("roundtrip"), value);
    }

    #[test]
    fn thread_status_is_strict_for_known_and_lossless_for_future() {
        let thread = json!({
            "id": "cthr_1",
            "object": "chatkit.thread",
            "created_at": 1,
            "title": null,
            "status": {"type": "locked", "reason": null},
            "user": "user_1",
            "future": {"retained": true}
        });
        let decoded: ChatKitThread = serde_json::from_value(thread.clone()).expect("thread");
        assert!(matches!(decoded.status, ChatKitThreadStatus::Locked(_)));
        assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), thread);

        assert!(serde_json::from_value::<ChatKitThreadStatus>(json!({"type":"locked"})).is_err());
        let future = json!({"type":"archived", "reason":"future"});
        let decoded: ChatKitThreadStatus =
            serde_json::from_value(future.clone()).expect("future status");
        assert!(matches!(decoded, ChatKitThreadStatus::Unknown(_)));
        assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), future);
    }

    fn item_base(id: &str, kind: &str) -> serde_json::Map<String, Value> {
        let Value::Object(value) = json!({
            "id": id,
            "object": "chatkit.thread_item",
            "created_at": 1,
            "thread_id": "cthr_1",
            "type": kind
        }) else {
            unreachable!();
        };
        value
    }

    #[test]
    fn all_six_thread_item_variants_decode_and_roundtrip() {
        let mut user = item_base("item_user", "chatkit.user_message");
        user.insert(
            "content".into(),
            json!([{"type":"input_text","text":"hello"}]),
        );
        user.insert("attachments".into(), json!([]));
        user.insert("inference_options".into(), Value::Null);

        let mut assistant = item_base("item_assistant", "chatkit.assistant_message");
        assistant.insert(
            "content".into(),
            json!([{"type":"output_text","text":"hi","annotations":[]}]),
        );

        let mut widget = item_base("item_widget", "chatkit.widget");
        widget.insert("widget".into(), json!("{\"type\":\"card\"}"));

        let mut tool = item_base("item_tool", "chatkit.client_tool_call");
        tool.insert("status".into(), json!("in_progress"));
        tool.insert("call_id".into(), json!("call_1"));
        tool.insert("name".into(), json!("lookup"));
        tool.insert("arguments".into(), json!("{\"q\":\"rust\"}"));
        tool.insert("output".into(), Value::Null);

        let mut task = item_base("item_task", "chatkit.task");
        task.insert("task_type".into(), json!("thought"));
        task.insert("heading".into(), Value::Null);
        task.insert("summary".into(), json!("working"));

        let mut group = item_base("item_group", "chatkit.task_group");
        group.insert(
            "tasks".into(),
            json!([{"type":"custom","heading":"step","summary":null}]),
        );

        let values = [user, assistant, widget, tool, task, group]
            .into_iter()
            .map(Value::Object)
            .collect::<Vec<_>>();
        let decoded = values
            .iter()
            .cloned()
            .map(serde_json::from_value::<ChatKitThreadItem>)
            .collect::<Result<Vec<_>, _>>()
            .expect("all item variants");
        assert!(matches!(decoded[0], ChatKitThreadItem::UserMessage(_)));
        assert!(matches!(decoded[1], ChatKitThreadItem::AssistantMessage(_)));
        assert!(matches!(decoded[2], ChatKitThreadItem::Widget(_)));
        assert!(matches!(decoded[3], ChatKitThreadItem::ClientToolCall(_)));
        assert!(matches!(decoded[4], ChatKitThreadItem::Task(_)));
        assert!(matches!(decoded[5], ChatKitThreadItem::TaskGroup(_)));
        assert_eq!(
            decoded
                .into_iter()
                .map(|value| serde_json::to_value(value).expect("serialize item"))
                .collect::<Vec<_>>(),
            values
        );
    }

    #[test]
    fn malformed_known_item_fails_and_future_item_roundtrips() {
        assert!(
            serde_json::from_value::<ChatKitThreadItem>(json!({
                "id":"item_1","object":"chatkit.thread_item","created_at":1,
                "thread_id":"cthr_1","type":"chatkit.task","task_type":"custom",
                "heading":null
            }))
            .is_err()
        );
        let future = json!({
            "id":"item_future","object":"chatkit.thread_item","created_at":1,
            "thread_id":"cthr_1","type":"chatkit.timeline","events":[]
        });
        let decoded: ChatKitThreadItem =
            serde_json::from_value(future.clone()).expect("future item");
        assert!(matches!(decoded, ChatKitThreadItem::Unknown(_)));
        assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), future);
    }

    #[test]
    fn page_cursors_are_required_nullable() {
        let value = json!({
            "object":"list","data":[],"first_id":null,"last_id":null,"has_more":false
        });
        let page: ChatKitThreadItemList =
            serde_json::from_value(value.clone()).expect("empty page");
        assert!(page.next_after().is_none());
        assert_eq!(serde_json::to_value(page).expect("roundtrip"), value);

        assert!(
            serde_json::from_value::<ChatKitThreadItemList>(json!({
                "object":"list","data":[],"first_id":null,"has_more":false
            }))
            .is_err()
        );
    }

    proptest! {
        #[test]
        fn open_session_status_roundtrips(raw in "[a-z_]{1,24}") {
            let status = ChatKitSessionStatus::from_raw(raw.clone());
            let encoded = serde_json::to_vec(&status).expect("encode");
            let decoded: ChatKitSessionStatus = serde_json::from_slice(&encoded).expect("decode");
            prop_assert_eq!(decoded.as_str(), raw);
        }
    }
}
