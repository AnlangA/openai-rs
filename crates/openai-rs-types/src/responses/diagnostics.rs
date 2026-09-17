//! Typed prompt-cache and safety diagnostics shared by GA and beta Responses.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use crate::{ExtraFields, Omittable, safety::SafetyAlertErrorType};

crate::open_string_enum! {
    /// The change responsible for a prompt-cache miss.
    pub enum PromptCacheMissReason {
        ModelChanged = "model_changed",
        PromptCacheKeyChanged = "prompt_cache_key_changed",
        ToolsChanged = "tools_changed",
        TextFormatChanged = "text_format_changed",
        ReasoningEffortChanged = "reasoning_effort_changed",
        VerbosityChanged = "verbosity_changed",
        ContextCompacted = "context_compacted",
        InputChanged = "input_changed",
        ServiceTierChanged = "service_tier_changed",
    }
}

literal_tag!(PromptCacheMissTag, "cache_miss");
literal_tag!(PromptCacheHitTag, "cache_hit");
literal_tag!(
    ComparisonResponseNotFoundTag,
    "comparison_response_not_found"
);
literal_tag!(PromptCacheUnavailableTag, "unavailable");

/// Details explaining a prompt-cache miss relative to the comparison response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheMiss {
    #[serde(rename = "type")]
    kind: PromptCacheMissTag,
    cache_missed_tokens: i64,
    reason: PromptCacheMissReason,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    comparison_reusable_tokens: Omittable<i64>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl PromptCacheMiss {
    /// Returns the tokens that missed the cache.
    #[must_use]
    pub const fn cache_missed_tokens(&self) -> i64 {
        self.cache_missed_tokens
    }

    /// Returns the change responsible for the miss.
    #[must_use]
    pub const fn reason(&self) -> &PromptCacheMissReason {
        &self.reason
    }

    /// Returns reusable comparison tokens when reported.
    #[must_use]
    pub fn comparison_reusable_tokens(&self) -> Option<i64> {
        match self.comparison_reusable_tokens {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future diagnostic fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

macro_rules! diagnostic_marker {
    ($name:ident, $tag:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            #[serde(default, flatten)]
            extra: ExtraFields,
        }
        impl $name {
            /// Returns future diagnostic fields.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

diagnostic_marker!(PromptCacheHit, PromptCacheHitTag, "A prompt-cache hit.");
diagnostic_marker!(
    ComparisonResponseNotFound,
    ComparisonResponseNotFoundTag,
    "The requested comparison response could not be found."
);
diagnostic_marker!(
    PromptCacheUnavailable,
    PromptCacheUnavailableTag,
    "Prompt-cache diagnostics are unavailable."
);

tagged_union! {
    /// Prompt-cache diagnostics requested for a response.
    pub enum PromptCacheDiagnostics {
        CacheMiss(PromptCacheMiss) => "cache_miss",
        CacheHit(PromptCacheHit) => "cache_hit",
        ComparisonResponseNotFound(ComparisonResponseNotFound) => "comparison_response_not_found",
        Unavailable(PromptCacheUnavailable) => "unavailable",
    }
}

/// Safety information associated with a failed response.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseMisalignment {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detailed_explanation: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    error_type: Omittable<SafetyAlertErrorType>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    steer: Omittable<ResponseMisalignmentSteer>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ResponseMisalignment {
    /// Returns the explanation when supplied.
    #[must_use]
    pub fn detailed_explanation(&self) -> Option<&str> {
        match &self.detailed_explanation {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the safety category, preserving unrecognized strings.
    #[must_use]
    pub fn error_type(&self) -> Option<&SafetyAlertErrorType> {
        match &self.error_type {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns the supplied steering message.
    #[must_use]
    pub fn steer(&self) -> Option<&ResponseMisalignmentSteer> {
        match &self.steer {
            Omittable::Value(value) => Some(value),
            Omittable::Omitted => None,
        }
    }

    /// Returns future safety fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A steering message supplied with a response safety error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseMisalignmentSteer {
    message: String,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ResponseMisalignmentSteer {
    /// Returns the message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns future steering fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostics_dispatch_all_known_variants_and_reject_malformed_known_tags() {
        for value in [
            json!({"type":"cache_miss","cache_missed_tokens":4,"reason":"model_changed","comparison_reusable_tokens":0,"future":true}),
            json!({"type":"cache_hit","future":1}),
            json!({"type":"comparison_response_not_found"}),
            json!({"type":"unavailable"}),
            json!({"type":"future_diagnostic","payload":[1,2]}),
        ] {
            let decoded: PromptCacheDiagnostics =
                serde_json::from_value(value.clone()).expect("valid test fixture");
            assert_eq!(
                serde_json::to_value(decoded).expect("valid test fixture"),
                value
            );
        }
        for value in [
            json!({"type":"cache_miss"}),
            json!({"type":"cache_miss","cache_missed_tokens":1,"reason":null}),
            json!({"type":"cache_hit","type_typo":1, "other":null})
                .get("type")
                .expect("valid test fixture")
                .clone(),
            json!(null),
        ] {
            assert!(serde_json::from_value::<PromptCacheDiagnostics>(value).is_err());
        }
    }

    #[test]
    fn misalignment_preserves_optional_fields_and_requires_steer_message() {
        for value in [
            json!({}),
            json!({"error_type":"future","detailed_explanation":"detail","steer":{"message":"review","future":1}}),
        ] {
            let decoded: ResponseMisalignment =
                serde_json::from_value(value.clone()).expect("valid test fixture");
            assert_eq!(
                serde_json::to_value(decoded).expect("valid test fixture"),
                value
            );
        }
        for value in [
            json!({"steer":{}}),
            json!({"steer":null}),
            json!({"error_type":null}),
        ] {
            assert!(serde_json::from_value::<ResponseMisalignment>(value).is_err());
        }
    }
}
