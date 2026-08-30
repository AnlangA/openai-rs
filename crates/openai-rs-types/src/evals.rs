//! Typed wire models for Evals, runs, output items, and experimental graders.
//!
//! Stable Evals resources and their tagged unions live at this module's root.
//! Fine-tuning alpha grader endpoints are isolated under [`experimental`].

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use crate::{ExtraFields, Nullable, Omittable, open_string_enum, responses};

/// String-to-string metadata accepted by Evals resources.
pub type EvalMetadata = BTreeMap<String, String>;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Box<str>);

        impl $name {
            /// Creates an opaque id without validating a prefix.
            #[must_use]
            pub fn new(value: impl Into<Box<str>>) -> Self {
                Self(value.into())
            }

            /// Borrows the wire value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
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

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

opaque_id!(EvalId);
opaque_id!(EvalRunId);
opaque_id!(EvalRunOutputItemId);

macro_rules! literal_tag {
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

fn discriminator(value: &Value) -> Result<String, &'static str> {
    value
        .as_object()
        .ok_or("tagged eval value must be a JSON object")?
        .get("type")
        .ok_or("tagged eval object is missing string field `type`")?
        .as_str()
        .map(str::to_owned)
        .ok_or("tagged eval object field `type` must be a string")
}

macro_rules! tagged_union {
    ($(#[$meta:meta])* pub enum $name:ident {
        $($variant:ident($ty:ty) => $tag:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// A future tagged variant retained verbatim.
            Unknown(responses::UnknownTaggedObject),
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
                match discriminator(&value).map_err(D::Error::custom)?.as_str() {
                    $($tag => serde_json::from_value(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => responses::UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

open_string_enum! {
    /// Role used in an eval grader prompt.
    pub enum EvalMessageRole {
        User = "user",
        Assistant = "assistant",
        System = "system",
        Developer = "developer",
    }
}

literal_tag!(EvalInputTextTag, InputText, "input_text");
literal_tag!(EvalOutputTextTag, OutputText, "output_text");
literal_tag!(EvalInputImageTag, InputImage, "input_image");
literal_tag!(EvalInputAudioTag, InputAudio, "input_audio");

/// Tagged input text used in rich eval prompts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalInputText {
    #[serde(rename = "type")]
    kind: EvalInputTextTag,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalInputText {
    /// Creates an input-text content part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: EvalInputTextTag::InputText,
            text: text.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// Tagged output text used in rich eval prompts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalOutputText {
    #[serde(rename = "type")]
    kind: EvalOutputTextTag,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalOutputText {
    /// Creates an output-text content part.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: EvalOutputTextTag::OutputText,
            text: text.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// Image input used by an eval grader prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalInputImage {
    #[serde(rename = "type")]
    kind: EvalInputImageTag,
    image_url: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    detail: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalInputImage {
    /// Creates an image input from a URL.
    #[must_use]
    pub fn new(image_url: impl Into<String>) -> Self {
        Self {
            kind: EvalInputImageTag::InputImage,
            image_url: image_url.into(),
            detail: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets image detail.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Omittable::Value(detail.into());
        self
    }
}

open_string_enum! {
    /// Audio encoding accepted in eval prompt content.
    pub enum EvalAudioFormat {
        Mp3 = "mp3",
        Wav = "wav",
    }
}

/// Base64 audio descriptor nested in an eval input-audio part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalAudioData {
    data: String,
    format: EvalAudioFormat,
}

/// Audio input used by an eval grader prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalInputAudio {
    #[serde(rename = "type")]
    kind: EvalInputAudioTag,
    input_audio: EvalAudioData,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalInputAudio {
    /// Creates a base64 audio input.
    #[must_use]
    pub fn new(data: impl Into<String>, format: EvalAudioFormat) -> Self {
        Self {
            kind: EvalInputAudioTag::InputAudio,
            input_audio: EvalAudioData {
                data: data.into(),
                format,
            },
            extra: ExtraFields::new(),
        }
    }
}

/// One scalar or tagged content part in an eval prompt.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EvalContentItem {
    /// Simple text shorthand.
    Text(String),
    /// Tagged input text.
    InputText(EvalInputText),
    /// Tagged output text.
    OutputText(EvalOutputText),
    /// Image input.
    InputImage(EvalInputImage),
    /// Audio input.
    InputAudio(EvalInputAudio),
    /// Future tagged content.
    Unknown(responses::UnknownTaggedObject),
}

impl Serialize for EvalContentItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Text(value) => value.serialize(serializer),
            Self::InputText(value) => value.serialize(serializer),
            Self::OutputText(value) => value.serialize(serializer),
            Self::InputImage(value) => value.serialize(serializer),
            Self::InputAudio(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for EvalContentItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if let Value::String(text) = value {
            return Ok(Self::Text(text));
        }
        match discriminator(&value).map_err(D::Error::custom)?.as_str() {
            "input_text" => serde_json::from_value(value)
                .map(Self::InputText)
                .map_err(D::Error::custom),
            "output_text" => serde_json::from_value(value)
                .map(Self::OutputText)
                .map_err(D::Error::custom),
            "input_image" => serde_json::from_value(value)
                .map(Self::InputImage)
                .map_err(D::Error::custom),
            "input_audio" => serde_json::from_value(value)
                .map(Self::InputAudio)
                .map_err(D::Error::custom),
            _ => responses::UnknownTaggedObject::from_value(value)
                .map(Self::Unknown)
                .map_err(D::Error::custom),
        }
    }
}

/// One content part or an ordered content array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EvalMessageContent {
    /// One content part.
    One(EvalContentItem),
    /// Multiple content parts.
    Many(Vec<EvalContentItem>),
}

impl From<String> for EvalMessageContent {
    fn from(value: String) -> Self {
        Self::One(EvalContentItem::Text(value))
    }
}

impl From<&str> for EvalMessageContent {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl From<Vec<EvalContentItem>> for EvalMessageContent {
    fn from(value: Vec<EvalContentItem>) -> Self {
        Self::Many(value)
    }
}

literal_tag!(EvalMessageTag, Message, "message");

/// Prompt message accepted by eval graders and run templates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalMessage {
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Omittable::is_omitted"
    )]
    kind: Omittable<EvalMessageTag>,
    role: EvalMessageRole,
    content: EvalMessageContent,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalMessage {
    /// Creates a compact eval prompt message.
    #[must_use]
    pub fn new(role: EvalMessageRole, content: impl Into<EvalMessageContent>) -> Self {
        Self {
            kind: Omittable::Omitted,
            role,
            content: content.into(),
            extra: ExtraFields::new(),
        }
    }

    /// Emits the optional `type: "message"` field.
    #[must_use]
    pub fn with_type(mut self) -> Self {
        self.kind = Omittable::Value(EvalMessageTag::Message);
        self
    }
}

open_string_enum! {
    /// String comparison performed by a string-check grader.
    pub enum StringCheckOperation {
        Equal = "eq",
        NotEqual = "ne",
        Like = "like",
        ILike = "ilike",
    }
}

open_string_enum! {
    /// Metric used by a text-similarity grader.
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
        RougeL = "rouge_l",
    }
}

literal_tag!(LabelModelGraderTag, LabelModel, "label_model");
literal_tag!(StringCheckGraderTag, StringCheck, "string_check");
literal_tag!(TextSimilarityGraderTag, TextSimilarity, "text_similarity");
literal_tag!(PythonGraderTag, Python, "python");
literal_tag!(ScoreModelGraderTag, ScoreModel, "score_model");
literal_tag!(MultiGraderTag, Multi, "multi");

/// A model grader that assigns one label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelModelGrader {
    #[serde(rename = "type")]
    kind: LabelModelGraderTag,
    name: String,
    model: String,
    input: Vec<EvalMessage>,
    labels: Vec<String>,
    passing_labels: Vec<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl LabelModelGrader {
    /// Creates a label-model criterion.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        model: impl Into<String>,
        input: Vec<EvalMessage>,
        labels: Vec<String>,
        passing_labels: Vec<String>,
    ) -> Self {
        Self {
            kind: LabelModelGraderTag::LabelModel,
            name: name.into(),
            model: model.into(),
            input,
            labels,
            passing_labels,
            extra: ExtraFields::new(),
        }
    }
}

/// A string comparison grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StringCheckGrader {
    #[serde(rename = "type")]
    kind: StringCheckGraderTag,
    name: String,
    input: String,
    reference: String,
    operation: StringCheckOperation,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl StringCheckGrader {
    /// Creates a string-check criterion.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        input: impl Into<String>,
        reference: impl Into<String>,
        operation: StringCheckOperation,
    ) -> Self {
        Self {
            kind: StringCheckGraderTag::StringCheck,
            name: name.into(),
            input: input.into(),
            reference: reference.into(),
            operation,
            extra: ExtraFields::new(),
        }
    }
}

/// A text-similarity grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSimilarityGrader {
    #[serde(rename = "type")]
    kind: TextSimilarityGraderTag,
    name: String,
    input: String,
    reference: String,
    evaluation_metric: TextSimilarityMetric,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pass_threshold: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl TextSimilarityGrader {
    /// Creates a similarity grader.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        input: impl Into<String>,
        reference: impl Into<String>,
        evaluation_metric: TextSimilarityMetric,
    ) -> Self {
        Self {
            kind: TextSimilarityGraderTag::TextSimilarity,
            name: name.into(),
            input: input.into(),
            reference: reference.into(),
            evaluation_metric,
            pass_threshold: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Sets a pass threshold used by Eval criteria.
    #[must_use]
    pub fn pass_threshold(mut self, threshold: f64) -> Self {
        self.pass_threshold = Omittable::Value(threshold);
        self
    }
}

/// A Python grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonGrader {
    #[serde(rename = "type")]
    kind: PythonGraderTag,
    name: String,
    source: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_tag: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pass_threshold: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl PythonGrader {
    /// Creates a Python grader.
    #[must_use]
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            kind: PythonGraderTag::Python,
            name: name.into(),
            source: source.into(),
            image_tag: Omittable::Omitted,
            pass_threshold: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// Sampling controls for a model grader or run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EvalSamplingParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    seed: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_p: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_completion_tokens: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_completions_tokens: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning_effort: Omittable<responses::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    response_format: Omittable<Value>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text: Omittable<Value>,
}

impl EvalSamplingParams {
    /// Creates empty sampling controls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets sampling temperature.
    #[must_use]
    pub fn temperature(mut self, value: f64) -> Self {
        self.temperature = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Sets nucleus sampling probability.
    #[must_use]
    pub fn top_p(mut self, value: f64) -> Self {
        self.top_p = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Sets a deterministic seed.
    #[must_use]
    pub fn seed(mut self, value: i64) -> Self {
        self.seed = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Sets maximum generated tokens using the current run field name.
    #[must_use]
    pub fn max_completion_tokens(mut self, value: u64) -> Self {
        self.max_completion_tokens = Omittable::Value(Nullable::Value(value));
        self
    }

    /// Sets reasoning effort.
    #[must_use]
    pub fn reasoning_effort(mut self, value: responses::ReasoningEffort) -> Self {
        self.reasoning_effort = Omittable::Value(value);
        self
    }

    /// Serializes and adds one tool without requiring JSON text.
    pub fn tool<T: Serialize>(mut self, tool: &T) -> Result<Self, serde_json::Error> {
        let mut tools = match std::mem::take(&mut self.tools) {
            Omittable::Value(tools) => tools,
            Omittable::Omitted => Vec::new(),
        };
        tools.push(serde_json::to_value(tool)?);
        self.tools = Omittable::Value(tools);
        Ok(self)
    }
}

/// A score-model grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreModelGrader {
    #[serde(rename = "type")]
    kind: ScoreModelGraderTag,
    name: String,
    input: Vec<EvalMessage>,
    model: String,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sampling_params: Omittable<EvalSamplingParams>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    range: Omittable<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pass_threshold: Omittable<f64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl ScoreModelGrader {
    /// Creates a score-model grader.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        model: impl Into<String>,
        input: Vec<EvalMessage>,
    ) -> Self {
        Self {
            kind: ScoreModelGraderTag::ScoreModel,
            name: name.into(),
            input,
            model: model.into(),
            sampling_params: Omittable::Omitted,
            range: Omittable::Omitted,
            pass_threshold: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

tagged_union! {
    /// One stable testing criterion attached to an Eval.
    pub enum TestingCriterion {
        LabelModel(LabelModelGrader) => "label_model",
        StringCheck(StringCheckGrader) => "string_check",
        TextSimilarity(TextSimilarityGrader) => "text_similarity",
        Python(PythonGrader) => "python",
        ScoreModel(ScoreModelGrader) => "score_model"
    }
}

/// One or several members accepted by the historically inconsistent
/// `multi.graders` wire field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MultiGraderMembers {
    /// Pinned schema and generated SDK shape.
    One(Box<Grader>),
    /// Official example compatibility shape.
    Many(Vec<Grader>),
}

/// A formula-composed grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiGrader {
    #[serde(rename = "type")]
    kind: MultiGraderTag,
    name: String,
    graders: MultiGraderMembers,
    calculate_output: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl MultiGrader {
    /// Creates the pinned single-member shape.
    #[must_use]
    pub fn one(
        name: impl Into<String>,
        grader: Grader,
        calculate_output: impl Into<String>,
    ) -> Self {
        Self {
            kind: MultiGraderTag::Multi,
            name: name.into(),
            graders: MultiGraderMembers::One(Box::new(grader)),
            calculate_output: calculate_output.into(),
            extra: ExtraFields::new(),
        }
    }
}

tagged_union! {
    /// Grader union used by the experimental run/validate endpoints.
    pub enum Grader {
        StringCheck(StringCheckGrader) => "string_check",
        TextSimilarity(TextSimilarityGrader) => "text_similarity",
        Python(PythonGrader) => "python",
        ScoreModel(ScoreModelGrader) => "score_model",
        LabelModel(LabelModelGrader) => "label_model",
        Multi(MultiGrader) => "multi"
    }
}

/// Stable testing-criterion discriminator manifest.
pub const TESTING_CRITERION_DISCRIMINATORS: [&str; 5] = [
    "label_model",
    "string_check",
    "text_similarity",
    "python",
    "score_model",
];

/// Experimental grader discriminator manifest.
pub const GRADER_DISCRIMINATORS: [&str; 6] = [
    "string_check",
    "text_similarity",
    "python",
    "score_model",
    "label_model",
    "multi",
];

literal_tag!(CreateCustomDataSourceTag, Custom, "custom");
literal_tag!(CreateLogsDataSourceTag, Logs, "logs");
literal_tag!(CreateStoredCompletionsDataSourceTag, StoredCompletions, "stored_completions");
literal_tag!(EvalCustomDataSourceTag, Custom, "custom");
literal_tag!(EvalLogsDataSourceTag, Logs, "logs");
literal_tag!(EvalStoredCompletionsDataSourceTag, StoredCompletions, "stored_completions");

/// Custom schema supplied while creating an Eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCustomDataSourceConfig {
    #[serde(rename = "type")]
    kind: CreateCustomDataSourceTag,
    item_schema: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include_sample_schema: Omittable<bool>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CreateCustomDataSourceConfig {
    /// Creates a custom data-source config from a typed schema representation.
    pub fn from_serializable<T: Serialize>(schema: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            kind: CreateCustomDataSourceTag::Custom,
            item_schema: serde_json::to_value(schema)?,
            include_sample_schema: Omittable::Omitted,
            extra: ExtraFields::new(),
        })
    }

    /// Controls whether callers populate the `sample` namespace.
    #[must_use]
    pub fn include_sample_schema(mut self, include: bool) -> Self {
        self.include_sample_schema = Omittable::Value(include);
        self
    }
}

/// Logs filter supplied while creating an Eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateLogsDataSourceConfig {
    #[serde(rename = "type")]
    kind: CreateLogsDataSourceTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Value>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CreateLogsDataSourceConfig {
    /// Creates an unfiltered logs config.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: CreateLogsDataSourceTag::Logs,
            metadata: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Serializes log filters without requiring JSON text.
    pub fn metadata<T: Serialize>(mut self, value: &T) -> Result<Self, serde_json::Error> {
        self.metadata = Omittable::Value(serde_json::to_value(value)?);
        Ok(self)
    }
}

impl Default for CreateLogsDataSourceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Deprecated stored-completions filter used while creating an Eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateStoredCompletionsDataSourceConfig {
    #[serde(rename = "type")]
    kind: CreateStoredCompletionsDataSourceTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Value>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CreateStoredCompletionsDataSourceConfig {
    /// Creates an unfiltered legacy config.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: CreateStoredCompletionsDataSourceTag::StoredCompletions,
            metadata: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

impl Default for CreateStoredCompletionsDataSourceConfig {
    fn default() -> Self {
        Self::new()
    }
}

tagged_union! {
    /// Data-source configuration accepted by Eval creation.
    pub enum CreateEvalDataSourceConfig {
        Custom(CreateCustomDataSourceConfig) => "custom",
        Logs(CreateLogsDataSourceConfig) => "logs",
        StoredCompletions(CreateStoredCompletionsDataSourceConfig) => "stored_completions"
    }
}

macro_rules! response_data_source {
    ($name:ident, $tag:ident, $variant:ident) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            schema: Value,
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            metadata: Omittable<Nullable<EvalMetadata>>,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Returns the server-computed JSON Schema.
            #[must_use]
            pub const fn schema(&self) -> &Value {
                &self.schema
            }

            /// Returns forward-compatible response fields.
            #[must_use]
            pub const fn extra_fields(&self) -> &ExtraFields {
                &self.extra
            }
        }
    };
}

response_data_source!(EvalCustomDataSourceConfig, EvalCustomDataSourceTag, Custom);
response_data_source!(EvalLogsDataSourceConfig, EvalLogsDataSourceTag, Logs);
response_data_source!(
    EvalStoredCompletionsDataSourceConfig,
    EvalStoredCompletionsDataSourceTag,
    StoredCompletions
);

tagged_union! {
    /// Data-source configuration returned on an Eval resource.
    pub enum EvalDataSourceConfig {
        Custom(EvalCustomDataSourceConfig) => "custom",
        Logs(EvalLogsDataSourceConfig) => "logs",
        StoredCompletions(EvalStoredCompletionsDataSourceConfig) => "stored_completions"
    }
}

/// Create-config discriminator manifest.
pub const CREATE_EVAL_DATA_SOURCE_DISCRIMINATORS: [&str; 3] = [
    "custom",
    "logs",
    "stored_completions",
];

/// Resource-config discriminator manifest.
pub const EVAL_DATA_SOURCE_DISCRIMINATORS: [&str; 3] = [
    "custom",
    "logs",
    "stored_completions",
];

/// Body for `POST /evals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateEvalRequest {
    data_source_config: CreateEvalDataSourceConfig,
    testing_criteria: Vec<TestingCriterion>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<EvalMetadata>>,
}

impl CreateEvalRequest {
    /// Creates an Eval definition.
    #[must_use]
    pub fn new(
        data_source_config: CreateEvalDataSourceConfig,
        testing_criteria: Vec<TestingCriterion>,
    ) -> Self {
        Self {
            data_source_config,
            testing_criteria,
            name: Omittable::Omitted,
            metadata: Omittable::Omitted,
        }
    }

    /// Sets an Eval name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }

    /// Sets metadata.
    #[must_use]
    pub fn metadata(mut self, metadata: EvalMetadata) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }

    /// Explicitly clears metadata.
    #[must_use]
    pub fn metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }
}

/// Body for `POST /evals/{eval_id}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateEvalRequest {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<EvalMetadata>>,
}

impl UpdateEvalRequest {
    /// Creates an empty update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Renames the Eval.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }

    /// Replaces metadata.
    #[must_use]
    pub fn metadata(mut self, metadata: EvalMetadata) -> Self {
        self.metadata = Omittable::Value(Nullable::Value(metadata));
        self
    }

    /// Explicitly clears metadata.
    #[must_use]
    pub fn metadata_null(mut self) -> Self {
        self.metadata = Omittable::Value(Nullable::Null);
        self
    }
}

literal_tag!(EvalObjectTag, Eval, "eval");

/// Stable Eval resource returned by create/retrieve/update/list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Eval {
    #[serde(rename = "object")]
    object: EvalObjectTag,
    id: EvalId,
    name: String,
    data_source_config: EvalDataSourceConfig,
    testing_criteria: Vec<TestingCriterion>,
    created_at: i64,
    metadata: Nullable<EvalMetadata>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Eval {
    /// Returns the Eval id.
    #[must_use]
    pub const fn id(&self) -> &EvalId {
        &self.id
    }

    /// Returns testing criteria.
    #[must_use]
    pub fn testing_criteria(&self) -> &[TestingCriterion] {
        &self.testing_criteria
    }

    /// Returns unknown response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Result returned after deleting an Eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedEval {
    object: String,
    deleted: bool,
    eval_id: EvalId,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl DeletedEval {
    /// Returns whether deletion completed.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted
    }
}

open_string_enum! {
    /// Sort order shared by Evals list endpoints.
    pub enum EvalSortOrder {
        Ascending = "asc",
        Descending = "desc",
    }
}

open_string_enum! {
    /// Field used to order Eval definitions.
    pub enum EvalOrderBy {
        CreatedAt = "created_at",
        UpdatedAt = "updated_at",
    }
}

/// Query parameters for `GET /evals`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListEvalsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<EvalId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<EvalSortOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order_by: Omittable<EvalOrderBy>,
}

impl ListEvalsParams {
    /// Creates empty list parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an opaque cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<EvalId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Sets page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum EvalListObjectTag {
    #[serde(rename = "list")]
    List,
}

/// Cursor page of Eval definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalList {
    #[serde(rename = "object")]
    object: EvalListObjectTag,
    data: Vec<Eval>,
    first_id: Nullable<EvalId>,
    last_id: Nullable<EvalId>,
    has_more: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalList {
    /// Returns page items.
    #[must_use]
    pub fn data(&self) -> &[Eval] {
        &self.data
    }

    /// Returns whether another page exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}
