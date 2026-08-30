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

/// Deprecated top-level supervised hyperparameters accepted by job creation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyFineTuningHyperparameters {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_size: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub learning_rate_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n_epochs: Omittable<AutoOrInteger>,
}

/// Hyperparameters returned on a fine-tuning job.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuningJobHyperparameters {
    /// Batch size may explicitly be null in the frozen response schema.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_size: Omittable<Nullable<AutoOrInteger>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub learning_rate_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n_epochs: Omittable<AutoOrInteger>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningJobHyperparameters {
    /// Future response fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Supervised fine-tuning hyperparameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuneSupervisedHyperparameters {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_size: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub learning_rate_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n_epochs: Omittable<AutoOrInteger>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuneSupervisedHyperparameters {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// DPO fine-tuning hyperparameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuneDpoHyperparameters {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub beta: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_size: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub learning_rate_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n_epochs: Omittable<AutoOrInteger>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Alias matching the frozen `FineTuneDPOHyperparameters` schema name.
pub type FineTuneDPOHyperparameters = FineTuneDpoHyperparameters;

impl FineTuneDpoHyperparameters {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Reinforcement fine-tuning hyperparameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuneReinforcementHyperparameters {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub batch_size: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub learning_rate_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub n_epochs: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reasoning_effort: Omittable<ReinforcementReasoningEffort>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub compute_multiplier: Omittable<AutoOrNumber>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub eval_interval: Omittable<AutoOrInteger>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub eval_samples: Omittable<AutoOrInteger>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuneReinforcementHyperparameters {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Supervised method configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuneSupervisedMethodConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub hyperparameters: Omittable<FineTuneSupervisedHyperparameters>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Alias matching the frozen `FineTuneSupervisedMethod` schema name.
pub type FineTuneSupervisedMethod = FineTuneSupervisedMethodConfig;

impl FineTuneSupervisedMethodConfig {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// DPO method configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuneDpoMethodConfig {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub hyperparameters: Omittable<FineTuneDpoHyperparameters>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Alias matching the frozen `FineTuneDPOMethod` schema name.
pub type FineTuneDPOMethod = FineTuneDpoMethodConfig;

impl FineTuneDpoMethodConfig {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Reinforcement method configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuneReinforcementMethodConfig {
    /// Grader definition. This wire surface remains experimental.
    pub grader: experimental_graders::Grader,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub hyperparameters: Omittable<FineTuneReinforcementHyperparameters>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

/// Alias matching the frozen `FineTuneReinforcementMethod` schema name.
pub type FineTuneReinforcementMethod = FineTuneReinforcementMethodConfig;

impl FineTuneReinforcementMethodConfig {
    /// Construct a reinforcement configuration from an experimental grader.
    #[must_use]
    pub fn new(grader: experimental_graders::Grader) -> Self {
        Self {
            grader,
            hyperparameters: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Attach reinforcement hyperparameters.
    #[must_use]
    pub fn with_hyperparameters(
        mut self,
        hyperparameters: FineTuneReinforcementHyperparameters,
    ) -> Self {
        self.hyperparameters = Omittable::Value(hyperparameters);
        self
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(SupervisedMethodTag, Supervised, "supervised");

/// Fine-tune method envelope for supervised learning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupervisedFineTuneMethod {
    #[serde(rename = "type")]
    kind: SupervisedMethodTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub supervised: Omittable<FineTuneSupervisedMethodConfig>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl SupervisedFineTuneMethod {
    /// Construct with default supervised configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: SupervisedMethodTag::Supervised,
            supervised: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Attach supervised configuration.
    #[must_use]
    pub fn with_config(mut self, config: FineTuneSupervisedMethodConfig) -> Self {
        self.supervised = Omittable::Value(config);
        self
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

impl Default for SupervisedFineTuneMethod {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(DpoMethodTag, Dpo, "dpo");

/// Fine-tune method envelope for direct preference optimization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DpoFineTuneMethod {
    #[serde(rename = "type")]
    kind: DpoMethodTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub dpo: Omittable<FineTuneDpoMethodConfig>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DpoFineTuneMethod {
    /// Construct with default DPO configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: DpoMethodTag::Dpo,
            dpo: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Attach DPO configuration.
    #[must_use]
    pub fn with_config(mut self, config: FineTuneDpoMethodConfig) -> Self {
        self.dpo = Omittable::Value(config);
        self
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

impl Default for DpoFineTuneMethod {
    fn default() -> Self {
        Self::new()
    }
}

literal_tag!(ReinforcementMethodTag, Reinforcement, "reinforcement");

/// Fine-tune method envelope for reinforcement learning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReinforcementFineTuneMethod {
    #[serde(rename = "type")]
    kind: ReinforcementMethodTag,
    /// Reinforcement configuration is optional in the envelope schema, but the
    /// builder always supplies it.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub reinforcement: Omittable<FineTuneReinforcementMethodConfig>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ReinforcementFineTuneMethod {
    /// Construct with a reinforcement configuration.
    #[must_use]
    pub fn new(config: FineTuneReinforcementMethodConfig) -> Self {
        Self {
            kind: ReinforcementMethodTag::Reinforcement,
            reinforcement: Omittable::Value(config),
            extra: ExtraFields::new(),
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// Fine-tuning method and its method-specific configuration.
    pub enum FineTuneMethod {
        Supervised(SupervisedFineTuneMethod) = "supervised",
        Dpo(DpoFineTuneMethod) = "dpo",
        Reinforcement(Box<ReinforcementFineTuneMethod>) = "reinforcement"
    }
}

impl From<SupervisedFineTuneMethod> for FineTuneMethod {
    fn from(value: SupervisedFineTuneMethod) -> Self {
        Self::Supervised(value)
    }
}

impl From<DpoFineTuneMethod> for FineTuneMethod {
    fn from(value: DpoFineTuneMethod) -> Self {
        Self::Dpo(value)
    }
}

impl From<ReinforcementFineTuneMethod> for FineTuneMethod {
    fn from(value: ReinforcementFineTuneMethod) -> Self {
        Self::Reinforcement(Box::new(value))
    }
}

/// Weights & Biases integration settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WandbIntegrationSettings {
    /// Destination project.
    pub project: String,
    /// Optional run name or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub name: Omittable<Nullable<String>>,
    /// Optional entity or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub entity: Omittable<Nullable<String>>,
    /// Run tags.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub tags: Omittable<Vec<String>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl WandbIntegrationSettings {
    /// Construct W&B settings.
    #[must_use]
    pub fn new(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            name: Omittable::Omitted,
            entity: Omittable::Omitted,
            tags: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Attach tags.
    #[must_use]
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = String>) -> Self {
        self.tags = Omittable::Value(tags.into_iter().collect());
        self
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

literal_tag!(WandbIntegrationTag, Wandb, "wandb");

/// W&B integration envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WandbIntegration {
    #[serde(rename = "type")]
    kind: WandbIntegrationTag,
    /// W&B settings.
    pub wandb: WandbIntegrationSettings,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl WandbIntegration {
    /// Construct a W&B integration.
    #[must_use]
    pub fn new(settings: WandbIntegrationSettings) -> Self {
        Self {
            kind: WandbIntegrationTag::Wandb,
            wandb: settings,
            extra: ExtraFields::new(),
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

strict_tagged_union! {
    /// Fine-tuning metrics integration.
    pub enum FineTuningIntegration {
        Wandb(WandbIntegration) = "wandb"
    }
}

impl From<WandbIntegration> for FineTuningIntegration {
    fn from(value: WandbIntegration) -> Self {
        Self::Wandb(value)
    }
}

/// JSON body for creating a fine-tuning job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateFineTuningJobRequest {
    /// Base model.
    pub model: ModelId,
    /// Training JSONL file.
    pub training_file: FileId,
    /// Deprecated top-level hyperparameters.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub hyperparameters: Omittable<LegacyFineTuningHyperparameters>,
    /// Model-name suffix or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub suffix: Omittable<Nullable<String>>,
    /// Validation JSONL file or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub validation_file: Omittable<Nullable<FileId>>,
    /// Metrics integrations or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub integrations: Omittable<Nullable<Vec<FineTuningIntegration>>>,
    /// Reproducibility seed or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub seed: Omittable<Nullable<u32>>,
    /// Method-specific configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub method: Omittable<FineTuneMethod>,
    /// Job metadata or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
}

impl CreateFineTuningJobRequest {
    /// Construct a minimal fine-tuning job request.
    #[must_use]
    pub fn new(model: impl Into<ModelId>, training_file: impl Into<FileId>) -> Self {
        Self {
            model: model.into(),
            training_file: training_file.into(),
            hyperparameters: Omittable::Omitted,
            suffix: Omittable::Omitted,
            validation_file: Omittable::Omitted,
            integrations: Omittable::Omitted,
            seed: Omittable::Omitted,
            method: Omittable::Omitted,
            metadata: Omittable::Omitted,
        }
    }

    /// Select a fine-tuning method.
    #[must_use]
    pub fn with_method(mut self, method: impl Into<FineTuneMethod>) -> Self {
        self.method = Omittable::Value(method.into());
        self
    }

    /// Add a validation dataset.
    #[must_use]
    pub fn with_validation_file(mut self, file: impl Into<FileId>) -> Self {
        self.validation_file = Omittable::Value(Nullable::Value(file.into()));
        self
    }

    /// Set a model-name suffix.
    #[must_use]
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Omittable::Value(Nullable::Value(suffix.into()));
        self
    }

    /// Add one metrics integration.
    #[must_use]
    pub fn with_integration(mut self, integration: impl Into<FineTuningIntegration>) -> Self {
        match &mut self.integrations {
            Omittable::Value(Nullable::Value(integrations)) => {
                integrations.push(integration.into());
            }
            Omittable::Omitted | Omittable::Value(Nullable::Null) => {
                self.integrations = Omittable::Value(Nullable::Value(vec![integration.into()]));
            }
        }
        self
    }

    /// Attach job metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: BTreeMap<String, String>) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }
}

crate::open_string_enum! {
    /// Object discriminator for a fine-tuning job.
    pub enum FineTuningJobObject {
        Job = "fine_tuning.job"
    }
}

/// Failure details on a fine-tuning job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuningJobError {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable failure message.
    pub message: String,
    /// Invalid parameter or explicit null.
    pub param: Nullable<String>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningJobError {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Fine-tuning job returned by create/retrieve/cancel/pause/resume.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuningJob {
    /// Job identifier.
    pub id: FineTuningJobId,
    /// Creation timestamp.
    pub created_at: u64,
    /// Required failure details or explicit null.
    pub error: Nullable<FineTuningJobError>,
    /// Resulting fine-tuned model or explicit null.
    pub fine_tuned_model: Nullable<ModelId>,
    /// Completion timestamp or explicit null.
    pub finished_at: Nullable<u64>,
    /// Returned supervised hyperparameters.
    pub hyperparameters: FineTuningJobHyperparameters,
    /// Base model.
    pub model: ModelId,
    /// Object discriminator.
    pub object: FineTuningJobObject,
    /// Owning organization.
    pub organization_id: String,
    /// Result file identifiers.
    pub result_files: Vec<FileId>,
    /// Lifecycle status, open for new server values such as future pause states.
    pub status: FineTuningJobStatus,
    /// Trained token count or explicit null.
    pub trained_tokens: Nullable<u64>,
    /// Training file.
    pub training_file: FileId,
    /// Validation file or explicit null.
    pub validation_file: Nullable<FileId>,
    /// Reproducibility seed.
    pub seed: u32,
    /// Integrations or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub integrations: Omittable<Nullable<Vec<FineTuningIntegration>>>,
    /// Estimated completion timestamp or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub estimated_finish: Omittable<Nullable<u64>>,
    /// Method configuration.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub method: Omittable<FineTuneMethod>,
    /// Metadata or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningJob {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }

    /// Whether the status is terminal according to this crate's known states.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            FineTuningJobStatus::Succeeded
                | FineTuningJobStatus::Failed
                | FineTuningJobStatus::Cancelled
        )
    }
}

crate::open_string_enum! {
    /// Object discriminator for a fine-tuning event.
    pub enum FineTuningEventObject {
        Event = "fine_tuning.job.event"
    }
}

/// Fine-tuning job event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuningJobEvent {
    pub object: FineTuningEventObject,
    pub id: String,
    pub created_at: u64,
    pub level: FineTuningEventLevel,
    pub message: String,
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Omittable::is_omitted"
    )]
    pub kind: Omittable<FineTuningEventKind>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub data: Omittable<Map<String, Value>>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningJobEvent {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Metrics recorded at a fine-tuning checkpoint.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FineTuningCheckpointMetrics {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub step: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub train_loss: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub train_mean_token_accuracy: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub valid_loss: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub valid_mean_token_accuracy: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub full_valid_loss: Omittable<f64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub full_valid_mean_token_accuracy: Omittable<f64>,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningCheckpointMetrics {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// Object discriminator for a fine-tuning checkpoint.
    pub enum FineTuningCheckpointObject {
        Checkpoint = "fine_tuning.job.checkpoint"
    }
}

/// Model checkpoint produced during fine-tuning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuningJobCheckpoint {
    pub id: String,
    pub created_at: u64,
    pub fine_tuned_model_checkpoint: ModelId,
    pub step_number: u64,
    pub metrics: FineTuningCheckpointMetrics,
    pub fine_tuning_job_id: FineTuningJobId,
    pub object: FineTuningCheckpointObject,
    /// Future response fields.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningJobCheckpoint {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

crate::open_string_enum! {
    /// List response object discriminator.
    pub enum FineTuningListObject {
        List = "list"
    }
}

/// Paginated fine-tuning jobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListPaginatedFineTuningJobsResponse {
    pub object: FineTuningListObject,
    pub data: Vec<FineTuningJob>,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListPaginatedFineTuningJobsResponse {
    /// Cursor for the next page, when available.
    #[must_use]
    pub fn next_after(&self) -> Option<&FineTuningJobId> {
        self.has_more
            .then(|| self.data.last())
            .flatten()
            .map(|job| &job.id)
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Paginated fine-tuning job events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningJobEventsResponse {
    pub object: FineTuningListObject,
    pub data: Vec<FineTuningJobEvent>,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListFineTuningJobEventsResponse {
    /// Cursor for the next page, when available.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        self.has_more
            .then(|| self.data.last())
            .flatten()
            .map(|event| event.id.as_str())
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Paginated fine-tuning checkpoints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningJobCheckpointsResponse {
    pub object: FineTuningListObject,
    pub data: Vec<FineTuningJobCheckpoint>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub first_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_id: Omittable<Nullable<String>>,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListFineTuningJobCheckpointsResponse {
    /// Cursor returned by the server for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Omittable::Value(Nullable::Value(id)) => Some(id),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query for listing fine-tuning jobs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningJobsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<FineTuningJobId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub metadata: Omittable<Nullable<BTreeMap<String, String>>>,
}

/// Query for listing job events.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningEventsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
}

/// Query for listing job checkpoints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningCheckpointsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
}

crate::open_string_enum! {
    /// Checkpoint permission object discriminator.
    pub enum CheckpointPermissionObject {
        Permission = "checkpoint.permission"
    }
}

/// Permission granting a project access to a fine-tuned checkpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FineTuningCheckpointPermission {
    pub id: String,
    pub created_at: u64,
    pub project_id: String,
    pub object: CheckpointPermissionObject,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl FineTuningCheckpointPermission {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Body for creating checkpoint permissions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFineTuningCheckpointPermissionRequest {
    /// Projects receiving permission.
    pub project_ids: Vec<String>,
}

impl CreateFineTuningCheckpointPermissionRequest {
    /// Construct from project identifiers.
    #[must_use]
    pub fn new(project_ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            project_ids: project_ids.into_iter().collect(),
        }
    }
}

/// Paginated checkpoint permissions. Also returned by permission creation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningCheckpointPermissionResponse {
    pub object: FineTuningListObject,
    pub data: Vec<FineTuningCheckpointPermission>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub first_id: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub last_id: Omittable<Nullable<String>>,
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ListFineTuningCheckpointPermissionResponse {
    /// Cursor for the next page.
    #[must_use]
    pub fn next_after(&self) -> Option<&str> {
        if !self.has_more {
            return None;
        }
        match &self.last_id {
            Omittable::Value(Nullable::Value(id)) => Some(id),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Query for listing checkpoint permissions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListFineTuningCheckpointPermissionsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub project_id: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<CheckpointPermissionOrder>,
}

/// Confirmation returned after deleting a checkpoint permission.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteFineTuningCheckpointPermissionResponse {
    pub id: String,
    pub object: CheckpointPermissionObject,
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeleteFineTuningCheckpointPermissionResponse {
    /// Future fields retained during decode.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use static_assertions::assert_impl_all;

    use super::{experimental_graders::*, *};

    assert_impl_all!(CreateFineTuningJobRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FineTuneMethod: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FineTuningJob: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FineTuningJobEvent: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FineTuningJobCheckpoint: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FineTuningCheckpointPermission: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ListPaginatedFineTuningJobsResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ListFineTuningJobEventsResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ListFineTuningJobCheckpointsResponse: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Grader: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(RunGraderRequest: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(RunGraderResponse: Serialize, DeserializeOwned, Send, Sync);

    fn ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    #[test]
    fn minimal_create_and_supervised_builder_round_trip() {
        let hyperparameters = FineTuneSupervisedHyperparameters {
            batch_size: Omittable::Value(AutoOrInteger::Auto(FineTuneAuto::Auto)),
            learning_rate_multiplier: Omittable::Value(AutoOrNumber::Value(0.2)),
            n_epochs: Omittable::Value(AutoOrInteger::Value(3)),
            ..FineTuneSupervisedHyperparameters::default()
        };
        let method = SupervisedFineTuneMethod::new().with_config(FineTuneSupervisedMethodConfig {
            hyperparameters: Omittable::Value(hyperparameters),
            ..FineTuneSupervisedMethodConfig::default()
        });
        let request = CreateFineTuningJobRequest::new("gpt-4o-mini", "file_train")
            .with_validation_file("file_valid")
            .with_suffix("weather")
            .with_method(method)
            .with_metadata(BTreeMap::from([("team".to_owned(), "search".to_owned())]));

        let value = ok(serde_json::to_value(&request));
        assert_eq!(value["method"]["type"], "supervised");
        assert_eq!(
            value["method"]["supervised"]["hyperparameters"]["n_epochs"],
            3
        );
        assert_eq!(value["metadata"]["team"], "search");
        assert_eq!(
            ok(serde_json::to_value(ok(serde_json::from_value::<
                CreateFineTuningJobRequest,
            >(value.clone())))),
            value
        );
    }

    #[test]
    fn dpo_and_method_tags_are_strict_but_future_methods_are_lossless() {
        let method = DpoFineTuneMethod::new().with_config(FineTuneDpoMethodConfig {
            hyperparameters: Omittable::Value(FineTuneDpoHyperparameters {
                beta: Omittable::Value(AutoOrNumber::Value(0.1)),
                batch_size: Omittable::Omitted,
                learning_rate_multiplier: Omittable::Value(AutoOrNumber::Auto(FineTuneAuto::Auto)),
                n_epochs: Omittable::Omitted,
                ..FineTuneDpoHyperparameters::default()
            }),
            ..FineTuneDpoMethodConfig::default()
        });
        let value = ok(serde_json::to_value(FineTuneMethod::from(method)));
        assert_eq!(value["type"], "dpo");
        assert_eq!(value["dpo"]["hyperparameters"]["beta"], 0.1);

        assert!(
            serde_json::from_value::<FineTuneMethod>(json!({
                "type": "supervised",
                "supervised": "invalid"
            }))
            .is_err()
        );

        let future = json!({
            "type": "distillation",
            "distillation": {"teacher": "gpt-future"},
            "future": true
        });
        let decoded = ok(serde_json::from_value::<FineTuneMethod>(future.clone()));
        assert!(matches!(decoded, FineTuneMethod::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[derive(Serialize)]
    struct GraderInput<'a> {
        role: &'a str,
        content: &'a str,
    }

    #[derive(Serialize)]
    struct GraderItem<'a> {
        label: &'a str,
    }

    #[test]
    fn reinforcement_and_alpha_grader_builders_need_no_json() {
        let exact = StringCheckGrader::new(
            "exact",
            "{{sample.output_text}}",
            "{{item.label}}",
            StringCheckOperation::Equal,
        );
        let similarity = TextSimilarityGrader::new(
            "similar",
            "{{sample.output_text}}",
            "{{item.label}}",
            TextSimilarityMetric::Cosine,
        );
        let multi = MultiGrader::many(
            "combined",
            [exact.clone().into(), similarity.into()],
            "0.5 * exact + 0.5 * similar",
        );
        let grader: Grader = multi.into();
        let request = CreateFineTuningJobRequest::new("gpt-5-mini", "file_train").with_method(
            ReinforcementFineTuneMethod::new(FineTuneReinforcementMethodConfig::new(
                grader.clone(),
            )),
        );
        let value = ok(serde_json::to_value(request));
        assert_eq!(value["method"]["type"], "reinforcement");
        assert!(value["method"]["reinforcement"]["grader"]["graders"].is_array());

        let score = ok(ScoreModelGrader::from_serializable_inputs(
            "judge",
            "gpt-5-mini",
            [GraderInput {
                role: "user",
                content: "Score {{sample.output_text}}",
            }],
        ))
        .with_range(0.0, 1.0);
        let run = ok(RunGraderRequest::new(score.into(), "candidate")
            .with_item(&GraderItem { label: "answer" }));
        let run_value = ok(serde_json::to_value(run));
        assert_eq!(run_value["grader"]["input"][0]["role"], "user");
        assert_eq!(run_value["item"]["label"], "answer");

        assert!(
            serde_json::from_value::<Grader>(json!({
                "type": "string_check",
                "name": "broken"
            }))
            .is_err()
        );
        let future = json!({"type": "wasm", "name": "future", "module": "..."});
        let decoded = ok(serde_json::from_value::<Grader>(future.clone()));
        assert!(matches!(decoded, Grader::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    fn job_fixture(status: &str) -> Value {
        json!({
            "id": "ftjob_123",
            "created_at": 1700000000,
            "error": null,
            "fine_tuned_model": null,
            "finished_at": null,
            "hyperparameters": {
                "batch_size": null,
                "hyper_future": true
            },
            "model": "gpt-future",
            "object": "fine_tuning.job",
            "organization_id": "org_123",
            "result_files": [],
            "status": status,
            "trained_tokens": null,
            "training_file": "file_train",
            "validation_file": null,
            "seed": 42,
            "job_future": {"value": 1}
        })
    }

    #[test]
    fn job_required_nullable_and_open_status_round_trip() {
        let fixture = job_fixture("paused");
        let job = ok(serde_json::from_value::<FineTuningJob>(fixture.clone()));
        assert_eq!(job.status.as_str(), "paused");
        assert!(!job.is_terminal());
        assert!(job.error.is_null());
        assert!(job.hyperparameters.extra().contains_key("hyper_future"));
        assert!(job.extra().contains_key("job_future"));
        assert_eq!(ok(serde_json::to_value(job)), fixture);

        let mut missing = job_fixture("running");
        match &mut missing {
            Value::Object(object) => {
                object.remove("error");
            }
            _ => panic!("job fixture must be an object"),
        }
        assert!(serde_json::from_value::<FineTuningJob>(missing).is_err());
    }

    #[test]
    fn events_checkpoints_and_pages_preserve_fields_and_cursors() {
        let events_fixture = json!({
            "object": "list",
            "data": [{
                "object": "fine_tuning.job.event",
                "id": "evt_1",
                "created_at": 1,
                "level": "future_level",
                "message": "running",
                "type": "metrics",
                "data": {"loss": 0.2},
                "event_future": true
            }],
            "has_more": true,
            "page_future": 1
        });
        let events = ok(serde_json::from_value::<ListFineTuningJobEventsResponse>(
            events_fixture.clone(),
        ));
        assert_eq!(events.next_after(), Some("evt_1"));
        assert_eq!(events.data[0].level.as_str(), "future_level");
        assert!(events.data[0].extra().contains_key("event_future"));
        assert!(events.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(events)), events_fixture);

        let checkpoints_fixture = json!({
            "object": "list",
            "data": [{
                "id": "ckpt_1",
                "created_at": 2,
                "fine_tuned_model_checkpoint": "ft:model:ckpt",
                "step_number": 10,
                "metrics": {"step": 10.0, "metric_future": 3},
                "fine_tuning_job_id": "ftjob_123",
                "object": "fine_tuning.job.checkpoint",
                "checkpoint_future": true
            }],
            "first_id": "ckpt_1",
            "last_id": "ckpt_1",
            "has_more": true
        });
        let checkpoints = ok(
            serde_json::from_value::<ListFineTuningJobCheckpointsResponse>(
                checkpoints_fixture.clone(),
            ),
        );
        assert_eq!(checkpoints.next_after(), Some("ckpt_1"));
        assert!(
            checkpoints.data[0]
                .metrics
                .extra()
                .contains_key("metric_future")
        );
        assert!(
            checkpoints.data[0]
                .extra()
                .contains_key("checkpoint_future")
        );
        assert_eq!(ok(serde_json::to_value(checkpoints)), checkpoints_fixture);
    }

    #[test]
    fn jobs_page_and_query_preserve_metadata_nullability() {
        let jobs_fixture = json!({
            "object": "list",
            "data": [job_fixture("succeeded")],
            "has_more": true
        });
        let jobs =
            ok(serde_json::from_value::<ListPaginatedFineTuningJobsResponse>(jobs_fixture.clone()));
        assert_eq!(
            jobs.next_after().map(FineTuningJobId::as_str),
            Some("ftjob_123")
        );
        assert!(jobs.data[0].is_terminal());
        assert_eq!(ok(serde_json::to_value(jobs)), jobs_fixture);

        let missing = ok(serde_json::from_value::<ListFineTuningJobsParams>(
            json!({}),
        ));
        assert!(missing.metadata.is_omitted());
        let null = ok(serde_json::from_value::<ListFineTuningJobsParams>(json!({
            "metadata": null
        })));
        assert!(matches!(null.metadata, Omittable::Value(Nullable::Null)));
    }

    #[test]
    fn checkpoint_permissions_cover_create_list_and_delete() {
        let create = CreateFineTuningCheckpointPermissionRequest::new([
            "proj_a".to_owned(),
            "proj_b".to_owned(),
        ]);
        assert_eq!(
            ok(serde_json::to_value(create)),
            json!({"project_ids": ["proj_a", "proj_b"]})
        );

        let page_fixture = json!({
            "object": "list",
            "data": [{
                "id": "perm_1",
                "created_at": 1,
                "project_id": "proj_a",
                "object": "checkpoint.permission",
                "permission_future": true
            }],
            "first_id": "perm_1",
            "last_id": "perm_1",
            "has_more": true,
            "page_future": true
        });
        let page = ok(serde_json::from_value::<
            ListFineTuningCheckpointPermissionResponse,
        >(page_fixture.clone()));
        assert_eq!(page.next_after(), Some("perm_1"));
        assert!(page.data[0].extra().contains_key("permission_future"));
        assert!(page.extra().contains_key("page_future"));
        assert_eq!(ok(serde_json::to_value(page)), page_fixture);

        let deleted_fixture = json!({
            "id": "perm_1",
            "object": "checkpoint.permission",
            "deleted": true,
            "delete_future": 1
        });
        let deleted = ok(serde_json::from_value::<
            DeleteFineTuningCheckpointPermissionResponse,
        >(deleted_fixture.clone()));
        assert!(deleted.extra().contains_key("delete_future"));
        assert_eq!(ok(serde_json::to_value(deleted)), deleted_fixture);
    }

    #[test]
    fn integration_known_tag_is_strict_and_unknown_is_lossless() {
        let integration = WandbIntegration::new(
            WandbIntegrationSettings::new("training")
                .with_tags(["nightly".to_owned(), "rft".to_owned()]),
        );
        let request = CreateFineTuningJobRequest::new("gpt-4o-mini", "file_train")
            .with_integration(integration);
        let value = ok(serde_json::to_value(request));
        assert_eq!(value["integrations"][0]["type"], "wandb");
        assert_eq!(value["integrations"][0]["wandb"]["tags"][1], "rft");

        assert!(
            serde_json::from_value::<FineTuningIntegration>(json!({
                "type": "wandb",
                "wandb": {}
            }))
            .is_err()
        );
        let future = json!({"type": "mlflow", "mlflow": {"experiment": "x"}});
        let decoded = ok(serde_json::from_value::<FineTuningIntegration>(
            future.clone(),
        ));
        assert!(matches!(decoded, FineTuningIntegration::Unknown(_)));
        assert_eq!(ok(serde_json::to_value(decoded)), future);
    }

    #[test]
    fn experimental_grader_run_response_is_complete_and_lossless() {
        let fixture = json!({
            "reward": 1.0,
            "metadata": {
                "name": "exact",
                "type": "string_check",
                "errors": {
                    "formula_parse_error": false,
                    "sample_parse_error": false,
                    "truncated_observation_error": false,
                    "unresponsive_reward_error": false,
                    "invalid_variable_error": false,
                    "other_error": false,
                    "python_grader_server_error": false,
                    "python_grader_server_error_type": null,
                    "python_grader_runtime_error": false,
                    "python_grader_runtime_error_details": null,
                    "model_grader_server_error": false,
                    "model_grader_refusal_error": false,
                    "model_grader_parse_error": false,
                    "model_grader_server_error_details": null,
                    "errors_future": true
                },
                "execution_time": 0.1,
                "scores": {"exact": 1.0},
                "token_usage": null,
                "sampled_model_name": null,
                "metadata_future": true
            },
            "sub_rewards": {"exact": 1.0},
            "model_grader_token_usage_per_model": {},
            "response_future": true
        });
        let response = ok(serde_json::from_value::<RunGraderResponse>(fixture.clone()));
        assert!(response.extra().contains_key("response_future"));
        assert!(response.metadata.extra().contains_key("metadata_future"));
        assert!(
            response
                .metadata
                .errors
                .extra()
                .contains_key("errors_future")
        );
        assert_eq!(ok(serde_json::to_value(response)), fixture);
    }
}
