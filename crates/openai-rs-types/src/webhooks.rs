//! Typed OpenAI webhook receiver events.
//!
//! This module parses webhook JSON only. Signature verification must happen on
//! the original, unparsed request bytes in the client crate before wrapping a
//! parsed value in [`VerifiedWebhook`].

use std::{any::type_name, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use crate::{
    kernel::{ExtraFields, Omittable},
    responses::UnknownTaggedObject,
    scalar::{BatchId, FineTuningJobId, ResponseId},
};

/// The complete discriminator inventory from the pinned OpenAPI webhook map.
pub const WEBHOOK_EVENT_DISCRIMINATORS: [&str; 18] = [
    "batch.cancelled",
    "batch.completed",
    "batch.expired",
    "batch.failed",
    "eval.run.canceled",
    "eval.run.failed",
    "eval.run.succeeded",
    "fine_tuning.job.cancelled",
    "fine_tuning.job.failed",
    "fine_tuning.job.succeeded",
    "live.call.incoming",
    "realtime.call.incoming",
    "response.cancelled",
    "response.completed",
    "response.failed",
    "response.incomplete",
    "safety.alert.created",
    "safety.org_alert.created",
];

macro_rules! redacted_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Box<str>);

        impl $name {
            /// Creates an opaque identifier without imposing format assumptions.
            #[must_use]
            pub fn new(value: impl Into<Box<str>>) -> Self {
                Self(value.into())
            }

            /// Borrows the exact identifier from the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the wrapper and returns the identifier.
            #[must_use]
            pub fn into_boxed_str(self) -> Box<str> {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&"<redacted>").finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }
    };
}

redacted_id! {
    /// Opaque identifier of one webhook delivery event.
    WebhookEventId
}

redacted_id! {
    /// Opaque identifier of an evaluation run.
    EvalRunId
}

redacted_id! {
    /// Opaque Live session identifier carried by an incoming SIP webhook.
    LiveSessionId
}

redacted_id! {
    /// Opaque Realtime call identifier carried by an incoming SIP webhook.
    RealtimeCallId
}

redacted_id! {
    /// Opaque safety alert identifier.
    SafetyAlertId
}

macro_rules! id_payload {
    ($(#[$meta:meta])* $name:ident, $id:ty) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            id: $id,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the resource identifier carried by the webhook.
            #[must_use]
            pub const fn id(&self) -> &$id {
                &self.id
            }

            /// Returns future payload fields retained during decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("extra_fields", &self.extra)
                    .finish_non_exhaustive()
            }
        }
    };
}

id_payload! {
    /// Payload shared by Batch lifecycle webhook events.
    BatchWebhookData,
    BatchId
}

id_payload! {
    /// Payload shared by evaluation-run lifecycle webhook events.
    EvalRunWebhookData,
    EvalRunId
}

id_payload! {
    /// Payload shared by fine-tuning job lifecycle webhook events.
    FineTuningJobWebhookData,
    FineTuningJobId
}

id_payload! {
    /// Payload shared by background Response lifecycle webhook events.
    ResponseWebhookData,
    ResponseId
}

id_payload! {
    /// Payload shared by safety alert webhook events.
    SafetyAlertWebhookData,
    SafetyAlertId
}

/// A header copied from the incoming SIP INVITE.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SipHeader {
    name: String,
    value: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl SipHeader {
    /// Returns the SIP header name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the SIP header value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns future fields retained from this header object.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for SipHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SipHeader")
            .field("extra_fields", &self.extra)
            .finish_non_exhaustive()
    }
}

/// Payload for `live.call.incoming`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveCallIncomingWebhookData {
    session_id: LiveSessionId,
    sip_headers: Vec<SipHeader>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LiveCallIncomingWebhookData {
    /// Returns the pending Live session identifier.
    #[must_use]
    pub const fn session_id(&self) -> &LiveSessionId {
        &self.session_id
    }

    /// Returns headers copied from the SIP INVITE.
    #[must_use]
    pub fn sip_headers(&self) -> &[SipHeader] {
        &self.sip_headers
    }

    /// Returns future payload fields retained during decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for LiveCallIncomingWebhookData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveCallIncomingWebhookData")
            .field("sip_header_count", &self.sip_headers.len())
            .field("extra_fields", &self.extra)
            .finish_non_exhaustive()
    }
}

/// Payload for `realtime.call.incoming`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeCallIncomingWebhookData {
    call_id: RealtimeCallId,
    sip_headers: Vec<SipHeader>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl RealtimeCallIncomingWebhookData {
    /// Returns the pending Realtime call identifier.
    #[must_use]
    pub const fn call_id(&self) -> &RealtimeCallId {
        &self.call_id
    }

    /// Returns headers copied from the SIP INVITE.
    #[must_use]
    pub fn sip_headers(&self) -> &[SipHeader] {
        &self.sip_headers
    }

    /// Returns future payload fields retained during decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl fmt::Debug for RealtimeCallIncomingWebhookData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeCallIncomingWebhookData")
            .field("sip_header_count", &self.sip_headers.len())
            .field("extra_fields", &self.extra)
            .finish_non_exhaustive()
    }
}

literal_tag!(WebhookObject, "event");

macro_rules! event_accessors {
    ($name:ident, $data:ty, $wire:literal, $object_present:expr) => {
        impl $name {
            /// Returns the exact, known discriminator.
            #[must_use]
            pub const fn event_type(&self) -> &'static str {
                $wire
            }

            /// Returns the webhook delivery identifier.
            #[must_use]
            pub const fn id(&self) -> &WebhookEventId {
                &self.id
            }

            /// Returns the event creation time as Unix seconds.
            #[must_use]
            pub const fn created_at(&self) -> i64 {
                self.created_at
            }

            /// Returns the typed event payload.
            #[must_use]
            pub const fn data(&self) -> &$data {
                &self.data
            }

            /// Returns whether the `object = "event"` marker was present.
            #[must_use]
            pub fn object_marker_present(&self) -> bool {
                ($object_present)(self)
            }

            /// Returns future top-level fields retained during decoding.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("event_type", &$wire)
                    .field("created_at", &self.created_at)
                    .field("extra_fields", &self.extra)
                    .finish_non_exhaustive()
            }
        }
    };
}

macro_rules! webhook_event {
    ($(#[$meta:meta])* $name:ident, $tag:ident, $wire:literal, $data:ty, optional_object) => {
        literal_tag!($tag, $wire);

        $(#[$meta])*
        #[derive(Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            id: WebhookEventId,
            created_at: i64,
            data: $data,
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            object: Omittable<WebhookObject>,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        event_accessors!($name, $data, $wire, |event: &$name| event.object.is_value());
    };
    ($(#[$meta:meta])* $name:ident, $tag:ident, $wire:literal, $data:ty, required_object) => {
        literal_tag!($tag, $wire);

        $(#[$meta])*
        #[derive(Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            id: WebhookEventId,
            object: WebhookObject,
            created_at: i64,
            data: $data,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        event_accessors!($name, $data, $wire, |_event: &$name| true);
    };
}

webhook_event! {
    /// Sent when a Batch API request is cancelled.
    BatchCancelledWebhookEvent,
    BatchCancelledWebhookTag,
    "batch.cancelled",
    BatchWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a Batch API request completes.
    BatchCompletedWebhookEvent,
    BatchCompletedWebhookTag,
    "batch.completed",
    BatchWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a Batch API request expires.
    BatchExpiredWebhookEvent,
    BatchExpiredWebhookTag,
    "batch.expired",
    BatchWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a Batch API request fails.
    BatchFailedWebhookEvent,
    BatchFailedWebhookTag,
    "batch.failed",
    BatchWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an evaluation run is canceled.
    EvalRunCanceledWebhookEvent,
    EvalRunCanceledWebhookTag,
    "eval.run.canceled",
    EvalRunWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an evaluation run fails.
    EvalRunFailedWebhookEvent,
    EvalRunFailedWebhookTag,
    "eval.run.failed",
    EvalRunWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an evaluation run succeeds.
    EvalRunSucceededWebhookEvent,
    EvalRunSucceededWebhookTag,
    "eval.run.succeeded",
    EvalRunWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a fine-tuning job is cancelled.
    FineTuningJobCancelledWebhookEvent,
    FineTuningJobCancelledWebhookTag,
    "fine_tuning.job.cancelled",
    FineTuningJobWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a fine-tuning job fails.
    FineTuningJobFailedWebhookEvent,
    FineTuningJobFailedWebhookTag,
    "fine_tuning.job.failed",
    FineTuningJobWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a fine-tuning job succeeds.
    FineTuningJobSucceededWebhookEvent,
    FineTuningJobSucceededWebhookTag,
    "fine_tuning.job.succeeded",
    FineTuningJobWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an incoming SIP session is available for Live acceptance.
    LiveCallIncomingWebhookEvent,
    LiveCallIncomingWebhookTag,
    "live.call.incoming",
    LiveCallIncomingWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an incoming SIP session is available for Realtime acceptance.
    RealtimeCallIncomingWebhookEvent,
    RealtimeCallIncomingWebhookTag,
    "realtime.call.incoming",
    RealtimeCallIncomingWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a background Response is cancelled.
    ResponseCancelledWebhookEvent,
    ResponseCancelledWebhookTag,
    "response.cancelled",
    ResponseWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a background Response completes.
    ResponseCompletedWebhookEvent,
    ResponseCompletedWebhookTag,
    "response.completed",
    ResponseWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a background Response fails.
    ResponseFailedWebhookEvent,
    ResponseFailedWebhookTag,
    "response.failed",
    ResponseWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when a background Response finishes incomplete.
    ResponseIncompleteWebhookEvent,
    ResponseIncompleteWebhookTag,
    "response.incomplete",
    ResponseWebhookData,
    optional_object
}

webhook_event! {
    /// Sent when an approved safety alert is available for an API project.
    SafetyAlertCreatedWebhookEvent,
    SafetyAlertCreatedWebhookTag,
    "safety.alert.created",
    SafetyAlertWebhookData,
    required_object
}

webhook_event! {
    /// Sent when an approved safety alert is available for an enterprise workspace.
    SafetyOrgAlertCreatedWebhookEvent,
    SafetyOrgAlertCreatedWebhookTag,
    "safety.org_alert.created",
    SafetyAlertWebhookData,
    required_object
}

macro_rules! webhook_union {
    ($($variant:ident($event:ty) => $wire:literal),+ $(,)?) => {
        /// A webhook event from the complete pinned 18-event receiver surface.
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum WebhookEvent {
            $($variant($event),)+
            /// A future event retained as a complete semantic JSON object.
            Unknown(UnknownTaggedObject),
        }

        impl Serialize for WebhookEvent {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $(Self::$variant(event) => event.serialize(serializer),)+
                    Self::Unknown(event) => event.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for WebhookEvent {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Value::deserialize(deserializer)?;
                let event_type = object_discriminator(&value).map_err(D::Error::custom)?;
                match event_type {
                    $($wire => serde_json::from_value::<$event>(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }

        impl WebhookEvent {
            /// Returns the exact event discriminator.
            #[must_use]
            pub fn event_type(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $wire,)+
                    Self::Unknown(event) => event.discriminator(),
                }
            }

            /// Returns whether the event is one of the pinned 18 variants.
            #[must_use]
            pub const fn is_known(&self) -> bool {
                !matches!(self, Self::Unknown(_))
            }

            /// Returns an unknown event without exposing mutable raw fields.
            #[must_use]
            pub const fn unknown(&self) -> Option<&UnknownTaggedObject> {
                match self {
                    Self::Unknown(event) => Some(event),
                    _ => None,
                }
            }

            /// Returns the delivery id for a known event.
            #[must_use]
            pub const fn id(&self) -> Option<&WebhookEventId> {
                match self {
                    $(Self::$variant(event) => Some(event.id()),)+
                    Self::Unknown(_) => None,
                }
            }

            /// Returns the creation timestamp for a known event.
            #[must_use]
            pub const fn created_at(&self) -> Option<i64> {
                match self {
                    $(Self::$variant(event) => Some(event.created_at()),)+
                    Self::Unknown(_) => None,
                }
            }

            /// Returns retained top-level fields for a known event.
            #[must_use]
            pub const fn extra_fields(&self) -> Option<&ExtraFields> {
                match self {
                    $(Self::$variant(event) => Some(event.extra_fields()),)+
                    Self::Unknown(_) => None,
                }
            }
        }
    };
}

webhook_union! {
    BatchCancelled(BatchCancelledWebhookEvent) => "batch.cancelled",
    BatchCompleted(BatchCompletedWebhookEvent) => "batch.completed",
    BatchExpired(BatchExpiredWebhookEvent) => "batch.expired",
    BatchFailed(BatchFailedWebhookEvent) => "batch.failed",
    EvalRunCanceled(EvalRunCanceledWebhookEvent) => "eval.run.canceled",
    EvalRunFailed(EvalRunFailedWebhookEvent) => "eval.run.failed",
    EvalRunSucceeded(EvalRunSucceededWebhookEvent) => "eval.run.succeeded",
    FineTuningJobCancelled(FineTuningJobCancelledWebhookEvent) => "fine_tuning.job.cancelled",
    FineTuningJobFailed(FineTuningJobFailedWebhookEvent) => "fine_tuning.job.failed",
    FineTuningJobSucceeded(FineTuningJobSucceededWebhookEvent) => "fine_tuning.job.succeeded",
    LiveCallIncoming(LiveCallIncomingWebhookEvent) => "live.call.incoming",
    RealtimeCallIncoming(RealtimeCallIncomingWebhookEvent) => "realtime.call.incoming",
    ResponseCancelled(ResponseCancelledWebhookEvent) => "response.cancelled",
    ResponseCompleted(ResponseCompletedWebhookEvent) => "response.completed",
    ResponseFailed(ResponseFailedWebhookEvent) => "response.failed",
    ResponseIncomplete(ResponseIncompleteWebhookEvent) => "response.incomplete",
    SafetyAlertCreated(SafetyAlertCreatedWebhookEvent) => "safety.alert.created",
    SafetyOrgAlertCreated(SafetyOrgAlertCreatedWebhookEvent) => "safety.org_alert.created",
}

fn object_discriminator(value: &Value) -> Result<&str, &'static str> {
    let object = value
        .as_object()
        .ok_or("webhook event must be a JSON object")?;
    object
        .get("type")
        .ok_or("webhook event is missing string field `type`")?
        .as_str()
        .ok_or("webhook event field `type` must be a string")
}

/// A parsed webhook body whose original bytes have already passed signature
/// verification.
///
/// This wrapper intentionally does not implement [`Deserialize`]. The client
/// verification boundary must parse the body and then explicitly assert that
/// verification succeeded by calling [`VerifiedWebhook::from_verified`].
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedWebhook<T> {
    body: T,
}

impl<T> VerifiedWebhook<T> {
    /// Wraps a parsed body after its original bytes have been verified.
    ///
    /// This function does not perform cryptography. Callers must invoke it only
    /// after verification against the configured webhook secret succeeds.
    #[must_use]
    pub const fn from_verified(body: T) -> Self {
        Self { body }
    }

    /// Borrows the verified parsed body.
    #[must_use]
    pub const fn body(&self) -> &T {
        &self.body
    }

    /// Consumes the wrapper and returns the parsed body.
    #[must_use]
    pub fn into_body(self) -> T {
        self.body
    }

    /// Maps a verified representation without weakening its verification state.
    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> VerifiedWebhook<U> {
        VerifiedWebhook {
            body: map(self.body),
        }
    }
}

impl<T> AsRef<T> for VerifiedWebhook<T> {
    fn as_ref(&self) -> &T {
        self.body()
    }
}

impl<T> fmt::Debug for VerifiedWebhook<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedWebhook")
            .field("body_type", &type_name::<T>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::{Map, Value, json};

    use super::{VerifiedWebhook, WEBHOOK_EVENT_DISCRIMINATORS, WebhookEvent};

    fn fixture(event_type: &str) -> Value {
        let data = match event_type {
            "live.call.incoming" => json!({
                "session_id": "rtc_do-not-log",
                "sip_headers": [{"name": "Authorization", "value": "payload-secret"}]
            }),
            "realtime.call.incoming" => json!({
                "call_id": "rtc_do-not-log",
                "sip_headers": [{"name": "Authorization", "value": "payload-secret"}]
            }),
            "safety.alert.created" | "safety.org_alert.created" => {
                json!({"id": "alert_0123456789abcdef0123456789abcdef"})
            }
            _ => json!({"id": "resource-do-not-log"}),
        };
        json!({
            "id": "evt_do-not-log",
            "object": "event",
            "created_at": 1_788_000_000_i64,
            "type": event_type,
            "data": data
        })
    }

    #[test]
    fn discriminator_manifest_is_complete_and_unique() {
        let unique = WEBHOOK_EVENT_DISCRIMINATORS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(WEBHOOK_EVENT_DISCRIMINATORS.len(), 18);
        assert_eq!(unique.len(), 18);
        assert_eq!(
            WEBHOOK_EVENT_DISCRIMINATORS,
            [
                "batch.cancelled",
                "batch.completed",
                "batch.expired",
                "batch.failed",
                "eval.run.canceled",
                "eval.run.failed",
                "eval.run.succeeded",
                "fine_tuning.job.cancelled",
                "fine_tuning.job.failed",
                "fine_tuning.job.succeeded",
                "live.call.incoming",
                "realtime.call.incoming",
                "response.cancelled",
                "response.completed",
                "response.failed",
                "response.incomplete",
                "safety.alert.created",
                "safety.org_alert.created",
            ]
        );
    }

    #[test]
    fn every_manifest_variant_round_trips_through_its_known_branch()
    -> Result<(), Box<dyn std::error::Error>> {
        for event_type in WEBHOOK_EVENT_DISCRIMINATORS {
            let expected = fixture(event_type);
            let event: WebhookEvent = serde_json::from_value(expected.clone())?;
            assert!(event.is_known(), "{event_type} routed to unknown");
            assert_eq!(event.event_type(), event_type);
            assert_eq!(serde_json::to_value(event)?, expected);
        }
        Ok(())
    }

    #[test]
    fn every_known_tag_rejects_a_malformed_payload() -> Result<(), Box<dyn std::error::Error>> {
        for event_type in WEBHOOK_EVENT_DISCRIMINATORS {
            let mut malformed = fixture(event_type);
            let object = malformed
                .as_object_mut()
                .ok_or("fixture must be an object")?;
            object.remove("data");
            assert!(
                serde_json::from_value::<WebhookEvent>(malformed).is_err(),
                "{event_type} accepted missing data"
            );
        }
        Ok(())
    }

    #[test]
    fn unknown_event_and_extra_fields_are_lossless() -> Result<(), Box<dyn std::error::Error>> {
        let expected = json!({
            "type": "future.delivery.created",
            "id": "evt_future",
            "future": {"nested": [1, 2, 3]},
            "payload-secret": "do-not-log"
        });
        let event: WebhookEvent = serde_json::from_value(expected.clone())?;
        assert!(!event.is_known());
        assert_eq!(event.event_type(), "future.delivery.created");
        assert_eq!(serde_json::to_value(&event)?, expected);
        let debug = format!("{event:?}");
        assert!(!debug.contains("do-not-log"));
        assert!(!debug.contains("nested"));
        Ok(())
    }

    #[test]
    fn known_event_and_payload_extras_are_lossless() -> Result<(), Box<dyn std::error::Error>> {
        let mut expected = fixture("response.completed");
        let object = expected
            .as_object_mut()
            .ok_or("fixture must be an object")?;
        object.insert("future_top".to_owned(), json!({"secret": "top-secret"}));
        let data = object
            .get_mut("data")
            .and_then(Value::as_object_mut)
            .ok_or("fixture data must be an object")?;
        data.insert("future_data".to_owned(), json!(["payload-secret"]));

        let event: WebhookEvent = serde_json::from_value(expected.clone())?;
        assert_eq!(serde_json::to_value(&event)?, expected);
        let debug = format!("{event:?}");
        assert!(!debug.contains("top-secret"));
        assert!(!debug.contains("payload-secret"));
        assert!(!debug.contains("evt_do-not-log"));
        assert!(!debug.contains("resource-do-not-log"));
        Ok(())
    }

    #[test]
    fn optional_and_required_object_markers_follow_the_schema()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut batch = fixture("batch.completed");
        batch
            .as_object_mut()
            .ok_or("fixture must be an object")?
            .remove("object");
        let event: WebhookEvent = serde_json::from_value(batch)?;
        assert!(event.is_known());

        let mut safety = fixture("safety.alert.created");
        safety
            .as_object_mut()
            .ok_or("fixture must be an object")?
            .remove("object");
        assert!(serde_json::from_value::<WebhookEvent>(safety).is_err());

        let mut wrong_object = fixture("response.completed");
        wrong_object
            .as_object_mut()
            .ok_or("fixture must be an object")?
            .insert("object".to_owned(), Value::String("not-event".to_owned()));
        assert!(serde_json::from_value::<WebhookEvent>(wrong_object).is_err());
        Ok(())
    }

    #[test]
    fn missing_or_non_string_discriminator_is_rejected() {
        assert!(serde_json::from_value::<WebhookEvent>(json!({})).is_err());
        assert!(serde_json::from_value::<WebhookEvent>(json!({"type": 1})).is_err());
        assert!(serde_json::from_value::<WebhookEvent>(Value::Array(Vec::new())).is_err());
    }

    #[test]
    fn verified_wrapper_never_debugs_the_body() -> Result<(), Box<dyn std::error::Error>> {
        let event: WebhookEvent = serde_json::from_value(fixture("response.completed"))?;
        let verified = VerifiedWebhook::from_verified(event);
        let debug = format!("{verified:?}");
        assert!(debug.contains("VerifiedWebhook"));
        assert!(!debug.contains("do-not-log"));
        assert!(!debug.contains("response.completed"));
        Ok(())
    }

    #[test]
    fn payload_debug_does_not_leak_sip_headers() -> Result<(), Box<dyn std::error::Error>> {
        let event: WebhookEvent = serde_json::from_value(fixture("live.call.incoming"))?;
        let debug = format!("{event:?}");
        assert!(!debug.contains("Authorization"));
        assert!(!debug.contains("payload-secret"));
        assert!(!debug.contains("rtc_do-not-log"));
        Ok(())
    }

    #[test]
    fn extra_field_helpers_do_not_offer_mutation() -> Result<(), Box<dyn std::error::Error>> {
        let mut raw = fixture("batch.failed");
        let object = raw.as_object_mut().ok_or("fixture must be an object")?;
        object.insert("future".to_owned(), Value::Bool(true));
        let event: WebhookEvent = serde_json::from_value(raw)?;
        let extra = event
            .extra_fields()
            .ok_or("known event must expose extras")?;
        assert_eq!(extra.get("future"), Some(&Value::Bool(true)));
        Ok(())
    }

    #[test]
    fn unknown_raw_object_remains_immutable_through_public_api()
    -> Result<(), Box<dyn std::error::Error>> {
        let raw = Map::from_iter([
            ("type".to_owned(), Value::String("future.event".to_owned())),
            ("extra".to_owned(), Value::Bool(true)),
        ]);
        let event: WebhookEvent = serde_json::from_value(Value::Object(raw))?;
        let unknown = event.unknown().ok_or("future tag must remain unknown")?;
        assert_eq!(unknown.raw().get("extra"), Some(&Value::Bool(true)));
        Ok(())
    }
}
