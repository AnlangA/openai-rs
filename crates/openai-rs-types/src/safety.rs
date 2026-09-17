//! Project safety alert response types from the documented Safety API.

use crate::{ExtraFields, ModelId, Nullable};
use serde::{Deserialize, Serialize};

crate::open_string_enum! {
    /// The category of activity flagged for review.
    pub enum SafetyAlertErrorType {
        PotentiallyUnintendedDataTransfer = "potentially_unintended_data_transfer",
        PotentiallyUnintendedDataAccess = "potentially_unintended_data_access",
        PotentiallyUnintendedDestructiveActivity = "potentially_unintended_destructive_activity",
        Other = "other",
    }
}

/// Object discriminator for a project safety alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyAlertObject {
    /// A safety alert belonging to the authenticated API project.
    #[serde(rename = "safety.alert")]
    SafetyAlert,
}

impl SafetyAlertObject {
    /// Returns the wire discriminator.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SafetyAlert => "safety.alert",
        }
    }
}

/// Details of a safety alert belonging to the authenticated API project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SafetyAlert {
    id: String,
    created_at: f64,
    error_type: SafetyAlertErrorType,
    model: ModelId,
    object: SafetyAlertObject,
    reason: Nullable<String>,
    request_id: String,
    request_paused: bool,
    response_id: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SafetyAlert {
    /// Returns the object discriminator.
    #[must_use]
    pub const fn object(&self) -> &SafetyAlertObject {
        &self.object
    }

    /// The alert identifier carried by webhook `data.id`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Creation time in Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> f64 {
        self.created_at
    }
    /// The reported category, retaining future values.
    #[must_use]
    pub const fn error_type(&self) -> &SafetyAlertErrorType {
        &self.error_type
    }
    /// Model associated with the alert.
    #[must_use]
    pub const fn model(&self) -> &ModelId {
        &self.model
    }
    /// Customer-visible description; null is valid, including under ZDR.
    #[must_use]
    pub const fn reason(&self) -> &Nullable<String> {
        &self.reason
    }
    /// Identifier of the affected request, distinct from retrieval HTTP metadata.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    /// Whether block registration succeeded. This does not establish that execution stopped.
    #[must_use]
    pub const fn request_paused(&self) -> bool {
        self.request_paused
    }
    /// Identifier of the affected response.
    #[must_use]
    pub fn response_id(&self) -> &str {
        &self.response_id
    }
    /// Future response fields retained during decoding.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> serde_json::Value {
        json!({"id":"alert_1","created_at":1787659200.25,
            "error_type":"potentially_unintended_data_transfer","model":"gpt-6-astra",
            "object":"safety.alert","reason":null,"request_id":"req_affected",
            "request_paused":true,"response_id":"resp_1","future_field":{"kept":true}})
    }

    #[test]
    fn alert_preserves_nullable_reason_and_future_fields() {
        for reason in [
            serde_json::Value::Null,
            json!("Review the requested transfer"),
        ] {
            let mut value = fixture();
            value["reason"] = reason;
            value["error_type"] = json!("future_category");
            let alert: SafetyAlert = serde_json::from_value(value.clone()).expect("alert");
            assert_eq!(alert.error_type().as_str(), "future_category");
            assert_eq!(alert.created_at(), 1787659200.25);
            assert_eq!(serde_json::to_value(alert).expect("encode alert"), value);
        }
    }

    #[test]
    fn alert_rejects_missing_required_fields_and_wrong_object() {
        for field in [
            "id",
            "created_at",
            "error_type",
            "model",
            "object",
            "reason",
            "request_id",
            "request_paused",
            "response_id",
        ] {
            let mut value = fixture();
            value.as_object_mut().expect("object").remove(field);
            assert!(
                serde_json::from_value::<SafetyAlert>(value).is_err(),
                "missing {field}"
            );
        }
        let mut value = fixture();
        value["object"] = json!("response");
        assert!(serde_json::from_value::<SafetyAlert>(value).is_err());
    }
}
