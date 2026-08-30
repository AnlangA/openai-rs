//! Fine-tuning Jobs, events, checkpoints, permissions, and wire configuration.
//!
//! This module models the stable Jobs HTTP surface. Experimental grader DTOs
//! live in [`experimental_graders`] and intentionally contain no HTTP client or
//! route implementation.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use crate::{
    ExtraFields, FileId, FineTuningJobId, ModelId, Nullable, Omittable,
    responses::UnknownTaggedObject,
};

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

macro_rules! strict_tagged_union {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident($ty:ty) = $wire:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// Future variant retained with every JSON field.
            Unknown(UnknownTaggedObject),
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $(Self::$variant(value) => value.serialize(serializer),)+
                    Self::Unknown(value) => value.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Value::deserialize(deserializer)?;
                let tag = discriminator(&value).map_err(D::Error::custom)?;
                match tag {
                    $($wire => serde_json::from_value::<$ty>(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

fn discriminator(value: &Value) -> Result<&str, &'static str> {
    let Value::Object(object) = value else {
        return Err("tagged fine-tuning value must be a JSON object");
    };
    object
        .get("type")
        .ok_or("tagged fine-tuning object is missing string field `type`")?
        .as_str()
        .ok_or("tagged fine-tuning object field `type` must be a string")
}

fn serialize_object<T: Serialize>(
    value: &T,
    context: &'static str,
) -> Result<Map<String, Value>, serde_json::Error> {
    match serde_json::to_value(value)? {
        Value::Object(object) => Ok(object),
        _ => Err(<serde_json::Error as serde::ser::Error>::custom(context)),
    }
}

crate::open_string_enum! {
    /// Current lifecycle state of a fine-tuning job.
    pub enum FineTuningJobStatus {
        ValidatingFiles = "validating_files",
        Queued = "queued",
        Running = "running",
        Succeeded = "succeeded",
        Failed = "failed",
        Cancelled = "cancelled"
    }
}

crate::open_string_enum! {
    /// Fine-tuning job event severity.
    pub enum FineTuningEventLevel {
        Info = "info",
        Warn = "warn",
        Error = "error"
    }
}

crate::open_string_enum! {
    /// Fine-tuning event payload kind.
    pub enum FineTuningEventKind {
        Message = "message",
        Metrics = "metrics"
    }
}

crate::open_string_enum! {
    /// Checkpoint permission listing order.
    pub enum CheckpointPermissionOrder {
        Ascending = "ascending",
        Descending = "descending"
    }
}

crate::open_string_enum! {
    /// Service-selected hyperparameter mode.
    pub enum FineTuneAuto {
        Auto = "auto"
    }
}

crate::open_string_enum! {
    /// Reinforcement fine-tuning reasoning effort.
    pub enum ReinforcementReasoningEffort {
        Default = "default",
        Low = "low",
        Medium = "medium",
        High = "high"
    }
}

/// `"auto"` or an integer hyperparameter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AutoOrInteger {
    /// Automatic selection, retaining future string modes.
    Auto(FineTuneAuto),
    /// Explicit integer value.
    Value(u64),
}

impl From<u64> for AutoOrInteger {
    fn from(value: u64) -> Self {
        Self::Value(value)
    }
}

/// `"auto"` or a floating-point hyperparameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AutoOrNumber {
    /// Automatic selection, retaining future string modes.
    Auto(FineTuneAuto),
    /// Explicit numeric value.
    Value(f64),
}

impl From<f64> for AutoOrNumber {
    fn from(value: f64) -> Self {
        Self::Value(value)
    }
}

/// Experimental, feature-neutral grader wire types.
///
/// These DTOs support reinforcement fine-tuning and the alpha grader schemas.
/// They do not expose alpha HTTP operations, authentication, or routing.
pub mod experimental_graders {
    use super::*;

    crate::open_string_enum! {
        /// String comparison performed by a string-check grader.
        pub enum StringCheckOperation {
            Equal = "eq",
            NotEqual = "ne",
            Like = "like",
            CaseInsensitiveLike = "ilike"
        }
    }

    crate::open_string_enum! {
        /// Similarity metric used by a text-similarity grader.
        pub enum TextSimilarityMetric {
            Cosine = "cosine",
            FuzzyMatch = "fuzzy_match",
            Bleu = "bleu",
            Gleu = "gleu",
            Meteor = "meteor",
            Rouge1 = "rouge_1",
            Rouge2 = "rouge_2",
            Rouge3 = "rouge_3",
            Rouge4 = "rouge_4",
            Rouge5 = "rouge_5",
            RougeL = "rouge_l"
        }
    }

    crate::open_string_enum! {
        /// Reasoning effort for a model grader.
        pub enum GraderReasoningEffort {
            None = "none",
            Minimal = "minimal",
            Low = "low",
            Medium = "medium",
            High = "high",
            XHigh = "xhigh",
            Max = "max"
        }
    }

    literal_tag!(StringCheckTag, StringCheck, "string_check");

    /// Grader performing a string comparison.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct StringCheckGrader {
        #[serde(rename = "type")]
        kind: StringCheckTag,
        /// Grader name.
        pub name: String,
        /// Input expression or template.
        pub input: String,
        /// Reference expression or template.
        pub reference: String,
        /// Comparison operation.
        pub operation: StringCheckOperation,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl StringCheckGrader {
        /// Construct a string-check grader.
        #[must_use]
        pub fn new(
            name: impl Into<String>,
            input: impl Into<String>,
            reference: impl Into<String>,
            operation: StringCheckOperation,
        ) -> Self {
            Self {
                kind: StringCheckTag::StringCheck,
                name: name.into(),
                input: input.into(),
                reference: reference.into(),
                operation,
                extra: ExtraFields::new(),
            }
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    literal_tag!(TextSimilarityTag, TextSimilarity, "text_similarity");

    /// Grader comparing text with a similarity metric.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct TextSimilarityGrader {
        #[serde(rename = "type")]
        kind: TextSimilarityTag,
        /// Grader name.
        pub name: String,
        /// Input text/template.
        pub input: String,
        /// Reference text/template.
        pub reference: String,
        /// Similarity metric.
        pub evaluation_metric: TextSimilarityMetric,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl TextSimilarityGrader {
        /// Construct a text-similarity grader.
        #[must_use]
        pub fn new(
            name: impl Into<String>,
            input: impl Into<String>,
            reference: impl Into<String>,
            evaluation_metric: TextSimilarityMetric,
        ) -> Self {
            Self {
                kind: TextSimilarityTag::TextSimilarity,
                name: name.into(),
                input: input.into(),
                reference: reference.into(),
                evaluation_metric,
                extra: ExtraFields::new(),
            }
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    literal_tag!(PythonGraderTag, Python, "python");

    /// Grader executing supplied Python source in the grader environment.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct PythonGrader {
        #[serde(rename = "type")]
        kind: PythonGraderTag,
        /// Grader name.
        pub name: String,
        /// Python source code.
        pub source: String,
        /// Optional execution image tag.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub image_tag: Omittable<String>,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl PythonGrader {
        /// Construct a Python grader.
        #[must_use]
        pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
            Self {
                kind: PythonGraderTag::Python,
                name: name.into(),
                source: source.into(),
                image_tag: Omittable::Omitted,
                extra: ExtraFields::new(),
            }
        }

        /// Select a grader execution image tag.
        #[must_use]
        pub fn with_image_tag(mut self, image_tag: impl Into<String>) -> Self {
            self.image_tag = Omittable::Value(image_tag.into());
            self
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    /// Sampling settings for score-model graders.
    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    pub struct ScoreModelSamplingParams {
        /// Random seed or explicit null.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub seed: Omittable<Nullable<i64>>,
        /// Nucleus sampling or explicit null.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub top_p: Omittable<Nullable<f64>>,
        /// Temperature or explicit null.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub temperature: Omittable<Nullable<f64>>,
        /// Maximum completion tokens or explicit null.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub max_completions_tokens: Omittable<Nullable<u64>>,
        /// Reasoning effort or explicit null.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub reasoning_effort: Omittable<Nullable<GraderReasoningEffort>>,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl ScoreModelSamplingParams {
        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    literal_tag!(ScoreModelGraderTag, ScoreModel, "score_model");

    /// Grader asking another model to produce a numeric score.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct ScoreModelGrader {
        #[serde(rename = "type")]
        kind: ScoreModelGraderTag,
        /// Grader name.
        pub name: String,
        /// Model used for grading.
        pub model: ModelId,
        /// Typed semantic grader input items.
        pub input: Vec<Value>,
        /// Optional model sampling settings.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub sampling_params: Omittable<ScoreModelSamplingParams>,
        /// Optional output range.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub range: Omittable<Vec<f64>>,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl ScoreModelGrader {
        /// Construct from typed serializable input items.
        pub fn from_serializable_inputs<T: Serialize>(
            name: impl Into<String>,
            model: impl Into<ModelId>,
            input: impl IntoIterator<Item = T>,
        ) -> Result<Self, serde_json::Error> {
            let input = input
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self {
                kind: ScoreModelGraderTag::ScoreModel,
                name: name.into(),
                model: model.into(),
                input,
                sampling_params: Omittable::Omitted,
                range: Omittable::Omitted,
                extra: ExtraFields::new(),
            })
        }

        /// Set model sampling parameters.
        #[must_use]
        pub fn with_sampling_params(mut self, params: ScoreModelSamplingParams) -> Self {
            self.sampling_params = Omittable::Value(params);
            self
        }

        /// Set the score range.
        #[must_use]
        pub fn with_range(mut self, minimum: f64, maximum: f64) -> Self {
            self.range = Omittable::Value(vec![minimum, maximum]);
            self
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    literal_tag!(LabelModelGraderTag, LabelModel, "label_model");

    /// Grader assigning one of a declared label set.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct LabelModelGrader {
        #[serde(rename = "type")]
        kind: LabelModelGraderTag,
        /// Grader name.
        pub name: String,
        /// Structured-output-capable grading model.
        pub model: ModelId,
        /// Typed semantic grader input items.
        pub input: Vec<Value>,
        /// All labels.
        pub labels: Vec<String>,
        /// Labels considered passing.
        pub passing_labels: Vec<String>,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl LabelModelGrader {
        /// Construct from typed serializable input items.
        pub fn from_serializable_inputs<T: Serialize>(
            name: impl Into<String>,
            model: impl Into<ModelId>,
            input: impl IntoIterator<Item = T>,
            labels: impl IntoIterator<Item = String>,
            passing_labels: impl IntoIterator<Item = String>,
        ) -> Result<Self, serde_json::Error> {
            let input = input
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self {
                kind: LabelModelGraderTag::LabelModel,
                name: name.into(),
                model: model.into(),
                input,
                labels: labels.into_iter().collect(),
                passing_labels: passing_labels.into_iter().collect(),
                extra: ExtraFields::new(),
            })
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    strict_tagged_union! {
        /// Grader definition accepted by reinforcement and alpha wire schemas.
        pub enum Grader {
            StringCheck(StringCheckGrader) = "string_check",
            TextSimilarity(TextSimilarityGrader) = "text_similarity",
            Python(PythonGrader) = "python",
            ScoreModel(Box<ScoreModelGrader>) = "score_model",
            LabelModel(Box<LabelModelGrader>) = "label_model",
            Multi(Box<MultiGrader>) = "multi"
        }
    }

    /// The frozen schema describes one grader while official examples may carry
    /// an array; both wire forms are retained explicitly.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(untagged)]
    #[non_exhaustive]
    pub enum GraderCollection {
        /// One nested grader, matching the machine schema.
        One(Box<Grader>),
        /// Multiple nested graders, matching official examples/runtime behavior.
        Many(Vec<Grader>),
    }

    literal_tag!(MultiGraderTag, Multi, "multi");

    /// Grader combining one or more sub-grader rewards.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct MultiGrader {
        #[serde(rename = "type")]
        kind: MultiGraderTag,
        /// Grader name.
        pub name: String,
        /// Nested grader or grader list.
        pub graders: GraderCollection,
        /// Reward combination expression.
        pub calculate_output: String,
        /// Future fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl MultiGrader {
        /// Construct a multi grader from a list of graders.
        #[must_use]
        pub fn many(
            name: impl Into<String>,
            graders: impl IntoIterator<Item = Grader>,
            calculate_output: impl Into<String>,
        ) -> Self {
            Self {
                kind: MultiGraderTag::Multi,
                name: name.into(),
                graders: GraderCollection::Many(graders.into_iter().collect()),
                calculate_output: calculate_output.into(),
                extra: ExtraFields::new(),
            }
        }

        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    impl From<StringCheckGrader> for Grader {
        fn from(value: StringCheckGrader) -> Self {
            Self::StringCheck(value)
        }
    }

    impl From<TextSimilarityGrader> for Grader {
        fn from(value: TextSimilarityGrader) -> Self {
            Self::TextSimilarity(value)
        }
    }

    impl From<PythonGrader> for Grader {
        fn from(value: PythonGrader) -> Self {
            Self::Python(value)
        }
    }

    impl From<ScoreModelGrader> for Grader {
        fn from(value: ScoreModelGrader) -> Self {
            Self::ScoreModel(Box::new(value))
        }
    }

    impl From<LabelModelGrader> for Grader {
        fn from(value: LabelModelGrader) -> Self {
            Self::LabelModel(Box::new(value))
        }
    }

    impl From<MultiGrader> for Grader {
        fn from(value: MultiGrader) -> Self {
            Self::Multi(Box::new(value))
        }
    }

    /// Experimental wire request for validating a grader.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct ValidateGraderRequest {
        /// Grader definition to validate.
        pub grader: Grader,
    }

    /// Experimental wire response from grader validation.
    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    pub struct ValidateGraderResponse {
        /// Validated grader when returned.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub grader: Omittable<Grader>,
        /// Future response fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl ValidateGraderResponse {
        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    /// Experimental wire request for running a grader locally on one sample.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderRequest {
        /// Grader definition.
        pub grader: Grader,
        /// Optional semantic item object.
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        pub item: Omittable<Map<String, Value>>,
        /// Model output being graded.
        pub model_sample: String,
    }

    impl RunGraderRequest {
        /// Construct a grader run request without an item object.
        #[must_use]
        pub fn new(grader: Grader, model_sample: impl Into<String>) -> Self {
            Self {
                grader,
                item: Omittable::Omitted,
                model_sample: model_sample.into(),
            }
        }

        /// Serialize a typed item into the required JSON object shape.
        pub fn with_item<T: Serialize>(mut self, item: &T) -> Result<Self, serde_json::Error> {
            self.item = Omittable::Value(serialize_object(
                item,
                "grader item must serialize as a JSON object",
            )?);
            Ok(self)
        }
    }

    /// Detailed error flags from an experimental grader run.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct GraderRunErrors {
        pub formula_parse_error: bool,
        pub sample_parse_error: bool,
        pub truncated_observation_error: bool,
        pub unresponsive_reward_error: bool,
        pub invalid_variable_error: bool,
        pub other_error: bool,
        pub python_grader_server_error: bool,
        pub python_grader_server_error_type: Nullable<String>,
        pub python_grader_runtime_error: bool,
        pub python_grader_runtime_error_details: Nullable<String>,
        #[serde(rename = "model_grader_server_error")]
        pub api_model_grader_server_error: bool,
        #[serde(rename = "model_grader_refusal_error")]
        pub api_model_grader_refusal_error: bool,
        #[serde(rename = "model_grader_parse_error")]
        pub api_model_grader_parse_error: bool,
        #[serde(rename = "model_grader_server_error_details")]
        pub api_model_grader_server_error_details: Nullable<String>,
        /// Future response fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl GraderRunErrors {
        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    /// Metadata from an experimental grader run.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct GraderRunMetadata {
        pub name: String,
        #[serde(rename = "type")]
        pub kind: String,
        pub errors: GraderRunErrors,
        pub execution_time: f64,
        pub scores: BTreeMap<String, Value>,
        pub token_usage: Nullable<u64>,
        pub sampled_model_name: Nullable<String>,
        /// Future response fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl GraderRunMetadata {
        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }

    /// Experimental wire response from running a grader.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderResponse {
        pub reward: f64,
        pub metadata: GraderRunMetadata,
        pub sub_rewards: BTreeMap<String, Value>,
        #[serde(rename = "model_grader_token_usage_per_model")]
        pub api_model_grader_token_usage_per_model: BTreeMap<String, Value>,
        /// Future response fields.
        #[serde(default, flatten)]
        extra: ExtraFields,
    }

    impl RunGraderResponse {
        /// Future fields retained during decode.
        #[must_use]
        pub const fn extra(&self) -> &ExtraFields {
            &self.extra
        }
    }
}
